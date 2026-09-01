//! Ordered teardown for an interactive terminal session.

/// Minimal application-root contract needed during shutdown.
pub trait ShutdownRoot {
    fn unmount(&mut self);
}

/// Minimal renderer contract needed to restore the previous terminal screen.
pub trait ShutdownRenderer {
    fn destroy(&mut self);
}

impl<F> ShutdownRoot for F
where
    F: FnMut(),
{
    fn unmount(&mut self) {
        self();
    }
}

/// Tear down the application and renderer before reporting a successful exit.
///
/// The caller owns the once-only guard and supplies the exit operation so the
/// sequence remains testable and embedders may return instead of terminating
/// their process.
pub fn shutdown_session<R, D, E>(root: &mut R, renderer: &mut D, exit: E)
where
    R: ShutdownRoot,
    D: ShutdownRenderer,
    E: FnOnce(i32),
{
    root.unmount();
    renderer.destroy();
    exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Renderer {
        events: Rc<RefCell<Vec<String>>>,
    }

    impl ShutdownRenderer for Renderer {
        fn destroy(&mut self) {
            self.events.borrow_mut().push("destroy".into());
        }
    }

    #[test]
    fn unmounts_destroys_and_exits_without_clearing_the_restored_screen() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let root_events = Rc::clone(&events);
        let exit_events = Rc::clone(&events);
        let mut root = move || root_events.borrow_mut().push("unmount".into());
        let mut renderer = Renderer {
            events: Rc::clone(&events),
        };

        shutdown_session(&mut root, &mut renderer, move |code| {
            exit_events.borrow_mut().push(format!("exit:{code}"));
        });

        assert_eq!(
            &*events.borrow(),
            &[
                "unmount".to_owned(),
                "destroy".to_owned(),
                "exit:0".to_owned(),
            ]
        );
    }
}
