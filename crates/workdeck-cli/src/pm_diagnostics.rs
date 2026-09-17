//! Bounded transport diagnostics preserve uncertainty and receipt lookup identity.
use serde_json::{Value, json};
use workdeck_pm::{ContentHash, ErrorCode, PmError};

const MAX_ERROR_BYTES: usize = 16 * 1024;

fn clipped(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.into();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

pub(super) fn render(error: &PmError, source: &Value) -> Value {
    let recovery: &[&str] = match error.code {
        ErrorCode::InvalidInput | ErrorCode::InvalidSchema | ErrorCode::UnsupportedSchema => {
            &["inspect_schema", "correct_input"]
        }
        ErrorCode::StaleSource | ErrorCode::Conflict => {
            &["reload_source", "review_changed_preconditions"]
        }
        ErrorCode::IdempotencyConflict => &["inspect_original_request", "reconcile_request_intent"],
        ErrorCode::RecoveryRequired | ErrorCode::CorruptStore => {
            &["inspect_pending_operations", "reconcile_source"]
        }
        ErrorCode::LegacyStore | ErrorCode::AmbiguousSource => {
            &["inspect_source_selection", "review_migration"]
        }
        ErrorCode::Io => &["inspect_partial_outcome", "inspect_source_before_retry"],
        ErrorCode::Locked => &["wait_for_writer", "reload_source"],
        ErrorCode::NotInitialized => &["inspect_source_selection", "explicit_initialization"],
        ErrorCode::NotFound | ErrorCode::AmbiguousReference => {
            &["inspect_source", "resolve_reference"]
        }
        ErrorCode::PolicyBlocked | ErrorCode::ClaimLost => &["inspect_policy_and_preconditions"],
        ErrorCode::UnsafePath => &["inspect_path_without_following_unsafe_entries"],
        ErrorCode::Unsupported => &["inspect_capabilities"],
        ErrorCode::Canceled => &["inspect_partial_outcome"],
    };
    let mut recovery = recovery.to_vec();
    if error.details.as_ref().is_some_and(|details| {
        details["mutation_committed"] == true && details["run_id"].is_string()
    }) {
        recovery.extend(["inspect_run_status", "retain_original_request_identity"]);
    }
    let mut value = json!({"api_version":1,"ok":false,"source":source,"error":error,
        "retryable":error.code.retryable(),"recovery_actions":recovery});
    let encoded = serde_json::to_vec(&value).expect("error values serialize");
    if encoded.len() < MAX_ERROR_BYTES {
        return value;
    }
    value["diagnostic_truncated"] = json!(true);
    value["diagnostic_original_bytes"] = json!(encoded.len() + 1);
    value["diagnostic_content"] = json!(ContentHash::of(&encoded));
    for (field, limit) in [("message", 2048), ("path", 1024), ("hint", 2048)] {
        if let Some(text) = value["error"][field].as_str() {
            value["error"][field] = json!(clipped(text, limit));
        }
    }
    if let Some(details) = value["error"].get("details").cloned()
        && serde_json::to_vec(&details)
            .expect("details serialize")
            .len()
            > 4096
    {
        let mut summary = json!({"truncated":true});
        for key in [
            "mutation_committed",
            "current_context",
            "minimum_required_bytes",
            "minimum_stdout_bytes",
            "transport_overhead_bytes",
            "output_projection_failed",
            "run_id",
            "request_id",
        ] {
            if let Some(item) = details.get(key)
                && serde_json::to_vec(item).expect("detail serializes").len() <= 512
            {
                summary[key] = item.clone();
            }
        }
        if let Some(receipt) = details.get("receipt") {
            summary["receipt"] = json!({
                "operation_id":receipt["operation_id"],"request_id":receipt["request_id"],
                "repository":receipt["repository"],"operation":receipt["operation"],
                "input_hash":receipt["input_hash"],"full_receipt_omitted":true
            });
            summary["receipt_lookup"] = json!(
                "Inspect operation history or retry the original request; this summary does not replace its full receipt."
            );
        }
        if let Some(receipts) = details.get("receipts").and_then(Value::as_array) {
            summary["receipts"] = Value::Array(
                receipts
                    .iter()
                    .take(2)
                    .map(|receipt| {
                        let mut item = json!({"full_receipt_omitted":true});
                        for key in [
                            "operation_id",
                            "request_id",
                            "repository",
                            "operation",
                            "input_hash",
                        ] {
                            item[key] = receipt[key]
                                .as_str()
                                .map(|text| json!(clipped(text, 256)))
                                .unwrap_or(Value::Null);
                        }
                        item
                    })
                    .collect(),
            );
            summary["omitted_receipts"] = json!(receipts.len().saturating_sub(2));
            summary["receipt_lookup"] = json!(
                "Inspect check status using run_id or retry the exact original request; these summaries do not replace full receipts."
            );
        }
        value["error"]["details"] = summary;
    }
    if serde_json::to_vec(&value).expect("error serializes").len() + 1 > MAX_ERROR_BYTES {
        // Escaping can make a short control-heavy string much larger on the wire.
        for field in ["message", "path", "hint"] {
            if let Some(text) = value["error"][field].as_str() {
                value["error"][field] = json!(clipped(text, 256));
            }
        }
        value["source"] = json!({"repository":source["repository"],"root":source["root"].as_str().map(|path|clipped(path,256)),"truncated":true});
    }
    value
}

/// Recognize machine requests from parser metadata without consulting a source.
/// Consuming known option values prevents a path/title that names a command from
/// being mistaken for a PM command. Help/version retain Clap's normal behavior.
pub(super) fn parser_machine_request(argv: &[std::ffi::OsString]) -> bool {
    use clap::CommandFactory;
    let mut root = super::Args::command();
    root.build();
    let mut command = &root;
    let mut path = Vec::new();
    let mut machine = false;
    let mut cursor = 1;
    while cursor < argv.len() {
        let token = argv[cursor].to_string_lossy();
        if token == "--" {
            break;
        }
        if let Some(option) = token.strip_prefix("--") {
            let (name, inline) = option
                .split_once('=')
                .map_or((option, false), |(name, _)| (name, true));
            // A misplaced machine flag still requests a structured argument error.
            if matches!(name, "json" | "compact" | "fields") {
                machine = true;
            }
            if let Some(argument) = command
                .get_arguments()
                .find(|arg| arg.get_long() == Some(name))
            {
                let takes_value = argument
                    .get_num_args()
                    .is_some_and(|range| range.max_values() > 0);
                if takes_value
                    && !inline
                    && argv
                        .get(cursor + 1)
                        .is_some_and(|next| !next.to_string_lossy().starts_with('-'))
                {
                    cursor += 1;
                }
            }
        } else if !token.starts_with('-')
            && let Some(child) = command.find_subcommand(token.as_ref())
        {
            path.push(child.get_name());
            command = child;
        }
        cursor += 1;
    }
    machine
        && (matches!(
            path.first().copied(),
            Some(
                "context"
                    | "next"
                    | "question"
                    | "handoff"
                    | "protocol"
                    | "capabilities"
                    | "command"
                    | "check"
            )
        ) || path.starts_with(&["issue", "next"]))
}

pub(super) fn print_parser(error: &clap::Error) {
    let error = PmError::new(ErrorCode::InvalidInput, error.to_string())
        .hint("Inspect the command help and correct its arguments before retrying.");
    let value = render(
        &error,
        &json!({"repository":null,"root":null,"state":"arguments_not_validated"}),
    );
    let mut stdout = std::io::stdout().lock();
    use std::io::Write;
    if serde_json::to_writer(&mut stdout, &value).is_ok() {
        let _ = writeln!(stdout);
    }
}
