use super::*;
use crate::{
    ContentHash, ErrorCode, PmError, RepositoryId, Result, SourceObservation, SourceSelector,
};
use rusqlite::{Connection, config::DbConfig, limits::Limit};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};

const MAGIC: &[u8; 8] = b"WDPIDX01";
const MAX_HEADER: usize = 128 * 1024;
const MAX_METADATA: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub struct ProjectionReadView {
    pub(super) connection: Connection,
    pub(super) query_cache: RefCell<super::query::QueryCache>,
    pub(super) id: ProjectionViewId,
    pub(super) observation: SourceObservation,
    pub(super) binding: Option<ContentHash>,
    pub(super) limits: ProjectionLimits,
    pub(super) counts: BTreeMap<String, u64>,
}
impl ProjectionReadView {
    pub fn id(&self) -> &ProjectionViewId {
        &self.id
    }
    pub fn observation(&self) -> &SourceObservation {
        &self.observation
    }
    pub fn publication_binding(&self) -> Option<&ContentHash> {
        self.binding.as_ref()
    }
    pub fn limits(&self) -> &ProjectionLimits {
        &self.limits
    }
    pub fn counts(&self) -> &BTreeMap<String, u64> {
        &self.counts
    }
    pub(crate) fn with_connection<T>(
        &self,
        read: impl FnOnce(&Connection) -> Result<T>,
    ) -> Result<T> {
        read(&self.connection)
    }
}
#[derive(Debug)]
pub enum ProjectionRefresh {
    Published(Box<ProjectionReadView>),
    Unchanged(ProjectionViewId),
    Superseded(ProjectionViewId),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    id: ProjectionViewId,
    observation: SourceObservation,
    binding: Option<ContentHash>,
    manifest: ProjectionManifest,
    counts: BTreeMap<String, u64>,
    diagnostics: Vec<PmError>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    view: ProjectionViewId,
    metadata: ContentHash,
    image: ContentHash,
    bytes: usize,
}

