//! JSON is normalized only for the existing TOML mapper's value interface.
//! Persistent provenance always names the exact retained JSON, never this
//! transient transport. Semantic field/status conversion lives in convert.rs.
use super::{
    MigrationKind, PreviewOptions,
    convert::{self, Converted, JsonOrigin},
    scan::Input,
};
use crate::{ContentHash, ErrorCode, PmError, Result};
use serde_json::{Value, json};
use std::path::Path;

const TAG: &str = "x-workdeck-json-value";
const ORIGIN: &str = "x-workdeck-json-origin";

pub(crate) fn convert_export_row(
    kind: MigrationKind,
    row: &Value,
    selector: &str,
    source: &Path,
    source_hash: &ContentHash,
    options: &PreviewOptions,
) -> Result<Converted> {
    let object = row
        .as_object()
        .ok_or_else(|| invalid("legacy export records must be objects"))?;
    let key = if kind == MigrationKind::Issue {
        "key"
    } else {
        "id"
    };
    let id = object.get(key).and_then(Value::as_str).unwrap_or("");
    let (logical, bytes) = if kind == MigrationKind::ImportedEvents {
        (
            "events.jsonl".into(),
            format!(
                "{}\n",
                serde_json::to_string(row).map_err(|error| invalid(error.to_string()))?
            )
            .into_bytes(),
        )
    } else {
        let mut table = normalized(row)?
            .as_table()
            .cloned()
            .expect("object normalized as table");
        let logical = match kind {
            MigrationKind::Issue => format!("issues/{id}.toml"),
            MigrationKind::Project | MigrationKind::Cycle | MigrationKind::Labels => {
                let collection = match kind {
                    MigrationKind::Project => "projects",
                    MigrationKind::Cycle => "cycles",
                    _ => "labels",
                };
                table = toml::Table::from_iter([(
                    collection.into(),
                    toml::Value::Array(vec![toml::Value::Table(table)]),
                )]);
                format!("{collection}.toml")
            }
            MigrationKind::ImportedSession => {
                if table.contains_key(ORIGIN) {
                    return Err(invalid(
                        "legacy session contains reserved JSON origin metadata",
                    ));
                }
                table.insert(ORIGIN.into(), normalized(&json!({"format":"workdeck_json_v0","source_path":source,"source_content":source_hash,"record_selector":selector,"extra_encoding":"tagged-json-v1"}))?);
                format!("agents/{id}.toml")
            }
            _ => return Err(invalid("unsupported legacy export record kind")),
        };
        (
            logical,
            toml::to_string(&table)
                .map_err(|error| invalid(error.to_string()))?
                .into_bytes(),
        )
    };
    let input = Input {
        path: logical.into(),
        kind,
        hash: Some(source_hash.clone()),
        size: Some(bytes.len() as u64),
        bytes: Some(bytes),
    };
    let origin = JsonOrigin {
        record: row,
        selector,
    };
    let mut output =
        convert::convert_with_origin(&input, options, source, Some(&origin)).map_err(|error| {
            error
                .at(source)
                .details(json!({"record_selector":selector}))
        })?;
    for draft in &mut output.drafts {
        draft.source_path = Some(source.to_owned());
    }
    for notice in &mut output.notices {
        notice.path = source.to_owned();
        notice.message = format!("{selector}: {}", notice.message);
    }
    for error in &mut output.errors {
        error.path = Some(source.to_string_lossy().into_owned());
    }
    Ok(output)
}

/// All representable values retain their normal TOML type. JSON null and
/// unsigned integers above i64::MAX use an explicit reserved tagged object:
/// {x-workdeck-json-value={type="null"}} or type="unsigned_integer", value="N".
/// This applies recursively, including nested session extras. Source objects
/// cannot use the reserved tag, avoiding ambiguity when recovering JSON values.
fn normalized(value: &Value) -> Result<toml::Value> {
    Ok(match value {
        Value::Null => tag("null", None),
        Value::Bool(value) => toml::Value::Boolean(*value),
        Value::String(value) => toml::Value::String(value.clone()),
        Value::Number(value) if value.is_i64() => {
            toml::Value::Integer(value.as_i64().expect("signed integer"))
        }
        Value::Number(value) if value.is_u64() => tag("unsigned_integer", Some(value.to_string())),
        Value::Number(value) => toml::Value::Float(
            value
                .as_f64()
                .ok_or_else(|| invalid("unsupported JSON number"))?,
        ),
        Value::Array(values) => {
            toml::Value::Array(values.iter().map(normalized).collect::<Result<_>>()?)
        }
        Value::Object(values) => {
            if values.contains_key(TAG) {
                return Err(invalid(
                    "legacy JSON contains reserved tagged-value metadata",
                ));
            }
            toml::Value::Table(
                values
                    .iter()
                    .map(|(key, value)| Ok((key.clone(), normalized(value)?)))
                    .collect::<Result<_>>()?,
            )
        }
    })
}
fn tag(kind: &str, value: Option<String>) -> toml::Value {
    let mut fields = toml::Table::from_iter([("type".into(), toml::Value::String(kind.into()))]);
    if let Some(value) = value {
        fields.insert("value".into(), toml::Value::String(value));
    }
    toml::Value::Table(toml::Table::from_iter([(
        TAG.into(),
        toml::Value::Table(fields),
    )]))
}
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn decode(value: &toml::Value) -> Value {
        if let Some(tag) = value
            .as_table()
            .and_then(|v| v.get(TAG))
            .and_then(toml::Value::as_table)
        {
            return match tag["type"].as_str().unwrap() {
                "null" => Value::Null,
                "unsigned_integer" => json!(tag["value"].as_str().unwrap().parse::<u64>().unwrap()),
                _ => panic!("unknown tag"),
            };
        }
        match value {
            toml::Value::Array(values) => Value::Array(values.iter().map(decode).collect()),
            toml::Value::Table(values) => Value::Object(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), decode(value)))
                    .collect(),
            ),
            _ => serde_json::to_value(value).unwrap(),
        }
    }
    #[test]
    fn tagged_json_null_and_unsigned_values_roundtrip_without_stringification() {
        let source =
            json!({"plain":false,"values":[null,u64::MAX,{"nested":null}],"signed":i64::MIN});
        let normalized = normalized(&source).unwrap();
        let bytes = toml::to_string(normalized.as_table().unwrap()).unwrap();
        let parsed: toml::Value = toml::from_str(&bytes).unwrap();
        assert_eq!(decode(&parsed), source);
        assert!(super::normalized(&json!({TAG:{"type":"null"}})).is_err());
    }
}
