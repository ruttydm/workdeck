use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use workdeck_domain::{
    ActivityKind, ActivityReadCursor, CheckoutId, CheckoutRecord, InboxDisposition,
    InboxPreference, ProjectId, RepositoryId, RepositoryRecord, ReviewCheckpoint, ReviewDelta,
    ReviewMark, ReviewMarkState, ReviewSet, ReviewSetId, ReviewSource, ReviewUnitId,
    ReviewUnitVersion, ReviewUnitVersionId, SnapshotId, UnitTransition, WorkspaceProject,
    WorktreeAttention, WorktreeId, WorktreeRecord,
};

const SCHEMA_VERSION: i64 = 2;
const UPSERT_WORKTREE_ATTENTION: &str = "INSERT INTO worktree_attention(
        worktree_id, change_count, commit_count, base_ref, fingerprint,
        truncated, error, scanned_at, duration_ms
     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
     ON CONFLICT(worktree_id) DO UPDATE SET
        change_count = CASE
            WHEN excluded.error IS NULL THEN excluded.change_count
            ELSE worktree_attention.change_count
        END,
        commit_count = CASE
            WHEN excluded.error IS NULL THEN excluded.commit_count
            ELSE worktree_attention.commit_count
        END,
        base_ref = CASE
            WHEN excluded.error IS NULL THEN excluded.base_ref
            ELSE worktree_attention.base_ref
        END,
        fingerprint = CASE
            WHEN excluded.error IS NULL THEN excluded.fingerprint
            ELSE worktree_attention.fingerprint
        END,
        truncated = CASE
            WHEN excluded.error IS NULL THEN excluded.truncated
            ELSE worktree_attention.truncated
        END,
        error = excluded.error,
        scanned_at = excluded.scanned_at,
        duration_ms = excluded.duration_ms";

