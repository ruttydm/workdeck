//! Native subprocess fixture for Hunk's mounted AppHost extension-sidebar tests.

use std::io::{self, BufRead, Write};
use std::time::Duration;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use workdeck_extension_api::{
    API_VERSION, CommandExecution, CommandInvocation, CommandRegistration, ExtensionHostAction,
    ExtensionKeyEvent, ExtensionNotifyType, ExtensionPaneSize, HandshakeRequest, HandshakeResponse,
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, PaneAvailabilityRequest,
    PaneAvailabilityResponse, PanePlacement, PaneRegistration, PaneRenderRequest,
    PaneRenderResponse, Registration, ReviewEvent, ViewNode, ViewStyle, WORKDECK_FILES_PANE_KEY,
};

const EXTENSION_ID: &str = "app-host-extension-sidebar-probe";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Scenario {
    #[default]
    Extra,
    Keys,
    Selection,
    ReviewSnapshot,
    LineOff,
    Independent,
    Menu,
    Replacement,
    SlotOwner,
    MenuSlot,
    BottomSlot,
    Crash,
    AvailabilityFailure,
    FilteredAvailability,
    Scroll,
    Startup,
}

impl Scenario {
    fn parse(value: Option<&str>) -> io::Result<Self> {
        match value.unwrap_or("extra") {
            "extra" => Ok(Self::Extra),
            "keys" => Ok(Self::Keys),
            "selection" => Ok(Self::Selection),
            "review-snapshot" => Ok(Self::ReviewSnapshot),
            "line-off" => Ok(Self::LineOff),
            "independent" => Ok(Self::Independent),
            "menu" => Ok(Self::Menu),
            "replacement" => Ok(Self::Replacement),
            "slot-owner" => Ok(Self::SlotOwner),
            "menu-slot" => Ok(Self::MenuSlot),
            "bottom-slot" => Ok(Self::BottomSlot),
            "crash" => Ok(Self::Crash),
            "availability-failure" => Ok(Self::AvailabilityFailure),
            "filtered-availability" => Ok(Self::FilteredAvailability),
            "scroll" => Ok(Self::Scroll),
            "startup" => Ok(Self::Startup),
            value => Err(io::Error::other(format!("Unknown scenario: {value}"))),
        }
    }
}

fn pane(
    id: &str,
    placement: PanePlacement,
    default_open: bool,
    replaces: Option<&str>,
    available: bool,
) -> Registration {
    Registration::Pane(PaneRegistration {
        id: id.into(),
        title: id.replace('-', " "),
        placement,
        default_open,
        preferred_size: None,
        width: matches!(placement, PanePlacement::Left | PanePlacement::Right).then_some(
            ExtensionPaneSize {
                preferred: 34,
                min: Some(18),
                max: Some(72),
                fraction: None,
            },
        ),
        height: matches!(placement, PanePlacement::Top | PanePlacement::Bottom).then_some(
            ExtensionPaneSize {
                preferred: 1,
                min: Some(1),
                max: Some(1),
                fraction: None,
            },
        ),
        replaces: replaces.map(str::to_owned),
        current_line: false,
        available,
    })
}

fn command(id: &str, title: &str, keys: &[&str]) -> Registration {
    Registration::Command(CommandRegistration {
        id: id.into(),
        title: title.into(),
        description: None,
        default_keys: keys.iter().map(|key| (*key).into()).collect(),
    })
}

fn registrations(scenario: Scenario) -> Vec<Registration> {
    match scenario {
        Scenario::Extra => vec![
            pane("probe", PanePlacement::Right, false, None, false),
            command("toggle-probe", "Toggle probe", &["y"]),
            Registration::EventSubscription {
                names: vec!["selection_changed".into()],
            },
        ],
        Scenario::Keys => vec![
            pane("keys", PanePlacement::Right, true, None, false),
            command("blocked", "Blocked", &["s"]),
        ],
        Scenario::Selection => vec![
            command("probe", "Probe selection", &["y"]),
            command("delayed", "Probe delayed selection", &["x"]),
        ],
        Scenario::ReviewSnapshot => vec![command("snapshot", "Snapshot review", &["y"])],
        Scenario::LineOff => vec![command("probe", "Probe selection", &["y"])],
        Scenario::Independent => vec![
            pane("probe", PanePlacement::Right, false, None, false),
            command("open-probe", "Open probe", &["y"]),
        ],
        Scenario::Menu => vec![
            pane("probe", PanePlacement::Right, false, None, false),
            command("open-probe", "Open the probe pane", &[]),
        ],
        Scenario::Replacement => vec![pane(
            "replacement",
            PanePlacement::Left,
            false,
            Some(WORKDECK_FILES_PANE_KEY),
            false,
        )],
        Scenario::SlotOwner => vec![
            pane(
                "first-files",
                PanePlacement::Left,
                false,
                Some(WORKDECK_FILES_PANE_KEY),
                false,
            ),
            pane(
                "second-files",
                PanePlacement::Left,
                false,
                Some(WORKDECK_FILES_PANE_KEY),
                false,
            ),
        ],
        Scenario::MenuSlot => vec![
            pane(
                "files-owner",
                PanePlacement::Left,
                false,
                Some(WORKDECK_FILES_PANE_KEY),
                false,
            ),
            pane("independent", PanePlacement::Right, true, None, false),
        ],
        Scenario::BottomSlot => vec![pane(
            "bottom-files",
            PanePlacement::Bottom,
            false,
            Some(WORKDECK_FILES_PANE_KEY),
            false,
        )],
        Scenario::Crash => vec![pane(
            "broken",
            PanePlacement::Left,
            false,
            Some(WORKDECK_FILES_PANE_KEY),
            false,
        )],
        Scenario::AvailabilityFailure => vec![pane(
            "broken-files",
            PanePlacement::Left,
            false,
            Some(WORKDECK_FILES_PANE_KEY),
            true,
        )],
        Scenario::FilteredAvailability => {
            vec![pane("two-files", PanePlacement::Bottom, true, None, true)]
        }
        Scenario::Scroll => vec![pane("reflist", PanePlacement::Right, true, None, false)],
        Scenario::Startup => vec![
            pane("startup", PanePlacement::Right, false, None, false),
            Registration::EventSubscription {
                names: vec!["startup".into()],
            },
        ],
    }
}

