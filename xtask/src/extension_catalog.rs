//! Partial MIT port of Hunk website/src/data/extensions.ts (Modem Labs Inc.).
use anyhow::{Result, bail};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::io::Read;

mod loading;

pub fn validate_legacy_catalog(catalog: &Value) -> Result<()> {
    anyhow::ensure!(
        catalog["baseline"] == "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
        "unexpected catalog baseline"
    );
    anyhow::ensure!(
        catalog["blob"] == "23f2196dd3c7a91306e050db352d4b5518a31117",
        "unexpected catalog source blob"
    );
    let entries = catalog["entries"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("catalog entries missing"))?;
    anyhow::ensure!(
        entries.len() == 16,
        "pinned catalog must preserve all 16 listings"
    );
    let mut repositories = std::collections::BTreeSet::new();
    let categorized: Vec<CategorizedListing> = serde_json::from_value(catalog["entries"].clone())?;
    anyhow::ensure!(
        catalog["facets"] == serde_json::to_value(category_facets(&categorized))?,
        "catalog facet counts or order differ from entries"
    );
    let version = regex::Regex::new(r"^\d+\.\d+\.\d+")?;
    for entry in entries {
        let repo = entry["repo"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("repository missing"))?;
        anyhow::ensure!(
            repositories.insert(repo.to_lowercase()),
            "duplicate catalog repository"
        );
        let parts: Vec<_> = repo.split('/').collect();
        anyhow::ensure!(
            parts.len() == 2
                && parts.iter().all(|part| !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))),
            "invalid catalog repository"
        );
        for field in ["name", "summary"] {
            anyhow::ensure!(
                entry[field]
                    .as_str()
                    .is_some_and(|value| !value.trim().is_empty()),
                "catalog {field} missing"
            );
        }
        anyhow::ensure!(
            entry["version"]
                .as_str()
                .is_some_and(|value| version.is_match(value)),
            "invalid recorded version"
        );
        anyhow::ensure!(
            entry["apiVersion"].as_u64().is_some_and(|value| value > 0),
            "recorded source API missing"
        );
        anyhow::ensure!(
            entry["compatibility"] == "requires-rust-rewrite",
            "legacy extension cannot be advertised as a native replacement"
        );
        let categories: Vec<Category> = serde_json::from_value(entry["categories"].clone())?;
        anyhow::ensure!(!categories.is_empty(), "catalog categories missing");
    }
    Ok(())
}

