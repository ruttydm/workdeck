//! Complete selected execution inputs and private live bindings.
use super::input_fs;
use crate::*;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolBinding {
    chosen: PathBuf,
    resolved: PathBuf,
    file: input_fs::HashedFile,
}
/// Raw environment values never implement Serialize or escape through diagnostics.
pub(crate) struct CapturedInputs {
    pub manifest: InputManifest,
    root: PathBuf,
    selection: InputSelection,
    environment_spec: BTreeMap<String, EnvironmentValue>,
    tool_specs: Vec<ToolRequirement>,
    files: input_fs::InputFiles,
    environment: BTreeMap<OsString, OsString>,
    tools: BTreeMap<String, ToolBinding>,
    cwd: Option<(PathBuf, input_fs::FileIdentity)>,
}
impl CapturedInputs {
    pub(crate) fn worktree(&self) -> &Path {
        &self.root
    }
    pub(crate) fn environment(&self) -> BTreeMap<OsString, OsString> {
        self.environment.clone()
    }
    pub(crate) fn tool(&self, name: &str) -> Result<PathBuf> {
        self.tools
            .get(name)
            .map(|t| t.chosen.clone())
            .ok_or_else(|| {
                PmError::new(
                    ErrorCode::PolicyBlocked,
                    format!("declared tool {name} is unavailable"),
                )
            })
    }
    pub(crate) fn bind_cwd(&mut self, path: &Path) -> Result<()> {
        self.cwd = Some((path.into(), input_fs::directory_identity(&self.root, path)?));
        Ok(())
    }
    pub(crate) fn verify(&self) -> Result<()> {
        let current = capture_inner(
            &self.root,
            &self.selection,
            &self.environment_spec,
            &self.tool_specs,
            &self.manifest.limits,
        )
        .map_err(|_| input_fs::stale())?;
        if let Some((path, identity)) = &self.cwd
            && input_fs::directory_identity(&self.root, path).map_err(|_| input_fs::stale())?
                != *identity
        {
            return Err(input_fs::stale());
        }
        if current.manifest != self.manifest
            || current.files != self.files
            || current.environment != self.environment
            || current.tools != self.tools
        {
            return Err(input_fs::stale());
        }
        Ok(())
    }
}
pub(crate) fn worktree(planning_root: &Path) -> Result<PathBuf> {
    let parent = planning_root
        .parent()
        .filter(|_| planning_root.file_name().is_some_and(|n| n == ".workdeck"))
        .ok_or_else(|| {
            PmError::new(
                ErrorCode::Unsupported,
                "worktree_unbound: execution requires a verified canonical .workdeck worktree",
            )
        })?;
    if crate::repository::project_root(parent)? != parent {
        return Err(PmError::new(
            ErrorCode::Unsupported,
            "worktree_unbound: planning source is not the canonical worktree boundary",
        ));
    }
    Ok(parent.into())
}
fn os_hash(value: &std::ffi::OsStr) -> ContentHash {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        ContentHash::of(value.as_bytes())
    }
    #[cfg(not(unix))]
    {
        ContentHash::of(value.to_string_lossy().as_bytes())
    }
}
fn environment(
    spec: &BTreeMap<String, EnvironmentValue>,
) -> Result<(Vec<EnvironmentPin>, BTreeMap<OsString, OsString>)> {
    let mut pins = Vec::new();
    let mut values = BTreeMap::new();
    let mut bytes = 0;
    for (name, definition) in spec {
        crate::commands::validation::environment_name(name)?;
        let (source, value) = match definition {
            EnvironmentValue::Literal { value } => (None, Some(OsString::from(value))),
            EnvironmentValue::Inherit { source, .. } => {
                crate::commands::validation::environment_name(source)?;
                (Some(source.clone()), std::env::var_os(source))
            }
        };
        if let Some(value) = &value {
            bytes += value.len();
            if value.len() > 64 * 1024 || bytes > 1024 * 1024 {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "declared environment exceeds capture bounds",
                ));
            }
        }
        pins.push(EnvironmentPin {
            name: name.clone(),
            source,
            present: value.is_some(),
            content: value.as_ref().map(|v| os_hash(v)),
        });
        if let Some(value) = value {
            values.insert(OsString::from(name), value);
        }
    }
    Ok((pins, values))
}
fn tools(
    root: &Path,
    spec: &[ToolRequirement],
    env: &BTreeMap<OsString, OsString>,
    limits: &InputLimits,
    mut remaining_bytes: u64,
) -> Result<(Vec<ToolPin>, BTreeMap<String, ToolBinding>)> {
    let mut pins = Vec::new();
    let mut bindings = BTreeMap::new();
    for tool in spec {
        let path = Path::new(&tool.executable);
        let candidates = if path.is_absolute() {
            vec![path.into()]
        } else if path.components().count() > 1 {
            crate::commands::validation::relative(path, false)?;
            vec![root.join(path)]
        } else if let Some(search) = env.get(std::ffi::OsStr::new("PATH")) {
            let mut paths = Vec::new();
            for directory in std::env::split_paths(search) {
                if !directory.is_absolute() {
                    return Err(PmError::new(
                        ErrorCode::InvalidInput,
                        "declared PATH entries must be absolute; implicit current-directory lookup is unsupported",
                    ));
                }
                paths.push(directory.join(path));
            }
            paths
        } else {
            Vec::new()
        };
        let mut found = None;
        for candidate in candidates {
            match std::fs::symlink_metadata(&candidate) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(PmError::io(&candidate, e)),
                Ok(_) => (),
            }
            // Tool symlinks are explicit executable resolution (e.g. cargo -> rustup).
            // Source inputs remain no-follow. Both chosen path and resolved bytes are
            // rechecked; this is a cooperative pre-spawn check, not descriptor exec.
            let resolved = candidate
                .canonicalize()
                .map_err(|e| PmError::io(&candidate, e))?;
            let file =
                input_fs::hash_absolute(&resolved, limits.max_file_bytes.min(remaining_bytes))?;
            remaining_bytes = remaining_bytes.saturating_sub(file.size);
            if !file.executable {
                continue;
            }
            found = Some(ToolBinding {
                chosen: candidate,
                resolved,
                file,
            });
            break;
        }
        let location = found.as_ref().map(|t| match t.chosen.strip_prefix(root) {
            Ok(path) => ToolLocation::Worktree { path: path.into() },
            Err(_) => ToolLocation::Absolute {
                path: t.chosen.clone(),
            },
        });
        pins.push(ToolPin {
            name: tool.name.clone(),
            executable: tool.executable.clone(),
            resolved: location,
            content: found.as_ref().map(|t| t.file.content.clone()),
            size: found.as_ref().map(|t| t.file.size),
            executable_bit: found.is_some(),
        });
        if let Some(binding) = found {
            bindings.insert(tool.name.clone(), binding);
        }
    }
    pins.sort_by(|a, b| a.name.cmp(&b.name));
    Ok((pins, bindings))
}
pub(crate) fn manifest_hash(manifest: &InputManifest) -> Result<ContentHash> {
    crate::transactions::canonical_hash(
        &serde_json::json!({"schema":manifest.schema,"selection":manifest.selection,"limits":manifest.limits,"complete":manifest.complete,"entries":manifest.entries,"environment":manifest.environment,"tools":manifest.tools,"excluded_internal_paths":manifest.excluded_internal_paths,"total_bytes":manifest.total_bytes}),
    )
}
fn capture_inner(
    root: &Path,
    selection: &InputSelection,
    environment_spec: &BTreeMap<String, EnvironmentValue>,
    tool_specs: &[ToolRequirement],
    limits: &InputLimits,
) -> Result<CapturedInputs> {
    let files = input_fs::capture(root, selection, limits)?;
    let (environment_pins, environment) = environment(environment_spec)?;
    let (tool_pins, tools) = tools(
        root,
        tool_specs,
        &environment,
        limits,
        limits.max_total_bytes.saturating_sub(files.total_bytes),
    )?;
    let total_bytes = files.total_bytes + tool_pins.iter().filter_map(|p| p.size).sum::<u64>();
    let mut manifest = InputManifest {
        schema: SchemaVersion::CURRENT,
        selection: selection.clone(),
        limits: limits.clone(),
        complete: true,
        entries: files.entries.clone(),
        environment: environment_pins,
        tools: tool_pins,
        excluded_internal_paths: input_fs::EXCLUDED.iter().map(PathBuf::from).collect(),
        total_bytes,
        fingerprint: ContentHash::of(b""),
    };
    manifest.fingerprint = manifest_hash(&manifest)?;
    Ok(CapturedInputs {
        cwd: None,
        manifest,
        root: root.into(),
        selection: selection.clone(),
        environment_spec: environment_spec.clone(),
        tool_specs: tool_specs.to_vec(),
        files,
        environment,
        tools,
    })
}
pub(crate) fn capture(
    planning_root: &Path,
    selection: &InputSelection,
    environment: &BTreeMap<String, EnvironmentValue>,
    tools: &[ToolRequirement],
    limits: &InputLimits,
) -> Result<CapturedInputs> {
    let root = worktree(planning_root)?;
    let captured = capture_inner(&root, selection, environment, tools, limits)?;
    captured.verify()?;
    Ok(captured)
}
pub(crate) fn validate_manifest(manifest: &InputManifest) -> Result<()> {
    manifest.selection.validate()?;
    let maximum = InputLimits::default();
    if manifest.limits.max_entries == 0
        || manifest.limits.max_entries > maximum.max_entries
        || manifest.limits.max_file_bytes == 0
        || manifest.limits.max_file_bytes > maximum.max_file_bytes
        || manifest.limits.max_total_bytes == 0
        || manifest.limits.max_total_bytes > maximum.max_total_bytes
    {
        return Err(PmError::new(
            ErrorCode::InvalidSchema,
            "manifest capture limits exceed supported bounds",
        ));
    }
    if !manifest.complete
        || manifest.tools.len() > 64
        || manifest.environment.len() > 128
        || manifest.entries.len() > manifest.limits.max_entries
        || manifest.total_bytes > manifest.limits.max_total_bytes
        || manifest.excluded_internal_paths
            != input_fs::EXCLUDED
                .iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        || manifest_hash(manifest)? != manifest.fingerprint
    {
        return Err(PmError::new(
            ErrorCode::InvalidSchema,
            "input manifest completeness/bounds/fingerprint mismatch",
        ));
    }
    let mut previous = None;
    let mut total = 0;
    for entry in &manifest.entries {
        let path = match entry {
            InputEntry::File { path, size, .. } => {
                if *size > manifest.limits.max_file_bytes {
                    return Err(PmError::new(
                        ErrorCode::InvalidSchema,
                        "input file exceeds manifest bounds",
                    ));
                }
                total += size;
                path
            }
            InputEntry::Directory { path, .. } | InputEntry::Absent { path } => path,
        };
        crate::commands::validation::relative(path, matches!(entry, InputEntry::Directory { .. }))?;
        if input_fs::excluded(path) || previous.is_some_and(|p: &PathBuf| p >= path) {
            return Err(PmError::new(
                ErrorCode::InvalidSchema,
                "input manifest paths are excluded, duplicated, or unordered",
            ));
        }
        previous = Some(path);
    }
    for tool in &manifest.tools {
        if let Some(size) = tool.size {
            if size > manifest.limits.max_file_bytes {
                return Err(PmError::new(
                    ErrorCode::InvalidSchema,
                    "tool exceeds manifest file bounds",
                ));
            }
            total += size;
        }
    }
    if total != manifest.total_bytes {
        return Err(PmError::new(
            ErrorCode::InvalidSchema,
            "input manifest total byte mismatch",
        ));
    }
    validate_membership(manifest)?;
    Ok(())
}

