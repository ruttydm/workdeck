//! Failure-safe ownership of an interactive Crossterm session.

use std::io::IsTerminal;
use std::io::{self, Stdout, Write};
use std::panic;

use anyhow::Result;
use crossterm::cursor::Show;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
/// Platform signals which request a graceful interactive-app shutdown.
#[cfg(unix)]
pub const APP_SHUTDOWN_SIGNALS: &[&str] = &["SIGINT", "SIGTERM", "SIGHUP", "SIGQUIT", "SIGPIPE"];

/// Platform signals which request a graceful interactive-app shutdown.
#[cfg(windows)]
pub const APP_SHUTDOWN_SIGNALS: &[&str] = &["SIGINT", "SIGTERM", "SIGBREAK"];

trait TerminalRuntime {
    type Terminal;

    fn enable_raw_mode(&mut self) -> Result<()>;
    fn enter_alternate_screen(&mut self, mouse: bool) -> Result<()>;
    fn create_terminal(&mut self) -> Result<Self::Terminal>;
    fn restore_without_terminal(&mut self, mouse: bool) -> Result<()>;
    fn restore_terminal(&mut self, terminal: &mut Self::Terminal, mouse: bool) -> Result<()>;
}

struct ManagedTerminal<R>
where
    R: TerminalRuntime,
{
    runtime: R,
    terminal: Option<R::Terminal>,
    mouse: bool,
    active: bool,
}

impl<R> ManagedTerminal<R>
where
    R: TerminalRuntime,
{
    fn enter(mut runtime: R, mouse: bool) -> Result<Self> {
        if let Err(error) = runtime.enable_raw_mode() {
            let _ = runtime.restore_without_terminal(mouse);
            return Err(error);
        }
        if let Err(error) = runtime.enter_alternate_screen(mouse) {
            let _ = runtime.restore_without_terminal(mouse);
            return Err(error);
        }
        let terminal = match runtime.create_terminal() {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = runtime.restore_without_terminal(mouse);
                return Err(error);
            }
        };
        Ok(Self {
            runtime,
            terminal: Some(terminal),
            mouse,
            active: true,
        })
    }

    fn terminal_mut(&mut self) -> &mut R::Terminal {
        self.terminal
            .as_mut()
            .expect("interactive terminal remains allocated for its session")
    }

    fn restore(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        let mouse = self.mouse;
        let runtime = &mut self.runtime;
        match &mut self.terminal {
            Some(terminal) => runtime.restore_terminal(terminal, mouse),
            None => runtime.restore_without_terminal(mouse),
        }
    }

    fn resume(&mut self) -> Result<()> {
        if self.active {
            return Ok(());
        }
        if let Err(error) = self.runtime.enable_raw_mode() {
            let _ = self.runtime.restore_without_terminal(self.mouse);
            return Err(error);
        }
        if let Err(error) = self.runtime.enter_alternate_screen(self.mouse) {
            let _ = self.runtime.restore_without_terminal(self.mouse);
            return Err(error);
        }
        self.active = true;
        Ok(())
    }
}

impl<R> Drop for ManagedTerminal<R>
where
    R: TerminalRuntime,
{
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

struct CrosstermRuntime {
    stdout: Option<Stdout>,
}

impl CrosstermRuntime {
    fn new() -> Self {
        Self {
            stdout: Some(io::stdout()),
        }
    }
}

impl TerminalRuntime for CrosstermRuntime {
    type Terminal = Terminal<CrosstermBackend<Stdout>>;

    fn enable_raw_mode(&mut self) -> Result<()> {
        if io::stdin().is_terminal() || crate::open_controlling_terminal().is_some() {
            enable_raw_mode()?;
        }
        Ok(())
    }

    fn enter_alternate_screen(&mut self, mouse: bool) -> Result<()> {
        let stdout = self
            .stdout
            .as_mut()
            .expect("stdout is available until the Ratatui terminal is created");
        execute!(stdout, EnterAlternateScreen)?;
        if mouse {
            execute!(stdout, EnableMouseCapture)?;
        }
        Ok(())
    }

    fn create_terminal(&mut self) -> Result<Self::Terminal> {
        let stdout = self
            .stdout
            .take()
            .expect("stdout is transferred into Ratatui exactly once");
        let backend = CrosstermBackend::new(stdout);
        if io::stdout().is_terminal() {
            Ok(Terminal::new(backend)?)
        } else {
            // Match the upstream renderer's fallback when stdout has no terminal dimensions.
            Ok(Terminal::with_options(
                backend,
                ratatui::TerminalOptions {
                    viewport: ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 80, 24)),
                },
            )?)
        }
    }

    fn restore_without_terminal(&mut self, mouse: bool) -> Result<()> {
        match self.stdout.as_mut() {
            Some(stdout) => restore_output(stdout, mouse),
            None => restore_output(&mut io::stdout(), mouse),
        }
    }

    fn restore_terminal(&mut self, terminal: &mut Self::Terminal, mouse: bool) -> Result<()> {
        restore_output(terminal.backend_mut(), mouse)
    }
}

