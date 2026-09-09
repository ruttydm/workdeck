//! Partial MIT port of Hunk website/src/data/extensions.ts (Modem Labs Inc.).
use anyhow::{Result, bail};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::io::Read;

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
    if !matches!(command.as_deref(), Some("activity-index" | "json-ld")) || args.next().is_some() {
        bail!("extension-catalog requires activity-index or json-ld (JSON on stdin)");
    }
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let payload: Value = serde_json::from_str(&input)?;
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

fn json_ld_script_body(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?.replace('<', "\\u003c"))
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
