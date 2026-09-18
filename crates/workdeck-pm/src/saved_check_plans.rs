//! Disposable reviewed plans. Saving never records execution or grants authority.
use crate::execution::local;
use crate::{CheckPlan, ContentHash, ErrorCode, MAX_RUN_RECORD_BYTES, PmError, Repository, Result};
use std::path::{Path, PathBuf};

fn location(root: &Path, fingerprint: &ContentHash) -> PathBuf {
    root.join(".local/plans")
        .join(format!("{fingerprint}.json"))
}

impl Repository {
    /// Persist an exact plan under ignored local state, without running a recipe.
    pub fn save_check_plan(&self, plan: &CheckPlan) -> Result<PathBuf> {
        plan.validate()?;
        let bytes = serde_json::to_vec(plan)
            .map_err(|e| PmError::new(ErrorCode::InvalidInput, e.to_string()))?;
        if bytes.len() > MAX_RUN_RECORD_BYTES {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "saved plan exceeds the bounded plan size",
            ));
        }
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            if config.repository != plan.repository {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "plan belongs to another repository",
                ));
            }
            let directory = self.root().join(".local/plans");
            local::directory(self.root(), &directory)?;
            let ignore = directory.join(".gitignore");
            match local::read(self.root(), &ignore, 4096)? {
                Some(bytes) if bytes == b"*\n" => (),
                Some(_) => return Err(PmError::new(
                    ErrorCode::Conflict,
                    "preserve existing plan ignore policy; expected '*' before saving local plans",
                )
                .at(ignore)),
                None => local::publish(self.root(), &ignore, b"*\n", None)?,
            }
            let path = location(self.root(), &plan.fingerprint);
            match local::read(self.root(), &path, MAX_RUN_RECORD_BYTES)? {
                Some(existing) if existing == bytes => (),
                Some(_) => return Err(PmError::new(
                    ErrorCode::Conflict,
                    "saved plan was modified; preserve it and inspect the conflicting local file",
                )
                .at(path)),
                None => local::publish(self.root(), &path, &bytes, None)?,
            }
            Ok(path)
        })
    }

    /// Load historical plan bytes without implying that their inputs remain current.
    pub fn load_check_plan(&self, fingerprint: &ContentHash) -> Result<CheckPlan> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            let path = location(self.root(), fingerprint);
            let bytes =
                local::read(self.root(), &path, MAX_RUN_RECORD_BYTES)?.ok_or_else(|| {
                    PmError::new(ErrorCode::NotFound, "saved check plan was not found").at(&path)
                })?;
            let plan: CheckPlan = serde_json::from_slice(&bytes).map_err(|e| {
                PmError::new(ErrorCode::InvalidSchema, format!("invalid saved plan: {e}")).at(&path)
            })?;
            plan.validate()?;
            if &plan.fingerprint != fingerprint || plan.repository != config.repository {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "saved plan identity does not match this repository and filename",
                )
                .at(path));
            }
            Ok(plan)
        })
    }
}
