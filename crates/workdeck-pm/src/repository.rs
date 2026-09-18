use crate::{
    Config, ErrorCode, IssueId, OperationId, PmError, RepositoryId, Result, SchemaVersion,
    documents::{MAX_DOCUMENT_BYTES, MarkdownDocument, YamlDocument},
    transactions::{Snapshot, TransactionStore},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(crate) const IGNORE_ENTRIES: &[&str] = &[
    "/.index/",
    "/.tmp/",
    "/.local/",
    "/config.local.yml",
    "/config.local.toml",
    "/settings.local.yml",
];

/// Explicitly selected planning source. No repository-wide singleton or cached
/// mutable configuration is shared between callers.
#[derive(Debug, Clone)]
pub struct Repository {
    root: PathBuf,
    repository: RepositoryId,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub schema: SchemaVersion,
    pub root: PathBuf,
    pub valid: bool,
    /// Structural validity and current organization compliance are distinct.
    #[serde(default)]
    pub policy_compliant: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policy_violations: Vec<crate::PolicyViolation>,
    /// Issue, comment, attachment-descriptor, template, project, cycle, and label
    /// records inspected. An invalid aggregate label catalog counts as one.
    /// Binary payload bytes are not read or integrity-qualified by this report.
    pub checked_records: usize,
    pub errors: Vec<PmError>,
    /// Retained unresolved declarations remain readable but require explicit repair.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<PmError>,
}

impl Repository {
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Captured source identity, available without reading through an unfinished
    /// operation. Every ordinary/recovery operation checks this pin under lock.
    pub fn identity(&self) -> &RepositoryId {
        &self.repository
    }

    /// Resolve the nearest Git/workdeck boundary. Inspection never initializes
    /// authoritative files and never searches through a nested Git boundary.
    pub fn discover(start: &Path) -> Result<Self> {
        let project = project_root(start)?;
        let root = project.join(".workdeck");
        let legacy = has_legacy_store(&project);
        crate::restore::check_root(&root)?;
        let cutover = crate::migration::check_root(&root)?;
        if legacy && cutover.is_none() {
            let code = if root.join("config.yml").exists() {
                ErrorCode::AmbiguousSource
            } else {
                ErrorCode::LegacyStore
            };
            return Err(PmError::new(code, "legacy .agents/workdeck planning data requires an explicit migration or source selection").at(&root)
                .hint("Inspect workdeck migrate before choosing a planning source."));
        }
        Self::open_source(&root)
    }

    /// Select an existing `.workdeck` root explicitly, including when automatic
    /// discovery cannot choose between a legacy source and a new source.
    pub fn open_source(root: &Path) -> Result<Self> {
        Self::open_existing(root, false)
    }

    /// Open a validated, identity-pinned source for explicit operation recovery.
    /// This does not read through or recover an unfinished transaction. Normal
    /// repository operations still refuse that source until recovery completes.
    pub fn open_for_recovery(root: &Path) -> Result<Self> {
        Self::open_existing(root, true)
    }

    fn open_existing(root: &Path, for_recovery: bool) -> Result<Self> {
        reject_symlink(root)?;
        crate::restore::check_root(root)?;
        crate::migration::check_root(root)?;
        if !root.is_dir() || !root.join("config.yml").exists() {
            return Err(PmError::new(
                ErrorCode::NotInitialized,
                "project management is not initialized",
            )
            .at(root)
            .hint("Run workdeck init explicitly."));
        }
        crate::sources::reject_coordination_snapshot(&crate::transactions::Snapshot::new(root))?;
        reject_symlink(&root.join("config.yml"))?;
        // Validate before opening the local lock/index area, so a malformed
        // source can be inspected without an unrelated write being required.
        let config = parse_config(
            &root.join("config.yml"),
            &read_bounded(&root.join("config.yml"))?,
        )?;
        let root = root.canonicalize().map_err(|e| PmError::io(root, e))?;
        let repository = Self {
            root,
            repository: config.repository,
        };
        if !for_recovery {
            repository.config()?;
        }
        Ok(repository)
    }

    /// Initialize only on explicit request. Existing configuration and user
    /// preference files survive. The shared writer inode serializes init and
    /// ordinary writes; a crash before config publication is safely retryable.
    pub fn init(start: &Path, prefix: &str) -> Result<Self> {
        let proposed = Config::new(prefix)?;
        let project = project_root(start)?;
        crate::restore::check_root(&project.join(".workdeck"))?;
        let cutover = crate::migration::check_root(&project.join(".workdeck"))?;
        if has_legacy_store(&project) && cutover.is_none() {
            return Err(PmError::new(ErrorCode::LegacyStore, "legacy planning data exists; initialize through migration to preserve its identities").at(project.join(".agents/workdeck")));
        }
        let root = project.join(".workdeck");
        create_directory(&root)?;
        create_directory(&root.join(".tmp"))?;
        let lock_path = root.join(".tmp/writer.lock");
        require_regular_file_if_present(&lock_path)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|e| PmError::io(&lock_path, e))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(PmError::new(
                        ErrorCode::Locked,
                        "another process is initializing or writing planning data",
                    )
                    .at(&lock_path));
                }
                Err(TryLockError::Error(error)) => return Err(PmError::io(&lock_path, error)),
            }
        }
        // Migration may publish its pending barrier while init waits for the
        // shared writer inode. Recheck admission inside that lock before writes.
        crate::restore::check_root(&root)?;
        crate::migration::check_root(&root)?;
        let config_path = root.join("config.yml");
        reject_symlink(&config_path)?;
        if config_path.exists() {
            let current = parse_config(&config_path, &read_bounded(&config_path)?)?;
            if current.prefix != prefix {
                return Err(PmError::new(
                    ErrorCode::Conflict,
                    "repository is already initialized with a different issue prefix",
                )
                .at(config_path));
            }
        } else {
            validate_uninitialized_contents(&root)?;
        }
        for directory in ["issues", "comments", "operations", ".local", ".index"] {
            create_directory(&root.join(directory))?;
        }
        let ignore_path = root.join(".gitignore");
        reject_symlink(&ignore_path)?;
        let mut ignore = if ignore_path.exists() {
            String::from_utf8(read_bounded(&ignore_path)?).map_err(|_| {
                PmError::new(ErrorCode::InvalidSchema, "PM ignore file must be UTF-8")
                    .at(&ignore_path)
            })?
        } else {
            String::new()
        };
        let original_ignore = ignore.clone();
        for entry in IGNORE_ENTRIES {
            if !ignore.lines().any(|line| line == *entry) {
                if !ignore.is_empty() && !ignore.ends_with('\n') {
                    ignore.push('\n');
                }
                ignore.push_str(entry);
                ignore.push('\n');
            }
        }
        if ignore != original_ignore {
            atomic_init_write(&root, &ignore_path, ignore.as_bytes())?;
        }
        // Publication of config is the initialization marker, after the required
        // local directories/ignores exist. Repeats do not change its identity.
        if !config_path.exists() {
            let yaml = serde_yaml_ng::to_string(&proposed)
                .map_err(|e| PmError::new(ErrorCode::InvalidSchema, e.to_string()))?;
            atomic_init_write(&root, &config_path, yaml.as_bytes())?;
        }
        drop(lock);
        Self::open_source(&root)
    }

    pub fn config(&self) -> Result<Config> {
        self.store()?
            .with_snapshot(|snapshot| config_from_snapshot(&self.root, snapshot))
    }

    pub fn doctor(&self) -> Result<DoctorReport> {
        self.store()?
            .with_snapshot(|snapshot| inspect_snapshot(&self.root, snapshot))
    }

    pub(crate) fn store(&self) -> Result<TransactionStore> {
        Ok(TransactionStore::open(&self.root)?.for_repository(self.repository.clone()))
    }
}

