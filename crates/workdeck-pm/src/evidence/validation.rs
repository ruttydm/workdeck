use super::*;
use crate::gates::validation::{extensions, text};
use crate::*;
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
impl ProducerRef {
    pub fn validate(&self) -> Result<()> {
        text(&self.id, "producer identity", 256)
    }
}
impl CheckRef {
    pub fn validate(&self) -> Result<()> {
        text(&self.id, "check identity", 256)
    }
}
impl DeclareEvidence {
    pub fn validate(&self, repository: &RepositoryId, recorded_at: Timestamp) -> Result<()> {
        self.criterion.validate()?;
        self.producer.validate()?;
        self.check.validate()?;
        if &self.criterion.repository != repository || &self.subject.repository != repository {
            return Err(invalid(
                "evidence subject and criterion must belong to this repository",
            ));
        }
        text(&self.result.id, "result identity", 256)?;
        text(&self.provenance.actor, "evidence actor", 256)?;
        text(&self.provenance.reason, "declaration reason", 4096)?;
        if self.observed_at > recorded_at
            || self.expires_at.is_some_and(|time| time < self.observed_at)
        {
            return Err(invalid(
                "evidence observation cannot follow recording and expiry cannot precede observation",
            ));
        }
        if let Some(previous) = &self.supersedes {
            text(&previous.reason, "supersession reason", 4096)?;
        }
        if self.links.len() > 128 {
            return Err(invalid("evidence has more than 128 links"));
        }
        if self
            .links
            .iter()
            .filter(|link| matches!(link, EvidenceLink::Attestation { .. }))
            .count()
            > 1
        {
            return Err(invalid(
                "evidence can pin at most one attestation; supersede to select another proof",
            ));
        }
        for link in &self.links {
            match link {
                EvidenceLink::Source { link } => link.validate()?,
                EvidenceLink::Attestation { .. } => (),
                EvidenceLink::Url { url } => {
                    text(url, "evidence URL", 4096)?;
                    let authority = url
                        .strip_prefix("https://")
                        .and_then(|rest| rest.split('/').next())
                        .unwrap_or_default();
                    if authority.is_empty()
                        || authority.contains('@')
                        || authority.contains('\\')
                        || url.chars().any(char::is_whitespace)
                    {
                        return Err(invalid(
                            "evidence URLs must be inert HTTPS references without credentials or whitespace",
                        ));
                    }
                }
            }
        }
        extensions(&self.custom, &self.extra)
    }
}
impl EvidenceReference {
    pub fn validate(&self) -> Result<()> {
        self.declaration
            .validate(&self.repository, self.recorded_at)?;
        if self
            .declaration
            .supersedes
            .as_ref()
            .is_some_and(|s| s.id == self.id)
        {
            return Err(invalid("evidence cannot supersede itself"));
        }
        Ok(())
    }
}