fn text(value: impl Into<String>) -> ViewNode {
    ViewNode::Text {
        text: value.into(),
        style: ViewStyle::default(),
    }
}

fn render_pane(scenario: Scenario, request: PaneRenderRequest) -> io::Result<PaneRenderResponse> {
    let content = match scenario {
        Scenario::Extra | Scenario::Independent | Scenario::Menu => text(format!(
            "EXTSIDEBAR files={} selected={}",
            request.files.len(),
            request.selected_file_id.as_deref().unwrap_or("none")
        )),
        Scenario::Keys => {
            let event = ExtensionKeyEvent {
                name: "n".into(),
                ctrl: true,
                ..ExtensionKeyEvent::default()
            };
            let blocked = request
                .keybindings
                .get_keys(&format!("{EXTENSION_ID}.blocked"));
            text(format!(
                "EXTKEYS {} matched={} BLOCKED {}",
                request
                    .keybindings
                    .get_keys("workdeck.review.nextFile")
                    .join(","),
                request
                    .keybindings
                    .matches(&event, "workdeck.review.nextFile"),
                if blocked.is_empty() {
                    "none".into()
                } else {
                    blocked.join(",")
                }
            ))
        }
        Scenario::Replacement => text("REPLACEMENT SIDEBAR"),
        Scenario::SlotOwner => match request.pane_id.as_str() {
            "first-files" => text("FIRST FILES PANE"),
            "second-files" => text("SECOND FILES PANE"),
            id => return Err(io::Error::other(format!("Unknown pane: {id}"))),
        },
        Scenario::MenuSlot => match request.pane_id.as_str() {
            "files-owner" => text("FILES SLOT OWNER"),
            "independent" => text("INDEPENDENT PANE"),
            id => return Err(io::Error::other(format!("Unknown pane: {id}"))),
        },
        Scenario::BottomSlot => text("BOTTOM FILES SLOT"),
        Scenario::Crash => return Err(io::Error::other("sidebar exploded")),
        Scenario::AvailabilityFailure => text("BROKEN FILES"),
        Scenario::FilteredAvailability => text("TWO FILE PANE"),
        Scenario::Scroll => {
            let selected = request
                .selected_file_id
                .as_ref()
                .and_then(|selected| request.files.iter().position(|file| &file.id == selected));
            ViewNode::List {
                items: request
                    .files
                    .iter()
                    .map(|file| text(format!("ref:{}", file.path)))
                    .collect(),
                selected,
            }
        }
        Scenario::Startup => text("MOUNTED STARTUP SIDEBAR"),
        Scenario::Selection | Scenario::ReviewSnapshot | Scenario::LineOff => ViewNode::Empty,
    };
    Ok(PaneRenderResponse { content })
}

fn notify(message: impl Into<String>) -> ExtensionHostAction {
    ExtensionHostAction::Notify {
        message: message.into(),
        notification_type: ExtensionNotifyType::Info,
    }
}

