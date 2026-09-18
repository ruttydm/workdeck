use super::*;
use crate::*;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn text(value: &str, label: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{label} must be nonempty text without controls, at most {max} bytes"
        )));
    }
    Ok(())
}
pub(crate) fn extensions(
    custom: &BTreeMap<String, Value>,
    extra: &BTreeMap<String, Value>,
) -> Result<()> {
    for key in custom.keys() {
        text(key, "custom key", 128)?;
    }
    for key in extra.keys() {
        if !key
            .strip_prefix("x-")
            .is_some_and(crate::identity::valid_slug)
        {
            return Err(invalid(format!(
                "unknown metadata {key:?}; use custom or x- extensions"
            )));
        }
    }
    Ok(())
}
impl GateDefinition {
    pub fn validate(&self) -> Result<()> {
        text(&self.name, "gate name", 512)?;
        if self.description.len() > 32768
            || self.description.contains('\0')
            || self.updated_at < self.created_at
        {
            return Err(invalid("invalid gate description or timestamps"));
        }
        validate_requirements(&self.repository, &self.requirements)?;
        extensions(&self.custom, &self.extra)
    }
}
pub(super) fn validate_requirements(
    repository: &RepositoryId,
    requirements: &[GateRequirement],
) -> Result<()> {
    if requirements.is_empty() || requirements.len() > 256 {
        return Err(invalid(
            "gate requires between one and 256 AND requirements",
        ));
    }
    let mut ids = BTreeSet::new();
    for r in requirements {
        if !crate::identity::valid_slug(&r.id) || !ids.insert(&r.id) {
            return Err(invalid("gate requirement IDs must be unique stable slugs"));
        }
        r.criterion.validate()?;
        if &r.criterion.repository != repository {
            return Err(invalid("gate criterion belongs to another repository"));
        }
        r.producer.validate()?;
        r.check.validate()?;
        if r.max_age_seconds.is_some_and(|v| v == 0 || v > 315576000) {
            return Err(invalid(
                "evidence maximum age must be 1 through 315576000 seconds",
            ));
        }
    }
    Ok(())
}
