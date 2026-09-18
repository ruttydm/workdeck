/// A native callback value whose protocol contract forbids deferred work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SynchronousCallbackValue<T> {
    Returned(T),
    Thenable,
}

/// The three outcomes of invoking one synchronous extension callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SynchronousExtensionCallbackResult<T> {
    Returned(T),
    Thenable,
    Threw(String),
}

/// Invoke a callback whose answer decides the current input or lifecycle transition.
///
/// Native subprocess responses represent promise-like work as an explicit
/// protocol violation. Host callback panics are also contained at this boundary
/// so extension routing cannot unwind through the TUI.
pub fn call_extension_synchronously<T>(
    callback: impl FnOnce() -> Result<SynchronousCallbackValue<T>, String>,
) -> SynchronousExtensionCallbackResult<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)) {
        Ok(Ok(SynchronousCallbackValue::Returned(value))) => {
            SynchronousExtensionCallbackResult::Returned(value)
        }
        Ok(Ok(SynchronousCallbackValue::Thenable)) => SynchronousExtensionCallbackResult::Thenable,
        Ok(Err(error)) => SynchronousExtensionCallbackResult::Threw(error),
        Err(payload) => {
            let detail = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&str>()
                        .map(|value| (*value).to_owned())
                })
                .unwrap_or_else(|| "extension callback panicked".into());
            SynchronousExtensionCallbackResult::Threw(detail)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_returns_thenables_failures_and_panics() {
        assert_eq!(
            call_extension_synchronously(|| {
                Ok::<_, String>(SynchronousCallbackValue::Returned("handled"))
            }),
            SynchronousExtensionCallbackResult::Returned("handled")
        );
        assert_eq!(
            call_extension_synchronously(|| {
                Ok::<SynchronousCallbackValue<()>, String>(SynchronousCallbackValue::Thenable)
            }),
            SynchronousExtensionCallbackResult::Thenable
        );
        assert_eq!(
            call_extension_synchronously(|| {
                Err::<SynchronousCallbackValue<()>, _>("remote failure".into())
            }),
            SynchronousExtensionCallbackResult::Threw("remote failure".into())
        );
        assert_eq!(
            call_extension_synchronously(|| -> Result<SynchronousCallbackValue<()>, String> {
                panic!("callback exploded")
            }),
            SynchronousExtensionCallbackResult::Threw("callback exploded".into())
        );
    }
}