// Decode only the pinned declarative catalog literal, never execute source.
// Reject syntax outside JSON strings, integers, punctuation and known keys.
pub fn verify_legacy_source(catalog: &Value, source: &str) -> Result<()> {
    validate_legacy_catalog(catalog)?;
    let literal = source
        .split_once("export const EXTENSION_CATALOG: readonly ExtensionListing[] = [")
        .and_then(|(_, tail)| {
            tail.split_once("\n];")
                .map(|(body, _)| format!("[{body}\n]"))
        })
        .ok_or_else(|| anyhow::anyhow!("pinned catalog literal not found"))?;
    let token_pattern =
        regex::Regex::new(r#""(?:[^"\\]|\\.)*"|[A-Za-z_][A-Za-z_0-9]*|[0-9]+|[\[\]{},:]"#)?;
    let tokens: Vec<_> = token_pattern.find_iter(&literal).collect();
    let mut end = 0;
    let mut json = String::new();
    for (index, token) in tokens.iter().enumerate() {
        anyhow::ensure!(
            literal[end..token.start()].trim().is_empty(),
            "unsupported catalog literal syntax"
        );
        end = token.end();
        let text = token.as_str();
        if text == ","
            && tokens
                .get(index + 1)
                .is_some_and(|next| matches!(next.as_str(), "]" | "}"))
        {
            continue;
        }
        if text.as_bytes()[0].is_ascii_alphabetic() || text.starts_with('_') {
            anyhow::ensure!(
                matches!(
                    text,
                    "repo" | "name" | "summary" | "categories" | "version" | "apiVersion"
                ),
                "unsupported catalog identifier"
            );
            json.push_str(&serde_json::to_string(text)?);
        } else {
            json.push_str(text);
        }
    }
    anyhow::ensure!(
        literal[end..].trim().is_empty(),
        "unsupported trailing catalog syntax"
    );
    let expected: Value = serde_json::from_str(&json)?;
    let mut actual = catalog["entries"].clone();
    for entry in actual.as_array_mut().unwrap() {
        entry.as_object_mut().unwrap().remove("compatibility");
    }
    anyhow::ensure!(
        actual == expected,
        "migrated catalog differs from pinned source fields"
    );
    Ok(())
}

#[test]
fn legacy_source_comparison_rejects_field_drift_and_executable_syntax() {
    let catalog: Value =
        serde_json::from_str(include_str!("../../site/data/legacy-extensions.json")).unwrap();
    let mut entries = catalog["entries"].clone();
    for entry in entries.as_array_mut().unwrap() {
        entry.as_object_mut().unwrap().remove("compatibility");
    }
    let source = format!(
        "export const EXTENSION_CATALOG: readonly ExtensionListing[] = {};",
        serde_json::to_string_pretty(&entries).unwrap()
    );
    verify_legacy_source(&catalog, &source).unwrap();
    let mut changed = catalog.clone();
    changed["entries"][0]["summary"] =
        serde_json::json!("Different but structurally valid summary");
    assert!(validate_legacy_catalog(&changed).is_ok());
    assert!(verify_legacy_source(&changed, &source).is_err());
    let injected = source.replacen("= [", "= [compute(),", 1);
    assert!(verify_legacy_source(&catalog, &injected).is_err());
    assert!(verify_legacy_source(&catalog, "").is_err());
}

#[test]
fn legacy_catalog_rejects_missing_duplicate_or_misrepresented_listings() {
    let catalog: Value =
        serde_json::from_str(include_str!("../../site/data/legacy-extensions.json")).unwrap();
    validate_legacy_catalog(&catalog).unwrap();
    for (field, value) in [
        ("repo", catalog["entries"][1]["repo"].clone()),
        ("repo", serde_json::json!("owner/repo\" onclick=bad")),
        ("name", serde_json::json!("")),
        ("summary", serde_json::json!(" ")),
        ("version", serde_json::json!("unknown")),
        ("apiVersion", serde_json::json!(null)),
        ("categories", serde_json::json!([])),
        ("categories", serde_json::json!(["Unknown"])),
        ("compatibility", serde_json::json!("native")),
    ] {
        let mut changed = catalog.clone();
        changed["entries"][0][field] = value;
        assert!(
            validate_legacy_catalog(&changed).is_err(),
            "accepted changed {field}"
        );
    }
    let mut wrong_count = catalog.clone();
    wrong_count["facets"][0]["count"] = serde_json::json!(999);
    assert!(validate_legacy_catalog(&wrong_count).is_err());
    let mut wrong_order = catalog.clone();
    wrong_order["facets"].as_array_mut().unwrap().swap(0, 1);
    assert!(validate_legacy_catalog(&wrong_order).is_err());
    let mut missing_facet = catalog.clone();
    missing_facet["facets"].as_array_mut().unwrap().pop();
    assert!(validate_legacy_catalog(&missing_facet).is_err());
    let mut shortened = catalog;
    shortened["entries"].as_array_mut().unwrap().pop();
    assert!(validate_legacy_catalog(&shortened).is_err());
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize,
)]
enum Category {
    #[serde(rename = "Changeset transform")]
    ChangesetTransform,
    Command,
    #[serde(rename = "File view")]
    FileView,
    #[serde(rename = "Keyboard mode")]
    KeyboardMode,
    #[serde(rename = "Line highlighter")]
    LineHighlighter,
    Pane,
    Theme,
    #[serde(rename = "VCS backend")]
    VcsBackend,
}

#[derive(serde::Deserialize)]
struct CategorizedListing {
    categories: Vec<Category>,
}

#[derive(serde::Serialize)]
struct CategoryFacet {
    category: Category,
    count: usize,
}

fn category_facets(entries: &[CategorizedListing]) -> Vec<CategoryFacet> {
    let mut counts = BTreeMap::new();
    for entry in entries {
        for &category in &entry.categories {
            *counts.entry(category).or_insert(0) += 1;
        }
    }
    let mut facets: Vec<_> = counts
        .into_iter()
        .map(|(category, count)| CategoryFacet { category, count })
        .collect();
    // The closed source category vocabulary has the same alphabetical order
    // under localeCompare and this enum; do not extend it without an oracle.
    facets.sort_by_key(|facet| (std::cmp::Reverse(facet.count), facet.category));
    facets
}

fn activity(value: &Value) -> Value {
    let mut result = Map::new();
    for (source, destination, numeric) in [
        ("stargazers_count", "stars", true),
        ("pushed_at", "pushedAt", false),
        ("created_at", "createdAt", false),
    ] {
        if let Some(value) = value.get(source)
            && if numeric {
                value.is_number()
            } else {
                value.is_string()
            }
        {
            result.insert(destination.into(), value.clone());
        }
    }
    Value::Object(result)
}

