//! Native watch-mode ownership for one reloadable review input.

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use workdeck_core::CliInput;
use workdeck_vcs::{
    VcsCatalog, VcsWatchCoverage, VcsWatchPlan, VcsWatchTarget, VcsWatchTargetSource,
    WatchController, WatchControllerActions, WatchControllerConfig, WatchControllerState,
    WatchObserver, WatchObserverCallbacks, WatchPlanContext, WatchSignatureContext,
    WatchSourceError, compute_watch_signature, create_watch_event_source, resolve_watch_plan,
};

const DIRECT_FILE_WATCH_SAFETY_CHECK: Duration = Duration::from_secs(2);

/// Minimal lifecycle boundary implemented by native and injected event sources.
pub trait WatchedInputEventSource: Send {
    fn close(&mut self);
}

impl WatchedInputEventSource for WatchObserver {
    fn close(&mut self) {
        WatchObserver::close(self);
    }
}

/// Injectable watch operations used by production and deterministic tests.
pub trait WatchedInputRuntime: Send + Sync {
    fn resolve_plan(&self, input: &CliInput) -> Result<Option<VcsWatchPlan>, WatchSourceError>;
    fn signature(&self, input: &CliInput) -> Result<String, WatchSourceError>;
    fn create_event_source(
        &self,
        plan: &VcsWatchPlan,
        callbacks: WatchObserverCallbacks,
    ) -> Result<Option<Box<dyn WatchedInputEventSource>>, WatchSourceError>;
}

/// Native planner, signature resolver, and notify-backed event source.
#[derive(Clone)]
pub struct NativeWatchedInputRuntime {
    cwd: PathBuf,
    vcs_catalog: Option<VcsCatalog>,
}

impl NativeWatchedInputRuntime {
    #[must_use]
    pub fn new(cwd: impl Into<PathBuf>, vcs_catalog: Option<VcsCatalog>) -> Self {
        Self {
            cwd: cwd.into(),
            vcs_catalog,
        }
    }
}

impl WatchedInputRuntime for NativeWatchedInputRuntime {
    fn resolve_plan(&self, input: &CliInput) -> Result<Option<VcsWatchPlan>, WatchSourceError> {
        resolve_watch_plan(
            input,
            WatchPlanContext::current(&self.cwd, self.vcs_catalog.as_ref()),
        )
        .map_err(runtime_error)
    }

    fn signature(&self, input: &CliInput) -> Result<String, WatchSourceError> {
        compute_watch_signature(
            input,
            WatchSignatureContext {
                cwd: &self.cwd,
                vcs_catalog: self.vcs_catalog.as_ref(),
            },
        )
        .map_err(runtime_error)
    }

    fn create_event_source(
        &self,
        plan: &VcsWatchPlan,
        callbacks: WatchObserverCallbacks,
    ) -> Result<Option<Box<dyn WatchedInputEventSource>>, WatchSourceError> {
        create_watch_event_source(plan)
            .map(|factory| {
                factory
                    .create(callbacks)
                    .map(|source| Box::new(source) as Box<dyn WatchedInputEventSource>)
            })
            .transpose()
    }
}

fn runtime_error(error: impl std::fmt::Display) -> WatchSourceError {
    WatchSourceError::new(None::<String>, error.to_string())
}

#[derive(Debug)]
enum WatchSignal {
    Event,
    Error(WatchSourceError),
    Ready,
}

/// Observable work completed by one terminal-thread poll.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WatchedInputOutcome {
    pub reload_pending: bool,
    pub refresh_attempts: usize,
    pub refreshes: usize,
    pub errors: Vec<WatchSourceError>,
}

impl WatchedInputOutcome {
    fn merge(&mut self, mut other: Self) {
        self.reload_pending |= other.reload_pending;
        self.refresh_attempts += other.refresh_attempts;
        self.refreshes += other.refreshes;
        self.errors.append(&mut other.errors);
    }
}

/// Owns the observer and serialized watch controller for one mounted input.
pub struct WatchedInputDriver {
    input: CliInput,
    runtime: Arc<dyn WatchedInputRuntime>,
    controller: WatchController,
    event_source: Option<Box<dyn WatchedInputEventSource>>,
    signals: mpsc::Receiver<WatchSignal>,
    pending: WatchedInputOutcome,
}

