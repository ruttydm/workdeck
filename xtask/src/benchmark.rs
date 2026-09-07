//! MIT translation of Hunk benchmark result aggregation (Modem Labs Inc.).
//! Historical thresholds describe source reports, not the strict semantic-port release gate.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Run {
    version: u32,
    git_sha: Option<String>,
    #[serde(default)]
    accepted_regressions: Vec<AcceptedRegression>,
    results: Vec<Metric>,
}

#[derive(Debug, Deserialize)]
struct AcceptedRegression {
    name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Comparison {
    version: u32,
    generated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    head_sha: Option<String>,
    failed: bool,
    rows: Vec<Row>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    name: String,
    unit: String,
    base_median: f64,
    head_median: f64,
    absolute_delta: f64,
    relative_delta: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    threshold: Option<Threshold>,
    status: &'static str,
    source: String,
}

fn material_regression(base: f64, head: f64, threshold: &Threshold) -> bool {
    let delta = head - base;
    if delta <= 0.0 || delta < threshold.min_absolute_regression {
        return false;
    }
    if base == 0.0 {
        return head > 0.0;
    }
    head / base >= threshold.max_regression_ratio
}

fn compare(base: &Run, head: &Run, generated_at: String) -> Comparison {
    let base_by_name: BTreeMap<_, _> = base.results.iter().map(|m| (m.name.as_str(), m)).collect();
    let head_by_name: BTreeMap<_, _> = head.results.iter().map(|m| (m.name.as_str(), m)).collect();
    let mut names: Vec<_> = base_by_name
        .keys()
        .chain(head_by_name.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    // JavaScript's default sort compares UTF-16 code units, not Unicode scalar values.
    names.sort_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
    let accepted: BTreeSet<_> = head
        .accepted_regressions
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    let rows: Vec<_> = names
        .into_iter()
        .map(|name| {
            let base = base_by_name.get(name).copied();
            let head = head_by_name.get(name).copied();
            let metadata = head.or(base).expect("union contains a metric");
            let threshold = head
                .and_then(|m| m.threshold.clone())
                .or_else(|| base.and_then(|m| m.threshold.clone()));
            let base_median = base.map_or(0.0, |m| m.median);
            let head_median = head.map_or(0.0, |m| m.median);
            let (relative_delta, status) = match (base, head) {
                (None, Some(m)) => (
                    f64::INFINITY,
                    if m.comparable {
                        "missing-base"
                    } else {
                        "informational"
                    },
                ),
                (Some(m), None) => (
                    -1.0,
                    if m.comparable {
                        "missing-head"
                    } else {
                        "informational"
                    },
                ),
                (Some(_), Some(m)) => {
                    let delta = if base_median == 0.0 {
                        if head_median == 0.0 {
                            0.0
                        } else {
                            f64::INFINITY
                        }
                    } else {
                        head_median / base_median - 1.0
                    };
                    let status = match threshold.as_ref().filter(|_| m.comparable) {
                        None => "informational",
                        Some(t) if material_regression(base_median, head_median, t) => {
                            if accepted.contains(name) {
                                "accepted"
                            } else {
                                "fail"
                            }
                        }
                        Some(_) => "pass",
                    };
                    (delta, status)
                }
                (None, None) => unreachable!(),
            };
            Row {
                name: name.into(),
                unit: metadata.unit.clone(),
                base_median,
                head_median,
                absolute_delta: head_median - base_median,
                relative_delta,
                threshold,
                status,
                source: metadata.source.clone(),
            }
        })
        .collect();
    Comparison {
        version: 1,
        generated_at,
        base_sha: base.git_sha.clone(),
        head_sha: head.git_sha.clone(),
        failed: rows
            .iter()
            .any(|r| matches!(r.status, "fail" | "missing-head")),
        rows,
    }
}

// Number.toFixed rounds the exact binary value, with ties away from zero. Scaling an f64
// first can introduce a second rounding (for example 2.55 * 10), so use its integer significand.
fn fixed(value: f64, digits: u32) -> String {
    assert!(digits <= 2);
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .into();
    }
    if value.abs() >= 1e21 {
        let result = format!("{value:e}");
        return if result.contains("e-") {
            result
        } else {
            result.replace('e', "e+")
        };
    }
    let absolute = value.abs();
    let bits = absolute.to_bits();
    let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1_u64 << 52) - 1);
    let (significand, exponent) = if exponent_bits == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1_u64 << 52), exponent_bits - 1023 - 52)
    };
    let scaled = u128::from(significand) * 10_u128.pow(digits);
    let rounded = if exponent >= 0 {
        scaled << exponent
    } else if -exponent >= 128 {
        0
    } else {
        let shift = -exponent as u32;
        let quotient = scaled >> shift;
        let remainder = scaled & ((1_u128 << shift) - 1);
        quotient + u128::from(remainder >= (1_u128 << (shift - 1)))
    };
    let sign = if value < 0.0 { "-" } else { "" };
    if digits == 0 {
        return format!("{sign}{rounded}");
    }
    let divisor = 10_u128.pow(digits);
    format!(
        "{sign}{}.{:0width$}",
        rounded / divisor,
        rounded % divisor,
        width = digits as usize
    )
}

