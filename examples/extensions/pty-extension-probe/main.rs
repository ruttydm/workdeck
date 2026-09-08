//! Hunk MIT: native pane fixtures from test/pty/extensions-integration.test.ts.
use std::io::{self, BufRead, Write};
use workdeck_core::ReviewSide;
use workdeck_extension_api::*;

fn value(value: impl serde::Serialize) -> io::Result<serde_json::Value> {
    serde_json::to_value(value).map_err(io::Error::other)
}

fn pane(id: &str, placement: PanePlacement, open: bool) -> PaneRegistration {
    PaneRegistration {
        id: id.into(),
        title: "Fixture".into(),
        placement,
        default_open: open,
        preferred_size: None,
        width: None,
        height: None,
        replaces: None,
        current_line: false,
        available: false,
    }
}

fn registrations(kind: &str) -> Vec<Registration> {
    if matches!(kind, "highlight" | "reveal" | "held-reveal") {
        let mut registrations = vec![Registration::Command(CommandRegistration {
            id: "first".into(),
            title: "First fixture action".into(),
            description: None,
            default_keys: vec!["f7".into()],
        })];
        if kind != "held-reveal" {
            registrations.push(Registration::Command(CommandRegistration {
                id: "second".into(),
                title: "Second fixture action".into(),
                description: None,
                default_keys: vec!["f8".into()],
            }));
        }
        if kind == "highlight" {
            registrations.push(Registration::LineHighlighter {
                id: "needles".into(),
            });
        }
        if kind == "held-reveal" {
            let mut capture = pane("capture", PanePlacement::Bottom, true);
            capture.height = Some(ExtensionPaneSize {
                preferred: 1,
                min: Some(1),
                max: Some(1),
                fraction: None,
            });
            registrations.push(Registration::Pane(capture));
        }
        return registrations;
    }
    if matches!(kind, "notify" | "shutdown") {
        return vec![Registration::EventSubscription {
            names: if kind == "shutdown" {
                vec!["startup".into(), "shutdown".into()]
            } else {
                vec!["startup".into()]
            },
        }];
    }
    if matches!(kind, "transform" | "source-transform") {
        return vec![Registration::ChangesetTransform {
            id: "filter-beta".into(),
        }];
    }
    if kind == "dialog" {
        return vec![Registration::Command(CommandRegistration {
            id: "ask".into(),
            title: "Ask".into(),
            description: None,
            default_keys: vec!["y".into()],
        })];
    }
    let panes = match kind {
        "slots" => {
            let mut files = pane("files-slot", PanePlacement::Right, true);
            files.replaces = Some("workdeck:files".into());
            vec![files, pane("aux", PanePlacement::Left, true)]
        }
        "edges" => [
            ("top", PanePlacement::Top, 5),
            ("bottom", PanePlacement::Bottom, 2),
        ]
        .into_iter()
        .map(|(id, placement, max)| {
            let mut pane = pane(id, placement, false);
            pane.height = Some(ExtensionPaneSize {
                preferred: 2,
                min: Some(2),
                max: Some(max),
                fraction: None,
            });
            pane
        })
        .collect(),
        "sidebar" => vec![pane("fixture-sidebar", PanePlacement::Right, false)],
        _ => panic!("unknown fixture kind"),
    };
    let mut registrations = panes
        .into_iter()
        .map(Registration::Pane)
        .collect::<Vec<_>>();
    if kind != "slots" {
        registrations.push(Registration::Command(CommandRegistration {
            id: "toggle-fixture".into(),
            title: "Toggle fixture".into(),
            description: None,
            default_keys: vec!["y".into()],
        }));
    }
    registrations
}

