# Session cursor-off reveal outcomes

The session controller had separate reveal branches from extension actions.
They still returned `line` with cursor painting disabled, even after extension
navigation adopted the containing-hunk fallback. The new regression
`session_review_controller::tests::cursor_off_agent_highlight_reports_hunk_fallback`
first failed with `Some(Line)` instead of `Some(Hunk)`.

Session attention-mark reveal now takes its existing hunk fallback in cursor-off
mode. Session line navigation selects and positions the containing hunk and
returns `hunk`, retaining the requested side/line in its response. The latter
follows pinned `useTerminalReview.ts` lines 967 onward: a successful fallback
returns the reached target along with the requested side and line. Inspection of
the source attention-mark path at lines 1043 onward confirms that it also reports
the shared reveal outcome. This is source-inspection evidence, not a newly
captured wire oracle.

The regression covers both native session entry points and preservation of the
attention mark. It does not establish complete request counting, arbitrary
missing-stop behavior, or end-to-end protocol parity. No source ledger mapping
is added by this fix.

All 1,198 TUI library tests passed after the fix (8.47 seconds), including the
new regression. Formatting and diff checks passed.