pub(crate) fn inspect_snapshot(root: &Path, snapshot: &Snapshot<'_>) -> Result<DoctorReport> {
    let mut report = DoctorReport {
        schema: SchemaVersion::CURRENT,
        root: root.to_owned(),
        valid: true,
        policy_compliant: true,
        policy_violations: Vec::new(),
        checked_records: 0,
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    let config = config_from_snapshot(root, snapshot)?;
    let (organization_count, organization_errors) =
        crate::organization::inspect_documents(snapshot, &config.repository);
    report.checked_records += organization_count;
    if organization_errors.is_empty() {
        match crate::organization::inspect(root, snapshot, &config) {
            Ok(compliance) => {
                report.policy_compliant = compliance.compliant;
                report.policy_violations = compliance.violations;
            }
            Err(error) => {
                report.policy_compliant = false;
                report.errors.push(error);
            }
        }
    } else {
        report.policy_compliant = false;
        report.errors.extend(organization_errors);
    }
    inspect_issue_records(root, snapshot, &config, &mut report);
    let (graph_records, graph_errors) = crate::graph::inspect(root, snapshot, &config);
    report.checked_records += graph_records;
    report.errors.extend(graph_errors);
    let (time_count, time_errors) = crate::time_entries::inspect_snapshot(root, snapshot, &config);
    report.checked_records += time_count;
    report.errors.extend(time_errors);
    inspect_issue_templates(root, snapshot, &config, &mut report);
    inspect_planning_records(root, snapshot, &mut report);
    match crate::planning::hierarchy::diagnostics(root, snapshot, &config) {
        Ok(warnings) => report.warnings.extend(warnings),
        Err(error) => {
            if !report.errors.iter().any(|existing| {
                existing.code == error.code
                    && existing.path == error.path
                    && existing.message == error.message
            }) {
                report.errors.push(error);
            }
        }
    }
    let (features, feature_errors, feature_warnings) =
        crate::features::inspect(root, snapshot, &config);
    report.checked_records += features;
    report.errors.extend(feature_errors);
    report.warnings.extend(feature_warnings);
    let (views, view_errors, view_warnings) = crate::saved_views::inspect(root, snapshot, &config);
    report.checked_records += views;
    report.errors.extend(view_errors);
    report.warnings.extend(view_warnings);
    let (gates, gate_errors) = crate::gates::store::inspect_snapshot(snapshot, &config);
    report.checked_records += gates;
    report.errors.extend(gate_errors);
    if let Ok(warnings) = crate::gates::diagnostics(root, snapshot, &config) {
        report.warnings.extend(
            warnings
                .into_iter()
                .map(|message| crate::PmError::new(crate::ErrorCode::PolicyBlocked, message)),
        );
    }
    match crate::retained_reviews::load(snapshot, &config.repository) {
        Ok(records) => report.checked_records += records.len(),
        Err(error) => report.errors.push(error),
    }
    match crate::attestations::load(snapshot, &config.repository) {
        Ok(records) => report.checked_records += records.len(),
        Err(error) => report.errors.push(error),
    }
    let (evidence, evidence_errors) = crate::evidence::store::inspect_snapshot(snapshot, &config);
    report.checked_records += evidence;
    report.errors.extend(evidence_errors);
    let (questions, question_errors, question_warnings) =
        crate::questions::inspect(root, snapshot, &config);
    report.checked_records += questions;
    report.errors.extend(question_errors);
    report.warnings.extend(question_warnings);
    let (handoffs, handoff_errors) = crate::handoffs::inspect(root, snapshot, &config);
    report.checked_records += handoffs;
    report.errors.extend(handoff_errors);
    let (execution_catalog, catalog_errors) = crate::commands::catalog::scan(snapshot, &config);
    report.checked_records += execution_catalog.commands.len()
        + execution_catalog.checks.len()
        + execution_catalog.profiles.len()
        + catalog_errors.len();
    report.errors.extend(catalog_errors);
    match crate::execution::records::load_runs(root, snapshot) {
        Ok(runs) => {
            report.checked_records += runs
                .iter()
                .map(|(_, result)| 1 + usize::from(result.is_some()))
                .sum::<usize>()
        }
        Err(error) => {
            report.checked_records += 1;
            report.errors.push(error);
        }
    }
    let (wiki, wiki_errors) = crate::wiki::inspect_snapshot(snapshot);
    match crate::claims::load_claims(snapshot, &config) {
        Ok(claims) => report.checked_records += claims.len(),
        Err(error) => {
            report.checked_records += 1;
            report.errors.push(error);
        }
    }
    report.checked_records += wiki;
    report.errors.extend(wiki_errors);
    let (retirements, retirement_errors) = crate::retirement::doctor(root, snapshot, &config);
    report.checked_records += retirements;
    report.errors.extend(retirement_errors);
    let (history, history_errors) =
        crate::history::inspect_snapshot(root, snapshot, &config.repository);
    report.checked_records += history;
    report.errors.extend(history_errors);
    let (exports, export_errors) = crate::snapshots::inspect_export_artifacts(root, snapshot);
    report.checked_records += exports;
    report.errors.extend(export_errors);
    for path in doctor_listing(snapshot, Path::new("comments"), &mut report, root) {
        report.checked_records += 1;
        doctor_error(
            &mut report,
            root,
            &path,
            PmError::new(
                ErrorCode::InvalidSchema,
                "comment records belong under issues/<issue>/comments, not the planning root",
            ),
        );
    }
    report.valid = report.errors.is_empty();
    Ok(report)
}

// Every supported independent record is inspected in the same source snapshot.
// Item parse failures do not hide comments, attachment descriptors, or templates.
fn inspect_issue_records(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    report: &mut DoctorReport,
) {
    let mut groups = BTreeMap::<String, Vec<PathBuf>>::new();
    for path in doctor_listing(snapshot, Path::new("issues"), report, root) {
        let components = path.components().collect::<Vec<_>>();
        let id = components.get(1).and_then(|part| part.as_os_str().to_str());
        if components.len() < 3 || id.is_none() {
            report.checked_records += 1;
            doctor_error(
                report,
                root,
                &path,
                PmError::new(
                    ErrorCode::InvalidSchema,
                    "issue records belong in issues/<issue>/item.md",
                ),
            );
            continue;
        }
        groups
            .entry(id.expect("checked above").into())
            .or_default()
            .push(path);
    }
    for (id, files) in groups {
        let directory = PathBuf::from(format!("issues/{id}"));
        let issue: IssueId = match id.parse() {
            Ok(id) => id,
            Err(error) => {
                report.checked_records += 1;
                doctor_error(
                    report,
                    root,
                    &directory,
                    PmError::new(
                        ErrorCode::InvalidSchema,
                        format!("issue directory identity is invalid: {error}"),
                    ),
                );
                continue;
            }
        };
        let item_path = directory.join("item.md");
        let item = (|| -> Result<()> {
            let bytes = snapshot
                .read_bounded(&item_path, MAX_DOCUMENT_BYTES)?
                .ok_or_else(|| {
                    PmError::new(
                        ErrorCode::CorruptStore,
                        "independent issue records have no parent item.md",
                    )
                    .at(&item_path)
                })?;
            report.checked_records += 1;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| PmError::new(ErrorCode::InvalidSchema, "issue must be UTF-8"))?;
            let absolute = root.join(&item_path);
            let document = MarkdownDocument::parse(&absolute, text)?;
            let metadata = crate::issues::parse_issue_metadata(&absolute, &document)?;
            metadata.validate(config)?;
            crate::retirement::validate_associations(root, snapshot, config, &metadata)?;
            if metadata.id != issue {
                return Err(PmError::new(
                    ErrorCode::InvalidSchema,
                    "issue directory and identity disagree",
                ));
            }
            Ok(())
        })();
        if let Err(error) = item {
            doctor_error(report, root, &item_path, error);
        }
        let comment_prefix = directory.join("comments");
        let attachment_prefix = directory.join("attachments");
        let mut attachments = BTreeMap::<IssueId, Vec<PathBuf>>::new();
        for path in files {
            if path == item_path {
                continue;
            }
            if path.starts_with(&comment_prefix) {
                report.checked_records += 1;
                if let Err(error) = crate::issues::load_comment(root, snapshot, &path, &issue) {
                    doctor_error(report, root, &path, error);
                }
            } else if path.starts_with(directory.join("time"))
                || path.starts_with(directory.join("handoffs"))
            {
                // Time records and their cross-issue identity/amendment graph
                // are inspected independently in the same snapshot above.
                continue;
            } else if path.starts_with(&attachment_prefix) {
                let relative = path
                    .strip_prefix(&attachment_prefix)
                    .expect("prefix checked");
                let id = relative
                    .components()
                    .next()
                    .and_then(|part| part.as_os_str().to_str())
                    .ok_or_else(|| {
                        PmError::new(
                            ErrorCode::InvalidSchema,
                            "attachment has no directory identity",
                        )
                    })
                    .and_then(crate::attachments::parse_attachment_id);
                match id {
                    Ok(id) => attachments.entry(id).or_default().push(path),
                    Err(error) => {
                        report.checked_records += 1;
                        doctor_error(
                            report,
                            root,
                            &path,
                            PmError::new(ErrorCode::InvalidSchema, error.message),
                        );
                    }
                }
            } else {
                report.checked_records += 1;
                doctor_error(
                    report,
                    root,
                    &path,
                    PmError::new(
                        ErrorCode::InvalidSchema,
                        "unexpected file outside the issue item, comments, or declared attachment layout",
                    ),
                );
            }
        }
        for (id, files) in attachments {
            report.checked_records += 1;
            if let Err(error) = crate::attachments::load_attachment(snapshot, &issue, &id, &files) {
                doctor_error(report, root, &attachment_prefix.join(id.as_str()), error);
            }
        }
    }
}