pub struct Catalog {
    connection: Connection,
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogIntegrity {
    pub schema_version: i64,
    pub integrity: String,
    pub foreign_key_violations: usize,
}

impl Catalog {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create catalog directory {}", parent.display())
            })?;
        }
        let connection = Connection::open(&path)
            .with_context(|| format!("failed to open Workdeck catalog {}", path.display()))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let mut catalog = Self { connection, path };
        catalog.migrate()?;
        Ok(catalog)
    }

    pub fn open_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let mut catalog = Self {
            connection,
            path: PathBuf::from(":memory:"),
        };
        catalog.migrate()?;
        Ok(catalog)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn verify_integrity(&self) -> Result<CatalogIntegrity> {
        let schema_version = self.connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
        let integrity = self
            .connection
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))?;
        let mut statement = self.connection.prepare("PRAGMA foreign_key_check")?;
        let foreign_key_violations = statement.query_map([], |_| Ok(()))?.count();
        Ok(CatalogIntegrity {
            schema_version,
            integrity,
            foreign_key_violations,
        })
    }

    fn migrate(&mut self) -> Result<()> {
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
            ",
        )?;
        let current = self.connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        if current > SCHEMA_VERSION {
            bail!("catalog schema {current} is newer than supported schema {SCHEMA_VERSION}");
        }
        if current < 1 {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(
                "
                CREATE TABLE projects (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    archived INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE UNIQUE INDEX projects_name_active
                    ON projects(name) WHERE archived = 0;

                CREATE TABLE repositories (
                    id TEXT PRIMARY KEY,
                    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                    name TEXT NOT NULL,
                    provider TEXT,
                    provider_owner TEXT,
                    provider_name TEXT,
                    normalized_remotes_json TEXT NOT NULL DEFAULT '[]',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE INDEX repositories_project_id ON repositories(project_id);

                CREATE TABLE checkouts (
                    id TEXT PRIMARY KEY,
                    repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
                    path TEXT NOT NULL UNIQUE,
                    git_common_dir TEXT NOT NULL,
                    available INTEGER NOT NULL DEFAULT 1,
                    last_seen_at TEXT NOT NULL
                );
                CREATE INDEX checkouts_repository_id ON checkouts(repository_id);

                CREATE TABLE worktrees (
                    id TEXT PRIMARY KEY,
                    checkout_id TEXT NOT NULL REFERENCES checkouts(id) ON DELETE CASCADE,
                    path TEXT NOT NULL UNIQUE,
                    head TEXT,
                    branch TEXT,
                    locked INTEGER NOT NULL DEFAULT 0,
                    prunable INTEGER NOT NULL DEFAULT 0,
                    available INTEGER NOT NULL DEFAULT 1,
                    last_seen_at TEXT NOT NULL
                );
                CREATE INDEX worktrees_checkout_id ON worktrees(checkout_id);

                CREATE TABLE worktree_attention (
                    worktree_id TEXT PRIMARY KEY REFERENCES worktrees(id) ON DELETE CASCADE,
                    change_count INTEGER NOT NULL,
                    commit_count INTEGER NOT NULL DEFAULT 0,
                    base_ref TEXT,
                    fingerprint TEXT NOT NULL DEFAULT '',
                    truncated INTEGER NOT NULL DEFAULT 0,
                    error TEXT,
                    scanned_at TEXT NOT NULL,
                    duration_ms INTEGER NOT NULL DEFAULT 0
                );
                CREATE INDEX worktree_attention_scanned_at
                    ON worktree_attention(scanned_at);

                CREATE TABLE inbox_preferences (
                    worktree_id TEXT PRIMARY KEY REFERENCES worktrees(id) ON DELETE CASCADE,
                    disposition TEXT NOT NULL,
                    snoozed_until TEXT,
                    baseline_signature TEXT,
                    updated_at TEXT NOT NULL
                );
                CREATE INDEX inbox_preferences_disposition
                    ON inbox_preferences(disposition);

                CREATE TABLE review_sets (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    archived INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE review_sources (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    review_set_id TEXT NOT NULL REFERENCES review_sets(id) ON DELETE CASCADE,
                    source_json TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX review_sources_review_set_id ON review_sources(review_set_id);

                CREATE TABLE snapshots (
                    id TEXT PRIMARY KEY,
                    review_set_id TEXT NOT NULL REFERENCES review_sets(id) ON DELETE CASCADE,
                    sequence INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    sources_json TEXT NOT NULL,
                    UNIQUE(review_set_id, sequence)
                );
                CREATE INDEX snapshots_review_set_id ON snapshots(review_set_id);

                CREATE TABLE review_units (
                    id TEXT PRIMARY KEY,
                    logical_key TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE UNIQUE INDEX review_units_logical_key ON review_units(logical_key);

                CREATE TABLE review_unit_versions (
                    id TEXT PRIMARY KEY,
                    unit_id TEXT NOT NULL REFERENCES review_units(id) ON DELETE CASCADE,
                    snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
                    kind TEXT NOT NULL,
                    title TEXT NOT NULL,
                    anchor_json TEXT NOT NULL,
                    provenance TEXT NOT NULL,
                    confidence TEXT NOT NULL
                );
                CREATE INDEX review_unit_versions_snapshot_id
                    ON review_unit_versions(snapshot_id);
                CREATE INDEX review_unit_versions_unit_id
                    ON review_unit_versions(unit_id);

                CREATE TABLE review_deltas (
                    snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
                    unit_id TEXT NOT NULL REFERENCES review_units(id) ON DELETE CASCADE,
                    from_version_id TEXT REFERENCES review_unit_versions(id) ON DELETE SET NULL,
                    to_version_id TEXT REFERENCES review_unit_versions(id) ON DELETE SET NULL,
                    transition TEXT NOT NULL,
                    carry_review_state INTEGER NOT NULL,
                    reason TEXT NOT NULL,
                    PRIMARY KEY(snapshot_id, unit_id)
                );
                CREATE INDEX review_deltas_to_version_id ON review_deltas(to_version_id);

                CREATE TABLE review_marks (
                    unit_version_id TEXT NOT NULL REFERENCES review_unit_versions(id) ON DELETE CASCADE,
                    reviewer TEXT NOT NULL,
                    state TEXT NOT NULL,
                    recorded_at TEXT NOT NULL,
                    PRIMARY KEY(unit_version_id, reviewer, state)
                );

                CREATE TABLE review_threads (
                    id TEXT PRIMARY KEY,
                    unit_version_id TEXT NOT NULL REFERENCES review_unit_versions(id) ON DELETE CASCADE,
                    status TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE review_comments (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    thread_id TEXT NOT NULL REFERENCES review_threads(id) ON DELETE CASCADE,
                    author TEXT NOT NULL,
                    body TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );

                CREATE TABLE events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    kind TEXT NOT NULL,
                    payload_json TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                ",
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
                params![1, Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }
        if current < 2 {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(
                "
                CREATE TABLE activity_read_cursors (
                    key TEXT PRIMARY KEY,
                    kind TEXT NOT NULL,
                    revision TEXT NOT NULL,
                    read_at TEXT NOT NULL
                );
                CREATE INDEX activity_read_cursors_kind
                    ON activity_read_cursors(kind);
                ",
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
                params![2, Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }
        Ok(())
    }

    pub fn save_project(&self, project: &WorkspaceProject) -> Result<()> {
        self.connection.execute(
            "INSERT INTO projects(id, name, description, archived, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                description = excluded.description,
                archived = excluded.archived,
                updated_at = excluded.updated_at",
            params![
                project.id.as_str(),
                project.name,
                project.description,
                project.archived,
                project.created_at.to_rfc3339(),
                project.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn project_by_name(&self, name: &str) -> Result<Option<WorkspaceProject>> {
        self.connection
            .query_row(
                "SELECT id, name, description, archived, created_at, updated_at
                 FROM projects WHERE name = ?1 AND archived = 0",
                [name],
                project_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_projects(&self, include_archived: bool) -> Result<Vec<WorkspaceProject>> {
        let sql = if include_archived {
            "SELECT id, name, description, archived, created_at, updated_at
             FROM projects ORDER BY archived, lower(name)"
        } else {
            "SELECT id, name, description, archived, created_at, updated_at
             FROM projects WHERE archived = 0 ORDER BY lower(name)"
        };
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map([], project_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn save_repository(&self, repository: &RepositoryRecord) -> Result<()> {
        self.connection.execute(
            "INSERT INTO repositories(
                id, project_id, name, provider, provider_owner, provider_name,
                normalized_remotes_json, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                project_id = excluded.project_id,
                name = excluded.name,
                provider = excluded.provider,
                provider_owner = excluded.provider_owner,
                provider_name = excluded.provider_name,
                normalized_remotes_json = excluded.normalized_remotes_json,
                updated_at = excluded.updated_at",
            params![
                repository.id.as_str(),
                repository.project_id.as_str(),
                repository.name,
                repository.provider,
                repository.provider_owner,
                repository.provider_name,
                serde_json::to_string(&repository.normalized_remotes)?,
                repository.created_at.to_rfc3339(),
                repository.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn save_checkout(&self, checkout: &CheckoutRecord) -> Result<()> {
        self.connection.execute(
            "INSERT INTO checkouts(id, repository_id, path, git_common_dir, available, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET
                repository_id = excluded.repository_id,
                git_common_dir = excluded.git_common_dir,
                available = excluded.available,
                last_seen_at = excluded.last_seen_at",
            params![
                checkout.id.as_str(),
                checkout.repository_id.as_str(),
                checkout.path.to_string_lossy(),
                checkout.git_common_dir.to_string_lossy(),
                checkout.available,
                checkout.last_seen_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn save_worktree(&self, worktree: &WorktreeRecord) -> Result<()> {
        self.connection.execute(
            "INSERT INTO worktrees(
                id, checkout_id, path, head, branch, locked, prunable, available, last_seen_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(path) DO UPDATE SET
                checkout_id = excluded.checkout_id,
                head = excluded.head,
                branch = excluded.branch,
                locked = excluded.locked,
                prunable = excluded.prunable,
                available = excluded.available,
                last_seen_at = excluded.last_seen_at",
            params![
                worktree.id.as_str(),
                worktree.checkout_id.as_str(),
                worktree.path.to_string_lossy(),
                worktree.head,
                worktree.branch,
                worktree.locked,
                worktree.prunable,
                worktree.available,
                worktree.last_seen_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_repositories(&self, project_id: &ProjectId) -> Result<Vec<RepositoryRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, name, provider, provider_owner, provider_name,
                    normalized_remotes_json, created_at, updated_at
             FROM repositories WHERE project_id = ?1 ORDER BY lower(name)",
        )?;
        let rows = statement.query_map([project_id.as_str()], |row| {
            Ok(RepositoryRecord {
                id: RepositoryId::from_string(row.get::<_, String>(0)?),
                project_id: ProjectId::from_string(row.get::<_, String>(1)?),
                name: row.get(2)?,
                provider: row.get(3)?,
                provider_owner: row.get(4)?,
                provider_name: row.get(5)?,
                normalized_remotes: serde_json::from_str(&row.get::<_, String>(6)?)
                    .unwrap_or_default(),
                created_at: parse_datetime(row.get::<_, String>(7)?),
                updated_at: parse_datetime(row.get::<_, String>(8)?),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn all_repositories(&self) -> Result<Vec<RepositoryRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, name, provider, provider_owner, provider_name,
                    normalized_remotes_json, created_at, updated_at
             FROM repositories ORDER BY lower(name)",
        )?;
        let rows = statement.query_map([], repository_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn repository(&self, id: &RepositoryId) -> Result<Option<RepositoryRecord>> {
        self.connection
            .query_row(
                "SELECT id, project_id, name, provider, provider_owner, provider_name,
                        normalized_remotes_json, created_at, updated_at
                 FROM repositories WHERE id = ?1",
                [id.as_str()],
                repository_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_checkouts(&self, repository_id: &RepositoryId) -> Result<Vec<CheckoutRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT id, repository_id, path, git_common_dir, available, last_seen_at
             FROM checkouts WHERE repository_id = ?1 ORDER BY lower(path)",
        )?;
        let rows = statement.query_map([repository_id.as_str()], |row| {
            Ok(CheckoutRecord {
                id: CheckoutId::from_string(row.get::<_, String>(0)?),
                repository_id: RepositoryId::from_string(row.get::<_, String>(1)?),
                path: PathBuf::from(row.get::<_, String>(2)?),
                git_common_dir: PathBuf::from(row.get::<_, String>(3)?),
                available: row.get(4)?,
                last_seen_at: parse_datetime(row.get::<_, String>(5)?),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn all_checkouts(&self) -> Result<Vec<CheckoutRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT id, repository_id, path, git_common_dir, available, last_seen_at
             FROM checkouts ORDER BY lower(path)",
        )?;
        let rows = statement.query_map([], checkout_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn checkout_by_path(&self, path: &Path) -> Result<Option<CheckoutRecord>> {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.connection
            .query_row(
                "SELECT id, repository_id, path, git_common_dir, available, last_seen_at
                 FROM checkouts WHERE path = ?1",
                [canonical.to_string_lossy().as_ref()],
                checkout_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_worktrees(&self, checkout_id: &CheckoutId) -> Result<Vec<WorktreeRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT id, checkout_id, path, head, branch, locked, prunable, available, last_seen_at
             FROM worktrees WHERE checkout_id = ?1 ORDER BY lower(path)",
        )?;
        let rows = statement.query_map([checkout_id.as_str()], |row| {
            Ok(WorktreeRecord {
                id: WorktreeId::from_string(row.get::<_, String>(0)?),
                checkout_id: CheckoutId::from_string(row.get::<_, String>(1)?),
                path: PathBuf::from(row.get::<_, String>(2)?),
                head: row.get(3)?,
                branch: row.get(4)?,
                locked: row.get(5)?,
                prunable: row.get(6)?,
                available: row.get(7)?,
                last_seen_at: parse_datetime(row.get::<_, String>(8)?),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn all_worktrees(&self) -> Result<Vec<WorktreeRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT id, checkout_id, path, head, branch, locked, prunable, available, last_seen_at
             FROM worktrees ORDER BY lower(path)",
        )?;
        let rows = statement.query_map([], worktree_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn find_worktree(&self, value: &str) -> Result<Option<WorktreeRecord>> {
        let canonical = std::fs::canonicalize(value).unwrap_or_else(|_| PathBuf::from(value));
        self.all_worktrees().map(|worktrees| {
            worktrees.into_iter().find(|worktree| {
                worktree.id.as_str() == value
                    || worktree.path == canonical
                    || worktree.path.to_string_lossy() == value
            })
        })
    }

    pub fn save_worktree_attention(&self, attention: &WorktreeAttention) -> Result<()> {
        self.connection.execute(
            UPSERT_WORKTREE_ATTENTION,
            params![
                attention.worktree_id.as_str(),
                attention.change_count as i64,
                attention.commit_count as i64,
                attention.base_ref,
                attention.fingerprint,
                attention.truncated,
                attention.error,
                attention.scanned_at.to_rfc3339(),
                attention.duration_ms as i64,
            ],
        )?;
        Ok(())
    }

    /// Store one scan generation in one transaction. This preserves the
    /// single SQLite owner while avoiding one durable commit per worktree.
    pub fn save_worktree_attentions(&self, attentions: &[WorktreeAttention]) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        for attention in attentions {
            transaction.execute(
                UPSERT_WORKTREE_ATTENTION,
                params![
                    attention.worktree_id.as_str(),
                    attention.change_count as i64,
                    attention.commit_count as i64,
                    attention.base_ref,
                    attention.fingerprint,
                    attention.truncated,
                    attention.error,
                    attention.scanned_at.to_rfc3339(),
                    attention.duration_ms as i64,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn all_worktree_attention(&self) -> Result<Vec<WorktreeAttention>> {
        let mut statement = self.connection.prepare(
            "SELECT worktree_id, change_count, commit_count, base_ref, fingerprint,
                    truncated, error, scanned_at, duration_ms
             FROM worktree_attention ORDER BY scanned_at DESC, worktree_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(WorktreeAttention {
                worktree_id: WorktreeId::from_string(row.get::<_, String>(0)?),
                change_count: row.get::<_, i64>(1)?.max(0) as usize,
                commit_count: row.get::<_, i64>(2)?.max(0) as usize,
                base_ref: row.get(3)?,
                fingerprint: row.get(4)?,
                truncated: row.get(5)?,
                error: row.get(6)?,
                scanned_at: parse_datetime(row.get::<_, String>(7)?),
                duration_ms: row.get::<_, i64>(8)?.max(0) as u64,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn save_inbox_preference(&self, preference: &InboxPreference) -> Result<()> {
        self.connection.execute(
            "INSERT INTO inbox_preferences(
                worktree_id, disposition, snoozed_until, baseline_signature, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(worktree_id) DO UPDATE SET
                disposition = excluded.disposition,
                snoozed_until = excluded.snoozed_until,
                baseline_signature = excluded.baseline_signature,
                updated_at = excluded.updated_at",
            params![
                preference.worktree_id.as_str(),
                inbox_disposition_name(preference.disposition),
                preference.snoozed_until.as_ref().map(DateTime::to_rfc3339),
                preference.baseline_signature.as_deref(),
                preference.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn remove_inbox_preference(&self, worktree_id: &WorktreeId) -> Result<()> {
        self.connection.execute(
            "DELETE FROM inbox_preferences WHERE worktree_id = ?1",
            [worktree_id.as_str()],
        )?;
        Ok(())
    }

    pub fn all_inbox_preferences(&self) -> Result<Vec<InboxPreference>> {
        let mut statement = self.connection.prepare(
            "SELECT worktree_id, disposition, snoozed_until, baseline_signature, updated_at
             FROM inbox_preferences ORDER BY updated_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(InboxPreference {
                worktree_id: WorktreeId::from_string(row.get::<_, String>(0)?),
                disposition: parse_inbox_disposition(&row.get::<_, String>(1)?),
                snoozed_until: row.get::<_, Option<String>>(2)?.map(parse_datetime),
                baseline_signature: row.get(3)?,
                updated_at: parse_datetime(row.get::<_, String>(4)?),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn save_activity_read_cursor(&self, cursor: &ActivityReadCursor) -> Result<()> {
        self.connection.execute(
            "INSERT INTO activity_read_cursors(key, kind, revision, read_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET
                kind = excluded.kind,
                revision = excluded.revision,
                read_at = excluded.read_at",
            params![
                cursor.key,
                activity_kind_name(cursor.kind),
                cursor.revision,
                cursor.read_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn all_activity_read_cursors(&self) -> Result<Vec<ActivityReadCursor>> {
        let mut statement = self.connection.prepare(
            "SELECT key, kind, revision, read_at
             FROM activity_read_cursors ORDER BY read_at DESC, key",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ActivityReadCursor {
                key: row.get(0)?,
                kind: parse_activity_kind(&row.get::<_, String>(1)?),
                revision: row.get(2)?,
                read_at: parse_datetime(row.get::<_, String>(3)?),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn repository_for_worktree(
        &self,
        worktree_id: &WorktreeId,
    ) -> Result<Option<RepositoryRecord>> {
        self.connection
            .query_row(
                "SELECT r.id, r.project_id, r.name, r.provider, r.provider_owner,
                        r.provider_name, r.normalized_remotes_json, r.created_at, r.updated_at
                 FROM repositories r
                 JOIN checkouts c ON c.repository_id = r.id
                 JOIN worktrees w ON w.checkout_id = c.id
                 WHERE w.id = ?1",
                [worktree_id.as_str()],
                repository_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn save_review(&self, review: &ReviewSet) -> Result<()> {
        self.connection.execute(
            "INSERT INTO review_sets(id, title, description, archived, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                description = excluded.description,
                archived = excluded.archived,
                updated_at = excluded.updated_at",
            params![
                review.id.as_str(),
                review.title,
                review.description,
                review.archived,
                review.created_at.to_rfc3339(),
                review.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_reviews(&self, include_archived: bool) -> Result<Vec<ReviewSet>> {
        let sql = if include_archived {
            "SELECT id, title, description, archived, created_at, updated_at
             FROM review_sets ORDER BY updated_at DESC"
        } else {
            "SELECT id, title, description, archived, created_at, updated_at
             FROM review_sets WHERE archived = 0 ORDER BY updated_at DESC"
        };
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map([], |row| {
            Ok(ReviewSet {
                id: ReviewSetId::from_string(row.get::<_, String>(0)?),
                title: row.get(1)?,
                description: row.get(2)?,
                archived: row.get(3)?,
                created_at: parse_datetime(row.get::<_, String>(4)?),
                updated_at: parse_datetime(row.get::<_, String>(5)?),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn attach_review_source(
        &self,
        review_set_id: &ReviewSetId,
        source: &ReviewSource,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO review_sources(review_set_id, source_json, created_at) VALUES (?1, ?2, ?3)",
            params![
                review_set_id.as_str(),
                serde_json::to_string(source)?,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn review_sources(&self, review_set_id: &ReviewSetId) -> Result<Vec<ReviewSource>> {
        let mut statement = self.connection.prepare(
            "SELECT source_json FROM review_sources WHERE review_set_id = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map([review_set_id.as_str()], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn next_snapshot_sequence(&self, review_set_id: &ReviewSetId) -> Result<u64> {
        let sequence = self.connection.query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM snapshots WHERE review_set_id = ?1",
            [review_set_id.as_str()],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(u64::try_from(sequence).unwrap_or(1))
    }

    pub fn save_checkpoint(&self, checkpoint: &ReviewCheckpoint) -> Result<()> {
        self.connection.execute(
            "INSERT INTO snapshots(id, review_set_id, sequence, created_at, sources_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                checkpoint.id.as_str(),
                checkpoint.review_set_id.as_str(),
                i64::try_from(checkpoint.sequence).unwrap_or(i64::MAX),
                checkpoint.created_at.to_rfc3339(),
                serde_json::to_string(&checkpoint.sources)?,
            ],
        )?;
        Ok(())
    }

    pub fn list_checkpoints(&self, review_set_id: &ReviewSetId) -> Result<Vec<ReviewCheckpoint>> {
        let mut statement = self.connection.prepare(
            "SELECT id, review_set_id, sequence, created_at, sources_json
             FROM snapshots WHERE review_set_id = ?1 ORDER BY sequence",
        )?;
        let rows = statement.query_map([review_set_id.as_str()], |row| {
            let sources = serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default();
            Ok(ReviewCheckpoint {
                id: workdeck_domain::SnapshotId::from_string(row.get::<_, String>(0)?),
                review_set_id: ReviewSetId::from_string(row.get::<_, String>(1)?),
                sequence: u64::try_from(row.get::<_, i64>(2)?).unwrap_or_default(),
                created_at: parse_datetime(row.get::<_, String>(3)?),
                sources,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn save_review_unit_version(
        &self,
        logical_key: &str,
        version: &ReviewUnitVersion,
    ) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO review_units(id, logical_key, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO NOTHING",
            params![
                version.unit_id.as_str(),
                logical_key,
                Utc::now().to_rfc3339(),
            ],
        )?;
        transaction.execute(
            "INSERT INTO review_unit_versions(
                id, unit_id, snapshot_id, kind, title, anchor_json, provenance, confidence
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                version.id.as_str(),
                version.unit_id.as_str(),
                version.snapshot_id.as_str(),
                serde_json::to_string(&version.kind)?,
                version.title,
                serde_json::to_string(&version.anchor)?,
                version.provenance,
                serde_json::to_string(&version.confidence)?,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn review_unit_id_for_key(&self, logical_key: &str) -> Result<Option<ReviewUnitId>> {
        self.connection
            .query_row(
                "SELECT id FROM review_units WHERE logical_key = ?1",
                [logical_key],
                |row| Ok(ReviewUnitId::from_string(row.get::<_, String>(0)?)),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn review_unit_versions_for_snapshot(
        &self,
        snapshot_id: &SnapshotId,
    ) -> Result<Vec<ReviewUnitVersion>> {
        let mut statement = self.connection.prepare(
            "SELECT id, unit_id, snapshot_id, kind, title, anchor_json, provenance, confidence
             FROM review_unit_versions WHERE snapshot_id = ?1 ORDER BY lower(title)",
        )?;
        let rows = statement.query_map([snapshot_id.as_str()], review_unit_version_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn review_unit_version(
        &self,
        id: &ReviewUnitVersionId,
    ) -> Result<Option<ReviewUnitVersion>> {
        self.connection
            .query_row(
                "SELECT id, unit_id, snapshot_id, kind, title, anchor_json, provenance, confidence
                 FROM review_unit_versions WHERE id = ?1",
                [id.as_str()],
                review_unit_version_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn save_review_delta(&self, snapshot_id: &SnapshotId, delta: &ReviewDelta) -> Result<()> {
        self.connection.execute(
            "INSERT INTO review_deltas(
                snapshot_id, unit_id, from_version_id, to_version_id, transition,
                carry_review_state, reason
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(snapshot_id, unit_id) DO UPDATE SET
                from_version_id = excluded.from_version_id,
                to_version_id = excluded.to_version_id,
                transition = excluded.transition,
                carry_review_state = excluded.carry_review_state,
                reason = excluded.reason",
            params![
                snapshot_id.as_str(),
                delta.unit_id.as_str(),
                delta.from_version.as_ref().map(|id| id.as_str()),
                delta.to_version.as_ref().map(|id| id.as_str()),
                serde_json::to_string(&delta.transition)?,
                delta.carry_review_state,
                delta.reason,
            ],
        )?;
        Ok(())
    }

    pub fn review_deltas_for_snapshot(&self, snapshot_id: &SnapshotId) -> Result<Vec<ReviewDelta>> {
        let mut statement = self.connection.prepare(
            "SELECT unit_id, from_version_id, to_version_id, transition,
                    carry_review_state, reason
             FROM review_deltas WHERE snapshot_id = ?1 ORDER BY unit_id",
        )?;
        let rows = statement.query_map([snapshot_id.as_str()], |row| {
            Ok(ReviewDelta {
                unit_id: ReviewUnitId::from_string(row.get::<_, String>(0)?),
                from_version: row
                    .get::<_, Option<String>>(1)?
                    .map(ReviewUnitVersionId::from_string),
                to_version: row
                    .get::<_, Option<String>>(2)?
                    .map(ReviewUnitVersionId::from_string),
                transition: serde_json::from_str(&row.get::<_, String>(3)?)
                    .unwrap_or(UnitTransition::Ambiguous),
                carry_review_state: row.get(4)?,
                reason: row.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn all_review_deltas(&self) -> Result<Vec<ReviewDelta>> {
        let mut statement = self.connection.prepare(
            "SELECT unit_id, from_version_id, to_version_id, transition,
                    carry_review_state, reason
             FROM review_deltas ORDER BY unit_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ReviewDelta {
                unit_id: ReviewUnitId::from_string(row.get::<_, String>(0)?),
                from_version: row
                    .get::<_, Option<String>>(1)?
                    .map(ReviewUnitVersionId::from_string),
                to_version: row
                    .get::<_, Option<String>>(2)?
                    .map(ReviewUnitVersionId::from_string),
                transition: serde_json::from_str(&row.get::<_, String>(3)?)
                    .unwrap_or(UnitTransition::Ambiguous),
                carry_review_state: row.get(4)?,
                reason: row.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn review_delta_for_version(
        &self,
        version_id: &ReviewUnitVersionId,
    ) -> Result<Option<ReviewDelta>> {
        self.connection
            .query_row(
                "SELECT unit_id, from_version_id, to_version_id, transition,
                        carry_review_state, reason
                 FROM review_deltas WHERE to_version_id = ?1",
                [version_id.as_str()],
                |row| {
                    Ok(ReviewDelta {
                        unit_id: ReviewUnitId::from_string(row.get::<_, String>(0)?),
                        from_version: row
                            .get::<_, Option<String>>(1)?
                            .map(ReviewUnitVersionId::from_string),
                        to_version: row
                            .get::<_, Option<String>>(2)?
                            .map(ReviewUnitVersionId::from_string),
                        transition: serde_json::from_str(&row.get::<_, String>(3)?)
                            .unwrap_or(UnitTransition::Ambiguous),
                        carry_review_state: row.get(4)?,
                        reason: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn save_review_mark(&self, mark: &ReviewMark) -> Result<()> {
        self.connection.execute(
            "INSERT INTO review_marks(unit_version_id, reviewer, state, recorded_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(unit_version_id, reviewer, state) DO UPDATE SET
                recorded_at = excluded.recorded_at",
            params![
                mark.unit_version_id.as_str(),
                mark.reviewer,
                review_mark_state(mark.state),
                mark.recorded_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn review_marks_for_version(
        &self,
        version_id: &ReviewUnitVersionId,
    ) -> Result<Vec<ReviewMark>> {
        let mut statement = self.connection.prepare(
            "SELECT unit_version_id, state, reviewer, recorded_at
             FROM review_marks WHERE unit_version_id = ?1 ORDER BY recorded_at",
        )?;
        let rows = statement.query_map([version_id.as_str()], |row| {
            Ok(ReviewMark {
                unit_version_id: ReviewUnitVersionId::from_string(row.get::<_, String>(0)?),
                state: parse_review_mark_state(&row.get::<_, String>(1)?),
                reviewer: row.get(2)?,
                recorded_at: parse_datetime(row.get::<_, String>(3)?),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn all_review_marks(&self) -> Result<Vec<ReviewMark>> {
        let mut statement = self.connection.prepare(
            "SELECT unit_version_id, state, reviewer, recorded_at
             FROM review_marks ORDER BY unit_version_id, recorded_at",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ReviewMark {
                unit_version_id: ReviewUnitVersionId::from_string(row.get::<_, String>(0)?),
                state: parse_review_mark_state(&row.get::<_, String>(1)?),
                reviewer: row.get(2)?,
                recorded_at: parse_datetime(row.get::<_, String>(3)?),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn append_event(&self, kind: &str, payload: &serde_json::Value) -> Result<()> {
        self.connection.execute(
            "INSERT INTO events(kind, payload_json, created_at) VALUES (?1, ?2, ?3)",
            params![
                kind,
                serde_json::to_string(payload)?,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }
}

fn project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceProject> {
    Ok(WorkspaceProject {
        id: ProjectId::from_string(row.get::<_, String>(0)?),
        name: row.get(1)?,
        description: row.get(2)?,
        archived: row.get(3)?,
        created_at: parse_datetime(row.get::<_, String>(4)?),
        updated_at: parse_datetime(row.get::<_, String>(5)?),
    })
}

fn repository_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepositoryRecord> {
    Ok(RepositoryRecord {
        id: RepositoryId::from_string(row.get::<_, String>(0)?),
        project_id: ProjectId::from_string(row.get::<_, String>(1)?),
        name: row.get(2)?,
        provider: row.get(3)?,
        provider_owner: row.get(4)?,
        provider_name: row.get(5)?,
        normalized_remotes: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
        created_at: parse_datetime(row.get::<_, String>(7)?),
        updated_at: parse_datetime(row.get::<_, String>(8)?),
    })
}

fn checkout_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CheckoutRecord> {
    Ok(CheckoutRecord {
        id: CheckoutId::from_string(row.get::<_, String>(0)?),
        repository_id: RepositoryId::from_string(row.get::<_, String>(1)?),
        path: PathBuf::from(row.get::<_, String>(2)?),
        git_common_dir: PathBuf::from(row.get::<_, String>(3)?),
        available: row.get(4)?,
        last_seen_at: parse_datetime(row.get::<_, String>(5)?),
    })
}

fn worktree_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorktreeRecord> {
    Ok(WorktreeRecord {
        id: WorktreeId::from_string(row.get::<_, String>(0)?),
        checkout_id: CheckoutId::from_string(row.get::<_, String>(1)?),
        path: PathBuf::from(row.get::<_, String>(2)?),
        head: row.get(3)?,
        branch: row.get(4)?,
        locked: row.get(5)?,
        prunable: row.get(6)?,
        available: row.get(7)?,
        last_seen_at: parse_datetime(row.get::<_, String>(8)?),
    })
}

fn review_unit_version_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewUnitVersion> {
    Ok(ReviewUnitVersion {
        id: ReviewUnitVersionId::from_string(row.get::<_, String>(0)?),
        unit_id: ReviewUnitId::from_string(row.get::<_, String>(1)?),
        snapshot_id: SnapshotId::from_string(row.get::<_, String>(2)?),
        kind: serde_json::from_str(&row.get::<_, String>(3)?)
            .unwrap_or(workdeck_domain::ReviewUnitKind::File),
        title: row.get(4)?,
        anchor: serde_json::from_str(&row.get::<_, String>(5)?)
            .unwrap_or_else(|_| workdeck_domain::ReviewAnchor::for_text(None, None, None, "", "")),
        provenance: row.get(6)?,
        confidence: serde_json::from_str(&row.get::<_, String>(7)?)
            .unwrap_or(workdeck_domain::AnalysisConfidence::Unresolved),
    })
}

fn parse_datetime(value: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(|_| DateTime::<Utc>::UNIX_EPOCH)
}

fn review_mark_state(state: ReviewMarkState) -> &'static str {
    match state {
        ReviewMarkState::Seen => "seen",
        ReviewMarkState::Reviewed => "reviewed",
        ReviewMarkState::Accepted => "accepted",
        ReviewMarkState::Questioned => "questioned",
        ReviewMarkState::Resolved => "resolved",
    }
}

fn parse_review_mark_state(value: &str) -> ReviewMarkState {
    match value {
        "seen" => ReviewMarkState::Seen,
        "reviewed" => ReviewMarkState::Reviewed,
        "accepted" => ReviewMarkState::Accepted,
        "questioned" => ReviewMarkState::Questioned,
        "resolved" => ReviewMarkState::Resolved,
        _ => ReviewMarkState::Seen,
    }
}

fn inbox_disposition_name(disposition: InboxDisposition) -> &'static str {
    match disposition {
        InboxDisposition::Active => "active",
        InboxDisposition::Pinned => "pinned",
        InboxDisposition::Snoozed => "snoozed",
        InboxDisposition::Baseline => "baseline",
    }
}

fn parse_inbox_disposition(value: &str) -> InboxDisposition {
    match value {
        "pinned" => InboxDisposition::Pinned,
        "snoozed" => InboxDisposition::Snoozed,
        "baseline" => InboxDisposition::Baseline,
        _ => InboxDisposition::Active,
    }
}

fn activity_kind_name(kind: ActivityKind) -> &'static str {
    match kind {
        ActivityKind::CommitBranch => "commit_branch",
        ActivityKind::PullRequest => "pull_request",
    }
}

fn parse_activity_kind(value: &str) -> ActivityKind {
    match value {
        "pull_request" => ActivityKind::PullRequest,
        _ => ActivityKind::CommitBranch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_domain::{
        ActivityKind, ActivityReadCursor, CheckoutRecord, RepositoryRecord, WorktreeRecord,
    };

    #[test]
    fn migrates_and_round_trips_catalog_hierarchy() {
        let catalog = Catalog::open_in_memory().expect("catalog");
        let project = WorkspaceProject::new("SamplePlatform");
        catalog.save_project(&project).expect("save project");
        let now = Utc::now();
        let repository = RepositoryRecord {
            id: RepositoryId::new(),
            project_id: project.id.clone(),
            name: "sampleplatform".to_string(),
            provider: Some("github".to_string()),
            provider_owner: Some("example".to_string()),
            provider_name: Some("sampleplatform".to_string()),
            normalized_remotes: vec!["github.com/example/sampleplatform".to_string()],
            created_at: now,
            updated_at: now,
        };
        catalog.save_repository(&repository).expect("save repo");
        let checkout = CheckoutRecord {
            id: CheckoutId::new(),
            repository_id: repository.id.clone(),
            path: "/tmp/sampleplatform".into(),
            git_common_dir: "/tmp/sampleplatform/.git".into(),
            available: true,
            last_seen_at: now,
        };
        catalog.save_checkout(&checkout).expect("save checkout");
        let worktree = WorktreeRecord {
            id: WorktreeId::new(),
            checkout_id: checkout.id.clone(),
            path: checkout.path.clone(),
            head: Some("abc".to_string()),
            branch: Some("main".to_string()),
            locked: false,
            prunable: false,
            available: true,
            last_seen_at: now,
        };
        catalog.save_worktree(&worktree).expect("save worktree");
        let attention = WorktreeAttention {
            worktree_id: worktree.id.clone(),
            change_count: 7,
            commit_count: 3,
            base_ref: Some("origin/main".into()),
            fingerprint: "attention-v1".into(),
            truncated: false,
            error: None,
            scanned_at: now,
            duration_ms: 12,
        };
        catalog
            .save_worktree_attention(&attention)
            .expect("save worktree attention");

        assert_eq!(
            catalog.list_projects(false).expect("projects"),
            vec![project]
        );
        assert_eq!(
            catalog
                .list_repositories(&repository.project_id)
                .expect("repos"),
            vec![repository]
        );
        assert_eq!(
            catalog
                .list_checkouts(&checkout.repository_id)
                .expect("checkouts"),
            vec![checkout]
        );
        assert_eq!(
            catalog
                .list_worktrees(&worktree.checkout_id)
                .expect("worktrees"),
            vec![worktree.clone()]
        );
        assert_eq!(
            catalog.all_worktree_attention().expect("attention"),
            vec![attention.clone()]
        );

        let failed_refresh = WorktreeAttention {
            change_count: 0,
            commit_count: 0,
            base_ref: None,
            truncated: false,
            error: Some("offline".into()),
            ..attention
        };
        catalog
            .save_worktree_attentions(&[failed_refresh])
            .expect("save failed refresh");
        let retained = catalog.all_worktree_attention().expect("attention");
        assert_eq!(retained[0].change_count, 7);
        assert_eq!(retained[0].commit_count, 3);
        assert_eq!(retained[0].base_ref.as_deref(), Some("origin/main"));
        assert_eq!(retained[0].error.as_deref(), Some("offline"));

        let preference = InboxPreference {
            worktree_id: worktree.id,
            disposition: InboxDisposition::Baseline,
            snoozed_until: None,
            baseline_signature: Some("abc:7:3".into()),
            updated_at: now,
        };
        catalog
            .save_inbox_preference(&preference)
            .expect("save inbox preference");
        assert_eq!(
            catalog.all_inbox_preferences().expect("preferences"),
            vec![preference.clone()]
        );
        catalog
            .remove_inbox_preference(&preference.worktree_id)
            .expect("clear preference");
        assert!(
            catalog
                .all_inbox_preferences()
                .expect("preferences")
                .is_empty()
        );

        let cursor = ActivityReadCursor {
            key: format!("commit_branch:{}", preference.worktree_id),
            kind: ActivityKind::CommitBranch,
            revision: "head-v1".into(),
            read_at: now,
        };
        catalog
            .save_activity_read_cursor(&cursor)
            .expect("save activity cursor");
        assert_eq!(
            catalog
                .all_activity_read_cursors()
                .expect("activity cursors"),
            vec![cursor.clone()]
        );
        let advanced = ActivityReadCursor {
            revision: "head-v2".into(),
            ..cursor
        };
        catalog
            .save_activity_read_cursor(&advanced)
            .expect("advance activity cursor");
        assert_eq!(
            catalog
                .all_activity_read_cursors()
                .expect("activity cursors"),
            vec![advanced]
        );
    }

    #[test]
    fn review_sources_keep_typed_payloads() {
        let catalog = Catalog::open_in_memory().expect("catalog");
        let review = ReviewSet::new("Migration");
        catalog.save_review(&review).expect("save review");
        let source = ReviewSource::PullRequest {
            provider: "github".to_string(),
            repository: "example/repo".to_string(),
            number: 42,
        };
        catalog
            .attach_review_source(&review.id, &source)
            .expect("attach source");
        assert_eq!(
            catalog.review_sources(&review.id).expect("sources"),
            vec![source]
        );
        assert_eq!(
            catalog
                .next_snapshot_sequence(&review.id)
                .expect("sequence"),
            1
        );
    }
}
