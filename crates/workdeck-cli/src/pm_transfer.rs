//! Native snapshot transfer. Unsupported restoration remains an explicit plan
//! blocker; these adapters never invoke destructive prototype import handlers.
use super::{Command, pm_cli};
use clap::Args;
use serde_json::Value;
use std::{
    io::{Read, Write},
    path::Path,
};
use workdeck_pm::{
    ErrorCode, ImportSource, LegacyImportContext, MAX_SNAPSHOT_INPUT_BYTES, PmError, Repository,
    RequestId, Result, SnapshotImportMode, decode_transfer,
};

#[derive(Debug, Default, Args)]
pub(super) struct ImportOptions {
    #[arg(long, conflicts_with_all = ["replace", "merge", "imported_at"], help = "Restore missing native authority, including original receipts and repository identity")]
    restore: bool,
    #[arg(long, requires_all = ["restore", "request_id"], conflicts_with_all = ["path", "dry_run", "expected_plan"], help = "Resume a pending native restoration from its retained input")]
    resume: bool,
    #[arg(long, help = "Stable idempotency key for native import")]
    request_id: Option<String>,
    #[arg(
        long,
        help = "Require the exact reviewed native import-plan fingerprint"
    )]
    expected_plan: Option<String>,
    #[arg(
        long,
        help = "RFC 3339 legacy import timestamp from the reviewed preview"
    )]
    imported_at: Option<String>,
    #[arg(long, help = "Stage only paths changed by this native import")]
    stage: bool,
    #[arg(long, help = "Require the native noninteractive import contract")]
    no_input: bool,
}
impl ImportOptions {
    pub(super) fn is_native(&self) -> bool {
        self.restore
            || self.resume
            || self.request_id.is_some()
            || self.expected_plan.is_some()
            || self.imported_at.is_some()
            || self.stage
            || self.no_input
    }

    pub(super) fn restores(&self) -> bool {
        self.restore
    }
}

pub(super) fn run_restore(cwd: &Path, source: &mut Value, command: &Command) -> Result<()> {
    let Command::Import {
        options,
        path,
        dry_run,
        json,
        ..
    } = command
    else {
        return Err(pm_cli::invalid("expected snapshot restoration import"));
    };
    if *dry_run
        && (options.request_id.is_some() || options.expected_plan.is_some() || options.stage)
    {
        return Err(pm_cli::invalid(
            "dry-run cannot take mutation request, staging or expected-plan flags",
        ));
    }
    let destination = pm_cli::source_root(cwd);
    let request = options
        .request_id
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or_else(RequestId::new);
    let expected = options
        .expected_plan
        .as_deref()
        .map(str::parse)
        .transpose()?;
    let restored = if options.resume {
        workdeck_pm::resume_snapshot_restore(&destination, &request)?
    } else {
        let path = path
            .as_ref()
            .ok_or_else(|| pm_cli::invalid("restore requires a snapshot path"))?;
        let input = match decode_transfer(&read_input(cwd, path)?)? {
            ImportSource::Native(snapshot) => snapshot,
            ImportSource::Legacy(_) => {
                return Err(pm_cli::invalid(
                    "restore requires a native snapshot; use ordinary import for legacy JSON/JSONL conversion",
                ));
            }
        };
        if *dry_run {
            let plan = workdeck_pm::preview_snapshot_restore(&destination, &input)?;
            *source =
                serde_json::json!({"root":plan.destination_root,"repository":plan.repository});
            return pm_cli::emit(*json, "import_restore_preview", source, &plan, None);
        }
        workdeck_pm::restore_snapshot(&destination, &input, expected.as_ref(), &request)?
    };
    *source = serde_json::json!({"root":restored.plan.destination_root,"repository":restored.plan.repository});
    if options.stage {
        let repository = Repository::open_source(&destination).map_err(|error| error.details(
            serde_json::json!({"mutation_committed":true,"receipt":restored.receipt,"staging":{"state":"failed"}})
        ))?;
        return pm_cli::emit_mutation(
            &repository,
            &pm_cli::IssueOptions {
                stage: true,
                ..Default::default()
            },
            *json,
            "import_restore",
            source,
            &restored.receipt,
        );
    }
    pm_cli::emit(
        *json,
        "import_restore",
        source,
        &restored.receipt.result,
        Some(&restored.receipt),
    )
}

