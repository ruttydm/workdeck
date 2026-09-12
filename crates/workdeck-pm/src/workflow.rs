use crate::{ErrorCode, PmError, Result, identity::valid_slug};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowCategory {
    Triage,
    Backlog,
    Unstarted,
    Started,
    Review,
    Verification,
    Completed,
    Canceled,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowState {
    pub id: String,
    pub name: String,
    pub category: WorkflowCategory,
    pub transitions: Vec<String>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub initial: String,
    pub states: Vec<WorkflowState>,
}

impl Default for Workflow {
    fn default() -> Self {
        let definitions = [
            ("inbox", "Inbox", WorkflowCategory::Triage),
            ("backlog", "Backlog", WorkflowCategory::Backlog),
            ("ready", "Ready", WorkflowCategory::Unstarted),
            ("in_progress", "In progress", WorkflowCategory::Started),
            ("in_review", "In review", WorkflowCategory::Review),
            (
                "verification",
                "Verification",
                WorkflowCategory::Verification,
            ),
            ("done", "Done", WorkflowCategory::Completed),
            ("canceled", "Canceled", WorkflowCategory::Canceled),
        ];
        let states = definitions
            .iter()
            .map(|(id, name, category)| WorkflowState {
                id: (*id).into(),
                name: (*name).into(),
                category: *category,
                transitions: if matches!(
                    category,
                    WorkflowCategory::Completed | WorkflowCategory::Canceled
                ) {
                    Vec::new()
                } else {
                    definitions
                        .iter()
                        .filter(|(next, _, _)| next != id)
                        .map(|(next, _, _)| (*next).into())
                        .collect()
                },
            })
            .collect();
        Self {
            initial: "ready".into(),
            states,
        }
    }
}

impl Workflow {
    pub fn validate(&self) -> Result<()> {
        let mut ids = BTreeSet::new();
        for state in &self.states {
            if !valid_slug(&state.id) || state.name.trim().is_empty() || !ids.insert(&state.id) {
                return Err(PmError::new(
                    ErrorCode::InvalidSchema,
                    "workflow states require unique path-safe IDs and nonempty names",
                ));
            }
        }
        self.state(&self.initial)?;
        for state in &self.states {
            for next in &state.transitions {
                self.state(next)?;
            }
            if matches!(
                state.category,
                WorkflowCategory::Completed | WorkflowCategory::Canceled
            ) && !state.transitions.is_empty()
            {
                return Err(PmError::new(
                    ErrorCode::InvalidSchema,
                    "terminal workflow states must be reopened explicitly",
                ));
            }
        }
        Ok(())
    }

    pub fn state(&self, id: &str) -> Result<&WorkflowState> {
        self.states.iter().find(|s| s.id == id).ok_or_else(|| {
            PmError::new(
                ErrorCode::InvalidSchema,
                format!("unknown workflow state {id:?}"),
            )
        })
    }

    pub fn canonical_status<'a>(&'a self, input: &str) -> Result<&'a str> {
        if let Some(state) = self.states.iter().find(|s| s.id == input) {
            return Ok(&state.id);
        }
        let normalized = normalize_input(input);
        let mut configured = self
            .states
            .iter()
            .filter(|state| normalize_input(&state.id) == normalized);
        if let Some(state) = configured.next() {
            if configured.next().is_some() {
                return Err(PmError::new(
                    ErrorCode::AmbiguousReference,
                    format!("status input {input:?} matches multiple configured state IDs"),
                )
                .hint("Use an exact configured state ID."));
            }
            return Ok(&state.id);
        }
        let alias = match normalized.as_str() {
            "todo" => "ready",
            "inprogress" | "progress" => "in_progress",
            "inreview" | "review" => "in_review",
            "closed" => "done",
            "cancelled" => "canceled",
            _ => input,
        };
        self.state(alias).map(|s| s.id.as_str())
    }

    pub fn transition(&self, from: &str, to: &str) -> Result<()> {
        let from = self.state(from)?;
        self.state(to)?;
        if from.id == to || from.transitions.iter().any(|next| next == to) {
            return Ok(());
        }
        Err(PmError::new(
            ErrorCode::PolicyBlocked,
            format!("transition {} -> {to} is not allowed", from.id),
        )
        .hint("Use issue reopen for a completed or canceled issue."))
    }
}

/// Input convenience only. Stored IDs and enum serialization remain exact.
pub(crate) fn normalize_input(input: &str) -> String {
    input
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_status_spellings_normalize_to_canonical_state_ids() {
        let workflow = Workflow::default();
        for (input, canonical) in [
            ("progress", "in_progress"),
            ("IN progress", "in_progress"),
            ("inprogress", "in_progress"),
            ("Review", "in_review"),
            ("in-review", "in_review"),
            ("CLOSED", "done"),
            ("TO DO", "ready"),
            ("cancelled", "canceled"),
        ] {
            assert_eq!(
                workflow.canonical_status(input).unwrap(),
                canonical,
                "{input}"
            );
        }
    }

    #[test]
    fn custom_state_identity_wins_and_normalized_ambiguity_is_explicit() {
        let mut workflow = Workflow::default();
        for id in ["progress", "quality_gate", "quality-gate"] {
            workflow.states.push(WorkflowState {
                id: id.into(),
                name: id.into(),
                category: WorkflowCategory::Review,
                transitions: Vec::new(),
            });
        }
        workflow.validate().unwrap();
        assert_eq!(workflow.canonical_status("progress").unwrap(), "progress");
        assert_eq!(workflow.canonical_status("PROGRESS").unwrap(), "progress");
        assert_eq!(
            workflow.canonical_status("quality_gate").unwrap(),
            "quality_gate"
        );
        assert_eq!(
            workflow.canonical_status("quality-gate").unwrap(),
            "quality-gate"
        );
        assert_eq!(
            workflow.canonical_status("Quality Gate").unwrap_err().code,
            ErrorCode::AmbiguousReference
        );
    }
}
