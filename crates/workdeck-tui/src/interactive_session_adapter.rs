//! Session-layer adapter owned for exactly one interactive review lifetime.

use anyhow::Result;
use workdeck_core::{Changeset, ChangesetSource, CliInput};
use workdeck_review::{PublishReviewInput, ReviewProducer, ReviewProducerOptions};
use workdeck_session::{
    SessionRegistrationBootstrap, WorkdeckSessionBrokerClient, WorkdeckSessionInputKind,
    create_initial_session_snapshot, create_session_registration,
};

use crate::ReviewOptions;

#[must_use]
pub const fn session_input_kind(
    input: Option<&CliInput>,
    source: &ChangesetSource,
) -> WorkdeckSessionInputKind {
    match input {
        Some(CliInput::Vcs(_)) => WorkdeckSessionInputKind::Vcs,
        Some(CliInput::Show(_)) => WorkdeckSessionInputKind::Show,
        Some(CliInput::StashShow(_)) => WorkdeckSessionInputKind::StashShow,
        Some(CliInput::Files(_)) => WorkdeckSessionInputKind::Diff,
        Some(CliInput::Patch(_)) => WorkdeckSessionInputKind::Patch,
        Some(CliInput::DiffTool(_)) => WorkdeckSessionInputKind::Difftool,
        None => match source {
            ChangesetSource::WorkingTree { .. } => WorkdeckSessionInputKind::Vcs,
            ChangesetSource::Revision { .. } => WorkdeckSessionInputKind::Show,
            ChangesetSource::Stash { .. } => WorkdeckSessionInputKind::StashShow,
            ChangesetSource::Patch { .. } => WorkdeckSessionInputKind::Patch,
            ChangesetSource::Files { .. } => WorkdeckSessionInputKind::Diff,
        },
    }
}

/// One producer and daemon client owned for exactly the lifetime of an interactive review.
pub struct InteractiveSessionBroker {
    client: StopOnDrop<WorkdeckSessionBrokerClient>,
    producer: ReviewProducer,
}

trait StoppableSessionClient {
    fn stop_session(&self);
}

impl StoppableSessionClient for WorkdeckSessionBrokerClient {
    fn stop_session(&self) {
        self.stop();
    }
}

struct StopOnDrop<C>
where
    C: StoppableSessionClient,
{
    client: C,
    active: bool,
}

impl<C> StopOnDrop<C>
where
    C: StoppableSessionClient,
{
    const fn new(client: C) -> Self {
        Self {
            client,
            active: true,
        }
    }

    fn stop(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        self.client.stop_session();
    }
}

impl<C> Drop for StopOnDrop<C>
where
    C: StoppableSessionClient,
{
    fn drop(&mut self) {
        self.stop();
    }
}

impl InteractiveSessionBroker {
    pub fn start(changeset: &Changeset, options: &ReviewOptions) -> Result<Self> {
        let producer = ReviewProducer::new(
            PublishReviewInput {
                files: changeset.files.clone(),
                source_label: Some(changeset.effective_source_label().to_owned()),
            },
            ReviewProducerOptions {
                source_loader: crate::source_controller::publication_source_loader(
                    options.source_capabilities.clone(),
                ),
                ..Default::default()
            },
        )?;
        let publication = producer.get_publication();
        let registration_bootstrap = SessionRegistrationBootstrap {
            input_kind: session_input_kind(options.review_input.as_ref(), &changeset.source),
            changeset: changeset.clone(),
            source_label: changeset.effective_source_label().to_owned(),
            experimental: options
                .review_input
                .as_ref()
                .and_then(|input| input.options().experimental)
                .unwrap_or(false),
            initial_show_agent_notes: options.agent_notes,
        };
        let registration = create_session_registration(&registration_bootstrap, &publication)?;
        let snapshot = create_initial_session_snapshot(&registration_bootstrap, &publication);
        let client = WorkdeckSessionBrokerClient::new(registration, snapshot);
        let _startup = client.start();
        Ok(Self {
            client: StopOnDrop::new(client),
            producer,
        })
    }

    #[must_use]
    pub fn client(&self) -> WorkdeckSessionBrokerClient {
        self.client.client.clone()
    }

    #[must_use]
    pub fn producer(&self) -> ReviewProducer {
        self.producer.clone()
    }

    pub fn stop(&mut self) {
        self.client.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use workdeck_core::{CommonOptions, PatchCommandInput};

    use super::*;

    #[test]
    fn session_input_kind_uses_the_exact_cli_variant_and_source_fallback() {
        let patch = CliInput::Patch(PatchCommandInput {
            file: None,
            text: None,
            options: CommonOptions::default(),
        });
        assert_eq!(
            session_input_kind(
                Some(&patch),
                &ChangesetSource::WorkingTree { staged: false }
            ),
            WorkdeckSessionInputKind::Patch
        );
        assert_eq!(
            session_input_kind(
                None,
                &ChangesetSource::Stash {
                    reference: "stash@{0}".into()
                }
            ),
            WorkdeckSessionInputKind::StashShow
        );
        assert_eq!(
            session_input_kind(
                None,
                &ChangesetSource::Files {
                    left: "a".into(),
                    right: "b".into()
                }
            ),
            WorkdeckSessionInputKind::Diff
        );
    }

    #[test]
    fn session_client_is_stopped_exactly_once_when_its_owner_drops() {
        struct Client(Rc<Cell<usize>>);

        impl StoppableSessionClient for Client {
            fn stop_session(&self) {
                self.0.set(self.0.get() + 1);
            }
        }

        let stops = Rc::new(Cell::new(0));
        let owner = StopOnDrop::new(Client(Rc::clone(&stops)));
        assert_eq!(stops.get(), 0);
        drop(owner);
        assert_eq!(stops.get(), 1);

        let mut owner = StopOnDrop::new(Client(Rc::clone(&stops)));
        owner.stop();
        owner.stop();
        drop(owner);
        assert_eq!(stops.get(), 2);
    }
}