pub(super) fn read_input(cwd: &Path, path: &Path) -> Result<Vec<u8>> {
    if path != Path::new("-") {
        return pm_cli::read_regular_input(cwd, path, MAX_SNAPSHOT_INPUT_BYTES as u64);
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(MAX_SNAPSHOT_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| PmError::io("stdin", error))?;
    if bytes.len() > MAX_SNAPSHOT_INPUT_BYTES {
        return Err(pm_cli::invalid("transfer input exceeds 48 MiB"));
    }
    Ok(bytes)
}

pub(super) fn run(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    command: &Command,
) -> Result<()> {
    match command {
        Command::Export { json, jsonl } => {
            let snapshot = repository.export_snapshot()?;
            if *jsonl {
                let output = snapshot.to_jsonl()?;
                std::io::stdout()
                    .lock()
                    .write_all(output.as_bytes())
                    .map_err(|error| PmError::io("stdout", error))
            } else if *json {
                pm_cli::emit(true, "export", source, &snapshot, None)
            } else {
                let mut output = std::io::stdout().lock();
                serde_json::to_writer(&mut output, &snapshot)
                    .map_err(|error| PmError::io("stdout", error))?;
                writeln!(output).map_err(|error| PmError::io("stdout", error))
            }
        }
        Command::Import {
            options,
            path,
            replace,
            dry_run,
            json,
            ..
        } => {
            if *dry_run
                && (options.request_id.is_some()
                    || options.stage
                    || options.expected_plan.is_some())
            {
                return Err(pm_cli::invalid(
                    "dry-run cannot take request-id, stage, or an expected-plan mutation precondition",
                ));
            }
            let path = path
                .as_ref()
                .ok_or_else(|| pm_cli::invalid("import requires a source path"))?;
            let bytes = read_input(cwd, path)?;
            let input = decode_transfer(&bytes)?;
            let context = options
                .imported_at
                .as_ref()
                .map(|time| {
                    time.parse()
                        .map(|imported_at| LegacyImportContext { imported_at })
                        .map_err(|_| pm_cli::invalid("imported-at must be an RFC 3339 timestamp"))
                })
                .transpose()?;
            if context.is_some() && matches!(input, ImportSource::Native(_)) {
                return Err(pm_cli::invalid(
                    "imported-at applies only to legacy JSON/JSONL conversion",
                ));
            }
            let mode = if *replace {
                SnapshotImportMode::ReplaceMatching
            } else {
                SnapshotImportMode::Merge
            };
            if *dry_run {
                return match &input {
                    ImportSource::Native(snapshot) => pm_cli::emit(
                        *json,
                        "import_preview",
                        source,
                        &repository.preview_snapshot_import(snapshot, mode)?,
                        None,
                    ),
                    ImportSource::Legacy(export) => pm_cli::emit(
                        *json,
                        "import_preview",
                        source,
                        &repository.preview_legacy_import(export, context.as_ref(), mode)?,
                        None,
                    ),
                };
            }
            let request = options
                .request_id
                .as_deref()
                .map(str::parse)
                .transpose()?
                .unwrap_or_else(RequestId::new);
            let expected = options
                .expected_plan
                .as_deref()
                .map(str::parse)
                .transpose()?;
            let receipt = match &input {
                ImportSource::Native(snapshot) => {
                    repository.import_snapshot(snapshot, mode, expected.as_ref(), &request)?
                }
                ImportSource::Legacy(export) => repository.import_legacy_export(
                    export,
                    context.as_ref(),
                    mode,
                    expected.as_ref(),
                    &request,
                )?,
            };
            pm_cli::emit_mutation(
                repository,
                &pm_cli::IssueOptions {
                    stage: options.stage,
                    ..Default::default()
                },
                *json,
                "import",
                source,
                &receipt,
            )
        }
        _ => Err(PmError::new(
            ErrorCode::InvalidInput,
            "expected import or export",
        )),
    }
}