fn restore_output(output: &mut impl Write, mouse: bool) -> Result<()> {
    let mut first_error = disable_raw_mode().err().map(anyhow::Error::from);
    if mouse
        && let Err(error) = execute!(output, DisableMouseCapture)
        && first_error.is_none()
    {
        first_error = Some(error.into());
    }
    if let Err(error) = execute!(output, LeaveAlternateScreen)
        && first_error.is_none()
    {
        first_error = Some(error.into());
    }
    if let Err(error) = execute!(output, Show)
        && first_error.is_none()
    {
        first_error = Some(error.into());
    }
    if let Err(error) = output.flush()
        && first_error.is_none()
    {
        first_error = Some(error.into());
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// RAII owner for raw mode, the alternate screen, mouse capture, and cursor restoration.
pub struct InteractiveTerminalSession {
    inner: ManagedTerminal<CrosstermRuntime>,
    #[cfg(unix)]
    _piped_input: Option<crate::piped_input::PipedInputBridge>,
    #[cfg(unix)]
    terminal_descriptors: [bool; 2],
}

impl InteractiveTerminalSession {
    /// Classify errors from terminal I/O only. macOS PTYs can return EIO while
    /// poll and tcgetattr still succeed after their master is closed.
    pub fn disconnected_during_io(&self, error: &io::Error) -> bool {
        if self.host_disconnected() {
            return true;
        }
        #[cfg(unix)]
        if self.terminal_descriptors.iter().any(|terminal| *terminal) {
            return matches!(error.raw_os_error(), Some(libc::EIO | libc::ENXIO))
                || matches!(
                    error.kind(),
                    io::ErrorKind::BrokenPipe
                        | io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::WriteZero
                );
        }
        let _ = error;
        false
    }

    /// A vanished PTY is a normal host shutdown, not a rendering failure.
    #[cfg(unix)]
    pub fn host_disconnected(&self) -> bool {
        let mut descriptors = [libc::STDIN_FILENO, libc::STDOUT_FILENO].map(|fd| libc::pollfd {
            fd,
            events: 0,
            revents: 0,
        });
        // SAFETY: the array contains two initialized pollfd values and lives through the call.
        let result = unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, 0) };
        (result > 0
            && descriptors.iter().zip(self.terminal_descriptors).any(
                |(descriptor, was_terminal)| {
                    was_terminal
                        && descriptor.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL)
                            != 0
                },
            ))
            || descriptors.iter().zip(self.terminal_descriptors).any(
                |(descriptor, was_terminal)| {
                    if !was_terminal {
                        return false;
                    }
                    let mut attributes = std::mem::MaybeUninit::<libc::termios>::uninit();
                    // SAFETY: tcgetattr writes into a properly sized allocation; it is never read.
                    let result = unsafe { libc::tcgetattr(descriptor.fd, attributes.as_mut_ptr()) };
                    result < 0
                        && matches!(
                            io::Error::last_os_error().raw_os_error(),
                            Some(
                                libc::EIO | libc::ENXIO | libc::ENODEV | libc::EBADF | libc::ENOTTY
                            )
                        )
                },
            )
    }

    #[cfg(not(unix))]
    pub fn host_disconnected(&self) -> bool {
        false
    }

    pub fn enter(mouse: bool) -> Result<Self> {
        let mouse =
            mouse && (io::stdin().is_terminal() || crate::open_controlling_terminal().is_some());
        #[cfg(unix)]
        let piped_input = if !io::stdin().is_terminal() && !io::stdout().is_terminal() {
            Some(crate::piped_input::PipedInputBridge::start()?)
        } else {
            None
        };
        Ok(Self {
            #[cfg(unix)]
            terminal_descriptors: [io::stdin().is_terminal(), io::stdout().is_terminal()],
            inner: ManagedTerminal::enter(CrosstermRuntime::new(), mouse)?,
            #[cfg(unix)]
            _piped_input: piped_input,
        })
    }

    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        self.inner.terminal_mut()
    }

    /// Restore before SIGTSTP and re-enter after the shell resumes this process.
    #[cfg(unix)]
    pub fn suspend_foreground_process_group(&mut self) -> Result<()> {
        self.inner.restore()?;
        // SAFETY: group zero and SIGTSTP are fixed libc values; no pointer crosses the FFI boundary.
        let _ = unsafe { libc::kill(0, libc::SIGTSTP) };
        self.inner.resume()
    }

    #[cfg(not(unix))]
    pub fn suspend_foreground_process_group(&mut self) -> Result<()> {
        Ok(())
    }
}

type PanicHook = Box<dyn Fn(&panic::PanicHookInfo<'_>) + Sync + Send + 'static>;

/// Restores the terminal before printing a panic and reinstates the caller's hook on drop.
pub struct InteractiveTerminalPanicHook {
    previous: Option<PanicHook>,
}

impl InteractiveTerminalPanicHook {
    pub fn install(mouse: bool) -> Self {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let _ = restore_output(&mut io::stdout(), mouse);
            eprintln!("{info}");
        }));
        Self {
            previous: Some(previous),
        }
    }
}