fn validate_membership(manifest: &InputManifest) -> Result<()> {
    let invalid = || {
        PmError::new(
            ErrorCode::InvalidSchema,
            "complete input manifest does not cover its selected paths and directory membership",
        )
    };
    let entries = manifest
        .entries
        .iter()
        .map(|entry| {
            let path = match entry {
                InputEntry::File { path, .. }
                | InputEntry::Directory { path, .. }
                | InputEntry::Absent { path } => path,
            };
            (path.clone(), entry)
        })
        .collect::<BTreeMap<_, _>>();
    let selection = &manifest.selection;
    for path in selection
        .files
        .iter()
        .chain(&selection.dependency_files)
        .chain(&selection.toolchain_files)
    {
        if !matches!(entries.get(path), Some(InputEntry::File { .. })) {
            return Err(invalid());
        }
    }
    for path in &selection.optional_files {
        if !matches!(
            entries.get(path),
            Some(InputEntry::File { .. } | InputEntry::Absent { .. })
        ) {
            return Err(invalid());
        }
    }
    for path in &selection.trees {
        if !matches!(entries.get(path), Some(InputEntry::Directory { .. })) {
            return Err(invalid());
        }
    }
    let parent = |path: &Path| {
        path.parent().map(|p| {
            if p.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                p.into()
            }
        })
    };
    let mut memberships = BTreeMap::<PathBuf, Vec<(String, &str)>>::new();
    for (path, entry) in &entries {
        if path == Path::new(".") {
            continue;
        }
        let kind = match entry {
            InputEntry::File { .. } => "file",
            InputEntry::Directory { .. } => "directory",
            InputEntry::Absent { .. } => continue,
        };
        let parent = parent(path).ok_or_else(invalid)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(invalid)?;
        memberships
            .entry(parent)
            .or_default()
            .push((name.into(), kind));
    }
    for (path, entry) in &entries {
        let tree = selection
            .trees
            .iter()
            .find(|tree| tree.as_path() == Path::new(".") || path.starts_with(tree));
        match entry {
            InputEntry::Absent { .. } => {
                if !selection.optional_files.contains(path) {
                    return Err(invalid());
                }
            }
            InputEntry::File { .. } => {
                if tree.is_none()
                    && !selection.files.contains(path)
                    && !selection.dependency_files.contains(path)
                    && !selection.toolchain_files.contains(path)
                    && !selection.optional_files.contains(path)
                {
                    return Err(invalid());
                }
            }
            InputEntry::Directory { membership, .. } => {
                if tree.is_none() {
                    return Err(invalid());
                }
                let children = memberships.get(path).map(Vec::as_slice).unwrap_or_default();
                if crate::transactions::canonical_hash(&serde_json::json!(children))? != *membership
                {
                    return Err(invalid());
                }
            }
        }
        if tree.is_some_and(|tree| tree != path) && !matches!(entry, InputEntry::Absent { .. }) {
            let parent = parent(path).ok_or_else(invalid)?;
            if !matches!(entries.get(&parent), Some(InputEntry::Directory { .. })) {
                return Err(invalid());
            }
        }
    }
    let mut env_names = std::collections::BTreeSet::new();
    for env in &manifest.environment {
        crate::commands::validation::environment_name(&env.name)?;
        if let Some(source) = &env.source {
            crate::commands::validation::environment_name(source)?
        }
        if !env_names.insert(&env.name) || env.present != env.content.is_some() {
            return Err(invalid());
        }
    }
    let mut tool_names = std::collections::BTreeSet::new();
    for tool in &manifest.tools {
        crate::commands::validation::id(&tool.name)?;
        if !tool_names.insert(&tool.name)
            || tool.content.is_some() != tool.resolved.is_some()
            || tool.content.is_some() != tool.size.is_some()
            || tool.executable_bit != tool.content.is_some()
        {
            return Err(invalid());
        }
        if let Some(location) = &tool.resolved {
            match location {
                ToolLocation::Worktree { path } => {
                    crate::commands::validation::relative(path, false)?
                }
                ToolLocation::Absolute { path } => {
                    if !path.is_absolute()
                        || path.components().any(|c| {
                            matches!(
                                c,
                                std::path::Component::ParentDir | std::path::Component::CurDir
                            )
                        })
                    {
                        return Err(invalid());
                    }
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_bindings(
    manifest: &InputManifest,
    environment: &BTreeMap<String, EnvironmentValue>,
    tools: &[ToolRequirement],
) -> Result<()> {
    let invalid = || {
        PmError::new(
            ErrorCode::InvalidSchema,
            "input environment/tool pins differ from their declared bindings",
        )
    };
    if manifest.environment.len() != environment.len() || manifest.tools.len() != tools.len() {
        return Err(invalid());
    }
    for pin in &manifest.environment {
        match environment.get(&pin.name).ok_or_else(invalid)? {
            EnvironmentValue::Literal { value } => {
                if pin.source.is_some()
                    || !pin.present
                    || pin.content != Some(ContentHash::of(value.as_bytes()))
                {
                    return Err(invalid());
                }
            }
            EnvironmentValue::Inherit { source, .. } => {
                if pin.source.as_ref() != Some(source) {
                    return Err(invalid());
                }
            }
        }
    }
    for pin in &manifest.tools {
        if !tools
            .iter()
            .any(|tool| tool.name == pin.name && tool.executable == pin.executable)
        {
            return Err(invalid());
        }
    }
    Ok(())
}
