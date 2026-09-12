//! Reopen one exact working-tree citation for an existing authoring controller.
//! The result supplies a native source token, never completion or write authority.
use super::{PROJECTION_SCHEMA_VERSION, ProjectionRowToken, cache::slot_hash};
use crate::{
    Config, ContentHash, ErrorCode, FeatureRecord, IssueId, IssueRecord, PmError, Repository,
    Result, RetirementKind, RetirementTarget, SnapshotKind, SourceRole, SourceSelector,
    documents::MarkdownDocument, sources::fs, transactions::Snapshot,
};
use std::path::Path;

fn stale() -> PmError {
    PmError::new(
        ErrorCode::StaleSource,
        "indexed citation differs from this working-tree source; refresh and select it again",
    )
}
fn slot(repository: &Repository) -> Result<ContentHash> {
    let worktree = repository.root().parent().ok_or_else(stale)?;
    let worktree = worktree
        .canonicalize()
        .map_err(|error| PmError::io(worktree, error))?;
    slot_hash(
        &worktree,
        fs::directory(&worktree)?,
        fs::directory(repository.root())?,
        &SourceSelector::WorkingTree,
    )
}

fn read<T>(
    repository: &Repository,
    token: &ProjectionRowToken,
    kind: SnapshotKind,
    parse: impl FnOnce(&Snapshot<'_>, &Config, &[u8]) -> Result<T>,
) -> Result<T> {
    if token.view.schema != PROJECTION_SCHEMA_VERSION
        || token.key.kind != kind
        || token.key.repository != *repository.identity()
        || token.view.source.repository != *repository.identity()
        || token.view.slot != slot(repository)?
    {
        return Err(stale());
    }
    let result = repository.store()?.with_snapshot(|snapshot| {
        let config = crate::repository::config_from_snapshot(repository.root(), snapshot)?;
        // Shared working-tree captures carry Proposal role. The physical slot
        // above pins the WorkingTree selector, excluding ref-backed proposals.
        let role = if config.sources.is_some() {
            SourceRole::Proposal
        } else {
            SourceRole::Local
        };
        if token.view.source.role != role {
            return Err(stale());
        }
        let bytes = snapshot
            .read_bounded(&token.path, crate::documents::MAX_DOCUMENT_BYTES)?
            .ok_or_else(stale)?;
        if ContentHash::of(&bytes) != token.content {
            return Err(stale());
        }
        parse(snapshot, &config, &bytes)
    })?;
    // Keep the physical checkout pin through the native locked read as well.
    if token.view.slot != slot(repository)? {
        return Err(stale());
    }
    Ok(result)
}

impl Repository {
    /// Resolve exactly this indexed working-tree record. Unrelated source changes
    /// may coexist; ordinary mutations still validate current policy and the returned
    /// source token. Ref-backed rows must not be silently routed to a working tree.
    pub fn issue_from_projection(&self, token: &ProjectionRowToken) -> Result<IssueRecord> {
        let id: IssueId = token.key.id.parse()?;
        if token.path != Path::new(&format!("issues/{id}/item.md")) {
            return Err(stale());
        }
        read(
            self,
            token,
            SnapshotKind::Issue,
            |snapshot, config, bytes| {
                let text = std::str::from_utf8(bytes).map_err(|_| stale())?;
                let document = MarkdownDocument::parse(&token.path, text)?;
                let metadata = crate::issues::parse_issue_metadata(&token.path, &document)?;
                metadata.validate(config)?;
                if metadata.id != id {
                    return Err(stale());
                }
                let mut record =
                    crate::issues::record_from_document(metadata, &document, token.path.clone());
                record.retirement = crate::retirement::read_tombstone(
                    self.root(),
                    snapshot,
                    config,
                    &RetirementTarget::new(RetirementKind::Issue, id.as_str())?,
                )?;
                Ok(record)
            },
        )
    }

    /// Native-feature counterpart to `issue_from_projection`; exact path and
    /// source checks preserve independently pinned feature authoring state.
    pub fn feature_from_projection(&self, token: &ProjectionRowToken) -> Result<FeatureRecord> {
        read(
            self,
            token,
            SnapshotKind::Feature,
            |snapshot, config, bytes| {
                let mut record = crate::features::parse(&token.path, bytes, &config.repository)?;
                if record.metadata.id.as_str() != token.key.id {
                    return Err(stale());
                }
                record.retirement = crate::retirement::read_tombstone(
                    self.root(),
                    snapshot,
                    config,
                    &RetirementTarget::new(RetirementKind::Feature, &token.key.id)?,
                )?;
                Ok(record)
            },
        )
    }
}

impl Repository {
    /// Reopen one indexed planning record in its exact native working-tree slot.
    /// Labels share a canonical source file; normal mutation policy still applies.
    pub fn planning_from_projection(
        &self,
        kind: crate::PlanningKind,
        token: &ProjectionRowToken,
    ) -> Result<crate::PlanningRecord> {
        let expected_kind = super::projector::extract::planning_kind(kind);
        if token.path != crate::planning::store::record_path(kind, &token.key.id) {
            return Err(stale());
        }
        read(self, token, expected_kind, |snapshot, config, bytes| {
            let mut record = if kind == crate::PlanningKind::Label {
                crate::planning::store::load_planning(self.root(), snapshot, kind, &token.key.id)?
            } else {
                crate::planning::store::parse_planning_bytes(
                    self.root(),
                    kind,
                    &token.key.id,
                    bytes,
                )?
            };
            if record.source.content != token.content {
                return Err(stale());
            }
            record.retirement = crate::retirement::read_tombstone(
                self.root(),
                snapshot,
                config,
                &RetirementTarget::new(kind.into(), &token.key.id)?,
            )?;
            Ok(record)
        })
    }
}
