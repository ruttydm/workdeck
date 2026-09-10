//! Native translation of the collapse-generated example in Hunk's
//! website/src/content/docs/docs/extend/extensions.md (2c00f435, MIT).

use regex::Regex;
use std::io::{self, BufRead, Write};
use workdeck_extension_api::{
    API_VERSION, ExtensionNotifyType, HandshakeRequest, HandshakeResponse, JsonRpcError,
    JsonRpcRequest, JsonRpcResponse, Registration, TransformNotification, TransformRequest,
    TransformResponse,
};

pub struct CollapseGenerated {
    patterns: Vec<Regex>,
}

impl CollapseGenerated {
    pub fn from_config(config: &serde_json::Value) -> Result<Self, String> {
        let patterns: Vec<String> = match config.get("patterns") {
            None | Some(serde_json::Value::Null) => ["*.lock", "*-lock.json", "dist/*"]
                .map(String::from)
                .to_vec(),
            Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string())?,
        };
        let patterns = patterns
            .into_iter()
            .map(|pattern| {
                // JavaScript's dot excludes all four ECMAScript line terminators.
                let source = pattern
                    .split('*')
                    .map(regex::escape)
                    .collect::<Vec<_>>()
                    .join("[^\\r\\n\\u{2028}\\u{2029}]*");
                Regex::new(&format!("\\A(?:{source})\\z")).map_err(|e| e.to_string())
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { patterns })
    }

    pub fn transform(&self, request: TransformRequest) -> TransformResponse {
        let mut changeset = request.changeset;
        let before = changeset.files.len();
        changeset
            .files
            .retain(|file| !self.patterns.iter().any(|p| p.is_match(&file.path)));
        let hidden = before - changeset.files.len();
        let notifications = if hidden == 0 {
            Vec::new()
        } else {
            vec![TransformNotification {
                message: format!(
                    "Collapsed {hidden} generated {}",
                    if hidden == 1 { "file" } else { "files" }
                ),
                notification_type: ExtensionNotifyType::Info,
            }]
        };
        TransformResponse {
            changeset,
            notifications,
        }
    }
}