fn number(value: f64) -> String {
    if !value.is_finite() {
        return "∞".into();
    }
    fixed(value, if value.abs() >= 100.0 { 1 } else { 2 })
}

fn markdown(comparison: &Comparison, base: &str, head: &str) -> String {
    let failures = comparison
        .rows
        .iter()
        .filter(|r| matches!(r.status, "fail" | "missing-head"))
        .count();
    let result = if comparison.failed {
        format!(
            "❌ {failures} material benchmark regression{} found.",
            if failures == 1 { "" } else { "s" }
        )
    } else {
        "✅ No unaccepted material benchmark regressions found.".into()
    };
    let mut lines = vec![
        "## Release benchmark gate".into(),
        String::new(),
        result,
        String::new(),
        format!("Base: `{base}`  "),
        format!("Head: `{head}`"),
        String::new(),
        "| Status | Metric | Base median | Head median | Δ | Threshold |".into(),
        "| --- | --- | ---: | ---: | ---: | --- |".into(),
    ];
    for row in &comparison.rows {
        let unit = if row.unit == "bytes" { "B" } else { &row.unit };
        let icon = match row.status {
            "fail" | "missing-head" => "❌",
            "accepted" => "⚠️",
            _ => "✅",
        };
        let delta = if !row.relative_delta.is_finite() {
            "+∞".into()
        } else {
            format!(
                "{}{}%",
                if row.relative_delta > 0.0 { "+" } else { "" },
                fixed(row.relative_delta * 100.0, 1)
            )
        };
        let threshold = row.threshold.as_ref().map_or_else(
            || "—".into(),
            |t| {
                let absolute = if row.unit == "bytes" {
                    format!(
                        "{} MiB",
                        number(t.min_absolute_regression / (1024.0 * 1024.0))
                    )
                } else {
                    format!("{} {unit}", number(t.min_absolute_regression))
                };
                format!(
                    "+{}% and +{absolute}",
                    fixed((t.max_regression_ratio - 1.0) * 100.0, 0)
                )
            },
        );
        lines.push(format!(
            "| {icon} {} | `{}` | {} {unit} | {} {unit} | {delta} | {threshold} |",
            row.status,
            row.name,
            number(row.base_median),
            number(row.head_median)
        ));
    }
    format!("{}\n", lines.join("\n"))
}