fn index_activity(payload: &Value) -> BTreeMap<String, Value> {
    let mut result = BTreeMap::new();
    if let Some(items) = payload.get("items").and_then(Value::as_array) {
        for item in items {
            if let Some(name) = item.get("full_name").and_then(Value::as_str) {
                result.insert(name.to_lowercase(), activity(item));
            }
        }
    }
    result
}

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let command = args.next();
    if !matches!(
        command.as_deref(),
        Some("activity-index" | "json-ld" | "format-updated" | "category-facets" | "load")
    ) || args.next().is_some()
    {
        bail!(
            "extension-catalog requires activity-index, json-ld, format-updated, category-facets or load (JSON on stdin)"
        );
    }
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let payload: Value = serde_json::from_str(&input)?;
    if command.as_deref() == Some("load") {
        let entries = loading::load(&payload)?;
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }
    if command.as_deref() == Some("category-facets") {
        let entries: Vec<CategorizedListing> = serde_json::from_value(payload)?;
        println!("{}", serde_json::to_string(&category_facets(&entries))?);
        return Ok(());
    }
    if command.as_deref() == Some("format-updated") {
        let pushed = payload
            .get("pushedAt")
            .and_then(Value::as_str)
            .and_then(parse_catalog_timestamp);
        let now = match payload.get("now") {
            None => Some(chrono::Utc::now().timestamp_millis()),
            Some(value) => value.as_str().and_then(parse_catalog_timestamp),
        };
        let formatted = pushed
            .zip(now)
            .and_then(|(pushed, now)| format_updated_millis(pushed, now));
        println!("{}", serde_json::to_string(&formatted)?);
        return Ok(());
    }
    if command.as_deref() == Some("json-ld") {
        println!("{}", json_ld_script_body(&payload)?);
        return Ok(());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&index_activity(&payload))?
    );
    Ok(())
}

// Date-only ISO inputs are UTC in the source runtime. Days through 31 roll
// into the following month; larger days and invalid months are rejected.
// This is deliberately not a claim to implement all legacy Date spellings.
fn parse_catalog_timestamp(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        let year = value[..4].parse().ok()?;
        let month = value[5..7].parse().ok()?;
        let day: u64 = value[8..].parse().ok()?;
        if !(1..=31).contains(&day) {
            return None;
        }
        return chrono::NaiveDate::from_ymd_opt(year, month, 1)?
            .checked_add_days(chrono::Days::new(day - 1))?
            .and_hms_opt(0, 0, 0)
            .map(|date| date.and_utc().timestamp_millis());
    }
    let date = chrono::DateTime::parse_from_rfc3339(value).ok()?;
    // Chrono accepts leap seconds, whereas JavaScript Date rejects them.
    (date.timestamp_subsec_nanos() < 1_000_000_000).then(|| date.timestamp_millis())
}

fn format_updated_millis(pushed: i64, now: i64) -> Option<String> {
    let elapsed = i128::from(now) - i128::from(pushed);
    if elapsed < 0 {
        return None;
    }
    let days = elapsed / 86_400_000;
    Some(match days {
        0 => "today".into(),
        1 => "yesterday".into(),
        2..30 => format!("{days} days ago"),
        _ => {
            let months = days / 30;
            if months < 12 {
                format!("{months} month{} ago", if months == 1 { "" } else { "s" })
            } else {
                let years = days / 365;
                format!("{years} year{} ago", if years == 1 { "" } else { "s" })
            }
        }
    })
}

#[test]
fn recency_thresholds_preserve_source_day_month_year_boundaries() {
    for (days, expected) in [
        (0, "today"),
        (1, "yesterday"),
        (14, "14 days ago"),
        (29, "29 days ago"),
        (30, "1 month ago"),
        (59, "1 month ago"),
        (60, "2 months ago"),
        (359, "11 months ago"),
        (360, "0 years ago"),
        (364, "0 years ago"),
        (365, "1 year ago"),
        (730, "2 years ago"),
    ] {
        assert_eq!(
            format_updated_millis(0, days * 86_400_000).as_deref(),
            Some(expected)
        );
    }
    assert_eq!(format_updated_millis(1, 0), None);
    assert_eq!(
        format_updated_millis(0, 86_399_999).as_deref(),
        Some("today")
    );
    assert!(format_updated_millis(i64::MIN, i64::MAX).is_some());
}

fn json_ld_script_body(value: &Value) -> Result<String> {
    let mut output = String::new();
    write_javascript_json(&javascript_property_order(value), &mut output)?;
    Ok(output.replace('<', "\\u003c"))
}

