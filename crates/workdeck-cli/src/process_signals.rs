//! One process-wide signal callback with revocable command-local ownership.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

type SignalCallback = Arc<dyn Fn() + Send + Sync + 'static>;

struct ActiveSignalCallback {
    id: u64,
    callback: SignalCallback,
}

#[derive(Default)]
struct SignalCallbackSlot {
    active: Option<ActiveSignalCallback>,
}

impl SignalCallbackSlot {
    fn register(&mut self, id: u64, callback: SignalCallback) -> Result<(), String> {
        if self.active.is_some() {
            return Err("another Workdeck command already owns process signal handling".into());
        }
        self.active = Some(ActiveSignalCallback { id, callback });
        Ok(())
    }

    fn callback(&self) -> Option<SignalCallback> {
        self.active
            .as_ref()
            .map(|active| Arc::clone(&active.callback))
    }

    fn retire(&mut self, id: u64) {
        if self.active.as_ref().is_some_and(|active| active.id == id) {
            self.active = None;
        }
    }
}

fn callback_slot() -> &'static Mutex<SignalCallbackSlot> {
    static SLOT: OnceLock<Mutex<SignalCallbackSlot>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(SignalCallbackSlot::default()))
}

fn dispatch_process_signal() {
    let callback = callback_slot()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .callback();
    if let Some(callback) = callback {
        callback();
    }
}

fn install_process_signal_handler() -> Result<(), String> {
    static INSTALLATION: OnceLock<Result<(), String>> = OnceLock::new();
    INSTALLATION
        .get_or_init(|| {
            ctrlc::set_handler(dispatch_process_signal).map_err(|error| error.to_string())
        })
        .clone()
}

/// Revocable ownership of the installed process signal callback.
pub struct ProcessSignalRegistration {
    id: u64,
    active: bool,
}

impl ProcessSignalRegistration {
    pub fn retire(&mut self) {
        if !self.active {
            return;
        }
        callback_slot()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retire(self.id);
        self.active = false;
    }
}

impl Drop for ProcessSignalRegistration {
    fn drop(&mut self) {
        self.retire();
    }
}

/// Install the shared backend once and transfer signal ownership to one active command.
pub fn register_process_signal_callback(
    callback: impl Fn() + Send + Sync + 'static,
) -> Result<ProcessSignalRegistration, String> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    install_process_signal_handler()?;
    let id = NEXT_ID.fetch_add(1, Ordering::AcqRel);
    callback_slot()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .register(id, Arc::new(callback))?;
    Ok(ProcessSignalRegistration { id, active: true })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn callback_slot_has_exact_revocable_ownership() {
        let calls = Arc::new(AtomicUsize::new(0));
        let first_calls = Arc::clone(&calls);
        let mut slot = SignalCallbackSlot::default();
        slot.register(
            10,
            Arc::new(move || {
                first_calls.fetch_add(1, Ordering::AcqRel);
            }),
        )
        .unwrap();
        assert!(slot.register(11, Arc::new(|| {})).is_err());
        slot.callback().unwrap()();
        assert_eq!(calls.load(Ordering::Acquire), 1);

        slot.retire(11);
        assert!(slot.callback().is_some());
        slot.retire(10);
        assert!(slot.callback().is_none());
        slot.register(12, Arc::new(|| {})).unwrap();
    }
}