fn inspect_issue_templates(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    report: &mut DoctorReport,
) {
    for path in doctor_listing(snapshot, Path::new("templates/issues"), report, root) {
        if path.extension().is_none_or(|extension| extension != "md") {
            continue;
        }
        report.checked_records += 1;
        let id = path.file_stem().and_then(|stem| stem.to_str());
        if path.components().count() != 3 || id.is_none() {
            doctor_error(
                report,
                root,
                &path,
                PmError::new(
                    ErrorCode::InvalidSchema,
                    "issue templates belong directly in templates/issues/<slug>.md",
                ),
            );
            continue;
        }
        if let Err(error) = crate::templates::load_template_structural(
            root,
            snapshot,
            config,
            id.expect("checked above"),
        ) {
            doctor_error(report, root, &path, error);
        }
    }
}

fn doctor_listing(
    snapshot: &Snapshot<'_>,
    prefix: &Path,
    report: &mut DoctorReport,
    root: &Path,
) -> Vec<PathBuf> {
    match snapshot.list(prefix) {
        Ok(paths) => paths,
        Err(error) => {
            doctor_error(report, root, prefix, error);
            Vec::new()
        }
    }
}

fn inspect_planning_records(root: &Path, snapshot: &Snapshot<'_>, report: &mut DoctorReport) {
    use crate::{PlanningKind, planning::store};
    for (kind, directory) in [
        (PlanningKind::Initiative, "initiatives"),
        (PlanningKind::Project, "projects"),
        (PlanningKind::Milestone, "milestones"),
        (PlanningKind::Cycle, "cycles"),
        (PlanningKind::Target, "targets"),
    ] {
        for path in doctor_listing(snapshot, Path::new(directory), report, root) {
            report.checked_records += 1;
            let id = path
                .parent()
                .and_then(Path::file_name)
                .and_then(|part| part.to_str());
            if path.components().count() != 3
                || path.file_name().is_none_or(|name| name != "item.md")
                || id.is_none()
            {
                doctor_error(
                    report,
                    root,
                    &path,
                    PmError::new(
                        ErrorCode::InvalidSchema,
                        "planning records belong in <kind>/<id>/item.md",
                    ),
                );
                continue;
            }
            if let Err(error) =
                store::load_planning(root, snapshot, kind, id.expect("checked above"))
            {
                doctor_error(report, root, &path, error);
            }
        }
    }
    let path = Path::new("labels.yml");
    match snapshot.read_bounded(path, MAX_DOCUMENT_BYTES) {
        Ok(None) => {}
        Ok(Some(_)) => match store::list_planning(root, snapshot, PlanningKind::Label) {
            Ok(labels) => report.checked_records += labels.len(),
            Err(error) => {
                report.checked_records += 1;
                doctor_error(report, root, path, error);
            }
        },
        Err(error) => {
            report.checked_records += 1;
            doctor_error(report, root, path, error);
        }
    }
}

