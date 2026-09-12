//! Explicit, local-only publication of a thin repository instruction pointer.
//! This coordinator does not create canonical PM receipts or stage Git files.
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::fs::Permissions;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use workdeck_pm::{
    ContentHash, ErrorCode, PmError, Repository, RepositoryId, RequestId, Result,
    transactions::TransactionStore,
};
const MAX_AGENTS: usize = 128 * 1024;
const MAX_PROOF: usize = 2 * 1024 * 1024;
const START: &str = "<!-- workdeck:pm-protocol:start -->";
const END: &str = "<!-- workdeck:pm-protocol:end -->";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mode {
    Install,
    Update,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProtocolPreview {
    pub schema_version: u32,
    pub repository: RepositoryId,
    pub project_root: PathBuf,
    pub target: PathBuf,
    pub mode: Mode,
    pub expected_content: Option<ContentHash>,
    pub after: ContentHash,
    pub changed: bool,
    pub previous_pointer: Option<String>,
    pub proposed_pointer: String,
    pub scope: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProtocolReceipt {
    pub schema_version: u32,
    pub repository: RepositoryId,
    pub project_root: PathBuf,
    pub target: PathBuf,
    pub request_id: RequestId,
    pub mode: Mode,
    pub before: Option<ContentHash>,
    pub after: ContentHash,
    pub changed: bool,
    pub scope: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Proof {
    receipt: ProtocolReceipt,
    input: ContentHash,
    before: Option<String>,
    after: String,
    pointer_version: u32,
    permissions: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FaultPoint {
    BeforeJournal,
    AfterJournal,
    BeforePublish,
    AfterPublish,
    AfterReceipt,
}
fn fail(code: ErrorCode, message: impl Into<String>) -> PmError {
    PmError::new(code, message)
}
fn invalid(message: impl Into<String>) -> PmError {
    fail(ErrorCode::InvalidInput, message)
}
fn pointer(newline: &str) -> String {
    format!(
        "{START}{newline}For project-management operations, load the Workdeck PM skill returned by `workdeck skill path workdeck-pm`.{newline}{END}{newline}"
    )
}
fn span(text: &str) -> Result<Option<(usize, usize)>> {
    let mut start = None;
    let mut end = None;
    let mut offset = 0usize;
    let mut fence: Option<(u8, usize)> = None;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        let trimmed = bare.trim_start();
        if let Some(marker @ (b'`' | b'~')) = trimmed.as_bytes().first().copied() {
            let count = trimmed.bytes().take_while(|byte| *byte == marker).count();
            if count >= 3 {
                if let Some((opening, length)) = fence {
                    if marker == opening && count >= length && trimmed[count..].trim().is_empty() {
                        fence = None;
                    }
                } else if marker != b'`' || !trimmed[count..].contains('`') {
                    fence = Some((marker, count));
                }
            }
        }
        if bare.contains(START) || bare.contains(END) {
            if fence.is_some() || !matches!(bare, START | END) {
                return Err(invalid(
                    "protocol markers must be unindented whole lines outside code fences",
                ));
            }
            if bare == START {
                if start.is_some() || end.is_some() {
                    return Err(invalid("duplicate or reversed protocol markers"));
                }
                start = Some(offset);
            } else {
                if start.is_none() || end.is_some() {
                    return Err(invalid("duplicate or reversed protocol markers"));
                }
                end = Some(offset + line.len());
            }
        }
        offset += line.len();
    }
    if fence.is_some() {
        return Err(invalid(
            "close the existing Markdown code fence before publishing a protocol pointer",
        ));
    }
    match (start, end) {
        (None, None) => Ok(None),
        (Some(a), Some(b)) => Ok(Some((a, b))),
        _ => Err(invalid(
            "incomplete protocol marker pair; repair the managed block before updating",
        )),
    }
}
fn document(before: Option<&str>, mode: Mode) -> Result<(String, Option<String>, String)> {
    let before = before.unwrap_or("");
    let newline = if before.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let block = pointer(newline);
    let range = span(before)?;
    let (after, previous) = match (range, mode) {
        (Some((a, b)), Mode::Install) => {
            let old = &before[a..b];
            if old != block {
                return Err(fail(
                    ErrorCode::Conflict,
                    "existing protocol pointer differs; preview and use protocol update",
                ));
            }
            (before.into(), Some(old.into()))
        }
        (Some((a, b)), Mode::Update) => (
            format!("{}{}{}", &before[..a], block, &before[b..]),
            Some(before[a..b].into()),
        ),
        (None, Mode::Update) => {
            return Err(fail(
                ErrorCode::NotFound,
                "no managed protocol pointer exists; use protocol install",
            ));
        }
        (None, Mode::Install) => {
            let mut after = before.to_owned();
            if !after.is_empty() {
                if !after.ends_with('\n') {
                    after.push_str(newline);
                }
                after.push_str(newline);
            }
            after.push_str(&block);
            (after, None)
        }
    };
    if after.len() > MAX_AGENTS {
        return Err(invalid(
            "repository instructions with pointer exceed 128 KiB",
        ));
    }
    let (start, end) =
        span(&after)?.ok_or_else(|| invalid("generated pointer is not outside a code fence"))?;
    if after[start..end] != block {
        return Err(invalid("generated protocol pointer is ambiguous"));
    }
    Ok((after, previous, block))
}
fn safe_path(root: &Path, path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(root).map_err(|error| PmError::io(root, error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(fail(
            ErrorCode::UnsafePath,
            "selected repository root must remain a real directory",
        )
        .at(root));
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| invalid("protocol path is outside selected repository"))?;
    let mut current = root.to_path_buf();
    for part in relative.components() {
        let Component::Normal(part) = part else {
            return Err(invalid("invalid protocol path component"));
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(fail(
                        ErrorCode::UnsafePath,
                        "protocol paths cannot contain symlinks",
                    )
                    .at(&current));
                }
                if current != path && !meta.is_dir() {
                    return Err(
                        fail(ErrorCode::UnsafePath, "protocol parent must be a directory")
                            .at(&current),
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(PmError::io(&current, e)),
        }
    }
    Ok(())
}
fn open_options(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
}
fn read_optional(root: &Path, path: &Path, max: usize) -> Result<Option<Vec<u8>>> {
    safe_path(root, path)?;
    match fs::symlink_metadata(path) {
        Ok(m) if !m.is_file() => {
            return Err(fail(
                ErrorCode::UnsafePath,
                "protocol source must be a regular file",
            )
            .at(path));
        }
        Ok(m) if m.len() > max as u64 => {
            return Err(invalid("protocol file exceeds supported size").at(path));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(PmError::io(path, e)),
    }
    let mut options = OpenOptions::new();
    options.read(true);
    open_options(&mut options);
    let file = options.open(path).map_err(|e| PmError::io(path, e))?;
    if !file.metadata().map_err(|e| PmError::io(path, e))?.is_file() {
        return Err(fail(
            ErrorCode::UnsafePath,
            "protocol source must be a regular file",
        )
        .at(path));
    }
    let mut bytes = Vec::new();
    file.take((max + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| PmError::io(path, e))?;
    if bytes.len() > max {
        return Err(invalid("protocol file exceeds supported size").at(path));
    }
    Ok(Some(bytes))
}
fn text_optional(root: &Path, path: &Path) -> Result<Option<String>> {
    read_optional(root, path, MAX_AGENTS)?
        .map(|b| {
            let text = String::from_utf8(b).map_err(|_| invalid("AGENTS.md must be UTF-8"))?;
            if text.contains('\0') {
                return Err(invalid("AGENTS.md contains NUL"));
            }
            Ok(text)
        })
        .transpose()
}
fn hash(text: Option<&str>) -> Option<ContentHash> {
    text.map(|s| ContentHash::of(s.as_bytes()))
}
fn input_hash(
    repository: &RepositoryId,
    root: &Path,
    mode: Mode,
    expected: &Option<ContentHash>,
) -> Result<ContentHash> {
    let bytes = serde_json::to_vec(&(
        "workdeck.local-protocol.v1",
        repository,
        root,
        "AGENTS.md",
        mode,
        expected,
    ))
    .map_err(|error| invalid(error.to_string()))?;
    Ok(ContentHash::of(&bytes))
}
fn context<T: Serialize>(
    cwd: &Path,
    mutating: bool,
    run: impl FnOnce(&Repository, &Path, &ContentHash) -> Result<T>,
) -> Result<T> {
    let repository = Repository::discover(cwd)?;
    let root = repository
        .root()
        .parent()
        .ok_or_else(|| invalid("native planning root has no repository parent"))?
        .to_path_buf();
    let mut completed = None;
    let result = TransactionStore::open(repository.root())?.with_snapshot(|snapshot| {
        let bytes = snapshot
            .read_bounded(
                Path::new("config.yml"),
                workdeck_pm::documents::MAX_DOCUMENT_BYTES,
            )?
            .ok_or_else(|| fail(ErrorCode::NotInitialized, "configuration disappeared"))?;
        let text =
            std::str::from_utf8(&bytes).map_err(|_| invalid("configuration must be UTF-8"))?;
        let config: workdeck_pm::Config =
            workdeck_pm::documents::YamlDocument::parse(Path::new("config.yml"), text)?
                .deserialize()?;
        config.validate()?;
        if &config.repository != repository.identity() {
            return Err(fail(
                ErrorCode::StaleSource,
                "repository identity changed before protocol publication",
            ));
        }
        let result = run(&repository, &root, &ContentHash::of(&bytes))?;
        if mutating {
            completed =
                Some(serde_json::to_value(&result).map_err(|error| invalid(error.to_string()))?);
        }
        Ok(result)
    });
    result.map_err(|mut error| {
        let mut details = error
            .details
            .take()
            .map(|v| *v)
            .unwrap_or_else(|| serde_json::json!({}));
        if !details.is_object() {
            details = serde_json::json!({"cause":details});
        }
        if let Some(result) = completed {
            details["request_effect_recorded"] = serde_json::json!(true);
            details["local_protocol_receipt"] = result;
        }
        details["repository"] = serde_json::json!(repository.identity());
        details["project_root"] = serde_json::json!(root);
        error.details = Some(Box::new(details));
        error
    })
}
fn directory(root: &Path) -> PathBuf {
    root.join(".workdeck/.local/protocol")
}
fn journal(root: &Path) -> PathBuf {
    directory(root).join("journal.json")
}
fn receipt_path(root: &Path, id: &RequestId) -> PathBuf {
    directory(root).join("requests").join(format!("{id}.json"))
}
fn decode(root: &Path, path: &Path) -> Result<Option<Proof>> {
    read_optional(root, path, MAX_PROOF)?
        .map(|bytes| {
            serde_json::from_slice(&bytes).map_err(|e| {
                fail(
                    ErrorCode::CorruptStore,
                    format!("invalid local protocol record: {e}"),
                )
                .at(path)
            })
        })
        .transpose()
}
fn validate_proof(proof: &Proof, repository: &RepositoryId, root: &Path) -> Result<()> {
    let r = &proof.receipt;
    let expected = hash(proof.before.as_deref());
    if r.schema_version != 1
        || proof.pointer_version != 1
        || &r.repository != repository
        || r.project_root != root
        || r.target != Path::new("AGENTS.md")
        || r.scope != "local_protocol_pointer"
        || r.before != expected
        || r.after != ContentHash::of(proof.after.as_bytes())
        || r.changed != (proof.before.as_deref() != Some(proof.after.as_str()))
        || proof.input != input_hash(repository, root, r.mode, &expected)?
        || proof.before.as_ref().is_some_and(|s| s.len() > MAX_AGENTS)
        || proof.after.len() > MAX_AGENTS
        || proof.permissions.is_some() != proof.before.is_some()
    {
        return Err(fail(
            ErrorCode::CorruptStore,
            "local protocol receipt identity or source proof is inconsistent",
        ));
    }
    if document(proof.before.as_deref(), r.mode)?.0 != proof.after {
        return Err(fail(
            ErrorCode::CorruptStore,
            "local protocol record alters bytes outside its managed pointer",
        ));
    }
    Ok(())
}
fn pending(root: &Path, repository: &RepositoryId) -> Result<Option<Proof>> {
    let proof = decode(root, &journal(root))?;
    if let Some(p) = &proof {
        validate_proof(p, repository, root)?;
    }
    Ok(proof)
}
fn require_journal(root: &Path, proof: &Proof) -> Result<()> {
    let expected = serde_json::to_vec(proof).map_err(|error| invalid(error.to_string()))?;
    if read_optional(root, &journal(root), MAX_PROOF)?.as_deref() != Some(expected.as_slice()) {
        return Err(fail(
            ErrorCode::StaleSource,
            "local protocol journal changed or disappeared; publication stopped",
        )
        .at(journal(root)));
    }
    Ok(())
}
pub(super) fn preview(cwd: &Path, mode: Mode) -> Result<ProtocolPreview> {
    context(cwd, false, |repo, root, _source| {
        if let Some(p) = pending(root, repo.identity())? {
            return Err(recovery(&p));
        }
        let before = text_optional(root, &root.join("AGENTS.md"))?;
        let (after, previous, proposed) = document(before.as_deref(), mode)?;
        Ok(ProtocolPreview {
            schema_version: 1,
            repository: repo.identity().clone(),
            project_root: root.into(),
            target: "AGENTS.md".into(),
            mode,
            expected_content: hash(before.as_deref()),
            after: ContentHash::of(after.as_bytes()),
            changed: before.as_deref() != Some(after.as_str()),
            previous_pointer: previous,
            proposed_pointer: proposed,
            scope: "local_protocol_pointer".into(),
        })
    })
}
fn recovery(proof: &Proof) -> PmError {
    fail(ErrorCode::RecoveryRequired,"an interrupted protocol publication must be resumed with its original request and source precondition").details(serde_json::json!({"repository":proof.receipt.repository,"request_id":proof.receipt.request_id,"mode":proof.receipt.mode,"expected_content":proof.receipt.before,"scope":"local_protocol_pointer"}))
}
fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|e| PmError::io(path, e))?;
    }
    let _ = path;
    Ok(())
}
fn ensure_directory(root: &Path, path: &Path) -> Result<()> {
    safe_path(root, path)?;
    fs::create_dir_all(path).map_err(|e| PmError::io(path, e))?;
    safe_path(root, path)?;
    if !fs::symlink_metadata(path)
        .map_err(|e| PmError::io(path, e))?
        .is_dir()
    {
        return Err(fail(
            ErrorCode::UnsafePath,
            "local protocol parent must be a directory",
        )
        .at(path));
    }
    Ok(())
}
fn ensure_local(root: &Path) -> Result<()> {
    let dir = directory(root);
    ensure_directory(root, &dir)?;
    let ignore = dir.join(".gitignore");
    match read_optional(root, &ignore, 4096)? {
        Some(bytes) => {
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| invalid("local protocol ignore file must be UTF-8"))?;
            if text
                .lines()
                .rfind(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
                .map(str::trim)
                != Some("*")
            {
                return Err(fail(
                    ErrorCode::Conflict,
                    "local protocol ignore rules must end with '*' before writing recovery state",
                )
                .at(ignore));
            }
        }
        None => write_new(
            root,
            &ignore,
            b"# Machine-local protocol recovery and retry records.\n*\n",
        )?,
    }
    ensure_directory(root, &dir.join("requests"))?;
    sync_dir(&dir)?;
    Ok(())
}
fn write_new(root: &Path, path: &Path, bytes: &[u8]) -> Result<()> {
    safe_path(root, path)?;
    let parent = path
        .parent()
        .ok_or_else(|| invalid("local file has no parent"))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| PmError::io(parent, e))?;
    temp.write_all(bytes)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|e| PmError::io(path, e))?;
    temp.persist_noclobber(path)
        .map_err(|e| PmError::io(path, e.error))?;
    sync_dir(parent)
}
fn permissions(path: &Path) -> Result<Option<u32>> {
    match fs::metadata(path) {
        Ok(metadata) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                Ok(Some(metadata.permissions().mode()))
            }
            #[cfg(not(unix))]
            {
                Ok(Some(u32::from(metadata.permissions().readonly())))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(PmError::io(path, e)),
    }
}
fn set_permissions(file: &File, mode: Option<u32>) -> Result<()> {
    if let Some(mode) = mode {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(Permissions::from_mode(mode))
                .map_err(|e| PmError::io("protocol target permissions", e))?;
        }
        #[cfg(not(unix))]
        {
            let mut p = file
                .metadata()
                .map_err(|e| PmError::io("protocol target permissions", e))?
                .permissions();
            p.set_readonly(mode != 0);
            file.set_permissions(p)
                .map_err(|e| PmError::io("protocol target permissions", e))?;
        }
    }
    Ok(())
}
fn publish(root: &Path, proof: &Proof) -> Result<()> {
    let target = root.join("AGENTS.md");
    let current = text_optional(root, &target)?;
    if current.as_deref() == Some(proof.after.as_str()) {
        return Ok(());
    }
    if current != proof.before {
        return Err(fail(ErrorCode::StaleSource,"repository instructions changed; pending protocol publication preserved").details(serde_json::json!({"request_id":proof.receipt.request_id,"expected_content":proof.receipt.before,"current_content":hash(current.as_deref())})));
    }
    if permissions(&target)? != proof.permissions {
        return Err(fail(
            ErrorCode::StaleSource,
            "repository instruction permissions changed during protocol publication",
        ));
    }
    let local = directory(root);
    safe_path(root, &local)?;
    if !fs::symlink_metadata(&local)
        .map_err(|error| PmError::io(&local, error))?
        .is_dir()
    {
        return Err(fail(
            ErrorCode::UnsafePath,
            "local protocol publication directory changed",
        )
        .at(local));
    }
    let mut temp = tempfile::NamedTempFile::new_in(&local).map_err(|e| PmError::io(&local, e))?;
    temp.write_all(proof.after.as_bytes())
        .map_err(|e| PmError::io(&target, e))?;
    set_permissions(temp.as_file(), proof.permissions)?;
    temp.as_file()
        .sync_all()
        .map_err(|e| PmError::io(&target, e))?;
    if text_optional(root, &target)? != proof.before || permissions(&target)? != proof.permissions {
        return Err(fail(
            ErrorCode::StaleSource,
            "repository instructions changed before pointer publication",
        ));
    }
    if proof.before.is_some() {
        temp.persist(&target)
            .map_err(|e| PmError::io(&target, e.error))?;
    } else {
        temp.persist_noclobber(&target)
            .map_err(|e| PmError::io(&target, e.error))?;
    }
    sync_dir(root)
}
pub(super) fn apply(
    cwd: &Path,
    mode: Mode,
    expected_repository: &RepositoryId,
    expected: Option<ContentHash>,
    request: &RequestId,
) -> Result<ProtocolReceipt> {
    apply_with_faults(
        cwd,
        mode,
        expected_repository,
        expected,
        request,
        |_| Ok(()),
    )
}
pub(super) fn apply_with_faults(
    cwd: &Path,
    mode: Mode,
    expected_repository: &RepositoryId,
    expected: Option<ContentHash>,
    request: &RequestId,
    mut fault: impl FnMut(FaultPoint) -> Result<()>,
) -> Result<ProtocolReceipt> {
    context(cwd, true, |repo, root, source| {
        if repo.identity() != expected_repository {
            return Err(fail(
                ErrorCode::StaleSource,
                "selected repository differs from --expected-repository; preview again",
            )
            .details(serde_json::json!({"expected_repository":expected_repository})));
        }
        let intent = input_hash(repo.identity(), root, mode, &expected)?;
        if let Some(proof) = decode(root, &receipt_path(root, request))? {
            validate_proof(&proof, repo.identity(), root)?;
            if proof.receipt.request_id != *request || proof.input != intent {
                return Err(fail(
                    ErrorCode::IdempotencyConflict,
                    "protocol request ID was already used for another intent",
                ));
            }
            if let Some(pending) = pending(root, repo.identity())?
                && pending.receipt.request_id == *request
            {
                if serde_json::to_vec(&pending).map_err(|error| invalid(error.to_string()))?
                    != serde_json::to_vec(&proof).map_err(|error| invalid(error.to_string()))?
                {
                    return Err(fail(
                        ErrorCode::CorruptStore,
                        "local completed receipt and pending journal disagree",
                    ));
                }
                require_journal(root, &proof)?;
                fs::remove_file(journal(root)).map_err(|e| PmError::io(journal(root), e))?;
                sync_dir(&directory(root))?;
            }
            return Ok(proof.receipt);
        }
        let proof = if let Some(proof) = pending(root, repo.identity())? {
            if proof.receipt.request_id != *request {
                return Err(recovery(&proof));
            }
            if proof.input != intent {
                return Err(fail(
                    ErrorCode::IdempotencyConflict,
                    "pending protocol request has different arguments",
                ));
            }
            proof
        } else {
            let target = root.join("AGENTS.md");
            let before = text_optional(root, &target)?;
            if hash(before.as_deref()) != expected {
                return Err(fail(
                    ErrorCode::StaleSource,
                    "AGENTS.md differs from --expected-content/--expect-absent; preview again",
                ));
            }
            let (after, _, _) = document(before.as_deref(), mode)?;
            let receipt = ProtocolReceipt {
                schema_version: 1,
                repository: repo.identity().clone(),
                project_root: root.into(),
                target: "AGENTS.md".into(),
                request_id: request.clone(),
                mode,
                before: expected.clone(),
                after: ContentHash::of(after.as_bytes()),
                changed: before.as_deref() != Some(after.as_str()),
                scope: "local_protocol_pointer".into(),
            };
            let proof = Proof {
                receipt,
                input: intent,
                before,
                after,
                pointer_version: 1,
                permissions: permissions(&target)?,
            };
            validate_proof(&proof, repo.identity(), root)?;
            let bytes = serde_json::to_vec(&proof).map_err(|e| invalid(e.to_string()))?;
            if bytes.len() > MAX_PROOF {
                return Err(invalid("local protocol recovery proof exceeds 2 MiB"));
            }
            fault(FaultPoint::BeforeJournal)?;
            if text_optional(root, &target)? != proof.before
                || permissions(&target)? != proof.permissions
            {
                return Err(fail(
                    ErrorCode::StaleSource,
                    "repository instructions changed before protocol journal publication",
                ));
            }
            validate_source(root, source)?;
            ensure_local(root)?;
            write_new(root, &journal(root), &bytes)?;
            fault(FaultPoint::AfterJournal)?;
            proof
        };
        let finish = (|| {
            fault(FaultPoint::BeforePublish)?;
            require_journal(root, &proof)?;
            validate_source(root, source)?;
            publish(root, &proof)?;
            fault(FaultPoint::AfterPublish)?;
            require_journal(root, &proof)?;
            let encoded = serde_json::to_vec(&proof).map_err(|e| invalid(e.to_string()))?;
            write_new(root, &receipt_path(root, request), &encoded)?;
            fault(FaultPoint::AfterReceipt)?;
            require_journal(root, &proof)?;
            fs::remove_file(journal(root)).map_err(|e| PmError::io(journal(root), e))?;
            sync_dir(&directory(root))?;
            Ok(proof.receipt.clone())
        })();
        finish.map_err(|error| publication_error(root, &proof, error))
    })
}

fn validate_source(root: &Path, expected: &ContentHash) -> Result<()> {
    let path = root.join(".workdeck/config.yml");
    let bytes = read_optional(root, &path, workdeck_pm::documents::MAX_DOCUMENT_BYTES)?
        .ok_or_else(|| {
            fail(
                ErrorCode::StaleSource,
                "native configuration disappeared during protocol publication",
            )
        })?;
    if ContentHash::of(&bytes) != *expected {
        return Err(fail(
            ErrorCode::StaleSource,
            "native configuration changed during protocol publication",
        ));
    }
    Ok(())
}
fn publication_error(root: &Path, proof: &Proof, mut error: PmError) -> PmError {
    let published = text_optional(root, &root.join("AGENTS.md"))
        .ok()
        .map(|text| text.as_deref() == Some(proof.after.as_str()));
    let mut details = error
        .details
        .take()
        .map(|v| *v)
        .unwrap_or_else(|| serde_json::json!({}));
    if !details.is_object() {
        details = serde_json::json!({"cause":details});
    }
    details["pointer_published"] = serde_json::json!(published);
    details["local_protocol_receipt"] = serde_json::json!(proof.receipt);
    details["scope"] = serde_json::json!("local_protocol_pointer");
    error.details = Some(Box::new(details));
    error
}