fn compare_files(mut args: impl Iterator<Item = String>, as_markdown: bool) -> Result<()> {
    let (Some(base), Some(head)) = (args.next(), args.next()) else {
        bail!("benchmark compare-json requires BASE_JSON HEAD_JSON");
    };
    if args.next().is_some() {
        bail!("unexpected benchmark compare-json argument");
    }
    let load = |path: &str| -> Result<Run> {
        let run: Run = serde_json::from_slice(&std::fs::read(path)?)?;
        if run.version != 1 {
            bail!("Invalid benchmark result file: {path}");
        }
        Ok(run)
    };
    let comparison = compare(
        &load(&base)?,
        &load(&head)?,
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    );
    if as_markdown {
        print!("{}", markdown(&comparison, &base, &head));
    } else {
        println!("{}", serde_json::to_string_pretty(&comparison)?);
    }
    if comparison.failed {
        bail!(
            "Historical benchmark comparison failed; this command is not the strict semantic-port performance gate"
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Threshold {
    pub max_regression_ratio: f64,
    pub min_absolute_regression: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Metric {
    pub name: String,
    pub unit: String,
    pub samples: Vec<f64>,
    pub median: f64,
    pub p75: f64,
    pub p95: f64,
    pub min: f64,
    pub max: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<Threshold>,
    pub comparable: bool,
    pub source: String,
}

fn percentile(samples: &[f64], percent: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|left, right| (left - right).partial_cmp(&0.0).unwrap_or(Ordering::Equal));
    let index = ((percent / 100.0 * sorted.len() as f64).ceil() - 1.0)
        .max(0.0)
        .min((sorted.len() - 1) as f64) as usize;
    sorted[index]
}

fn classify(name: &str) -> (&'static str, bool, Option<Threshold>) {
    if name.starts_with("competitor_") {
        return ("ms", false, None);
    }
    if name.ends_with("_ms") {
        return (
            "ms",
            true,
            Some(Threshold {
                max_regression_ratio: 1.15,
                min_absolute_regression: 5.0,
            }),
        );
    }
    if name.starts_with("is_")
        || name.ends_with("_ready_before_move")
        || name.ends_with("_available")
    {
        return ("boolean", false, None);
    }
    if name.contains("rss") || name.contains("heap") {
        return (
            "bytes",
            true,
            Some(Threshold {
                max_regression_ratio: 1.2,
                min_absolute_regression: 8.0 * 1024.0 * 1024.0,
            }),
        );
    }
    if name.ends_with("_bytes") {
        return ("bytes", false, None);
    }
    ("count", false, None)
}

fn aggregate(source: &str, name: &str, samples: Vec<f64>) -> Metric {
    let (unit, comparable, threshold) = classify(name);
    Metric {
        name: format!("{source}/{name}"),
        source: source.into(),
        median: percentile(&samples, 50.0),
        p75: percentile(&samples, 75.0),
        p95: percentile(&samples, 95.0),
        min: percentile(&samples, 0.0),
        max: percentile(&samples, 100.0),
        samples,
        unit: unit.into(),
        comparable,
        threshold,
    }
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let command = args.next();
    if command.as_deref() == Some("compare-json") {
        return compare_files(args, false);
    }
    if command.as_deref() == Some("compare-markdown") {
        return compare_files(args, true);
    }
    if command.as_deref() != Some("aggregate") {
        bail!("benchmark requires aggregate SOURCE METRIC SAMPLES_JSON");
    }
    let source = args.next();
    let name = args.next();
    let samples = args.next();
    let (Some(source), Some(name), Some(samples)) = (source, name, samples) else {
        bail!("benchmark requires aggregate SOURCE METRIC SAMPLES_JSON");
    };
    if args.next().is_some() {
        bail!("unexpected benchmark aggregate argument");
    }
    let samples: Vec<f64> = serde_json::from_str(&samples)?;
    println!(
        "{}",
        serde_json::to_string(&aggregate(&source, &name, samples))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_dual_pin_markdown_and_decimal_rounding_match_exactly() {
        assert_eq!(fixed(f64::INFINITY, 1), "Infinity");
        assert_eq!(fixed(f64::NEG_INFINITY, 1), "-Infinity");
        assert_eq!(fixed(f64::NAN, 1), "NaN");
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../port/hunk/oracles/benchmark-markdown.json"
        ))
        .unwrap();
        let base = serde_json::from_value(oracle["base"].clone()).unwrap();
        let head = serde_json::from_value(oracle["head"].clone()).unwrap();
        let rendered = markdown(&compare(&base, &head, "frozen".into()), "base", "head");
        assert_eq!(rendered, oracle["markdown"].as_str().unwrap());
        assert!(rendered.contains("+15% and +5.00 ms"));
        assert!(rendered.contains("+20% and +8.00 MiB"));
        for case in oracle["fixed"].as_array().unwrap() {
            assert_eq!(
                fixed(
                    case["value"].as_f64().unwrap(),
                    case["digits"].as_u64().unwrap() as u32
                ),
                case["expected"].as_str().unwrap(),
                "{case}"
            );
        }
    }

    #[test]
    fn frozen_dual_pin_comparisons_preserve_all_statuses_and_duplicate_resolution() {
        fn numeric_json(value: serde_json::Value) -> serde_json::Value {
            use serde_json::Value;
            match value {
                Value::Number(n) => serde_json::json!(n.as_f64().unwrap()),
                Value::Array(values) => {
                    Value::Array(values.into_iter().map(numeric_json).collect())
                }
                Value::Object(values) => Value::Object(
                    values
                        .into_iter()
                        .map(|(k, v)| (k, numeric_json(v)))
                        .collect(),
                ),
                other => other,
            }
        }
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../port/hunk/oracles/benchmark-comparison.json"
        ))
        .unwrap();
        for case in oracle["cases"].as_array().unwrap() {
            let base = serde_json::from_value(case["base"].clone()).unwrap();
            let head = serde_json::from_value(case["head"].clone()).unwrap();
            let actual = serde_json::to_value(compare(&base, &head, "frozen".into())).unwrap();
            assert_eq!(
                numeric_json(actual),
                numeric_json(case["expected"].clone()),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn material_regression_requires_both_thresholds_and_positive_growth() {
        let threshold = Threshold {
            max_regression_ratio: 1.15,
            min_absolute_regression: 5.0,
        };
        for (base, head, expected) in [
            (100.0, 116.0, true),
            (100.0, 104.0, false),
            (10.0, 12.0, false),
            (100.0, 90.0, false),
            (0.0, 5.0, true),
            (0.0, 4.0, false),
        ] {
            assert_eq!(material_regression(base, head, &threshold), expected);
        }
    }

    #[test]
    fn comparison_cli_reads_versioned_files_and_propagates_failure() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("base.json");
        let head = root.path().join("head.json");
        let document = |value| serde_json::json!({"version":1,"results":[aggregate("fixture","render_ms",vec![value]) ]});
        std::fs::write(&base, serde_json::to_vec(&document(100.0)).unwrap()).unwrap();
        std::fs::write(&head, serde_json::to_vec(&document(110.0)).unwrap()).unwrap();
        let args = || {
            [
                base.to_string_lossy().into_owned(),
                head.to_string_lossy().into_owned(),
            ]
            .into_iter()
        };
        compare_files(args(), false).unwrap();
        compare_files(args(), true).unwrap();
        std::fs::write(&head, serde_json::to_vec(&document(120.0)).unwrap()).unwrap();
        assert!(
            compare_files(args(), false)
                .unwrap_err()
                .to_string()
                .contains("Historical benchmark comparison failed")
        );
        std::fs::write(&head, b"{\"version\":2,\"results\":[]}").unwrap();
        assert!(
            compare_files(args(), false)
                .unwrap_err()
                .to_string()
                .contains("Invalid benchmark result file")
        );
        assert!(compare_files(std::iter::empty(), false).is_err());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn frozen_dual_pin_aggregation_matches_every_native_field() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../port/hunk/oracles/benchmark-aggregation.json"
        ))
        .unwrap();
        for expected in oracle["metrics"].as_array().unwrap() {
            let expected: Metric = serde_json::from_value(expected.clone()).unwrap();
            let name = expected.name.strip_prefix("fixture/").unwrap();
            assert_eq!(
                aggregate("fixture", name, vec![9.0, 1.0, 5.0, 3.0]),
                expected
            );
        }
        for case in oracle["percentiles"].as_array().unwrap() {
            assert_eq!(
                percentile(&[9.0, 1.0, 5.0, 3.0], case["percent"].as_f64().unwrap()),
                case["value"].as_f64().unwrap()
            );
        }
        let empty: Metric = serde_json::from_value(oracle["empty"].clone()).unwrap();
        assert_eq!(aggregate("fixture", "render_ms", vec![]), empty);
    }

    #[test]
    fn nearest_rank_keeps_input_order_and_handles_empty_and_boundary_percentiles() {
        let samples = [9.0, 1.0, 5.0, 3.0];
        for (percent, expected) in [
            (-10.0, 1.0),
            (0.0, 1.0),
            (50.0, 3.0),
            (75.0, 5.0),
            (95.0, 9.0),
            (110.0, 9.0),
        ] {
            assert_eq!(percentile(&samples, percent), expected);
        }
        assert_eq!(percentile(&[], 50.0), 0.0);
        let result = aggregate("fixture", "render_ms", samples.to_vec());
        assert_eq!(result.samples, samples);
        assert_eq!(
            (
                result.median,
                result.p75,
                result.p95,
                result.min,
                result.max
            ),
            (3.0, 5.0, 9.0, 1.0, 9.0)
        );
    }

    #[test]
    fn classification_preserves_ordered_source_rules() {
        for (name, unit, comparable) in [
            ("competitor_heap_ms", "ms", false),
            ("is_ready_ms", "ms", true),
            ("is_ready", "boolean", false),
            ("scroll_ready_before_move", "boolean", false),
            ("git_available", "boolean", false),
            ("peak_rss", "bytes", true),
            ("heap_used", "bytes", true),
            ("patch_bytes", "bytes", false),
            ("rows", "count", false),
        ] {
            let metric = aggregate("fixture", name, vec![]);
            assert_eq!(metric.unit, unit, "{name}");
            assert_eq!(metric.comparable, comparable, "{name}");
            assert_eq!(metric.threshold.is_some(), comparable, "{name}");
            assert_eq!(metric.min, 0.0);
            assert_eq!(metric.max, 0.0);
        }
        assert_eq!(classify("render_ms").2.unwrap().max_regression_ratio, 1.15);
        assert_eq!(
            classify("rss_bytes").2.unwrap().min_absolute_regression,
            8_388_608.0
        );
    }

    #[test]
    fn aggregate_cli_rejects_missing_extra_and_malformed_samples() {
        for args in [
            vec![],
            vec!["aggregate"],
            vec!["aggregate", "source", "metric", "null"],
            vec!["aggregate", "source", "metric", "[]", "extra"],
        ] {
            assert!(run(args.into_iter().map(str::to_owned)).is_err());
        }
        run(["aggregate", "source", "render_ms", "[9,1,5,3]"]
            .into_iter()
            .map(str::to_owned))
        .unwrap();
    }
}