fn doctor_error(report: &mut DoctorReport, root: &Path, fallback: &Path, error: PmError) {
    let path = error.path.as_deref().map(Path::new).unwrap_or(fallback);
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    };
    report.errors.push(error.at(absolute));
}

pub(crate) fn config_from_snapshot(root: &Path, snapshot: &Snapshot) -> Result<Config> {
    let bytes = snapshot.read(Path::new("config.yml"))?.ok_or_else(|| {
        PmError::new(
            ErrorCode::CorruptStore,
            "initialized repository has no PM config",
        )
        .at(root.join("config.yml"))
    })?;
    parse_config(&root.join("config.yml"), &bytes)
}

pub(crate) fn parse_config(path: &Path, bytes: &[u8]) -> Result<Config> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| PmError::new(ErrorCode::InvalidSchema, "PM config must be UTF-8").at(path))?;
    let document = YamlDocument::parse(path, text)?;
    // Preserve the structured unsupported-schema category before serde's
    // diagnostic conversion erases the typed error.
    if let Some(version) = document
        .metadata()
        .get("schema")
        .and_then(serde_yaml_ng::Value::as_u64)
    {
        SchemaVersion::try_from(version).map_err(|error| error.at(path))?;
    }
    let config: Config = document.deserialize()?;
    config.validate().map_err(|error| error.at(path))?;
    Ok(config)
}

