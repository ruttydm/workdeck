# Native interactive review runtime

Workdeck owns each full-screen review as one failure-safe Rust lifetime. The composition root builds
one `ReviewProducer`, then hands registration to the named TUI session adapter. That adapter
registers the first immutable publication with the authenticated local session broker and starts
the broker client before Crossterm enters raw mode and Ratatui's alternate screen.
The same producer advances on reload; it is never replaced with an unrelated generation owner.

`InteractiveTerminalSession` is the rendering authority's RAII boundary. Startup is transactional:
failure while enabling raw mode, entering the alternate screen, enabling mouse capture, or creating
the Ratatui terminal attempts the complete rollback sequence. Normal return, input failure, session
startup failure, and panic all restore raw mode, mouse capture, the primary screen, and the cursor.
Each restoration is once-only, but all individual cleanup operations are attempted even if an
earlier operation fails.

Each Ratatui draw is enclosed in terminal synchronized-output boundaries (DEC mode 2026).
The PTY parity harness evaluates screen predicates only outside an active synchronized update,
so a partially received frame cannot satisfy an assertion. This preserves cell content and geometry;
it does not normalize either. Terminals without synchronized-output support retain ordinary drawing.
A frame guard attempts to end synchronization after draw errors, panic unwinding, or a failed
begin flush, and terminal restoration also ends synchronization before leaving the alternate screen.
Unit tests exercise these failure paths. The native terminal and lifecycle suites cover normal
interactive rendering and teardown. These harness improvements do not mark the remaining pinned
`test/pty/harness.ts` source interval as translated.

The shipped executable installs graceful signal handling only while the review owns the process
callback lease. Unix listens for `SIGINT`, `SIGTERM`, `SIGHUP`, `SIGQUIT`, and `SIGPIPE`; Windows
uses the console Ctrl-C, Ctrl-Break, and close-event equivalents. The first signal requests a clean
loop exit. A second signal exits with status 130 instead of leaving a wedged alternate screen.
Exact Ctrl-C keystrokes use the same graceful path. On Unix, exact Ctrl-Z restores the terminal,
sends `SIGTSTP` to foreground process group zero, and re-enters the same Ratatui session after
`SIGCONT`.

Session and application objects are declared inside the terminal lifetime so teardown order is
deterministic: the direct Workdeck compatibility session stops, watch state drops, the mounted
review detaches its broker bridge and retires native extension authority, the broker client stops,
the native highlight-worker queues are disposed, and the terminal is restored. The modern broker
client and producer are passed into the mounted review host; command-bridge attachment and live
snapshot projection belong to the separately tracked AppHost boundary. Session imports stay inside
the composition shell and named adapters; terminal RAII itself has no session dependency.

Piped patch input remains outside this lifecycle until startup has consumed it. Interactive routes
retain the controlling terminal guard selected by the CLI, while Crossterm's `use-dev-tty` backend
reads events from the terminal. Pager routes without an available controlling terminal use the
static renderer. Merely viewing a review does not create `.agents/workdeck` state.
