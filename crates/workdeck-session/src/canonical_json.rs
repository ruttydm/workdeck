//! RFC 8785 canonical JSON for broker authentication transcripts.

use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CanonicalJsonError {
    #[error("canonical JSON supports JSON values only: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub fn canonicalize_json(value: &Value) -> Result<String, CanonicalJsonError> {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            Ok(serde_json::to_string(value)?)
        }
        Value::Array(values) => {
            let entries = values
                .iter()
                .map(canonicalize_json)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("[{}]", entries.join(",")))
        }
        Value::Object(record) => {
            let mut keys = record.keys().collect::<Vec<_>>();
            keys.sort_by(|left, right| {
                left.encode_utf16()
                    .collect::<Vec<_>>()
                    .cmp(&right.encode_utf16().collect::<Vec<_>>())
            });
            let entries = keys
                .into_iter()
                .map(|key| {
                    Ok(format!(
                        "{}:{}",
                        serde_json::to_string(key)?,
                        canonicalize_json(&record[key])?
                    ))
                })
                .collect::<Result<Vec<_>, CanonicalJsonError>>()?;
            Ok(format!("{{{}}}", entries.join(",")))
        }
    }
}

pub fn canonical_json_bytes(value: &Value) -> Result<Vec<u8>, CanonicalJsonError> {
    Ok(canonicalize_json(value)?.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_rfc_primitives_and_utf16_property_ordering() {
        let value = json!({
            "numbers": [333333333.3333333_f64, 1e30_f64, 4.5_f64, 0.002_f64, 1e-27_f64],
            "string": "€$\u{000f}\nA'B\"\\\"/",
            "literals": [null, true, false]
        });
        assert_eq!(
            canonicalize_json(&value).unwrap(),
            "{\"literals\":[null,true,false],\"numbers\":[333333333.3333333,1e+30,4.5,0.002,1e-27],\"string\":\"€$\\u000f\\nA'B\\\"\\\\\\\"/\"}"
        );
        assert_eq!(
            canonicalize_json(&json!({ "€": "Euro", "\r": "CR", "1": "one" })).unwrap(),
            "{\"\\r\":\"CR\",\"1\":\"one\",\"€\":\"Euro\"}"
        );
    }

    #[test]
    fn orders_non_bmp_keys_by_utf16_code_units() {
        let value = json!({ "\u{e000}": "bmp", "😀": "astral" });
        assert_eq!(
            canonicalize_json(&value).unwrap(),
            "{\"😀\":\"astral\",\"\":\"bmp\"}"
        );
        assert_eq!(canonical_json_bytes(&json!([true])).unwrap(), b"[true]");
    }
}