fn write_javascript_json(value: &Value, output: &mut String) -> Result<()> {
    match value {
        Value::Number(number) => {
            let number = number.as_f64().ok_or_else(|| {
                anyhow::anyhow!("JSON number is not representable as a JavaScript number")
            })?;
            if number.is_finite() {
                output.push_str(ryu_js::Buffer::new().format_finite(number));
            } else {
                output.push_str("null");
            }
        }
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_javascript_json(value, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            for (index, (key, value)) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                output.push_str(&serde_json::to_string(key)?);
                output.push(':');
                write_javascript_json(value, output)?;
            }
            output.push('}');
        }
        _ => output.push_str(&serde_json::to_string(value)?),
    }
    Ok(())
}

fn array_index(key: &str) -> Option<u32> {
    let index = key.parse::<u32>().ok()?;
    (index != u32::MAX && index.to_string() == key).then_some(index)
}

fn javascript_property_order(value: &Value) -> Value {
    match value {
        Value::Array(values) => {
            Value::Array(values.iter().map(javascript_property_order).collect())
        }
        Value::Object(values) => {
            let mut indexed = values
                .iter()
                .filter_map(|(key, value)| array_index(key).map(|index| (index, key, value)))
                .collect::<Vec<_>>();
            indexed.sort_by_key(|(index, _, _)| *index);
            let mut ordered = Map::new();
            for (_, key, value) in indexed {
                ordered.insert(key.clone(), javascript_property_order(value));
            }
            for (key, value) in values {
                if array_index(key).is_none() {
                    ordered.insert(key.clone(), javascript_property_order(value));
                }
            }
            Value::Object(ordered)
        }
        _ => value.clone(),
    }
}

#[test]
fn json_ld_orders_array_indices_before_insertion_ordered_keys_recursively() {
    let value: Value = serde_json::from_str(r#"{"z":{"10":1,"2":2},"4294967295":3,"01":4,"4294967294":5,"0":6,"-0":7,"a":[{"3":8,"1":9}]}"#).unwrap();
    assert_eq!(
        json_ld_script_body(&value).unwrap(),
        r#"{"0":6,"4294967294":5,"z":{"2":2,"10":1},"4294967295":3,"01":4,"-0":7,"a":[{"1":9,"3":8}]}"#
    );
    let fixture: Value = serde_json::from_str(include_str!(
        "../../port/hunk/oracles/json-ld-serialization-gaps.json"
    ))
    .unwrap();
    for capture in fixture["sourceCaptures"].as_array().unwrap() {
        for case in capture["cases"].as_array().unwrap() {
            let input: Value = serde_json::from_str(case["input"].as_str().unwrap()).unwrap();
            assert_eq!(json_ld_script_body(&input).unwrap(), case["expected"]);
        }
    }
}

#[test]
fn json_ld_neutralizes_markup_without_changing_decoded_values() {
    for value in [
        serde_json::json!({"name":"</script><img src=x onerror=alert(1)>"}),
        serde_json::json!({"<key>":["<!--", "<SCRIPT>", "\\u003c", "雪", null]}),
        Value::Null,
    ] {
        let body = json_ld_script_body(&value).unwrap();
        assert!(!body.contains('<'));
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), value);
    }
}

#[test]
fn activity_index_matches_both_pinned_source_captures() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../port/hunk/oracles/extension-activity-index.json"
    ))
    .unwrap();
    let captures = fixture["captures"].as_array().unwrap();
    assert_eq!(captures.len(), 2);
    for capture in captures {
        assert_eq!(capture["exitCode"], 0);
        let cases = capture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 7);
        for case in cases {
            assert_eq!(
                serde_json::to_value(index_activity(&case["input"])).unwrap(),
                case["expected"],
                "{}: {}",
                capture["kind"],
                case["input"]
            );
        }
    }
}

#[test]
fn activity_index_preserves_missing_fields_and_last_case_insensitive_entry() {
    let result = index_activity(&serde_json::json!({"items":[
        {"full_name":"Owner/Repo", "stargazers_count":12, "pushed_at":"today", "created_at":"earlier"},
        {"full_name":"someone/other", "stargazers_count":4},
        {"stargazers_count":9}, null, 7,
        {"full_name":"OWNER/REPO", "stargazers_count":"wrong", "pushed_at":false, "created_at":"replacement"}
    ]}));
    assert_eq!(result.len(), 2);
    assert_eq!(
        result["owner/repo"],
        serde_json::json!({"createdAt":"replacement"})
    );
    assert_eq!(result["someone/other"], serde_json::json!({"stars":4}));
    assert_eq!(
        activity(&serde_json::json!({"stargazers_count":-1.5})),
        serde_json::json!({"stars":-1.5})
    );
    for payload in [
        Value::Null,
        serde_json::json!({}),
        serde_json::json!({"items":"nope"}),
        serde_json::json!({"items":[null,7]}),
    ] {
        assert!(index_activity(&payload).is_empty());
    }
    assert!(run(["unexpected".into()].into_iter()).is_err());
}
