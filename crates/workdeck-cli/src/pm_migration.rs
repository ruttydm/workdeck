//! Explicit preview -> reviewed plan -> apply/resume composition. Preview output
//! is not an authorization marker, and a saved plan never changes source files.
use super::pm_cli;
use clap::Args;
use serde_json::json;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use workdeck_pm::{
    Config, ErrorCode, PmError, RequestId, Result,
    migration::{self, MigrationPreview, PreviewOptions},
};

const MAX_PLAN_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Args)]
pub(super) struct Options {
    #[arg(long, conflicts_with_all = ["apply", "resume"])]
    dry_run: bool,
    #[arg(long, conflicts_with = "resume")]
    apply: bool,
    #[arg(
        long,
        help = "Resume the persisted migration without generating a new plan"
    )]
    resume: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Prototype root (default: <repository>/.agents/workdeck)"
    )]
    source: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Native root (default: <repository>/.workdeck)"
    )]
    destination: Option<PathBuf>,
    #[arg(long, help = "New record prefix for preview (default: WD)")]
    prefix: Option<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Save the exact preview to a new file for review and application"
    )]
    plan_out: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Apply this exact saved preview; '-' reads stdin"
    )]
    plan: Option<PathBuf>,
    #[arg(long, help = "Stable idempotency key; required for apply and resume")]
    request_id: Option<String>,
    #[arg(long)]
    pub json: bool,
}

pub(super) fn run(cwd: &Path, options: &Options) -> Result<()> {
    let native = pm_cli::source_root(cwd);
    let project = native
        .parent()
        .ok_or_else(|| pm_cli::invalid("planning source has no project root"))?;
    let destination = canonical_candidate(
        &options
            .destination
            .as_ref()
            .map_or(native.clone(), |path| cwd.join(path)),
    )?;
    let source_identity = json!({"repository":null,"root":destination});
    if options.apply || options.resume {
        if options.plan_out.is_some() || options.prefix.is_some() || options.source.is_some() {
            return Err(pm_cli::invalid(
                "apply/resume use persisted plan context; source, prefix and plan-out are preview options",
            ));
        }
        let request: RequestId = options
            .request_id
            .as_deref()
            .ok_or_else(|| pm_cli::invalid("apply/resume require --request-id"))?
            .parse()?;
        let receipt = if options.resume {
            if options.plan.is_some() {
                return Err(pm_cli::invalid(
                    "resume reads the persisted migration; omit --plan",
                ));
            }
            migration::resume(&destination, &request)?
        } else {
            let path = options.plan.as_ref().ok_or_else(|| {
                pm_cli::invalid("apply requires --plan with an exact reviewed preview")
            })?;
            let plan = read_plan(cwd, path)?;
            if canonical_candidate(&plan.destination_root)? != destination {
                return Err(pm_cli::invalid(
                    "saved plan targets a different planning root; select that exact root with --destination",
                ));
            }
            migration::apply(&plan, &request)?
        };
        let source = json!({"repository":receipt.repository,"root":destination});
        return pm_cli::emit(
            options.json,
            if options.resume {
                "migration_resume"
            } else {
                "migration_apply"
            },
            &source,
            &receipt,
            Some(&receipt.receipt),
        );
    }
    if options.plan.is_some() || options.request_id.is_some() {
        return Err(pm_cli::invalid(
            "plan and request-id apply only to --apply or --resume",
        ));
    }
    let source = options
        .source
        .as_ref()
        .map_or_else(|| project.join(".agents/workdeck"), |path| cwd.join(path));
    let config = Config::new(options.prefix.as_deref().unwrap_or("WD"))?;
    let plan = migration::preview(
        &source,
        &destination,
        &PreviewOptions {
            config,
            imported_at: chrono::Utc::now(),
        },
    )?;
    if let Some(path) = &options.plan_out {
        save_plan(&cwd.join(path), &plan)?;
    }
    if options.json {
        pm_cli::emit(true, "migration_preview", &source_identity, &plan, None)
    } else {
        let mut stdout = std::io::stdout().lock();
        writeln!(
            stdout,
            "migration preview: {} source files, {} planned files, {} blockers",
            plan.inventory.len(),
            plan.drafts.len(),
            plan.blockers.len()
        )
        .map_err(|error| PmError::io("stdout", error))?;
        for blocker in &plan.blockers {
            writeln!(
                stdout,
                "  {}",
                workdeck_diff::sanitize_terminal_line(&blocker.message)
            )
            .map_err(|error| PmError::io("stdout", error))?;
        }
        for notice in &plan.notices {
            writeln!(
                stdout,
                "  {}",
                workdeck_diff::sanitize_terminal_line(&notice.message)
            )
            .map_err(|error| PmError::io("stdout", error))?;
        }
        writeln!(stdout, "Review a saved --plan-out file, then apply it with --apply --plan PATH --request-id ID.").map_err(|error| PmError::io("stdout", error))
    }
}

fn canonical_candidate(path: &Path) -> Result<PathBuf> {
    if path
        .components()
        .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(pm_cli::invalid(
            "migration paths must not contain parent traversal",
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(PmError::new(
            ErrorCode::UnsafePath,
            "migration paths must not be symbolic links",
        )
        .at(path)),
        Ok(_) => path
            .canonicalize()
            .map_err(|error| PmError::io(path, error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or_else(|| pm_cli::invalid("migration path needs an existing parent"))?;
            Ok(canonical_candidate(parent)?.join(
                path.file_name()
                    .ok_or_else(|| pm_cli::invalid("migration path needs a filename"))?,
            ))
        }
        Err(error) => Err(PmError::io(path, error)),
    }
}

fn read_plan(cwd: &Path, path: &Path) -> Result<MigrationPreview> {
    let bytes = if path == Path::new("-") {
        let mut bytes = Vec::new();
        std::io::stdin()
            .lock()
            .take(MAX_PLAN_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| PmError::io(path, error))?;
        if bytes.len() as u64 > MAX_PLAN_BYTES {
            return Err(pm_cli::invalid("migration plan exceeds 256 MiB limit"));
        }
        bytes
    } else {
        pm_cli::read_regular_input(cwd, path, MAX_PLAN_BYTES)?
    };
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        pm_cli::invalid(format!("invalid migration plan JSON: {error}")).at(path)
    })?;
    if value.get("api_version").is_some() {
        if value["api_version"] != 1 || value["ok"] != true || value["kind"] != "migration_preview"
        {
            return Err(pm_cli::invalid(
                "plan envelope must contain a successful API v1 migration preview",
            ));
        }
        value = value["result"].take();
    }
    serde_json::from_value(value)
        .map_err(|error| pm_cli::invalid(format!("invalid migration preview: {error}")).at(path))
}

fn save_plan(path: &Path, plan: &MigrationPreview) -> Result<()> {
    let path = canonical_candidate(path)?;
    if path.starts_with(&plan.source_root) || path.starts_with(&plan.destination_root) {
        return Err(pm_cli::invalid(
            "save migration plans outside both source and destination roots",
        ));
    }
    let bytes =
        serde_json::to_vec_pretty(plan).map_err(|error| pm_cli::invalid(error.to_string()))?;
    if bytes.len() as u64 > MAX_PLAN_BYTES {
        return Err(pm_cli::invalid("migration plan exceeds 256 MiB limit"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                PmError::new(
                    ErrorCode::Conflict,
                    "plan output already exists; preserve it or choose a new file",
                )
                .at(&path)
            } else {
                PmError::io(&path, error)
            }
        })?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| PmError::io(&path, error))?;
    Ok(())
}
