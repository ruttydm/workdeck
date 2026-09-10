//! Generated JSON layout translated from MIT-licensed Hunk generate-changelog.ts.
//! Copyright (c) Modem Labs Inc. See THIRD_PARTY_NOTICES.
use serde_json::Value;

pub(super) fn format(value: &Value) -> String {
    render(value, 0, 0, false)
}

fn render(value: &Value, indent: usize, column: usize, comma: bool) -> String {
    let pad = " ".repeat(indent);
    let inner = " ".repeat(indent + 2);
    match value {
        Value::Array(items) => {
            if items.is_empty() {
                return "[]".into();
            }
            let parts: Vec<_> = items
                .iter()
                .enumerate()
                .map(|(i, item)| render(item, indent + 2, indent + 2, i + 1 != items.len()))
                .collect();
            if items.iter().all(|v| !v.is_array() && !v.is_object()) {
                let inline = format!("[{}]", parts.join(", "));
                if column + inline.encode_utf16().count() + usize::from(comma) <= 100 {
                    return inline;
                }
            }
            format!(
                "[\n{}\n{pad}]",
                parts
                    .iter()
                    .map(|p| format!("{inner}{p}"))
                    .collect::<Vec<_>>()
                    .join(",\n")
            )
        }
        Value::Object(items) => {
            if items.is_empty() {
                return "{}".into();
            }
            let mut entries: Vec<_> = items.iter().collect();
            // ECMAScript enumerates canonical array-index keys before other keys.
            fn index(key: &str) -> Option<u32> {
                let n: u32 = key.parse().ok()?;
                (n != u32::MAX && n.to_string() == key).then_some(n)
            }
            entries.sort_by_key(|(k, _)| index(k).map_or((1, 0), |n| (0, n)));
            let body = entries
                .iter()
                .enumerate()
                .map(|(i, (key, value))| {
                    let label = format!("{}: ", serde_json::to_string(key).unwrap());
                    let printed = render(
                        value,
                        indent + 2,
                        indent + 2 + label.encode_utf16().count(),
                        i + 1 != entries.len(),
                    );
                    format!("{inner}{label}{printed}")
                })
                .collect::<Vec<_>>()
                .join(",\n");
            format!("{{\n{body}\n{pad}}}")
        }
        Value::Number(n) => {
            let number = n.as_f64().unwrap();
            if number.is_finite() {
                ryu_js::Buffer::new().format_finite(number).into()
            } else {
                "null".into()
            }
        }
        _ => serde_json::to_string(value).unwrap(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_formatter_keeps_short_primitive_array_inline() {
        assert_eq!(
            format(&serde_json::json!({"chips":["0.18.2","0.18.1"]})),
            "{\n  \"chips\": [\"0.18.2\", \"0.18.1\"]\n}"
        );
    }

    #[test]
    fn source_formatter_expands_arrays_past_print_width() {
        let long: Vec<_> = (0..14).map(|i| format!("0.17.{i}")).collect();
        let printed = format(&serde_json::json!({"chips":long}));
        assert!(printed.contains("\"chips\": [\n"));
        for line in printed.split('\n') {
            assert!(line.encode_utf16().count() <= 100);
        }
    }

    #[test]
    fn source_formatter_counts_key_toward_array_width() {
        let chips: Vec<_> = (0..9).map(|i| format!("0.20.{i}")).collect();
        let printed = format(&serde_json::json!([{"slug":"0.20","chips":chips,"alt":"x"}]));
        assert!(printed.contains("\"chips\": [\n"));
        for line in printed.split('\n') {
            assert!(line.encode_utf16().count() <= 100);
        }
    }

    #[test]
    fn source_formatter_final_key_has_no_comma_budget() {
        let chips: Vec<_> = (0..8).map(|i| format!("0.20.{i}")).collect();
        assert!(format(&serde_json::json!([{"chips":chips}])).contains("\"chips\": [\"0.20.0\""));
    }

    #[test]
    fn source_formatter_expands_object_arrays() {
        assert_eq!(
            format(&serde_json::json!([{"a":1}])),
            "[\n  {\n    \"a\": 1\n  }\n]"
        );
    }

    #[test]
    fn source_formatter_empty_containers_stay_compact() {
        assert_eq!(
            format(&serde_json::json!({"a":[],"b":{}})),
            "{\n  \"a\": [],\n  \"b\": {}\n}"
        );
    }

    #[test]
    fn generated_json_matches_dual_pin_formatter_oracles() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../port/hunk/website-changelog-json-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for result in results {
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 8);
            for case in cases {
                assert_eq!(format(&case["input"]), case["expected"]);
            }
        }
    }
}
