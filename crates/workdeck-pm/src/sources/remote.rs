use super::*;
use crate::{ErrorCode, PmError, Repository, RequestId, Result};

impl Repository {
    pub fn source_status(&self) -> Result<SourceStatus> {
        let root = self.root().parent().ok_or_else(|| {
            PmError::new(ErrorCode::UnsafePath, "planning source has no worktree")
        })?;
        if self
            .root()
            .file_name()
            .is_none_or(|name| name != ".workdeck")
        {
            return Err(PmError::new(
                ErrorCode::UnsafePath,
                "Git sources require canonical .workdeck binding",
            ));
        }
        let working = capture(
            root,
            &SourceSelector::WorkingTree,
            &SourceCaptureLimits::default(),
        )?;
        if &working.observation.identity.repository != self.identity() {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "planning repository identity changed before source status capture",
            ));
        }
        let config = working.snapshot.config()?;
        let binding = working.publication_binding().cloned();
        let hash = crate::ContentHash::of(
            working
                .snapshot
                .files()
                .get(std::path::Path::new("config.yml"))
                .ok_or_else(|| PmError::new(ErrorCode::CorruptStore, "source config missing"))?,
        );
        let mut status = SourceStatus {
            repository: self.identity().clone(),
            config: hash,
            binding: binding.clone(),
            shared: config.sources.clone(),
            working: working.observation.clone(),
            accepted: None,
            coordination: None,
            errors: if config.sources.is_some() && binding.is_none() {
                vec![PmError::new(
                    ErrorCode::PolicyBlocked,
                    "shared publication binding is unavailable; local source inspection does not authorize publication",
                )]
            } else {
                Vec::new()
            },
        };
        if config.sources.is_some() {
            for (selector, role) in [
                (SourceSelector::Accepted, SourceRole::Accepted),
                (SourceSelector::Coordination, SourceRole::Coordination),
            ] {
                match capture(root, &selector, &SourceCaptureLimits::default()) {
                    Ok(view) => {
                        if role == SourceRole::Accepted {
                            status.accepted = Some(view.observation);
                        } else {
                            status.coordination = Some(view.observation);
                        }
                    }
                    Err(error) => status.errors.push(error),
                }
            }
        }
        working.revalidate()?;
        Ok(status)
    }
    pub fn fetch_sources(
        &self,
        request: &SourceFetchRequest,
        id: &RequestId,
    ) -> Result<SourceFetchOutcome> {
        super::remote_impl::fetch(self, request, id, false)
    }
    pub fn sync_sources(
        &self,
        request: &SourceFetchRequest,
        id: &RequestId,
    ) -> Result<SourceFetchOutcome> {
        super::remote_impl::fetch(self, request, id, true)
    }
}

pub(crate) fn reject_coordination_snapshot(
    snapshot: &crate::transactions::Snapshot<'_>,
) -> Result<()> {
    if snapshot
        .read_bounded(std::path::Path::new("coordination.yml"), 64 * 1024)?
        .is_some()
    {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "coordination source cannot be opened for ordinary planning writes",
        )
        .at("coordination.yml"));
    }
    Ok(())
}
pub(crate) fn parse_coordination_marker(
    path: &std::path::Path,
    bytes: &[u8],
    repository: &crate::RepositoryId,
) -> Result<CoordinationMarker> {
    if bytes.len() > 64 * 1024 {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "coordination marker exceeds 64 KiB",
        )
        .at(path));
    }
    let marker: CoordinationMarker = serde_yaml_ng::from_slice(bytes)
        .map_err(|e| PmError::new(ErrorCode::InvalidSchema, e.to_string()).at(path))?;
    if &marker.repository != repository
        || !marker.coordination_ref.as_str().starts_with("refs/heads/")
    {
        return Err(PmError::new(
            ErrorCode::InvalidSchema,
            "coordination marker repository or ref identity is invalid",
        )
        .at(path));
    }
    Ok(marker)
}
