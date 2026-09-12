//! Explicit immutable Git planning views. Observation never performs a fetch.
mod batch;
#[cfg(all(test, unix))]
mod batch_tests;
mod capture;
mod capture_core;
mod check_binding_proof;
mod check_input_entries;
mod check_inputs;
pub(crate) use check_binding_proof::validate as validate_check_binding;
mod commits;
pub(crate) use check_inputs::bind_check_inputs;
mod evaluators;
pub(crate) use commits::{capture_commits, resolve_ci_criterion, resolve_ci_gate};
mod coordination;
pub(crate) mod fs;
pub(crate) mod git;
mod git_transport;
mod local;
mod process;
mod publication;
mod remote;
mod remote_impl;
mod types;
pub(crate) use coordination::{ConfirmedSources, confirm_sources_bound};
mod git_write;
mod proposal_impl;
mod proposals;
pub(crate) use capture::capture_with_deadline;
pub use capture::{PlanningSourceView, SourceSnapshot, capture};
pub(crate) use capture_core::{capture_local_delta, identity_from_snapshot};
pub use proposals::{MAX_PROPOSAL_PLAN_BYTES, ProposalOutcome, ProposalPlan, ProposalRequest};
pub use publication::PublicationFaultPoint;
pub(crate) use publication::{PublicationAdmission, coordinate_with_faults_before};
pub use publication::{
    PublicationAttempt, PublicationConfirmation, PublicationOutcome, PublicationState,
};
pub(crate) use remote::{parse_coordination_marker, reject_coordination_snapshot};
pub use types::*;
