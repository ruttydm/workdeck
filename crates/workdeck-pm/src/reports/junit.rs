//! Explicit Ant-style testsuite/testcase subset; no DTD or external entity loading.
use super::*;
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use std::collections::{BTreeMap, BTreeSet};

type ParseResult<T> = std::result::Result<T, &'static str>;

#[derive(Default)]
struct Suite {
    name: String,
    counts: ReportCounts,
    declared: BTreeMap<String, u64>,
    cases: BTreeSet<(String, String)>,
}
struct Case {
    identity: Option<JUnitCaseIdentity>,
    id: String,
    path: Option<String>,
    line: Option<u64>,
    outcome: Option<String>,
    message: String,
}
#[derive(Default)]
struct Parser {
    stack: Vec<String>,
    suites: Vec<Suite>,
    active_suites: Vec<usize>,
    case: Option<Case>,
    roots: usize,
    root_declared: BTreeMap<String, u64>,
    nodes: usize,
    expected: BTreeSet<String>,
    expected_counts: ReportCounts,
    captured_cases: Option<Vec<JUnitTestCase>>,
    captured_bytes: usize,
}

fn attrs(event: &BytesStart<'_>, reader: &Reader<&[u8]>) -> ParseResult<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    for attr in event.attributes() {
        let attr = attr.map_err(|_| "junit_invalid_attributes")?;
        let name = std::str::from_utf8(attr.key.as_ref()).map_err(|_| "junit_invalid_utf8")?;
        let value = attr
            .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|_| "junit_invalid_entity")?;
        values.insert(name.into(), value.into_owned());
    }
    Ok(values)
}

impl Parser {
    fn start(&mut self, name: String, attrs: BTreeMap<String, String>) -> ParseResult<()> {
        self.nodes += 1;
        if self.nodes > 100_000 || self.stack.len() >= 32 {
            return Err("junit_structure_limit");
        }
        let parent = self.stack.last().map(String::as_str);
        if parent.is_none() {
            self.roots += 1;
            if self.roots > 1 || !matches!(name.as_str(), "testsuite" | "testsuites") {
                return Err("junit_invalid_root");
            }
        }
        match name.as_str() {
            "testsuites" => {
                if parent.is_some() {
                    return Err("junit_invalid_structure");
                }
                for key in ["tests", "failures", "errors", "skipped"] {
                    if let Some(value) = attrs.get(key) {
                        self.root_declared.insert(
                            key.into(),
                            value.parse().map_err(|_| "junit_invalid_counts")?,
                        );
                    }
                }
            }
            "testsuite" => {
                if !matches!(parent, None | Some("testsuites" | "testsuite")) {
                    return Err("junit_invalid_structure");
                }
                let suite_name = attrs
                    .get("name")
                    .filter(|name| !name.is_empty())
                    .ok_or("junit_suite_identity_missing")?;
                let mut suite = Suite {
                    name: suite_name.clone(),
                    ..Suite::default()
                };
                for key in ["tests", "failures", "errors", "skipped"] {
                    if let Some(value) = attrs.get(key) {
                        suite.declared.insert(
                            key.into(),
                            value.parse().map_err(|_| "junit_invalid_counts")?,
                        );
                    }
                }
                self.active_suites.push(self.suites.len());
                self.suites.push(suite);
            }
            "testcase" => {
                if parent != Some("testsuite") || self.case.is_some() {
                    return Err("junit_invalid_structure");
                }
                let case_name = attrs
                    .get("name")
                    .filter(|name| !name.is_empty())
                    .ok_or("junit_case_identity_missing")?;
                let id = format!(
                    "{}::{case_name}",
                    attrs
                        .get("classname")
                        .map(String::as_str)
                        .unwrap_or_default()
                );
                let suite = self
                    .active_suites
                    .last()
                    .copied()
                    .ok_or("junit_invalid_structure")?;
                if !self.suites[suite].cases.insert((
                    attrs.get("classname").cloned().unwrap_or_default(),
                    case_name.clone(),
                )) {
                    return Err("junit_duplicate_case");
                }
                let line = attrs
                    .get("line")
                    .map(|line| line.parse::<u64>().map_err(|_| "junit_invalid_location"))
                    .transpose()?;
                self.case = Some(Case {
                    identity: if self.captured_cases.is_some() {
                        if self
                            .active_suites
                            .iter()
                            .any(|i| self.suites[*i].name.len() > 256)
                            || attrs.get("classname").is_some_and(|name| name.len() > 1024)
                            || case_name.len() > 1024
                        {
                            return Err("junit_case_identity_limit");
                        }
                        Some(JUnitCaseIdentity {
                            suites: self
                                .active_suites
                                .iter()
                                .map(|i| self.suites[*i].name.clone())
                                .collect(),
                            class_name: attrs.get("classname").cloned().unwrap_or_default(),
                            name: case_name.clone(),
                        })
                    } else {
                        None
                    },
                    id: format!("{}::{id}", self.suites[suite].name),
                    path: attrs.get("file").cloned(),
                    line,
                    outcome: None,
                    message: String::new(),
                });
            }
            "failure" | "error" | "skipped" => {
                if parent != Some("testcase") {
                    return Err("junit_invalid_structure");
                }
                let case = self.case.as_mut().ok_or("junit_invalid_structure")?;
                if case.outcome.is_some() {
                    return Err("junit_conflicting_case_outcomes");
                }
                case.outcome = Some(name.clone());
                case.message = clipped(
                    attrs.get("message").map(String::as_str).unwrap_or_default(),
                    1024,
                );
            }
            "properties" if matches!(parent, Some("testsuite" | "testcase")) => (),
            "property" if parent == Some("properties") => (),
            "system-out" | "system-err" if matches!(parent, Some("testsuite" | "testcase")) => (),
            _ => return Err("junit_unsupported_element"),
        }
        self.stack.push(name);
        Ok(())
    }

