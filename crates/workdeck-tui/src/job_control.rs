//! Parsed raw-mode interrupt and Unix job-control suspension semantics.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobControlPlatform {
    Unix,
    Windows,
}

impl JobControlPlatform {
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobControlAction {
    Interrupt,
    Suspend,
}

/// Renderer-lifetime key routing state. Dropping or disposing it removes all authority to act.
#[derive(Debug, Default)]
pub struct JobControlSupport {
    disposed: bool,
}

impl JobControlSupport {
    pub fn dispose(&mut self) {
        self.disposed = true;
    }

    #[must_use]
    pub fn action(
        &self,
        key: KeyEvent,
        platform: JobControlPlatform,
        renderer_destroyed: bool,
    ) -> Option<JobControlAction> {
        if self.disposed || renderer_destroyed || key.modifiers != KeyModifiers::CONTROL {
            return None;
        }
        match key.code {
            KeyCode::Char('c') => Some(JobControlAction::Interrupt),
            KeyCode::Char('z') if platform == JobControlPlatform::Unix => {
                Some(JobControlAction::Suspend)
            }
            _ => None,
        }
    }
}

pub trait JobControlRuntime {
    fn is_destroyed(&self) -> bool;
    fn suspend_renderer(&mut self) -> Result<(), String>;
    fn signal_foreground_process_group(&mut self) -> Result<(), String>;
    fn resume_renderer(&mut self) -> Result<(), String>;
}

/// Restore the terminal, suspend process group zero, and re-enter after the shell sends SIGCONT.
pub fn suspend_foreground_process_group(
    runtime: &mut impl JobControlRuntime,
) -> Result<(), String> {
    if runtime.is_destroyed() {
        return Ok(());
    }
    runtime.suspend_renderer()?;
    let signal_result = runtime.signal_foreground_process_group();
    if !runtime.is_destroyed() {
        runtime.resume_renderer()?;
    }
    // A refused SIGTSTP must leave the application usable, not half-restored.
    let _ = signal_result;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Runtime {
        destroyed: bool,
        destroy_during_signal: bool,
        signal_fails: bool,
        events: Vec<&'static str>,
    }

    impl JobControlRuntime for Runtime {
        fn is_destroyed(&self) -> bool {
            self.destroyed
        }

        fn suspend_renderer(&mut self) -> Result<(), String> {
            self.events.push("suspend");
            Ok(())
        }

        fn signal_foreground_process_group(&mut self) -> Result<(), String> {
            self.events.push("signal:0:SIGTSTP");
            self.destroyed |= self.destroy_during_signal;
            if self.signal_fails {
                Err("unsupported signal".into())
            } else {
                Ok(())
            }
        }

        fn resume_renderer(&mut self) -> Result<(), String> {
            self.events.push("resume");
            Ok(())
        }
    }

    fn key(character: char, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), modifiers)
    }

    #[test]
    fn routes_ctrl_c_through_the_interrupt_action() {
        let support = JobControlSupport::default();
        assert_eq!(
            support.action(
                key('c', KeyModifiers::CONTROL),
                JobControlPlatform::Unix,
                false
            ),
            Some(JobControlAction::Interrupt)
        );
    }

    #[test]
    fn ignores_non_ctrl_c_and_disposed_interrupts() {
        let mut support = JobControlSupport::default();
        assert_eq!(
            support.action(
                key('z', KeyModifiers::CONTROL),
                JobControlPlatform::Windows,
                false
            ),
            None
        );
        support.dispose();
        assert_eq!(
            support.action(
                key('c', KeyModifiers::CONTROL),
                JobControlPlatform::Unix,
                false
            ),
            None
        );
    }

    #[test]
    fn ignores_ctrl_c_after_renderer_destruction() {
        assert_eq!(
            JobControlSupport::default().action(
                key('c', KeyModifiers::CONTROL),
                JobControlPlatform::Unix,
                true
            ),
            None
        );
    }

    #[test]
    fn windows_never_routes_ctrl_z_to_job_control() {
        assert_eq!(
            JobControlSupport::default().action(
                key('z', KeyModifiers::CONTROL),
                JobControlPlatform::Windows,
                false
            ),
            None
        );
    }

    #[test]
    fn keys_other_than_exact_ctrl_z_are_ignored() {
        let support = JobControlSupport::default();
        for event in [
            key('z', KeyModifiers::NONE),
            key('z', KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            key('x', KeyModifiers::CONTROL),
        ] {
            assert_eq!(support.action(event, JobControlPlatform::Unix, false), None);
        }
    }

    #[test]
    fn suspends_group_zero_and_resumes_after_signal_continuation() {
        let mut runtime = Runtime::default();
        suspend_foreground_process_group(&mut runtime).unwrap();
        assert_eq!(runtime.events, ["suspend", "signal:0:SIGTSTP", "resume"]);
    }

    #[test]
    fn destroyed_renderer_is_not_resumed_after_continuation() {
        let mut runtime = Runtime {
            destroy_during_signal: true,
            ..Runtime::default()
        };
        suspend_foreground_process_group(&mut runtime).unwrap();
        assert_eq!(runtime.events, ["suspend", "signal:0:SIGTSTP"]);
    }

    #[test]
    fn refused_sigtstp_restores_the_renderer() {
        let mut runtime = Runtime {
            signal_fails: true,
            ..Runtime::default()
        };
        suspend_foreground_process_group(&mut runtime).unwrap();
        assert_eq!(runtime.events, ["suspend", "signal:0:SIGTSTP", "resume"]);
    }

    #[test]
    fn disposed_support_cannot_start_another_suspend() {
        let mut support = JobControlSupport::default();
        support.dispose();
        assert_eq!(
            support.action(
                key('z', KeyModifiers::CONTROL),
                JobControlPlatform::Unix,
                false
            ),
            None
        );
    }
}
