use crate::commands::validation::{extensions, id, invalid, relative, text};
use crate::*;
use std::collections::BTreeSet;
impl ReportExpectation {
    pub fn validate(&self) -> Result<()> {
        let codes = match self {
            Self::Process { allowed_exit_codes } => allowed_exit_codes,
            Self::JUnit {
                artifact,
                suites,
                minimum_tests,
                allowed_exit_codes,
                ..
            } => {
                id(artifact)?;
                if suites.is_empty()
                    || suites.len() > 128
                    || *minimum_tests == 0
                    || suites.iter().collect::<BTreeSet<_>>().len() != suites.len()
                {
                    return Err(invalid(
                        "JUnit needs unique expected suites and minimum_tests>=1",
                    ));
                }
                for suite in suites {
                    text(suite, "JUnit suite", 1024)?
                }
                allowed_exit_codes
            }
            Self::Sarif {
                artifact,
                tool,
                minimum_invocations,
                failure_levels,
                allowed_exit_codes,
            } => {
                id(artifact)?;
                text(tool, "SARIF tool", 1024)?;
                if *minimum_invocations == 0
                    || failure_levels.is_empty()
                    || failure_levels
                        .iter()
                        .enumerate()
                        .any(|(i, v)| failure_levels[..i].contains(v))
                {
                    return Err(invalid(
                        "SARIF needs minimum_invocations>=1 and unique failure levels",
                    ));
                }
                allowed_exit_codes
            }
        };
        if codes.is_empty()
            || codes.len() > 256
            || codes.iter().any(|n| !(0..=255).contains(n))
            || codes.iter().collect::<BTreeSet<_>>().len() != codes.len()
        {
            return Err(invalid("allowed_exit_codes must be unique integers0..255"));
        }
        Ok(())
    }
}
impl CheckDefinition {
    pub fn validate(&self) -> Result<()> {
        id(&self.id)?;
        text(&self.name, "name", 512)?;
        id(&self.command)?;
        self.expectation.validate()?;
        if let Some(requirement) = &self.red_green {
            requirement.validate(&self.expectation)?;
        }
        if let Some(selection) = &self.evaluator_inputs {
            selection.validate()?;
        }
        if self.description.len() > 64 * 1024
            || self.arguments.len() > 128
            || self.affected.len() > 4096
        {
            return Err(invalid("check exceeds bounded declaration counts"));
        }
        for path in &self.affected {
            relative(path, true)?
        }
        extensions(&self.custom, &self.extra)
    }
}
impl CheckProfileDefinition {
    pub fn validate(&self) -> Result<()> {
        id(&self.id)?;
        text(&self.name, "name", 512)?;
        if self.description.len() > 64 * 1024
            || self.checks.is_empty()
            || self.checks.len() > 4096
            || self.checks.iter().collect::<BTreeSet<_>>().len() != self.checks.len()
        {
            return Err(invalid("profile needs bounded unique required checks"));
        }
        for check in &self.checks {
            id(check)?
        }
        extensions(&self.custom, &self.extra)
    }
}
pub(crate) fn references(catalog: &CommandCatalogSnapshot) -> Vec<PmError> {
    let mut errors = Vec::new();
    for check in &catalog.checks {
        let Some(command) = catalog
            .commands
            .iter()
            .find(|c| c.definition.id == check.definition.command)
        else {
            errors.push(
                invalid(format!(
                    "check {} references missing command {}",
                    check.definition.id, check.definition.command
                ))
                .at(&check.path),
            );
            continue;
        };
        if let Some(selection) = &check.definition.evaluator_inputs
            && let Err(error) = selection.validate_runtime_inputs(&command.definition.inputs)
        {
            errors.push(error.at(&check.path));
        }
        let artifact = match &check.definition.expectation {
            ReportExpectation::Process { .. } => None,
            ReportExpectation::JUnit { artifact, .. }
            | ReportExpectation::Sarif { artifact, .. } => Some(artifact),
        };
        if artifact.is_some_and(|id| !command.definition.artifacts.iter().any(|a| &a.id == id)) {
            errors.push(invalid("check report references an undeclared artifact").at(&check.path))
        }
        for (name, value) in &check.definition.arguments {
            match command.definition.parameters.get(name) {
                Some(parameter) => {
                    if let Err(error) =
                        crate::commands::validation::parameter_values(&parameter.value_type, value)
                    {
                        errors.push(error.at(&check.path))
                    }
                }
                None => errors.push(
                    invalid(format!(
                        "check arguments reference unknown parameter {name}"
                    ))
                    .at(&check.path),
                ),
            }
        }
    }
    for profile in &catalog.profiles {
        for id in &profile.definition.checks {
            if !catalog.checks.iter().any(|c| &c.definition.id == id) {
                errors.push(
                    invalid(format!("profile references missing check {id}")).at(&profile.path),
                )
            }
        }
    }
    errors
}
