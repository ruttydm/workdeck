//! One foreground-owned worker. Completion is joined, never abandoned on tab changes.
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use workdeck_pm::RunControl;

/// Signal projection for the one run owned by a mounted review terminal.
/// The composition root retains process-wide signal registration.
#[derive(Debug, Default)]
pub struct ForegroundRunSignal {
    // 0 inactive; checks: 1 running, 2 cancel, 3 force;
    // publication: 4 running, 5/6 interrupted with exit deferred until join.
    state: AtomicU8,
    // Process signals are dispatched by composition outside a raw signal
    // handler. Retain the same control so delivery never depends on UI polling.
    control: Mutex<RunControl>,
}

impl ForegroundRunSignal {
    /// Returns true when this signal belongs to an active run. A repeated
    /// interrupt escalates cleanup instead of bypassing it with process::exit.
    pub fn interrupt_active(&self) -> bool {
        let control = self
            .control
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let active = self
            .state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                (state != 0).then_some((state + 1).min(if state >= 4 { 6 } else { 3 }))
            })
            .is_ok();
        if active {
            self.apply(&control);
        }
        active
    }

    pub fn is_active(&self) -> bool {
        self.state.load(Ordering::Acquire) != 0
    }

    pub fn is_publication(&self) -> bool {
        self.state.load(Ordering::Acquire) >= 4
    }

    pub fn interruption_requested(&self) -> bool {
        matches!(self.state.load(Ordering::Acquire), 2 | 3 | 5 | 6)
    }

    pub(super) fn claim_publication(&self) -> io::Result<()> {
        let _control = self
            .control
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.state
            .compare_exchange(0, 4, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "A foreground operation is still owned by this terminal",
                )
            })
    }

    pub(super) fn release_publication(&self) {
        self.release();
    }

    fn claim(&self) -> io::Result<RunControl> {
        let mut control = self
            .control
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "A foreground check run is still owned by this terminal",
                )
            })?;
        *control = RunControl::default();
        Ok(control.clone())
    }

    fn release(&self) {
        let _control = self
            .control
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.state.store(0, Ordering::Release);
    }

    fn apply(&self, control: &RunControl) {
        match self.state.load(Ordering::Acquire) {
            2 => control.cancel(),
            3 => control.force_cancel(),
            _ => {}
        }
    }
}

#[derive(Debug)]
pub(super) struct OwnedRun<T> {
    signal: Arc<ForegroundRunSignal>,
    control: RunControl,
    worker: Option<JoinHandle<T>>,
}

impl<T: Send + 'static> OwnedRun<T> {
    pub fn start(
        signal: Arc<ForegroundRunSignal>,
        task: impl FnOnce(RunControl) -> T + Send + 'static,
    ) -> io::Result<Self> {
        let control = signal.claim()?;
        let worker_control = control.clone();
        match thread::Builder::new()
            .name("workdeck-foreground-check".into())
            .spawn(move || task(worker_control))
        {
            Ok(worker) => Ok(Self {
                signal,
                control,
                worker: Some(worker),
            }),
            Err(error) => {
                signal.release();
                Err(error)
            }
        }
    }
}

impl<T> OwnedRun<T> {
    pub fn poll(&mut self) -> Option<thread::Result<T>> {
        self.signal.apply(&self.control);
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            Some(self.join())
        } else {
            None
        }
    }

    pub fn cleanup_complete(&self) -> bool {
        self.control.cleanup_complete()
    }

    fn join(&mut self) -> thread::Result<T> {
        let result = self
            .worker
            .take()
            .expect("owned worker exists until joined")
            .join();
        // Core's return guard acknowledges process cleanup before its worker
        // exits. Never make the signal inactive while a worker can still spawn.
        if self.control.cleanup_complete() {
            self.signal.release();
        }
        result
    }

    pub fn shutdown(&mut self) -> Option<thread::Result<T>> {
        self.worker.as_ref()?;
        self.control.cancel();
        let started = Instant::now();
        loop {
            if started.elapsed() >= Duration::from_millis(500) {
                self.control.force_cancel();
            }
            if let Some(result) = self.poll() {
                return Some(result);
            }
            // The bounded core cancellation contract owns process-group
            // escalation. Joining retains ownership even on UI errors.
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl<T> Drop for OwnedRun<T> {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicBool, mpsc};

    #[test]
    fn completion_and_interrupt_keep_single_worker_ownership_until_join() {
        let signal = Arc::new(ForegroundRunSignal::default());
        assert!(!signal.interrupt_active());
        let (sender, receiver) = mpsc::channel();
        let mut run = OwnedRun::start(signal.clone(), move |control| {
            sender.send(()).unwrap();
            while !control.cancellation_requested() {
                thread::yield_now();
            }
            42
        })
        .unwrap();
        receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(OwnedRun::start(signal.clone(), |_| ()).is_err());
        assert!(signal.interrupt_active());
        assert!(signal.interrupt_active());
        assert_eq!(run.shutdown().unwrap().unwrap(), 42);
        // This deliberately incomplete worker is not the core runner: it never
        // acknowledges cleanup. Its missing acknowledgement must stay visible.
        assert!(!run.cleanup_complete());
        assert!(signal.is_active());
        assert!(OwnedRun::start(signal, |_| ()).is_err());
    }

    #[test]
    fn drop_cancels_and_joins_without_leaving_a_worker() {
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = stopped.clone();
        let run = OwnedRun::start(Arc::new(ForegroundRunSignal::default()), move |control| {
            while !control.cancellation_requested() {
                thread::yield_now();
            }
            worker_stopped.store(true, Ordering::Release);
        })
        .unwrap();
        drop(run);
        assert!(stopped.load(Ordering::Acquire));
    }

    #[test]
    fn process_signal_reaches_runner_even_while_ui_is_not_polling() {
        let signal = Arc::new(ForegroundRunSignal::default());
        let (started, waiting) = mpsc::channel();
        let (stopped, finished) = mpsc::channel();
        let run = OwnedRun::start(signal.clone(), move |control| {
            started.send(()).unwrap();
            while !control.cancellation_requested() {
                thread::yield_now();
            }
            let _ = stopped.send(());
        })
        .unwrap();
        waiting.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(signal.interrupt_active());
        // A source read or renderer delay must not delay delivery to the owned
        // runner. In particular, this assertion never calls poll/shutdown.
        let delivered = finished.recv_timeout(Duration::from_millis(500)).is_ok();
        drop(run);
        assert!(
            delivered,
            "process cancellation incorrectly depends on the UI poll loop"
        );
    }
}
