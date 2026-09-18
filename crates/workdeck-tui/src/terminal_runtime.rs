//! Runtime defaults and lifecycle seams for interactive terminal input.

use std::fs::{File, OpenOptions};
use std::io::{self, IsTerminal};
use std::path::Path;

use workdeck_core::CliInput;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppMouseOptions {
    pub stdin_is_terminal: bool,
    pub has_controlling_terminal: bool,
}

#[must_use]
pub fn uses_piped_patch_input(input: &CliInput, stdin_is_terminal: bool) -> bool {
    matches!(
        input,
        CliInput::Patch(patch)
            if patch.file.as_deref().is_none_or(|file| file == "-") && !stdin_is_terminal
    )
}

#[must_use]
pub fn should_use_pager_mode(input: &CliInput, stdin_is_terminal: bool) -> bool {
    input.options().pager == Some(true) || uses_piped_patch_input(input, stdin_is_terminal)
}

#[must_use]
pub fn resolve_runtime_cli_input(mut input: CliInput, stdin_is_terminal: bool) -> CliInput {
    let pager = should_use_pager_mode(&input, stdin_is_terminal);
    input.options_mut().pager = Some(pager);
    input
}

#[must_use]
pub const fn should_use_mouse_for_app(options: AppMouseOptions) -> bool {
    options.stdin_is_terminal || options.has_controlling_terminal
}

