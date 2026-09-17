use super::*;
use crate::execution::local;
use std::path::PathBuf;

impl Repository {
    /// Save original request material; loading it later does not establish freshness.
    pub fn save_claim_contract(&self, contract: &ClaimWorkContract) -> Result<PathBuf> {
        contract.validate()?;
        let bytes = serde_json::to_vec(contract).map_err(|e| invalid(e.to_string()))?;
        if bytes.len() > MAX_CLAIM_BYTES {
            return Err(invalid("claim contract exceeds 128 KiB"));
        }
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            if contract.accepted_source.repository != config.repository {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "claim contract belongs to another repository",
                ));
            }
            let directory = self.root().join(".local/claim-contracts");
            local::directory(self.root(), &directory)?;
            let ignore = directory.join(".gitignore");
            match local::read(self.root(), &ignore, 4096)? {
                Some(bytes) if bytes == b"*\n" => (),
                Some(_) => {
                    return Err(PmError::new(
                        ErrorCode::Conflict,
                        "existing claim-contract ignore policy must be preserved",
                    )
                    .at(ignore));
                }
                None => local::publish(self.root(), &ignore, b"*\n", None)?,
            }
            let path = directory.join(format!("{}.json", contract.fingerprint()?));
            match local::read(self.root(), &path, MAX_CLAIM_BYTES)? {
                Some(existing) if existing == bytes => (),
                Some(_) => {
                    return Err(PmError::new(
                        ErrorCode::Conflict,
                        "saved claim contract was edited; preserve and inspect it",
                    )
                    .at(path));
                }
                None => local::publish(self.root(), &path, &bytes, None)?,
            }
            Ok(path)
        })
    }

    pub fn load_claim_contract(&self, fingerprint: &ContentHash) -> Result<ClaimWorkContract> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            let path = self
                .root()
                .join(format!(".local/claim-contracts/{fingerprint}.json"));
            let bytes = local::read(self.root(), &path, MAX_CLAIM_BYTES)?.ok_or_else(|| {
                PmError::new(ErrorCode::NotFound, "saved claim contract not found").at(&path)
            })?;
            let contract: ClaimWorkContract =
                serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()).at(&path))?;
            contract.validate()?;
            if contract.fingerprint()? != *fingerprint
                || contract.accepted_source.repository != config.repository
            {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "saved claim contract identity disagrees with filename or repository",
                )
                .at(path));
            }
            Ok(contract)
        })
    }
}