#[derive(Debug)]
pub struct ProjectionStore {
    cache: cache::Cache,
    writable: bool,
    selector: SourceSelector,
    limits: ProjectionLimits,
    repository: Option<RepositoryId>,
    status: ProjectionStatus,
    /// Last admitted source retained by this long-lived reader. Local working
    /// tree refreshes can reuse it after a metadata-only revalidation; a
    /// detected change falls back to complete content capture.
    last_source: Option<crate::PlanningSourceView>,
}
impl ProjectionStore {
    pub fn open(
        worktree: &Path,
        selector: SourceSelector,
        limits: ProjectionLimits,
    ) -> Result<Self> {
        Self::open_mode(worktree, selector, limits, true)
    }
    /// Inspect an existing checkpoint without creating or repairing cache paths.
    /// Loaded data remains Cached; this handle cannot refresh or publish.
    pub fn open_cached(
        worktree: &Path,
        selector: SourceSelector,
        limits: ProjectionLimits,
    ) -> Result<Self> {
        Self::open_mode(worktree, selector, limits, false)
    }
    fn open_mode(
        worktree: &Path,
        selector: SourceSelector,
        limits: ProjectionLimits,
        writable: bool,
    ) -> Result<Self> {
        limits.validate()?;
        let limit = limits.max_database_bytes + MAX_HEADER + 16;
        let cache = if writable {
            cache::Cache::open(worktree, &selector, limit)?
        } else {
            cache::Cache::open_cached(worktree, &selector, limit)?
        };
        // Invalid source may have a displayable last-good cache. Load never
        // declares it current; only a complete refresh can validate its source.
        let repository = crate::sources::fs::read(
            &cache.planning.join("config.yml"),
            limits.source.max_file_bytes,
        )
        .ok()
        .and_then(|(bytes, _)| serde_yaml_ng::from_slice::<crate::Config>(&bytes).ok())
        .filter(|config| config.validate().is_ok())
        .map(|config| config.repository);
        Ok(Self {
            cache,
            writable,
            selector,
            limits,
            repository,
            status: ProjectionStatus {
                state: ProjectionState::Empty,
                view: None,
                observation: None,
                publication_binding: None,
                diagnostics: Vec::new(),
            },
            last_source: None,
        })
    }
    pub fn status(&self) -> &ProjectionStatus {
        &self.status
    }
    pub fn load(&mut self) -> Result<Option<ProjectionReadView>> {
        let result = self.load_inner();
        if let Err(error) = &result {
            self.failed(error.clone());
        }
        result
    }
    fn load_inner(&mut self) -> Result<Option<ProjectionReadView>> {
        let Some(bytes) = self.cache.read()? else {
            return Ok(None);
        };
        let (connection, metadata) = match self.decode(&bytes, true) {
            Ok(value) => value,
            Err(error) if cache_invalid(&error) => {
                self.status.diagnostics.push(error);
                self.status.state = if self.status.view.is_some() {
                    ProjectionState::Stale
                } else {
                    ProjectionState::Empty
                };
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let prior_error = if self.status.state == ProjectionState::Stale {
            Some(self.status.diagnostics.clone())
        } else {
            None
        };
        self.adopt(&metadata, ProjectionState::Cached);
        if let Some(diagnostics) = prior_error {
            self.status.state = ProjectionState::Stale;
            self.status.diagnostics = diagnostics;
        }
        Ok(Some(read_view(connection, metadata, self.limits.clone())?))
    }
    pub fn refresh(&mut self, request: &ProjectionRefreshRequest) -> Result<ProjectionRefresh> {
        self.refresh_with_faults(request, |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn refresh_with_faults(
        &mut self,
        request: &ProjectionRefreshRequest,
        mut fault: impl FnMut(ProjectionFaultPoint) -> Result<()>,
    ) -> Result<ProjectionRefresh> {
        if !self.writable {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "Cached inspection cannot refresh or publish; open an explicit writable index operation",
            ));
        }
        let result = self.refresh_inner(request, &mut fault);
        if let Err(error) = &result {
            self.failed(error.clone());
        }
        result
    }
    fn failed(&mut self, error: PmError) {
        self.status.state = if self.status.view.is_some() {
            ProjectionState::Stale
        } else {
            ProjectionState::Error
        };
        self.status.diagnostics = vec![error];
    }
    fn refresh_inner(
        &mut self,
        request: &ProjectionRefreshRequest,
        fault: &mut impl FnMut(ProjectionFaultPoint) -> Result<()>,
    ) -> Result<ProjectionRefresh> {
        self.cache.verify()?;
        let previous_bytes = self.cache.read()?;
        let expected = previous_bytes.as_deref().map(ContentHash::of);
        let mut cache_diagnostics = Vec::new();
        let previous = match previous_bytes.as_deref() {
            Some(bytes) => match self.decode(bytes, false) {
                Ok(value) => Some(value),
                Err(error) if cache_invalid(&error) => {
                    cache_diagnostics.push(error);
                    None
                }
                Err(error) => return Err(error),
            },
            None => None,
        };
        if self.status.view.is_none()
            && let Some((_, metadata)) = &previous
        {
            self.adopt(metadata, ProjectionState::Cached);
        }
        let deadline = Instant::now() + Duration::from_secs(self.limits.source.timeout_seconds);
        let source = if !request.rebuild
            && matches!(self.selector, SourceSelector::WorkingTree)
            && let Some(previous_source) = self.last_source.as_ref()
        {
            match crate::sources::capture_local_delta(
                &self.cache.worktree,
                previous_source,
                &self.limits.source,
                deadline,
            )? {
                Some(source) => source,
                None => crate::sources::capture_with_deadline(
                    &self.cache.worktree,
                    &self.selector,
                    &self.limits.source,
                    deadline,
                )?,
            }
        } else {
            crate::sources::capture_with_deadline(
                &self.cache.worktree,
                &self.selector,
                &self.limits.source,
                deadline,
            )?
        };
        self.check_repository(&source.observation.identity.repository)?;
        fault(ProjectionFaultPoint::AfterCapture)?;
        let manifest = manifest(&source.snapshot)?;
        let binding = source.publication_binding().cloned();
        let id = ProjectionViewId {
            schema: PROJECTION_SCHEMA_VERSION,
            slot: self.cache.slot.clone(),
            source: source.observation.identity.clone(),
            generation: generation(
                &self.cache.slot,
                &source.observation.identity,
                &manifest.fingerprint,
                binding.as_ref(),
            )?,
        };
        if !request.rebuild
            && let Some((_, metadata)) = &previous
            && metadata.id == id
        {
            source.revalidate_before(deadline)?;
            self.cache.verify()?;
            if self.cache.read()?.as_deref().map(ContentHash::of) != expected {
                return self.superseded();
            }
            self.adopt(metadata, ProjectionState::Current);
            self.status.observation = Some(source.observation.clone());
            self.last_source = Some(source);
            return Ok(ProjectionRefresh::Unchanged(id));
        }
        let (mut connection, old_manifest) = if request.rebuild {
            (database(None, false, &self.limits)?, None)
        } else if let Some((connection, metadata)) = previous {
            (connection, Some(metadata.manifest))
        } else {
            (database(None, false, &self.limits)?, None)
        };
        let transaction = connection.transaction().map_err(sql_error)?;
        schema::initialize(&transaction)?;
        let build = projector::project(
            &transaction,
            &source.snapshot,
            old_manifest.as_ref(),
            &self.limits,
        )?;
        if build.manifest != manifest {
            return Err(invalid("projector manifest differs from admitted source"));
        }
        let metadata = Metadata {
            id,
            observation: source.observation.clone(),
            binding,
            manifest,
            counts: build.counts,
            diagnostics: build.diagnostics,
        };
        let metadata_bytes = serde_json::to_vec(&metadata).map_err(json_error)?;
        if metadata_bytes.len() > MAX_METADATA {
            return Err(invalid("projection metadata exceeds byte limit"));
        }
        transaction.execute_batch("CREATE TABLE IF NOT EXISTS _projection_meta (id INTEGER PRIMARY KEY CHECK(id=1), document TEXT NOT NULL);").map_err(sql_error)?;
        transaction
            .execute(
                "INSERT OR REPLACE INTO _projection_meta(id,document) VALUES(1,?1)",
                [std::str::from_utf8(&metadata_bytes)
                    .map_err(|_| invalid("projection metadata is not UTF-8"))?],
            )
            .map_err(sql_error)?;
        transaction.commit().map_err(sql_error)?;
        fault(ProjectionFaultPoint::AfterProject)?;
        schema::validate(&connection)?;
        // The source guard runs after the candidate is durably written, under the
        // publication lock and immediately before checkpoint replacement. Scanning
        // every source file here as well cannot protect that later publication;
        // retain the final guard and only check the shared deadline at this stage.
        if Instant::now() >= deadline {
            return Err(stale(
                "projection construction exceeded its source deadline",
            ));
        }
        let image = connection.serialize(rusqlite::MAIN_DB).map_err(sql_error)?;
        if image.len() > self.limits.max_database_bytes {
            return Err(invalid("projection database exceeds byte limit"));
        }
        let header = serde_json::to_vec(&Header {
            view: metadata.id.clone(),
            metadata: ContentHash::of(&metadata_bytes),
            image: ContentHash::of(&image),
            bytes: image.len(),
        })
        .map_err(json_error)?;
        if header.len() > MAX_HEADER {
            return Err(invalid("projection header exceeds byte limit"));
        }
        let mut checkpoint = Vec::with_capacity(16 + header.len() + image.len());
        checkpoint.extend_from_slice(MAGIC);
        checkpoint.extend_from_slice(&(header.len() as u64).to_le_bytes());
        checkpoint.extend_from_slice(&header);
        checkpoint.extend_from_slice(&image);
        drop(image);
        // Faults run outside the lock; publication independently compares CAS.
        fault(ProjectionFaultPoint::BeforePublish)?;
        let published = self.cache.publish(&checkpoint, expected.as_ref(), || {
            fault(ProjectionFaultPoint::AfterCheckpointWritten)?;
            source.revalidate_before(deadline)
        })?;
        if !published {
            return self.superseded();
        }
        self.adopt(&metadata, ProjectionState::Current);
        self.status.diagnostics.extend(cache_diagnostics);
        fault(ProjectionFaultPoint::AfterPublish)?;
        let view = read_view(connection, metadata, self.limits.clone())?;
        self.last_source = Some(source);
        Ok(ProjectionRefresh::Published(Box::new(view)))
    }
    fn superseded(&mut self) -> Result<ProjectionRefresh> {
        let bytes = self
            .cache
            .read()?
            .ok_or_else(|| stale("concurrent projection publication disappeared"))?;
        let (_, metadata) = self.decode(&bytes, true)?;
        self.adopt(&metadata, ProjectionState::Cached);
        Ok(ProjectionRefresh::Superseded(metadata.id))
    }
    fn check_repository(&self, repository: &RepositoryId) -> Result<()> {
        if self
            .repository
            .as_ref()
            .is_some_and(|expected| expected != repository)
        {
            return Err(stale(
                "repository identity changed; reopen this source slot",
            ));
        }
        Ok(())
    }
    fn adopt(&mut self, metadata: &Metadata, state: ProjectionState) {
        self.repository = Some(metadata.id.source.repository.clone());
        self.status = ProjectionStatus {
            state,
            view: Some(metadata.id.clone()),
            observation: Some(metadata.observation.clone()),
            publication_binding: metadata.binding.clone(),
            diagnostics: metadata.diagnostics.clone(),
        };
    }
    fn decode(&self, bytes: &[u8], readonly: bool) -> Result<(Connection, Metadata)> {
        if bytes.len() < 16 || &bytes[..8] != MAGIC {
            return Err(invalid(
                "invalid projection checkpoint framing; rebuild required",
            ));
        }
        let size = u64::from_le_bytes(
            bytes[8..16]
                .try_into()
                .map_err(|_| invalid("projection header length"))?,
        );
        let size =
            usize::try_from(size).map_err(|_| invalid("projection header length overflow"))?;
        if size > MAX_HEADER || size > bytes.len() - 16 {
            return Err(invalid("projection header exceeds available bytes"));
        }
        let header: Header = serde_json::from_slice(&bytes[16..16 + size]).map_err(json_error)?;
        let image = &bytes[16 + size..];
        if header.view.schema != PROJECTION_SCHEMA_VERSION || header.view.slot != self.cache.slot {
            return Err(invalid(
                "checkpoint belongs to another schema or checkout/source slot",
            ));
        }
        if self
            .repository
            .as_ref()
            .is_some_and(|repository| repository != &header.view.source.repository)
        {
            return Err(invalid(
                "checkpoint repository differs from selected native source",
            ));
        }
        if image.len() != header.bytes
            || image.len() > self.limits.max_database_bytes
            || ContentHash::of(image) != header.image
        {
            return Err(invalid(
                "projection checkpoint image is truncated or changed",
            ));
        }
        let connection = database(Some(image), readonly, &self.limits)?;
        schema::validate(&connection)?;
        let metadata_bytes: String = connection
            .query_row(
                "SELECT document FROM _projection_meta WHERE id=1",
                [],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if metadata_bytes.len() > MAX_METADATA
            || ContentHash::of(metadata_bytes.as_bytes()) != header.metadata
        {
            return Err(invalid(
                "projection metadata digest differs from checkpoint",
            ));
        }
        let metadata: Metadata = serde_json::from_str(&metadata_bytes).map_err(json_error)?;
        if metadata.id != header.view
            || metadata.id.source != metadata.observation.identity
            || metadata.manifest.files.len() > self.limits.source.max_entries
            || generation(
                &metadata.id.slot,
                &metadata.id.source,
                &metadata.manifest.fingerprint,
                metadata.binding.as_ref(),
            )? != metadata.id.generation
        {
            return Err(invalid("projection metadata identity is inconsistent"));
        }
        let check: String = connection
            .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
            .map_err(sql_error)?;
        if check != "ok" {
            return Err(invalid("projection database integrity check failed"));
        }
        Ok((connection, metadata))
    }
}
fn read_view(
    connection: Connection,
    metadata: Metadata,
    limits: ProjectionLimits,
) -> Result<ProjectionReadView> {
    connection
        .execute_batch("PRAGMA query_only=ON;")
        .map_err(sql_error)?;
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(sql_error)?;
    Ok(ProjectionReadView {
        connection,
        query_cache: RefCell::default(),
        id: metadata.id,
        observation: metadata.observation,
        binding: metadata.binding,
        limits,
        counts: metadata.counts,
    })
}
fn database(image: Option<&[u8]>, readonly: bool, limits: &ProjectionLimits) -> Result<Connection> {
    let mut connection = Connection::open_in_memory().map_err(sql_error)?;
    for (setting, value) in [
        (DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true),
        (DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false),
        (DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, false),
        (DbConfig::SQLITE_DBCONFIG_ENABLE_VIEW, false),
    ] {
        connection
            .set_db_config(setting, value)
            .map_err(sql_error)?;
    }
    connection
        .set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)
        .map_err(sql_error)?;
    connection
        .set_limit(
            Limit::SQLITE_LIMIT_LENGTH,
            MAX_METADATA.max(limits.source.max_file_bytes) as i32,
        )
        .map_err(sql_error)?;
    connection
        .set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, 1024 * 1024)
        .map_err(sql_error)?;
    connection
        .execute_batch("PRAGMA temp_store=MEMORY; PRAGMA mmap_size=0; PRAGMA journal_mode=MEMORY;")
        .map_err(sql_error)?;
    if let Some(bytes) = image {
        if bytes.len() > limits.max_database_bytes {
            return Err(invalid("projection image exceeds byte limit"));
        }
        // Complete bounded slice: no filesystem reader or early read failure
        // after rusqlite allocates its deserialization buffer.
        connection
            .deserialize_read_exact(rusqlite::MAIN_DB, bytes, bytes.len(), readonly)
            .map_err(sql_error)?;
    }
    // sqlite3_deserialize uses an internal ATTACH to reopen its memory VFS.
    // Install the SQL authorizer after that API call, before inspecting the image.
    connection
        .authorizer(Some(|context: rusqlite::hooks::AuthContext<'_>| {
            use rusqlite::hooks::{AuthAction, Authorization};
            match context.action {
                AuthAction::Attach { .. } | AuthAction::Detach { .. } => Authorization::Deny,
                AuthAction::Pragma { pragma_name, .. }
                    if matches!(
                        pragma_name.to_ascii_lowercase().as_str(),
                        "temp_store_directory" | "data_store_directory" | "writable_schema"
                    ) =>
                {
                    Authorization::Deny
                }
                _ => Authorization::Allow,
            }
        }))
        .map_err(sql_error)?;
    let page_size: usize = connection
        .query_row("PRAGMA page_size", [], |row| row.get::<_, i64>(0))
        .map_err(sql_error)?
        .try_into()
        .map_err(|_| invalid("invalid projection page size"))?;
    if !(512..=65536).contains(&page_size) || !page_size.is_power_of_two() {
        return Err(invalid("unsupported projection page size"));
    }
    let max_pages = i64::try_from(limits.max_database_bytes / page_size)
        .map_err(|_| invalid("projection page limit overflow"))?;
    connection
        .pragma_update(None, "max_page_count", max_pages)
        .map_err(sql_error)?;
    let deadline = Instant::now() + Duration::from_secs(limits.source.timeout_seconds);
    connection
        .progress_handler(1000, Some(move || Instant::now() >= deadline))
        .map_err(sql_error)?;
    Ok(connection)
}
pub(super) fn manifest(source: &crate::SourceSnapshot) -> Result<ProjectionManifest> {
    let files = source
        .files()
        .iter()
        .map(|(path, bytes)| {
            (
                path.clone(),
                ProjectionFile {
                    content: ContentHash::of(bytes),
                    bytes: bytes.len(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let fingerprint = crate::transactions::canonical_hash(
        &serde_json::json!({"schema":1,"files":files.iter().map(|(path,file)|(path,&file.content,file.bytes)).collect::<Vec<_>>()}),
    )?;
    Ok(ProjectionManifest { files, fingerprint })
}
fn generation(
    slot: &ContentHash,
    source: &crate::PlanningSourceIdentity,
    manifest: &ContentHash,
    binding: Option<&ContentHash>,
) -> Result<ContentHash> {
    crate::transactions::canonical_hash(
        &serde_json::json!({"schema":PROJECTION_SCHEMA_VERSION,"slot":slot,"source":source,"manifest":manifest,"binding":binding}),
    )
}
fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
fn stale(message: &str) -> PmError {
    PmError::new(ErrorCode::StaleSource, message)
}
fn json_error(error: serde_json::Error) -> PmError {
    PmError::new(
        ErrorCode::InvalidSchema,
        format!("projection checkpoint: {error}"),
    )
}
fn cache_invalid(error: &PmError) -> bool {
    matches!(
        error.code,
        ErrorCode::InvalidSchema | ErrorCode::InvalidInput
    )
}