fn dispatch(
    request: &JsonRpcRequest,
    kind: &str,
    captured_file: &mut Option<String>,
) -> io::Result<serde_json::Value> {
    match request.method.as_str() {
        "workdeck/handshake" => value(HandshakeResponse {
            extension_api_version: API_VERSION,
            extension_version: "0.1.0".into(),
            registrations: registrations(kind),
        }),
        "workdeck/command/invoke" => {
            if matches!(kind, "highlight" | "reveal" | "held-reveal") {
                let invocation: CommandInvocation =
                    serde_json::from_value(request.params.clone()).map_err(io::Error::other)?;
                let first = invocation.command_id == "first";
                let actions = if kind == "highlight" {
                    let mut actions = vec![ExtensionHostAction::RefreshLineHighlights {
                        id: if first { "needles" } else { "nope" }.into(),
                        file_id: None,
                    }];
                    if first {
                        actions.push(ExtensionHostAction::Notify {
                            message: "marks refreshed".into(),
                            notification_type: ExtensionNotifyType::Info,
                        });
                    }
                    actions
                } else {
                    let file_id = if kind == "held-reveal" {
                        captured_file.clone()
                    } else {
                        invocation.selection.file.map(|file| file.id)
                    };
                    file_id
                        .into_iter()
                        .map(|file_id| ExtensionHostAction::RevealReviewLine {
                            file_id,
                            side: ReviewSide::New,
                            line: if first { 111 } else { 9001 },
                        })
                        .collect()
                };
                return value(CommandExecution { actions });
            }
            if kind == "dialog" {
                return value(CommandExecution { actions: vec![ExtensionHostAction::OpenConfirmDialog {
                    id: "ask".into(), title: "Reformat the changeset?".into(),
                    body: "Nothing is written to disk. This deliberately long explanation wraps across many terminal rows while the actions remain pinned below it.".into(),
                    confirm_label: "reformat".into(), cancel_label: None,
                }] });
            }
            let ids = if kind == "edges" {
                vec!["top", "bottom"]
            } else {
                vec!["fixture-sidebar"]
            };
            value(CommandExecution {
                actions: ids
                    .into_iter()
                    .map(|id| ExtensionHostAction::TogglePane { id: id.into() })
                    .collect(),
            })
        }
        "workdeck/changeset/transform" => {
            let mut changeset = request.params["changeset"].clone();
            changeset["title"] = "REPO EXTENSION ACTIVE".into();
            changeset["files"]
                .as_array_mut()
                .ok_or_else(|| io::Error::other("missing files"))?
                .retain(|file| !file["path"].as_str().unwrap_or_default().contains("beta"));
            if kind == "source-transform" {
                for file in changeset["files"].as_array_mut().unwrap() {
                    file["path"] = "display-only.txt".into();
                    file["language"] = "rust".into();
                }
            }
            value(serde_json::json!({ "changeset": changeset }))
        }
        "workdeck/event" => {
            let event: ReviewEvent =
                serde_json::from_value(request.params.clone()).map_err(io::Error::other)?;
            let message = if kind == "shutdown" {
                "INTERRUPT FIXTURE READY"
            } else {
                "hello from the fixture extension"
            };
            value(CommandExecution {
                actions: if event.name == "startup" {
                    vec![ExtensionHostAction::Notify {
                        message: message.into(),
                        notification_type: ExtensionNotifyType::Info,
                    }]
                } else {
                    Vec::new()
                },
            })
        }
        "workdeck/dialog/confirm" => {
            let submission: ConfirmDialogSubmission =
                serde_json::from_value(request.params.clone()).map_err(io::Error::other)?;
            value(CommandExecution {
                actions: vec![ExtensionHostAction::Notify {
                    message: if submission.confirmed {
                        "DIALOG ANSWERED YES"
                    } else {
                        "DIALOG ANSWERED NO"
                    }
                    .into(),
                    notification_type: ExtensionNotifyType::Info,
                }],
            })
        }
        "workdeck/pane/render" => {
            let request: PaneRenderRequest =
                serde_json::from_value(request.params.clone()).map_err(io::Error::other)?;
            let text = match kind {
                "held-reveal" => {
                    if captured_file.is_none() {
                        *captured_file = request
                            .snapshot
                            .changeset
                            .files
                            .get(request.snapshot.selection.file_index)
                            .map(|file| file.runtime_id.clone());
                    }
                    "CAPTURE PANE".into()
                }
                "sidebar" => format!(
                    "EXTSIDEBAR {} FILES",
                    request.snapshot.changeset.files.len()
                ),
                "slots" => if request.pane_id == "aux" {
                    "AUX PANE LEFT"
                } else {
                    "FILES SLOT RIGHT"
                }
                .into(),
                "edges" => format!(
                    "PANE {} {}x{}",
                    request.pane_id.to_uppercase(),
                    request.width,
                    request.height
                ),
                _ => return Err(io::Error::other("unknown fixture kind")),
            };
            value(PaneRenderResponse {
                content: ViewNode::Text {
                    text,
                    style: ViewStyle {
                        foreground: Some(request.theme.text),
                        background: Some(request.theme.panel),
                        ..ViewStyle::default()
                    },
                },
            })
        }
        "workdeck/line-highlighter/highlight" => {
            let fixture_root = std::env::current_exe()?
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf();
            if fixture_root.join("exit-line-highlight").exists() {
                std::fs::write(fixture_root.join("line-highlight-exited"), "exiting\n")?;
                std::process::exit(0);
            }
            let hold = fixture_root.join("hold-line-highlight");
            if hold.exists() {
                std::fs::write(fixture_root.join("line-highlight-blocked"), "blocked\n")?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while hold.exists() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                std::fs::write(fixture_root.join("line-highlight-released"), "released\n")?;
            }
            if request.params["file"]["path"]
                .as_str()
                .is_some_and(|path| path.contains("alpha"))
            {
                value(
                    serde_json::json!([{ "side": "new", "line": 1, "range": [13, 23], "tone": "match" }]),
                )
            } else {
                value(serde_json::Value::Null)
            }
        }
        _ => Err(io::Error::other("unknown fixture protocol method")),
    }
}

fn main() -> io::Result<()> {
    let executable = std::env::current_exe()?;
    let kind = std::fs::read_to_string(
        executable
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixture-kind"),
    )?;
    let mut output = io::BufWriter::new(io::stdout().lock());
    let mut session_cwd = std::env::current_dir()?;
    let mut captured_file = None;
    for line in io::stdin().lock().lines() {
        let line = line?;
        let envelope: serde_json::Value = serde_json::from_str(&line).map_err(io::Error::other)?;
        // Highlight completion retires the parent with a notification. This
        // synchronous probe has no outstanding work once its response is sent.
        if envelope["method"] == "$/cancelRequest" && envelope.get("id").is_none() {
            continue;
        }
        if envelope["method"] == "workdeck/handshake" {
            let handshake: HandshakeRequest =
                serde_json::from_value(envelope["params"].clone()).map_err(io::Error::other)?;
            session_cwd = handshake.cwd;
        }
        if envelope["method"] == "workdeck/shutdown" {
            if kind.trim() == "shutdown" {
                let mut log = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(session_cwd.join(".workdeck-shutdown.log"))?;
                log.write_all(b"shutdown\n")?;
            }
            return Ok(());
        }
        let request: JsonRpcRequest = serde_json::from_value(envelope).map_err(io::Error::other)?;
        let response = match dispatch(&request, kind.trim(), &mut captured_file) {
            Ok(result) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: request.id,
                result: Some(result),
                error: None,
            },
            Err(error) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: request.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32000,
                    message: error.to_string(),
                    data: None,
                }),
            },
        };
        serde_json::to_writer(&mut output, &response).map_err(io::Error::other)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}