pub(crate) fn project_root(start: &Path) -> Result<PathBuf> {
    let start = start.canonicalize().map_err(|e| PmError::io(start, e))?;
    let start = if start.is_file() {
        start.parent().unwrap_or(&start).to_owned()
    } else {
        start
    };
    // A nested fixture or abandoned `.workdeck` directory is not a repository
    // boundary. Explicit `open_source` remains available for intentionally
    // selecting it. The nearest Git boundary also preserves nested repositories.
    if let Some(git_root) = start
        .ancestors()
        .find(|candidate| candidate.join(".git").exists())
    {
        return Ok(git_root.to_owned());
    }
    for candidate in start.ancestors() {
        if candidate.join(".workdeck").exists() || has_legacy_store(candidate) {
            return Ok(candidate.to_owned());
        }
    }
    Ok(start)
}

pub(crate) fn has_legacy_store(project: &Path) -> bool {
    let root = project.join(".agents/workdeck");
    root.join("issues").exists()
        || root.join("projects.toml").exists()
        || root.join("cycles.toml").exists()
        || root.join("labels.toml").exists()
}

fn reject_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(PmError::new(
            ErrorCode::UnsafePath,
            "planning paths must not be symbolic links",
        )
        .at(path)),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PmError::io(path, error)),
    }
}

