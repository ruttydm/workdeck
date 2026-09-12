//! A finite publication stays owned until its result and process cleanup are joined.
use super::ForegroundRunSignal;
use std::{
    io,
    sync::Arc,
    thread::{self, JoinHandle},
};

#[derive(Debug)]
pub(super) struct OwnedPublication<T> {
    signal: Arc<ForegroundRunSignal>,
    worker: Option<JoinHandle<T>>,
}
impl<T: Send + 'static> OwnedPublication<T> {
    pub fn start(
        signal: Arc<ForegroundRunSignal>,
        task: impl FnOnce() -> T + Send + 'static,
    ) -> io::Result<Self> {
        signal.claim_publication()?;
        match thread::Builder::new()
            .name("workdeck-planning-publication".into())
            .spawn(task)
        {
            Ok(worker) => Ok(Self {
                signal,
                worker: Some(worker),
            }),
            Err(error) => {
                signal.release_publication();
                Err(error)
            }
        }
    }
}
impl<T> OwnedPublication<T> {
    pub fn poll(&mut self) -> Option<thread::Result<T>> {
        self.worker
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
            .then(|| self.join())
    }
    fn join(&mut self) -> thread::Result<T> {
        let result = self
            .worker
            .take()
            .expect("owned publication exists until join")
            .join();
        // The source-operation runner reaps its owned process group on return
        // and unwind. This is separate from the check runner's acknowledgment.
        self.signal.release_publication();
        result
    }
    pub fn shutdown(&mut self) -> Option<thread::Result<T>> {
        self.worker.as_ref()?;
        self.signal.interrupt_active();
        Some(self.join())
    }
}
impl<T> Drop for OwnedPublication<T> {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::Duration,
    };

    #[test]
    fn repeated_interrupts_keep_publication_owned_without_claiming_check_cancellation() {
        let signal = Arc::new(ForegroundRunSignal::default());
        let (release, blocked) = mpsc::channel();
        let mut task = OwnedPublication::start(signal.clone(), move || {
            blocked.recv_timeout(Duration::from_secs(2)).unwrap()
        })
        .unwrap();
        assert!(signal.is_publication());
        assert!(signal.interrupt_active());
        assert!(signal.interrupt_active());
        assert!(signal.interruption_requested());
        assert!(signal.is_active());
        assert!(super::super::owned_run::OwnedRun::start(signal.clone(), |_| ()).is_err());
        release.send(42).unwrap();
        assert_eq!(task.shutdown().unwrap().unwrap(), 42);
        assert!(!signal.is_active());
        assert!(!signal.is_publication());
    }

    #[test]
    fn drop_joins_publication_and_panic_retains_unknown_result_until_join() {
        let signal = Arc::new(ForegroundRunSignal::default());
        let joined = Arc::new(AtomicBool::new(false));
        let marker = joined.clone();
        let task = OwnedPublication::start(signal.clone(), move || {
            marker.store(true, Ordering::Release)
        })
        .unwrap();
        drop(task);
        assert!(joined.load(Ordering::Acquire));
        assert!(!signal.is_active());
        let mut task =
            OwnedPublication::start(signal.clone(), || panic!("lost publication result")).unwrap();
        assert!(task.shutdown().unwrap().is_err());
        assert!(!signal.is_active());
    }
}
