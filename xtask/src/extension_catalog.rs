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
    if args.next().as_deref() != Some("activity-index") || args.next().is_some() {
        bail!("extension-catalog requires activity-index (JSON on stdin)");
    }
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let payload: Value = serde_json::from_str(&input)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&index_activity(&payload))?
    );
    Ok(())
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
