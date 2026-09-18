use crate::*;
use std::{
    collections::BTreeSet,
    path::{Component, Path},
};

pub(crate) fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
pub(crate) fn text(value: &str, name: &str, max: usize) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > max
        || value
            .chars()
            .any(|c| c == '\0' || (c.is_control() && c != '\n' && c != '\t'))
    {
        return Err(invalid(format!("{name} must be bounded nonempty text")));
    }
    Ok(())
}
pub(crate) fn leaf(value: &str) -> Result<()> {
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if value.is_empty()
        || value.len() > 255
        || value.ends_with(['.', ' '])
        || value
            .chars()
            .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
        || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(invalid("path components must be portable leaf names"));
    }
    Ok(())
}
pub(crate) fn id(value: &str) -> Result<()> {
    if !crate::identity::valid_slug(value) {
        return Err(invalid(
            "definition identities must be lowercase portable slugs",
        ));
    }
    leaf(value)
}
pub(crate) fn relative(path: &Path, allow_root: bool) -> Result<()> {
    let value = path
        .to_str()
        .ok_or_else(|| invalid("paths require UTF-8"))?;
    if allow_root && value == "." {
        return Ok(());
    }
    if value.is_empty()
        || path.is_absolute()
        || value.contains('\\')
        || value
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return Err(invalid("paths must be canonical repository-relative paths"));
    }
    for component in path.components() {
        if let Component::Normal(name) = component {
            leaf(
                name.to_str()
                    .ok_or_else(|| invalid("paths require UTF-8"))?,
            )?
        } else {
            return Err(invalid("paths cannot traverse outside the worktree"));
        }
    }
    Ok(())
}
pub(crate) fn environment_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 128
        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        || name.as_bytes()[0].is_ascii_digit()
    {
        return Err(invalid("environment names must be portable identifiers"));
    }
    Ok(())
}
pub(crate) fn extensions(
    custom: &std::collections::BTreeMap<String, serde_json::Value>,
    extra: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Result<()> {
    for key in custom.keys() {
        text(key, "custom key", 256)?;
    }
    for key in extra.keys() {
        if !key
            .strip_prefix("x-")
            .is_some_and(crate::identity::valid_slug)
        {
            return Err(invalid(format!(
                "unknown definition field {key:?}; use custom or x- extension fields"
            )));
        }
    }
    Ok(())
}
pub(crate) fn header(
    repository: &RepositoryId,
    expected: &RepositoryId,
    identity: &str,
    name: &str,
) -> Result<()> {
    if repository != expected {
        return Err(invalid("definition belongs to another repository"));
    }
    id(identity)?;
    text(name, "name", 512)
}
impl CommandDefinition {
    pub fn validate(&self) -> Result<()> {
        id(&self.id)?;
        text(&self.name, "name", 512)?;
        if self.description.len() > 64 * 1024 {
            return Err(invalid("description exceeds64KiB"));
        }
        relative(&self.cwd, true)?;
        self.inputs.validate()?;
        if self.bounds.timeout_seconds == 0
            || self.bounds.timeout_seconds > 86_400
            || self.bounds.stdout_bytes > 16 * 1024 * 1024
            || self.bounds.stderr_bytes > 16 * 1024 * 1024
        {
            return Err(invalid(
                "run bounds require timeout1..86400s and logs at most16MiB each",
            ));
        }
        if self.parameters.len() > 128
            || self.environment.len() > 128
            || self.tools.is_empty()
            || self.tools.len() > 64
            || self.artifacts.len() > 64
            || self.effects.len() > 128
        {
            return Err(invalid(
                "command definition exceeds bounded declaration counts",
            ));
        }
        for (name, param) in &self.parameters {
            id(name)?;
            match &param.value_type {
                ParameterType::String { max_bytes } | ParameterType::Strings { max_bytes, .. }
                    if *max_bytes == 0 || *max_bytes > 64 * 1024 =>
                {
                    return Err(invalid("parameter byte bound must be1..65536"));
                }
                ParameterType::Integer { minimum, maximum } if minimum > maximum => {
                    return Err(invalid("parameter integer range is reversed"));
                }
                ParameterType::Choice { values }
                    if values.is_empty()
                        || values.len() > 128
                        || values
                            .iter()
                            .any(|v| v.is_empty() || v.len() > 4096 || v.contains('\0'))
                        || values.iter().collect::<BTreeSet<_>>().len() != values.len() =>
                {
                    return Err(invalid(
                        "choice parameters need bounded unique nonempty values",
                    ));
                }
                _ => (),
            }
            if let ParameterType::Strings { max_items, .. } = &param.value_type
                && *max_items > 256
            {
                return Err(invalid("parameter list exceeds256 elements"));
            }
            if let Some(value) = &param.default {
                let _ = parameter_values(&param.value_type, value)?;
            }
        }
        for (name, value) in &self.environment {
            environment_name(name)?;
            match value {
                EnvironmentValue::Literal { value } => {
                    if value.len() > 64 * 1024 || value.contains('\0') {
                        return Err(invalid("literal environment value exceeds bounds"));
                    }
                }
                EnvironmentValue::Inherit { source, .. } => environment_name(source)?,
            }
        }
        let mut tools = BTreeSet::new();
        for tool in &self.tools {
            id(&tool.name)?;
            if !tools.insert(&tool.name) {
                return Err(invalid("duplicate tool identity"));
            }
            text(&tool.executable, "tool executable", 4096)?;
            if tool.executable.chars().any(char::is_control) {
                return Err(invalid("tool executable cannot contain controls"));
            }
        }
        let mut artifacts = BTreeSet::new();
        let mut artifact_names = BTreeSet::new();
        for artifact in &self.artifacts {
            id(&artifact.id)?;
            leaf(&artifact.name)?;
            if artifact.max_bytes == 0
                || artifact.max_bytes > 32 * 1024 * 1024
                || !artifacts.insert(&artifact.id)
                || !artifact_names.insert(artifact.name.to_ascii_lowercase())
            {
                return Err(invalid(
                    "artifacts need distinct identities/names and1..32MiB bounds",
                ));
            }
        }
        let tokens = match &self.recipe {
            CommandRecipe::Argv { argv } => {
                let Some(ArgumentToken::Literal { value }) = argv.first() else {
                    return Err(invalid("argv[0] must be a literal declared tool identity"));
                };
                if !tools.contains(value) {
                    return Err(invalid("argv[0] must name a declared tool"));
                }
                &argv[1..]
            }
            CommandRecipe::Shell {
                interpreter,
                script,
                args,
            } => {
                if !tools.contains(interpreter) {
                    return Err(invalid("shell interpreter must name a declared tool"));
                }
                text(script, "shell script", 64 * 1024)?;
                args.as_slice()
            }
        };
        if tokens.len() > 256 {
            return Err(invalid("recipe exceeds256 argument tokens"));
        }
        for token in tokens {
            match token {
                ArgumentToken::Literal { value } => {
                    if value.len() > 64 * 1024 || value.contains('\0') {
                        return Err(invalid("literal argv element exceeds bounds"));
                    }
                }
                ArgumentToken::Parameter { name } | ArgumentToken::Parameters { name } => {
                    let p = self.parameters.get(name).ok_or_else(|| {
                        invalid(format!(
                            "argument token references unknown parameter {name}"
                        ))
                    })?;
                    if matches!(token, ArgumentToken::Parameters { .. })
                        != matches!(p.value_type, ParameterType::Strings { .. })
                    {
                        return Err(invalid(
                            "parameter token cardinality does not match its schema",
                        ));
                    }
                }
                ArgumentToken::Artifact { id } => {
                    if !artifacts.contains(id) {
                        return Err(invalid("argument references unknown artifact"));
                    }
                }
            }
        }
        for effect in &self.effects {
            match effect {
                DeclaredEffect::Read { path } | DeclaredEffect::Write { path } => {
                    relative(path, true)?
                }
                DeclaredEffect::Network { description } | DeclaredEffect::Other { description } => {
                    text(description, "effect description", 4096)?
                }
            }
        }
        extensions(&self.custom, &self.extra)
    }
}
pub(crate) fn parameter_values(
    kind: &ParameterType,
    value: &serde_json::Value,
) -> Result<Vec<String>> {
    let bad = || {
        PmError::new(
            ErrorCode::InvalidInput,
            "argument value does not match its declared parameter schema",
        )
    };
    match kind {
        ParameterType::String { max_bytes } => {
            let value = value
                .as_str()
                .filter(|v| v.len() <= *max_bytes && !v.contains('\0'))
                .ok_or_else(bad)?;
            Ok(vec![value.into()])
        }
        ParameterType::Integer { minimum, maximum } => Ok(vec![
            value
                .as_i64()
                .filter(|v| v >= minimum && v <= maximum)
                .ok_or_else(bad)?
                .to_string(),
        ]),
        ParameterType::Boolean => Ok(vec![value.as_bool().ok_or_else(bad)?.to_string()]),
        ParameterType::Choice { values } => Ok(vec![
            value
                .as_str()
                .filter(|v| values.iter().any(|x| x == v))
                .ok_or_else(bad)?
                .into(),
        ]),
        ParameterType::Path => {
            let path = value.as_str().ok_or_else(bad)?;
            relative(Path::new(path), false).map_err(|_| bad())?;
            Ok(vec![path.into()])
        }
        ParameterType::Strings {
            max_items,
            max_bytes,
        } => {
            let values = value
                .as_array()
                .filter(|v| v.len() <= *max_items)
                .ok_or_else(bad)?;
            values
                .iter()
                .map(|v| {
                    v.as_str()
                        .filter(|v| v.len() <= *max_bytes && !v.contains('\0'))
                        .map(String::from)
                        .ok_or_else(bad)
                })
                .collect()
        }
    }
}