impl WatchedInputDriver {
    /// Start watching when enabled and the input has a reloadable plan.
    pub fn start(
        enabled: bool,
        input: CliInput,
        runtime: Arc<dyn WatchedInputRuntime>,
        initial_signature: Option<String>,
        now: Instant,
        mut config: WatchControllerConfig,
    ) -> Result<Option<Self>, WatchSourceError> {
        if !enabled {
            return Ok(None);
        }
        let Some(plan) = runtime.resolve_plan(&input)? else {
            return Ok(None);
        };
        if has_direct_file_content(&plan) {
            config.healthy_check = DIRECT_FILE_WATCH_SAFETY_CHECK;
        }
        let initial_signature = match initial_signature {
            Some(signature) => signature,
            None => runtime.signature(&input)?,
        };
        let has_event_source = plan.coverage != VcsWatchCoverage::PollOnly;
        let mut controller = WatchController::new(
            initial_signature,
            plan.coverage == VcsWatchCoverage::PollOnly,
            has_event_source,
            now,
            config,
        );
        let (sender, signals) = mpsc::channel();
        let callbacks = WatchObserverCallbacks {
            on_event: Arc::new({
                let sender = sender.clone();
                move || {
                    let _ = sender.send(WatchSignal::Event);
                }
            }),
            on_error: Arc::new({
                let sender = sender.clone();
                move |error| {
                    let _ = sender.send(WatchSignal::Error(error));
                }
            }),
            on_ready: Some(Arc::new(move || {
                let _ = sender.send(WatchSignal::Ready);
            })),
        };

        let (event_source, pending) = if has_event_source {
            match runtime.create_event_source(&plan, callbacks) {
                Ok(Some(source)) => (Some(source), WatchedInputOutcome::default()),
                Ok(None) => {
                    let error = WatchSourceError::new(
                        None::<String>,
                        "Watch plan requires an event source, but the runtime returned none.",
                    );
                    let actions = controller.on_source_start_failed(now, error);
                    (None, outcome_from_actions(actions))
                }
                Err(error) => {
                    let actions = controller.on_source_start_failed(now, error);
                    (None, outcome_from_actions(actions))
                }
            }
        } else {
            (None, WatchedInputOutcome::default())
        };

        Ok(Some(Self {
            input,
            runtime,
            controller,
            event_source,
            signals,
            pending,
        }))
    }

    #[must_use]
    pub fn state(&self) -> WatchControllerState {
        self.controller.state()
    }

    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        self.controller.next_deadline()
    }

    /// Drain native events and advance every due controller action.
    pub fn poll<E>(
        &mut self,
        now: Instant,
        refresh: &mut impl FnMut() -> Result<(), E>,
    ) -> WatchedInputOutcome
    where
        E: std::fmt::Display,
    {
        let mut outcome = std::mem::take(&mut self.pending);
        while let Ok(signal) = self.signals.try_recv() {
            let actions = match signal {
                WatchSignal::Event => self.controller.on_event(now),
                WatchSignal::Error(error) => self.controller.on_source_error(now, error),
                WatchSignal::Ready => self.controller.on_source_ready(now),
            };
            outcome.merge(self.run_actions(actions, now, refresh));
        }
        let actions = self.controller.tick(now);
        outcome.merge(self.run_actions(actions, now, refresh));
        outcome
    }

    pub fn close(&mut self) {
        let actions = self.controller.close();
        if actions.close_event_source {
            self.close_event_source();
        }
    }

    fn run_actions<E>(
        &mut self,
        mut actions: WatchControllerActions,
        now: Instant,
        refresh: &mut impl FnMut() -> Result<(), E>,
    ) -> WatchedInputOutcome
    where
        E: std::fmt::Display,
    {
        let mut outcome = WatchedInputOutcome::default();
        loop {
            outcome.reload_pending |= actions.reload_pending;
            outcome.errors.append(&mut actions.errors);
            if actions.close_event_source {
                self.close_event_source();
            }
            if actions.check_signature {
                let signature = self.runtime.signature(&self.input);
                actions = self.controller.finish_signature(now, signature);
                continue;
            }
            if actions.refresh {
                outcome.refresh_attempts += 1;
                let result = refresh().map_err(runtime_error);
                if result.is_ok() {
                    outcome.refreshes += 1;
                }
                actions = self.controller.finish_refresh(now, result);
                continue;
            }
            break;
        }
        outcome
    }

    fn close_event_source(&mut self) {
        if let Some(mut source) = self.event_source.take() {
            source.close();
        }
    }
}

impl Drop for WatchedInputDriver {
    fn drop(&mut self) {
        self.close();
    }
}

/// Replace one mounted watcher after its review input has committed a refresh.
///
/// Construct the successor before retiring the old source, then swap the whole
/// driver so callbacks retained by the old observer only address a dropped
/// channel. A failed replacement still retires the stale source: it must never
/// keep observing an input whose content authority has already advanced.
pub(crate) fn replace_watched_input_driver(
    current: &mut Option<WatchedInputDriver>,
    enabled: bool,
    input: CliInput,
    runtime: Arc<dyn WatchedInputRuntime>,
    initial_signature: Option<String>,
    now: Instant,
    config: WatchControllerConfig,
) -> Result<(), WatchSourceError> {
    match WatchedInputDriver::start(enabled, input, runtime, initial_signature, now, config) {
        Ok(replacement) => {
            *current = replacement;
            Ok(())
        }
        Err(error) => {
            current.take();
            Err(error)
        }
    }
}

fn outcome_from_actions(mut actions: WatchControllerActions) -> WatchedInputOutcome {
    WatchedInputOutcome {
        reload_pending: actions.reload_pending,
        errors: std::mem::take(&mut actions.errors),
        ..WatchedInputOutcome::default()
    }
}

fn has_direct_file_content(plan: &VcsWatchPlan) -> bool {
    plan.targets.iter().any(|target| {
        matches!(
            target,
            VcsWatchTarget::DirectoryEntries { sources, .. }
                if sources.contains(&VcsWatchTargetSource::Content)
        )
    })
}

#[cfg(test)]
mod tests;