fn invoke(scenario: Scenario, invocation: CommandInvocation) -> io::Result<CommandExecution> {
    let actions = match scenario {
        Scenario::Extra => {
            let key = format!("{EXTENSION_ID}:probe");
            if invocation.open_panes.iter().any(|open| open == &key) {
                vec![ExtensionHostAction::ClosePane { id: "probe".into() }]
            } else {
                let mut actions = vec![ExtensionHostAction::OpenPane { id: "probe".into() }];
                if let Some(file) = invocation.snapshot.changeset.files.get(1) {
                    actions.push(ExtensionHostAction::SelectReviewFile {
                        file_id: file.runtime_id.clone(),
                    });
                }
                actions
            }
        }
        Scenario::Independent | Scenario::Menu => {
            vec![ExtensionHostAction::OpenPane { id: "probe".into() }]
        }
        Scenario::Selection => {
            let line = invocation.selection.current_line.as_ref().map_or_else(
                || "none".into(),
                |line| format!("{:?}:{}", line.side, line.line).to_ascii_lowercase(),
            );
            if invocation.command_id == "delayed" {
                std::thread::sleep(Duration::from_millis(100));
                vec![notify(format!("delayed {line}"))]
            } else {
                let path = invocation
                    .selection
                    .file
                    .as_ref()
                    .map(|file| file.path.as_str())
                    .unwrap_or("none");
                vec![notify(format!(
                    "selection {path}#{} line={line}",
                    invocation
                        .selection
                        .hunk_index
                        .map_or_else(|| "none".into(), |index| index.to_string())
                ))]
            }
        }
        Scenario::ReviewSnapshot => {
            let review = invocation
                .review
                .ok_or_else(|| io::Error::other("missing review snapshot"))?;
            let notes = review
                .notes
                .iter()
                .map(|note| note.summary.as_str())
                .collect::<Vec<_>>()
                .join("|");
            let paths = review
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>()
                .join("|");
            let identities_distinct = review
                .files
                .iter()
                .all(|file| file.file_key != file.runtime_id);
            let note_detail = review.notes.first().map_or_else(
                || "none".into(),
                |note| {
                    let preferred = note.anchor.preferred.as_ref().map_or_else(
                        || "none".into(),
                        |address| {
                            format!("{:?}:{}", address.side, address.line).to_ascii_lowercase()
                        },
                    );
                    format!(
                        "{:?}:{}:{}:{preferred}",
                        note.source,
                        note.file_key == review.files[0].file_key,
                        note.summary
                    )
                    .to_ascii_lowercase()
                },
            );
            vec![notify(format!(
                "snapshot generation={} revision={} paths={paths} identities-distinct={identities_distinct} notes={} summaries={notes} detail={note_detail}",
                review.generation,
                review.state_revision,
                review.notes.len()
            ))]
        }
        Scenario::LineOff => vec![notify(format!(
            "line-null {}",
            invocation.selection.current_line.is_none()
        ))],
        _ => return Err(io::Error::other("scenario has no command")),
    };
    Ok(CommandExecution { actions })
}

fn event(scenario: Scenario, event: ReviewEvent) -> CommandExecution {
    if scenario == Scenario::Startup && event.name == "startup" {
        return CommandExecution {
            actions: vec![ExtensionHostAction::OpenPane {
                id: "startup".into(),
            }],
        };
    }
    if scenario == Scenario::Extra && event.name == "selection_changed" {
        return CommandExecution {
            actions: vec![notify(format!(
                "selection-event {}",
                event.payload["fileId"].as_str().unwrap_or("none")
            ))],
        };
    }
    CommandExecution::default()
}

fn value(value: impl Serialize) -> io::Result<Value> {
    serde_json::to_value(value).map_err(io::Error::other)
}

fn parse<T: DeserializeOwned>(request: &JsonRpcRequest) -> io::Result<T> {
    serde_json::from_value(request.params.clone()).map_err(io::Error::other)
}

fn write_response(output: &mut impl Write, id: u64, result: io::Result<Value>) -> io::Result<()> {
    let response = match result {
        Ok(result) => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        },
        Err(error) => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32_000,
                message: error.to_string(),
                data: None,
            }),
        },
    };
    serde_json::to_writer(&mut *output, &response).map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()
}

/// Serve one configured sidebar scenario over JSON-RPC 2.0 JSONL.
pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let mut scenario = Scenario::default();
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let request: JsonRpcRequest = serde_json::from_str(&line).map_err(io::Error::other)?;
        let result = match request.method.as_str() {
            "workdeck/handshake" => parse::<HandshakeRequest>(&request).and_then(|handshake| {
                scenario =
                    Scenario::parse(handshake.config.get("scenario").and_then(Value::as_str))?;
                value(HandshakeResponse {
                    extension_api_version: API_VERSION,
                    extension_version: env!("CARGO_PKG_VERSION").into(),
                    registrations: registrations(scenario),
                })
            }),
            "workdeck/pane/render" => parse(&request)
                .and_then(|request| render_pane(scenario, request))
                .and_then(value),
            "workdeck/pane/available" => {
                parse::<PaneAvailabilityRequest>(&request).and_then(|request| match scenario {
                    Scenario::AvailabilityFailure => Err(io::Error::other("availability exploded")),
                    Scenario::FilteredAvailability => value(PaneAvailabilityResponse {
                        available: request.files.len() == 2,
                    }),
                    _ => value(PaneAvailabilityResponse { available: true }),
                })
            }
            "workdeck/command/invoke" => parse(&request)
                .and_then(|invocation| invoke(scenario, invocation))
                .and_then(value),
            "workdeck/event" => parse(&request)
                .map(|review_event| event(scenario, review_event))
                .and_then(value),
            method => Err(io::Error::other(format!("Unknown method: {method}"))),
        };
        write_response(&mut output, request.id, result)?;
    }
}