pub fn serve(input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let mut extension = None;
    for line in input.lines() {
        let request: JsonRpcRequest = serde_json::from_str(&line?).map_err(io::Error::other)?;
        let result = (|| -> Result<serde_json::Value, (i32, String)> {
            match request.method.as_str() {
                "workdeck/handshake" => {
                    let handshake: HandshakeRequest = serde_json::from_value(request.params)
                        .map_err(|e| (-32602, e.to_string()))?;
                    let configured = CollapseGenerated::from_config(&handshake.config)
                        .map_err(|e| (-32602, e))?;
                    extension = Some(configured);
                    serde_json::to_value(HandshakeResponse {
                        extension_api_version: API_VERSION,
                        extension_version: env!("CARGO_PKG_VERSION").into(),
                        registrations: vec![Registration::ChangesetTransform {
                            id: "collapse-generated".into(),
                        }],
                    })
                    .map_err(|e| (-32603, e.to_string()))
                }
                "workdeck/changeset/transform" => {
                    let extension = extension
                        .as_ref()
                        .ok_or((-32000, "Handshake required".into()))?;
                    let transform: TransformRequest = serde_json::from_value(request.params)
                        .map_err(|e| (-32602, e.to_string()))?;
                    if transform.transform_id != "collapse-generated" {
                        return Err((-32602, "Unknown transform".into()));
                    }
                    let mut transformed = extension.transform(transform);
                    for notification in transformed.notifications.drain(..) {
                        serde_json::to_writer(&mut output, &serde_json::json!({
                            "jsonrpc": "2.0", "method": "workdeck/notify", "params": notification
                        })).map_err(|e| (-32603, e.to_string()))?;
                        output
                            .write_all(b"\n")
                            .and_then(|()| output.flush())
                            .map_err(|e| (-32603, e.to_string()))?;
                    }
                    serde_json::to_value(transformed).map_err(|e| (-32603, e.to_string()))
                }
                _ => Err((-32601, "Unknown method".into())),
            }
        })();
        let (result, error) = match result {
            Ok(value) => (Some(value), None),
            Err((code, message)) => (
                None,
                Some(JsonRpcError {
                    code,
                    message,
                    data: None,
                }),
            ),
        };
        serde_json::to_writer(
            &mut output,
            &JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: request.id,
                result,
                error,
            },
        )
        .map_err(io::Error::other)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> TransformRequest {
        let patch = ["src/keep.rs", "Cargo.lock", "dist/out", "src/last.rs"]
            .map(|path| format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-old\n+new\n"))
            .join("");
        let changeset = workdeck_diff::parse_patch(
            &patch,
            "fixture",
            "Generated files",
            workdeck_core::ChangesetSource::Patch {
                label: "fixture".into(),
            },
        )
        .unwrap();
        TransformRequest {
            transform_id: "collapse-generated".into(),
            changeset: workdeck_extension_host::project_extension_changeset(&changeset),
            cwd: "/review".into(),
        }
    }

    #[test]
    fn filtering_preserves_metadata_order_and_notification_pluralization() {
        let original = request();
        for (patterns, hidden, message) in [
            (
                serde_json::json!({}),
                vec![1, 2],
                Some("Collapsed 2 generated files"),
            ),
            (
                serde_json::json!({"patterns":["Cargo.lock"]}),
                vec![1],
                Some("Collapsed 1 generated file"),
            ),
            (serde_json::json!({"patterns":[]}), vec![], None),
        ] {
            let result = CollapseGenerated::from_config(&patterns)
                .unwrap()
                .transform(original.clone());
            let mut expected = original.changeset.clone();
            expected.files = expected
                .files
                .into_iter()
                .enumerate()
                .filter_map(|(i, file)| (!hidden.contains(&i)).then_some(file))
                .collect();
            assert_eq!(result.changeset, expected);
            assert_eq!(
                result.notifications.first().map(|n| n.message.as_str()),
                message
            );
            assert_eq!(result.notifications.len(), usize::from(message.is_some()));
            assert!(
                result
                    .notifications
                    .iter()
                    .all(|n| n.notification_type == ExtensionNotifyType::Info)
            );
        }
    }

    #[test]
    fn wire_handshake_transform_and_failures_are_framed() {
        let params = serde_json::to_value(request()).unwrap();
        let requests = [
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"workdeck/changeset/transform","params":params}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"workdeck/handshake","params":{
                "host_api_version":1,"host_version":"test","extension_id":"collapse-generated","cwd":"/review",
                "granted_capabilities":["configuration","changeset-transforms"],"config":{"patterns":["Cargo.lock"]}
            }}),
            serde_json::json!({"jsonrpc":"2.0","id":3,"method":"workdeck/changeset/transform","params":params}),
            serde_json::json!({"jsonrpc":"2.0","id":4,"method":"unknown","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":5,"method":"workdeck/changeset/transform","params":{}}),
        ];
        let input = requests
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let mut output = Vec::new();
        serve(input.as_bytes(), &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.ends_with('\n'));
        let responses: Vec<serde_json::Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(responses.len(), 6);
        assert_eq!(responses[2]["method"], "workdeck/notify");
        assert_eq!(
            responses[2]["params"]["message"],
            "Collapsed 1 generated file"
        );
        assert!(responses[2].get("id").is_none());
        let responses: Vec<_> = responses
            .into_iter()
            .filter(|value| value.get("id").is_some())
            .collect();
        for (index, response) in responses.iter().enumerate() {
            assert_eq!(response["id"], index + 1);
            assert_eq!(response["jsonrpc"], "2.0");
        }
        assert_eq!(responses[0]["error"]["code"], -32000);
        let handshake: HandshakeResponse =
            serde_json::from_value(responses[1]["result"].clone()).unwrap();
        assert_eq!(
            handshake.registrations,
            vec![Registration::ChangesetTransform {
                id: "collapse-generated".into()
            }]
        );
        let transformed: TransformResponse =
            serde_json::from_value(responses[2]["result"].clone()).unwrap();
        assert_eq!(transformed.changeset.files.len(), 3);
        assert!(transformed.notifications.is_empty());
        assert_eq!(responses[3]["error"]["code"], -32601);
        assert_eq!(responses[4]["error"]["code"], -32602);
    }

    #[test]
    fn star_only_patterns_are_anchored_literal_and_cross_directories() {
        let extension = CollapseGenerated::from_config(&serde_json::json!({
            "patterns": ["dist/*", "a[1]?.lock", "**.lock"]
        }))
        .unwrap();
        for path in ["dist/", "dist/a/b", "a[1]?.lock", "src/é.lock"] {
            assert!(
                extension.patterns.iter().any(|p| p.is_match(path)),
                "{path:?}"
            );
        }
        for path in [
            "prefix/dist/a",
            "dist/a\n",
            "dist/a\r",
            "dist/\u{2028}",
            "dist/\u{2029}",
            "a1x.lock.more",
        ] {
            assert!(
                !extension.patterns.iter().any(|p| p.is_match(path)),
                "{path:?}"
            );
        }
        let literal =
            CollapseGenerated::from_config(&serde_json::json!({"patterns":["a[1]?.lock"]}))
                .unwrap();
        assert!(!literal.patterns[0].is_match("a1x.lock"));
    }

    #[test]
    fn defaults_empty_override_and_invalid_configuration() {
        for config in [serde_json::json!({}), serde_json::json!({"patterns":null})] {
            let extension = CollapseGenerated::from_config(&config).unwrap();
            for path in ["Cargo.lock", "package-lock.json", "dist/output"] {
                assert!(extension.patterns.iter().any(|p| p.is_match(path)));
            }
        }
        assert!(
            CollapseGenerated::from_config(&serde_json::json!({"patterns":[]}))
                .unwrap()
                .patterns
                .is_empty()
        );
        for config in [
            serde_json::json!({"patterns":"*.lock"}),
            serde_json::json!({"patterns":[3]}),
        ] {
            assert!(CollapseGenerated::from_config(&config).is_err());
        }
    }
}
