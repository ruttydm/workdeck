use super::*;
use crate::bounded_files;
use std::collections::BTreeMap;
use workdeck_pm::RecordedSession;

pub(super) fn load(source: &PlanningSource) -> Result<Vec<RecordedSession>, PanelError> {
    let (root, directory) = match source {
        PlanningSource::Native(repository) => return load_native(repository),
        PlanningSource::Legacy(root) => (root.clone(), "agents"),
        PlanningSource::Empty => return Ok(Vec::new()),
    };
    let entries = match bounded_files::list(&root, Path::new(directory), MAX_ENTRIES) {
        Ok((_, true)) => {
            return Err(PanelError::new(
                "Recorded sessions exceed the 10,000-entry enumeration limit",
            ));
        }
        Ok((entries, false)) => entries,
        Err(failure)
            if failure
                .chain()
                .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
                .any(|cause| cause.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(Vec::new());
        }
        Err(failure) => return Err(error(failure)),
    };
    let mut sessions = Vec::new();
    let mut total_bytes = 0usize;
    for item in entries {
        let name = item.name;
        if Path::new(&name).extension().is_none_or(|ext| ext != "toml") {
            continue;
        }
        if item.kind != bounded_files::Kind::File {
            return Err(PanelError::new(format!(
                "Recorded session {name} must be an ordinary file"
            )));
        }
        let contents = files::read(&root, &format!("{directory}/{name}"), 2 * 1024 * 1024)?;
        if contents.truncated {
            return Err(PanelError::new(format!(
                "Recorded session {name} exceeds 2 MiB"
            )));
        }
        total_bytes += contents.bytes.len();
        if total_bytes > 64 * 1024 * 1024 {
            return Err(PanelError::new(
                "Recorded sessions exceed the 64 MiB preview budget",
            ));
        }
        let session: RecordedSession =
            toml::from_str(std::str::from_utf8(&contents.bytes).map_err(error)?).map_err(error)?;
        files::relative(&session.id, false)?;
        if session.id.contains('/')
            || session.title.trim().is_empty()
            || name != format!("{}.toml", session.id)
        {
            return Err(PanelError::new(format!(
                "Recorded session {name} has an invalid identity or title"
            )));
        }
        sessions.push(session);
    }
    sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at).then(a.id.cmp(&b.id)));
    Ok(sessions)
}

fn load_native(repository: &Repository) -> Result<Vec<RecordedSession>, PanelError> {
    let files = capture_native(repository.root())?;
    let records = workdeck_pm::validate_recorded_sessions_snapshot(
        repository.root(),
        repository.identity(),
        &files,
    )
    .map_err(pm_error)?;
    // Wait for any cooperating writer/recovery barrier before the second
    // capture. Two equal captures made during one paused write are insufficient.
    repository.config().map_err(pm_error)?;
    // A complete second capture checks membership and contents, including
    // read-only receipt dependencies. Direct edits never yield a mixed view.
    if capture_native(repository.root())? != files {
        return Err(PanelError::new(
            "Recorded session source changed during inspection; refresh the panel",
        ));
    }
    Ok(records.into_iter().map(|record| record.session).collect())
}