    fn text(&mut self, text: &str) -> ParseResult<()> {
        if self.stack.is_empty() && !text.trim().is_empty() {
            return Err("junit_text_outside_root");
        }
        if self
            .stack
            .last()
            .is_some_and(|name| matches!(name.as_str(), "failure" | "error"))
        {
            let case = self.case.as_mut().ok_or("junit_invalid_structure")?;
            if case.message.len() < 1024 {
                case.message
                    .push_str(&clipped(text, 1024 - case.message.len()));
            }
        }
        Ok(())
    }

    fn end(&mut self, name: &str, result: &mut ReportAssessment) -> ParseResult<()> {
        if self.stack.pop().as_deref() != Some(name) {
            return Err("junit_invalid_structure");
        }
        if name == "testcase" {
            let case = self.case.take().ok_or("junit_invalid_structure")?;
            let outcome = case.outcome.as_deref();
            if self
                .active_suites
                .iter()
                .any(|index| self.expected.contains(&self.suites[*index].name))
            {
                count(&mut self.expected_counts, outcome);
            }
            for index in &self.active_suites {
                count(&mut self.suites[*index].counts, outcome);
            }
            count(&mut result.counts, outcome);
            if let Some(cases) = &mut self.captured_cases {
                let identity = case
                    .identity
                    .as_ref()
                    .ok_or("junit_case_identity_missing")?;
                identity
                    .validate()
                    .map_err(|_| "junit_case_identity_limit")?;
                self.captured_bytes += identity.suites.iter().map(String::len).sum::<usize>()
                    + identity.class_name.len()
                    + identity.name.len();
                if cases.len() >= 20_000 || self.captured_bytes > 2 * 1024 * 1024 {
                    return Err("junit_case_inventory_limit");
                }
                cases.push(JUnitTestCase {
                    identity: identity.clone(),
                    outcome: match outcome {
                        Some("failure") => JUnitCaseOutcome::Failed,
                        Some("error") => JUnitCaseOutcome::Error,
                        Some("skipped") => JUnitCaseOutcome::Skipped,
                        _ => JUnitCaseOutcome::Passed,
                    },
                });
            }
            if matches!(outcome, Some("failure" | "error")) {
                result.failure(&case.id, &case.message, case.path.as_deref(), case.line);
            }
        } else if name == "testsuites" {
            for (key, value) in &self.root_declared {
                let actual = match key.as_str() {
                    "tests" => result.counts.discovered,
                    "failures" => result.counts.failed,
                    "errors" => result.counts.errors,
                    "skipped" => result.counts.skipped,
                    _ => unreachable!(),
                };
                if *value != actual {
                    return Err("junit_count_mismatch");
                }
            }
        } else if name == "testsuite" {
            let index = self.active_suites.pop().ok_or("junit_invalid_structure")?;
            let suite = &self.suites[index];
            for (key, value) in &suite.declared {
                let actual = match key.as_str() {
                    "tests" => suite.counts.discovered,
                    "failures" => suite.counts.failed,
                    "errors" => suite.counts.errors,
                    "skipped" => suite.counts.skipped,
                    _ => unreachable!(),
                };
                if *value != actual {
                    return Err("junit_count_mismatch");
                }
            }
        }
        Ok(())
    }
}

fn count(counts: &mut ReportCounts, outcome: Option<&str>) {
    counts.discovered += 1;
    match outcome {
        Some("failure") => counts.failed += 1,
        Some("error") => counts.errors += 1,
        Some("skipped") => counts.skipped += 1,
        _ => counts.passed += 1,
    }
}