#[must_use]
pub fn stdin_is_terminal() -> bool {
    io::stdin().is_terminal()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalDisconnectEvent {
    Close,
    End,
    Error,
}

/// Once-only disconnect gate used by terminal backends and host integrations.
pub struct TerminalDisconnectSupport<F>
where
    F: FnMut(),
{
    interactive: bool,
    disposed: bool,
    on_disconnect: F,
}

impl<F> TerminalDisconnectSupport<F>
where
    F: FnMut(),
{
    #[must_use]
    pub fn new(
        interactive: bool,
        already_destroyed: bool,
        readable_ended: bool,
        on_disconnect: F,
    ) -> Self {
        let mut support = Self {
            interactive,
            disposed: false,
            on_disconnect,
        };
        if interactive && (already_destroyed || readable_ended) {
            support.disconnect();
        }
        support
    }

    pub fn handle(&mut self, _event: TerminalDisconnectEvent) {
        self.disconnect();
    }

    pub fn dispose(&mut self) {
        self.disposed = true;
    }

    fn disconnect(&mut self) {
        if !self.interactive || self.disposed {
            return;
        }
        self.disposed = true;
        (self.on_disconnect)();
    }
}

pub struct ControllingTerminal<T = File> {
    pub input: T,
}

impl<T> ControllingTerminal<T> {
    pub fn close(self) {
        drop(self);
    }
}

#[must_use]
pub fn open_controlling_terminal() -> Option<ControllingTerminal<File>> {
    open_controlling_terminal_with(|path| OpenOptions::new().read(true).write(true).open(path))
}

#[must_use]
pub fn open_controlling_terminal_with<T>(
    open: impl FnOnce(&Path) -> io::Result<T>,
) -> Option<ControllingTerminal<T>> {
    #[cfg(windows)]
    const TERMINAL_PATH: &str = "CONIN$";
    #[cfg(not(windows))]
    const TERMINAL_PATH: &str = "/dev/tty";
    open(Path::new(TERMINAL_PATH))
        .ok()
        .map(|input| ControllingTerminal { input })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use workdeck_core::{CommonOptions, PatchCommandInput};

    fn patch(file: Option<&str>, pager: bool) -> CliInput {
        CliInput::Patch(PatchCommandInput {
            file: file.map(str::to_owned),
            text: None,
            options: CommonOptions {
                pager: Some(pager),
                ..CommonOptions::default()
            },
        })
    }

    #[test]
    fn piped_stdin_patch_enables_pager_runtime_default() {
        let input = patch(Some("-"), false);
        assert!(uses_piped_patch_input(&input, false));
        assert!(should_use_pager_mode(&input, false));
        assert_eq!(
            resolve_runtime_cli_input(input, false).options().pager,
            Some(true)
        );
    }

    #[test]
    fn files_and_interactive_stdin_do_not_force_pager_but_explicit_pager_wins() {
        assert!(!uses_piped_patch_input(
            &patch(Some("changes.patch"), false),
            false
        ));
        assert!(!should_use_pager_mode(
            &patch(Some("changes.patch"), false),
            false
        ));
        assert!(!should_use_pager_mode(&patch(Some("-"), false), true));
        assert!(should_use_pager_mode(&patch(None, true), true));
    }

    #[test]
    fn mouse_requires_stdin_or_an_attached_controlling_terminal() {
        assert!(should_use_mouse_for_app(AppMouseOptions {
            stdin_is_terminal: true,
            has_controlling_terminal: false,
        }));
        assert!(should_use_mouse_for_app(AppMouseOptions {
            stdin_is_terminal: false,
            has_controlling_terminal: true,
        }));
        assert!(!should_use_mouse_for_app(AppMouseOptions::default()));
    }

    struct FakeInput(Rc<Cell<bool>>);

    impl Drop for FakeInput {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    #[test]
    fn controlling_terminal_uses_platform_input_and_close_drops_it() {
        let requested = Rc::new(RefCell::new(None));
        let requested_copy = Rc::clone(&requested);
        let closed = Rc::new(Cell::new(false));
        let input = FakeInput(Rc::clone(&closed));
        let terminal = open_controlling_terminal_with(move |path| {
            *requested_copy.borrow_mut() = Some(path.to_owned());
            Ok(input)
        })
        .unwrap();
        #[cfg(windows)]
        assert_eq!(requested.borrow().as_deref(), Some(Path::new("CONIN$")));
        #[cfg(not(windows))]
        assert_eq!(requested.borrow().as_deref(), Some(Path::new("/dev/tty")));
        terminal.close();
        assert!(closed.get());

        assert!(
            open_controlling_terminal_with::<FakeInput>(|_| {
                Err(io::Error::new(io::ErrorKind::NotFound, "no terminal"))
            })
            .is_none()
        );
    }

    #[test]
    fn terminal_disconnect_is_once_only_disposable_and_terminal_only() {
        for event in [
            TerminalDisconnectEvent::Close,
            TerminalDisconnectEvent::End,
            TerminalDisconnectEvent::Error,
        ] {
            let calls = Rc::new(Cell::new(0));
            let callback_calls = Rc::clone(&calls);
            let mut support = TerminalDisconnectSupport::new(true, false, false, move || {
                callback_calls.set(callback_calls.get() + 1);
            });
            support.handle(event);
            support.handle(event);
            assert_eq!(calls.get(), 1);
        }

        let calls = Rc::new(Cell::new(0));
        let callback_calls = Rc::clone(&calls);
        let mut disposed = TerminalDisconnectSupport::new(true, false, false, move || {
            callback_calls.set(callback_calls.get() + 1);
        });
        disposed.dispose();
        disposed.handle(TerminalDisconnectEvent::Close);
        assert_eq!(calls.get(), 0);

        let calls = Rc::new(Cell::new(0));
        let callback_calls = Rc::clone(&calls);
        let mut non_terminal = TerminalDisconnectSupport::new(false, true, true, move || {
            callback_calls.set(callback_calls.get() + 1);
        });
        non_terminal.handle(TerminalDisconnectEvent::End);
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn already_destroyed_or_ended_terminal_disconnects_during_install() {
        for (destroyed, ended) in [(true, false), (false, true)] {
            let calls = Rc::new(Cell::new(0));
            let callback_calls = Rc::clone(&calls);
            let _support = TerminalDisconnectSupport::new(true, destroyed, ended, move || {
                callback_calls.set(callback_calls.get() + 1);
            });
            assert_eq!(calls.get(), 1);
        }
    }
}