/// Capture only the canonical files needed by the shared immutable history
/// validator. Descriptor opens keep special files and parent swaps from
/// redirecting a read; the provider independently pins repository admission.
fn capture_native(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>, PanelError> {
    const TOTAL_BYTES: usize = 64 * 1024 * 1024;
    const RECORD_BYTES: usize = 2 * 1024 * 1024;
    let mut files = BTreeMap::new();
    let mut total_bytes = 0;
    let mut count = 1; // config.yml also belongs to the shared snapshot budget.
    let mut capture = |path: PathBuf, limit: usize| -> Result<(), PanelError> {
        let contents = bounded_files::read(root, &path, limit.min(TOTAL_BYTES - total_bytes))
            .map_err(error)?;
        if contents.truncated {
            return Err(PanelError::new(format!(
                "Recorded history source {} exceeds its file limit or the 64 MiB snapshot budget",
                path.display()
            )));
        }
        total_bytes += contents.bytes.len();
        files.insert(path, contents.bytes);
        Ok(())
    };
    capture(PathBuf::from("config.yml"), TOTAL_BYTES)?;
    for (directory, file_limit) in [
        ("imported-sessions", RECORD_BYTES),
        ("imported-history/deleted-sessions", RECORD_BYTES),
        ("operations", TOTAL_BYTES),
    ] {
        let entries = match bounded_files::list(root, Path::new(directory), MAX_ENTRIES - count) {
            Ok((_, true)) => {
                return Err(PanelError::new(
                    "Recorded history exceeds the 10,000-entry snapshot limit",
                ));
            }
            Ok((entries, false)) => entries,
            Err(failure)
                if failure
                    .chain()
                    .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
                    .any(|cause| cause.kind() == std::io::ErrorKind::NotFound) =>
            {
                continue;
            }
            Err(failure) => return Err(error(failure)),
        };
        count += entries.len();
        for entry in entries {
            if entry.kind != bounded_files::Kind::File {
                return Err(PanelError::new(format!(
                    "Recorded history source {directory}/{} must be an ordinary file",
                    entry.name
                )));
            }
            capture(Path::new(directory).join(entry.name), file_limit)?;
        }
    }
    Ok(files)
}

pub(super) fn list(
    source: &PlanningSource,
    request: &PanelRequest,
) -> Result<PanelSnapshot, PanelError> {
    let query = request.query.trim().to_lowercase();
    let mut entries = load(source)?
        .into_iter()
        .filter(|session| {
            format!(
                "{} {} {} {} {} {}",
                session.id,
                session.title,
                session.agent,
                session.status,
                session.goal,
                session.summary
            )
            .to_lowercase()
            .contains(&query)
        })
        .map(|session| {
            entry(
                format!("agent:{}", session.id),
                format!("{} {}", session.id, session.title),
                format!("recorded · {} · {}", session.agent, session.status),
                "Recorded sessions",
                PanelTarget::AgentSession { id: session.id },
            )
        })
        .collect::<Vec<_>>();
    let truncated = entries.len() > limit(request);
    entries.truncate(limit(request));
    Ok(PanelSnapshot {
        page: PanelPage::Agents,
        title: "Recorded agents".into(),
        summary: format!("{} records · historical annotations", entries.len()),
        entries,
        truncated,
    })
}

pub(super) fn preview(source: &PlanningSource, id: &str) -> Result<PanelPreview, PanelError> {
    let session = load(source)?
        .into_iter()
        .find(|session| session.id == id)
        .ok_or_else(|| PanelError::new("Recorded session no longer exists"))?;
    Ok(text_preview(
        format!("{} {}", session.id, session.title),
        format!(
            "Recorded session history. Status, commands, tests, and handoff notes are annotations from the producer.\n\n{}",
            toml::to_string_pretty(&session).map_err(error)?
        ),
        PanelPreviewKind::Text,
    ))
}

pub(super) fn legacy_reference_preview(
    source: &PlanningSource,
    target: &PanelTarget,
) -> Result<PanelPreview, PanelError> {
    let PlanningSource::Legacy(root) = source else {
        return Err(PanelError::new("Project management is not initialized"));
    };
    let references = WorkdeckStore::new(root)
        .load_reference_data()
        .map_err(error)?;
    let value = match target {
        PanelTarget::Project { id } => references
            .projects
            .into_iter()
            .find(|record| record.id == *id)
            .map(serde_json::to_value),
        PanelTarget::Cycle { id } => references
            .cycles
            .into_iter()
            .find(|record| record.id == *id)
            .map(serde_json::to_value),
        PanelTarget::Label { id } => references
            .labels
            .into_iter()
            .find(|record| record.id == *id)
            .map(serde_json::to_value),
        _ => None,
    }
    .ok_or_else(|| PanelError::new("Planning reference no longer exists"))?
    .map_err(error)?;
    Ok(text_preview(
        value["name"]
            .as_str()
            .unwrap_or("Planning reference")
            .into(),
        serde_json::to_string_pretty(&value).map_err(error)?,
        PanelPreviewKind::Text,
    ))
}