fn parse(
    bytes: &[u8],
    result: &mut ReportAssessment,
    expected: &[String],
    capture_cases: bool,
) -> ParseResult<Parser> {
    // The supported report contract is UTF-8; decoding never consults external resources.
    std::str::from_utf8(bytes).map_err(|_| "junit_invalid_utf8")?;
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().expand_empty_elements = true;
    let mut parser = Parser {
        expected: expected.iter().cloned().collect(),
        captured_cases: capture_cases.then(Vec::new),
        ..Parser::default()
    };
    let mut declaration_seen = false;
    loop {
        match reader.read_event().map_err(|_| "junit_malformed_xml")? {
            Event::Start(event) | Event::Empty(event) => {
                // Empty events are expanded by the reader configuration below instead.
                let name = std::str::from_utf8(event.name().as_ref())
                    .map_err(|_| "junit_invalid_utf8")?
                    .to_owned();
                let attributes = attrs(&event, &reader)?;
                parser.start(name, attributes)?;
            }
            Event::End(event) => {
                let name = std::str::from_utf8(event.name().as_ref())
                    .map_err(|_| "junit_invalid_utf8")?
                    .to_owned();
                parser.end(&name, result)?;
            }
            Event::Text(event) => {
                parser.text(&event.decode().map_err(|_| "junit_invalid_utf8")?)?
            }
            Event::CData(event) => {
                parser.text(&event.decode().map_err(|_| "junit_invalid_utf8")?)?
            }
            Event::GeneralRef(event) => {
                let value = event.decode().map_err(|_| "junit_invalid_entity")?;
                let encoded = format!("&{value};");
                let decoded =
                    quick_xml::escape::unescape(&encoded).map_err(|_| "junit_invalid_entity")?;
                parser.text(&decoded)?;
            }
            Event::DocType(_) => return Err("junit_dtd_unsupported"),
            Event::Decl(event) => {
                if declaration_seen
                    || event
                        .version()
                        .map_err(|_| "junit_invalid_xml_version")?
                        .as_ref()
                        != b"1.0"
                {
                    return Err("junit_invalid_xml_version");
                }
                declaration_seen = true;
                if parser.roots != 0
                    || event
                        .encoding()
                        .transpose()
                        .map_err(|_| "junit_invalid_encoding")?
                        .is_some_and(|encoding| !encoding.eq_ignore_ascii_case(b"UTF-8"))
                {
                    return Err("junit_invalid_encoding");
                }
            }
            Event::PI(_) => return Err("junit_processing_instruction_unsupported"),
            Event::Comment(_) => (),
            Event::Eof => break,
        }
    }
    if parser.roots != 1 || !parser.stack.is_empty() {
        return Err("junit_incomplete_xml");
    }
    Ok(parser)
}

pub(super) fn assess(
    bytes: &[u8],
    expected: &[String],
    minimum: u64,
    maximum_skipped: Option<u64>,
) -> ReportAssessment {
    let mut result = ReportAssessment::new(ReportState::Unknown, "junit_unassessed");
    if expected.is_empty() || minimum == 0 {
        result.reason_codes = vec!["junit_invalid_expectation".into()];
        return result;
    }
    let parser = match parse(bytes, &mut result, expected, false) {
        Ok(parser) => parser,
        Err(reason) => {
            result.reason_codes = vec![reason.into()];
            return result;
        }
    };
    let expected_present = expected.iter().all(|name| {
        parser
            .suites
            .iter()
            .any(|suite| &suite.name == name && suite.counts.discovered > 0)
    });
    let expected_executed = expected.iter().all(|name| {
        parser
            .suites
            .iter()
            .any(|suite| &suite.name == name && suite.counts.discovered > suite.counts.skipped)
    });
    let (state, reason) = if !expected_present {
        (
            ReportState::Unknown,
            "junit_expected_suite_missing_or_empty",
        )
    } else if result.counts.failed + result.counts.errors > 0 {
        (ReportState::Failed, "junit_case_failures")
    } else if !expected_executed || result.counts.discovered == result.counts.skipped {
        (ReportState::Skipped, "junit_expected_suite_skipped")
    } else if parser.expected_counts.discovered - parser.expected_counts.skipped < minimum {
        (ReportState::Unknown, "junit_minimum_tests_unmet")
    } else if maximum_skipped.is_some_and(|max| result.counts.skipped > max) {
        (ReportState::Failed, "junit_skip_limit_exceeded")
    } else {
        (ReportState::Passed, "junit_expected_suite_passed")
    };
    result.state = state;
    result.reason_codes = vec![reason.into()];
    result
}

pub(super) fn inventory(bytes: &[u8]) -> ParseResult<Vec<JUnitTestCase>> {
    let mut result = ReportAssessment::new(ReportState::Unknown, "junit_inventory");
    Ok(parse(bytes, &mut result, &[], true)?
        .captured_cases
        .unwrap_or_default())
}