fn create_directory(path: &Path) -> Result<()> {
    reject_symlink(path)?;
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(error) => Err(PmError::io(path, error)),
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    // Opening a FIFO for reading blocks before File::metadata can diagnose it.
    // Reject static special files first, then verify the opened descriptor too.
    require_regular_file_if_present(path)?;
    let file = File::open(path).map_err(|e| PmError::io(path, e))?;
    if !file.metadata().map_err(|e| PmError::io(path, e))?.is_file() {
        return Err(PmError::new(
            ErrorCode::UnsafePath,
            "planning input must be a regular file",
        )
        .at(path));
    }
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| PmError::io(path, e))?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(
            PmError::new(ErrorCode::InvalidSchema, "planning document exceeds 2 MiB").at(path),
        );
    }
    Ok(bytes)
}

fn require_regular_file_if_present(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => Err(PmError::new(
            ErrorCode::UnsafePath,
            "planning input must be a regular file, not a symlink or special file",
        )
        .at(path)),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PmError::io(path, error)),
    }
}

/// Missing config is retryable only when no unexplained planning records exist.
/// Empty directories and unpublished init staging bytes do not establish an
/// accepted repository identity. Issues, receipts, or transaction journals do.
fn validate_uninitialized_contents(root: &Path) -> Result<()> {
    let mut directories = vec![root.to_owned()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory).map_err(|error| PmError::io(&directory, error))? {
            let entry = entry.map_err(|error| PmError::io(&directory, error))?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("entry belongs to planning root");
            let metadata =
                fs::symlink_metadata(&path).map_err(|error| PmError::io(&path, error))?;
            if metadata.is_dir() {
                directories.push(path);
                continue;
            }
            if !metadata.is_file() {
                return Err(PmError::new(
                    ErrorCode::UnsafePath,
                    "uninitialized planning source contains a symlink or special file",
                )
                .at(&path));
            }
            let preserved_preference = relative.components().count() == 1
                && matches!(
                    relative.to_str(),
                    Some(
                        "config.toml"
                            | ".gitignore"
                            | "config.local.toml"
                            | "config.local.yml"
                            | "settings.local.yml"
                    )
                );
            let init_local_file = relative == Path::new(".tmp/writer.lock")
                || (relative.parent() == Some(Path::new(".tmp"))
                    && relative
                        .file_name()
                        .and_then(|name| name.to_str())
                        .and_then(|name| name.strip_prefix("init-"))
                        .is_some_and(|id| id.parse::<OperationId>().is_ok()));
            if !preserved_preference && !init_local_file {
                return Err(PmError::new(
                    ErrorCode::RecoveryRequired,
                    "planning records exist without config.yml; initialization cannot assign a new repository identity",
                ).at(&path).hint("Restore the original configuration or inspect the interrupted migration/operation before initializing; preserve the existing records."));
            }
        }
    }
    Ok(())
}

fn atomic_init_write(root: &Path, target: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = root
        .join(".tmp")
        .join(format!("init-{}", OperationId::new()));
    let mut file = File::create_new(&temporary).map_err(|e| PmError::io(&temporary, e))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| PmError::io(&temporary, e))?;
    reject_symlink(target)?;
    fs::rename(&temporary, target).map_err(|e| PmError::io(target, e))?;
    #[cfg(unix)]
    File::open(root)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| PmError::io(root, e))?;
    Ok(())
}
