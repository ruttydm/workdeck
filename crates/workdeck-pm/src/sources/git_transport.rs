use super::{git::BoundGit, *};
use crate::{ContentHash, ErrorCode, PmError, Result};
use std::ffi::OsString;

fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
impl BoundGit {
    pub(super) fn remote_url(&self, name: &str) -> Result<String> {
        let bytes = self.output(
            &[
                "remote".into(),
                "get-url".into(),
                "--all".into(),
                name.into(),
            ],
            None,
            64 * 1024,
        )?;
        let value = std::str::from_utf8(&bytes)
            .map_err(|_| invalid("remote URL must be UTF8"))?
            .trim_end_matches('\n');
        if value.is_empty()
            || value.len() > 8192
            || value.chars().any(char::is_control)
            || value.starts_with('-')
            || value.contains("::")
        {
            return Err(invalid(
                "remote must have one explicit supported transport URL",
            ));
        }
        if let Some((scheme, _)) = value.split_once("://")
            && !matches!(scheme, "file" | "https" | "http" | "ssh")
        {
            return Err(invalid("unsupported Git remote transport"));
        }
        let push = self.output(
            &[
                "remote".into(),
                "get-url".into(),
                "--push".into(),
                "--all".into(),
                name.into(),
            ],
            None,
            64 * 1024,
        )?;
        if push != bytes {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "coordination currently requires matching fetch and push URLs for its configured remote",
            ));
        }
        Ok(value.into())
    }
    pub(super) fn observe(
        &self,
        name: &str,
        url: &str,
        references: &[GitRefName],
    ) -> Result<Vec<RemoteRefObservation>> {
        let mut args: Vec<OsString> =
            vec!["ls-remote".into(), "--refs".into(), "--".into(), url.into()];
        args.extend(references.iter().map(|r| r.as_str().into()));
        let result = self.run(&args, None, None, 64 * 1024)?;
        if !result.status.success() {
            let _ = result.stderr;
            return Err(PmError::new(
                ErrorCode::Io,
                "could not observe the configured remote; no publication is confirmed",
            ));
        }
        let mut found = std::collections::BTreeMap::new();
        for line in result
            .stdout
            .split(|&b| b == b'\n')
            .filter(|line| !line.is_empty())
        {
            let line = std::str::from_utf8(line)
                .map_err(|_| invalid("remote advertisement must be UTF8"))?;
            let (oid, reference) = line
                .split_once('\t')
                .ok_or_else(|| invalid("invalid remote advertisement"))?;
            let reference: GitRefName = reference.parse()?;
            if !references.contains(&reference)
                || found.insert(reference, oid.parse::<GitOid>()?).is_some()
            {
                return Err(invalid(
                    "remote advertised an unexpected or duplicate reference",
                ));
            }
        }
        let now = chrono::Utc::now();
        Ok(references
            .iter()
            .map(|reference| RemoteRefObservation {
                remote: name.into(),
                reference: reference.clone(),
                commit: found.get(reference).cloned(),
                observed_at: now,
            })
            .collect())
    }
    pub(super) fn fetch_object(&self, url: &str, commit: &GitOid) -> Result<()> {
        let output = self.run(
            &[
                "fetch".into(),
                "--no-tags".into(),
                "--no-write-fetch-head".into(),
                "--no-recurse-submodules".into(),
                "--no-auto-maintenance".into(),
                "--".into(),
                url.into(),
                commit.as_str().into(),
            ],
            None,
            None,
            64 * 1024,
        )?;
        if !output.status.success() {
            return Err(PmError::new(
                ErrorCode::Io,
                "could not fetch the exact observed source commit",
            ));
        }
        let kind = self.output(
            &["cat-file".into(), "-t".into(), commit.as_str().into()],
            None,
            64,
        )?;
        if kind != b"commit\n" {
            return Err(invalid("observed source must identify a commit"));
        }
        Ok(())
    }
    pub(super) fn update_cache(
        &self,
        reference: &GitRefName,
        value: Option<&GitOid>,
    ) -> Result<()> {
        if !reference.as_str().starts_with("refs/workdeck/cache/") {
            return Err(invalid("cache updates cannot modify developer refs"));
        }
        let previous = self.resolve(reference.as_str())?;
        if previous.as_ref() == value {
            return Ok(());
        }
        let args = if let Some(value) = value {
            vec![
                "update-ref".into(),
                reference.as_str().into(),
                value.as_str().into(),
                previous
                    .as_ref()
                    .map(|p| p.as_str().to_owned())
                    .unwrap_or_else(|| "0".repeat(value.as_str().len()))
                    .into(),
            ]
        } else if let Some(previous) = previous {
            vec![
                "update-ref".into(),
                "-d".into(),
                reference.as_str().into(),
                previous.as_str().into(),
            ]
        } else {
            return Ok(());
        };
        let output = self.run(&args, None, None, 4096)?;
        if !output.status.success() {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "private source cache changed concurrently",
            ));
        }
        Ok(())
    }
    pub(super) fn binding(
        &self,
        repository: &crate::RepositoryId,
        shared: &SharedSources,
        url: &str,
    ) -> Result<ContentHash> {
        crate::transactions::canonical_hash(
            &serde_json::json!({"repository":repository,"shared":shared,"remote_url":ContentHash::of(url.as_bytes()),"git":self.local_identity()}),
        )
    }
}