impl Drop for InteractiveTerminalPanicHook {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            panic::set_hook(previous);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Failure {
        Raw,
        Screen,
        Terminal,
        Restore,
    }

    struct FakeRuntime {
        events: Rc<RefCell<Vec<&'static str>>>,
        failure: Option<Failure>,
    }

    impl FakeRuntime {
        fn record(&self, event: &'static str) {
            self.events.borrow_mut().push(event);
        }

        fn fail(&self, stage: Failure) -> Result<()> {
            if self.failure == Some(stage) {
                anyhow::bail!("{stage:?}")
            }
            Ok(())
        }
    }

    impl TerminalRuntime for FakeRuntime {
        type Terminal = ();

        fn enable_raw_mode(&mut self) -> Result<()> {
            self.record("raw:on");
            self.fail(Failure::Raw)
        }

        fn enter_alternate_screen(&mut self, mouse: bool) -> Result<()> {
            self.record("screen:alternate");
            if mouse {
                self.record("mouse:on");
            }
            self.fail(Failure::Screen)
        }

        fn create_terminal(&mut self) -> Result<Self::Terminal> {
            self.record("terminal:create");
            self.fail(Failure::Terminal)
        }

        fn restore_without_terminal(&mut self, mouse: bool) -> Result<()> {
            self.record("raw:off");
            if mouse {
                self.record("mouse:off");
            }
            self.record("screen:primary");
            self.record("cursor:show");
            self.fail(Failure::Restore)
        }

        fn restore_terminal(&mut self, _terminal: &mut Self::Terminal, mouse: bool) -> Result<()> {
            self.record("terminal:destroy");
            self.restore_without_terminal(mouse)
        }
    }

    fn runtime(failure: Option<Failure>) -> (FakeRuntime, Rc<RefCell<Vec<&'static str>>>) {
        let events = Rc::new(RefCell::new(Vec::new()));
        (
            FakeRuntime {
                events: Rc::clone(&events),
                failure,
            },
            events,
        )
    }

    #[test]
    fn every_startup_failure_attempts_complete_terminal_rollback() {
        for failure in [Failure::Raw, Failure::Screen, Failure::Terminal] {
            let (runtime, events) = runtime(Some(failure));
            assert!(ManagedTerminal::enter(runtime, true).is_err());
            let events = events.borrow();
            assert!(events.ends_with(&["raw:off", "mouse:off", "screen:primary", "cursor:show"]));
        }
    }

    #[test]
    fn drop_restores_a_live_session_exactly_once() {
        let (runtime, events) = runtime(None);
        let mut terminal = ManagedTerminal::enter(runtime, true).unwrap();
        terminal.restore().unwrap();
        terminal.restore().unwrap();
        drop(terminal);
        assert_eq!(
            &*events.borrow(),
            &[
                "raw:on",
                "screen:alternate",
                "mouse:on",
                "terminal:create",
                "terminal:destroy",
                "raw:off",
                "mouse:off",
                "screen:primary",
                "cursor:show",
            ]
        );
    }

    #[test]
    fn restore_failure_is_not_retried_and_cannot_hide_the_primary_session_result() {
        let (runtime, events) = runtime(Some(Failure::Restore));
        let mut terminal = ManagedTerminal::enter(runtime, false).unwrap();
        assert!(terminal.restore().is_err());
        drop(terminal);
        assert_eq!(
            events
                .borrow()
                .iter()
                .filter(|event| **event == "screen:primary")
                .count(),
            1
        );
    }

    #[test]
    fn suspension_restores_then_resumes_the_same_terminal() {
        let (runtime, events) = runtime(None);
        let mut terminal = ManagedTerminal::enter(runtime, true).unwrap();
        terminal.restore().unwrap();
        terminal.resume().unwrap();
        drop(terminal);
        assert_eq!(
            &*events.borrow(),
            &[
                "raw:on",
                "screen:alternate",
                "mouse:on",
                "terminal:create",
                "terminal:destroy",
                "raw:off",
                "mouse:off",
                "screen:primary",
                "cursor:show",
                "raw:on",
                "screen:alternate",
                "mouse:on",
                "terminal:destroy",
                "raw:off",
                "mouse:off",
                "screen:primary",
                "cursor:show",
            ]
        );
    }

    #[test]
    fn shutdown_signal_set_matches_the_platform_contract() {
        #[cfg(unix)]
        assert_eq!(
            APP_SHUTDOWN_SIGNALS,
            ["SIGINT", "SIGTERM", "SIGHUP", "SIGQUIT", "SIGPIPE"]
        );
        #[cfg(windows)]
        assert_eq!(APP_SHUTDOWN_SIGNALS, ["SIGINT", "SIGTERM", "SIGBREAK"]);
    }
}
