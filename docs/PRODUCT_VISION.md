# Workdeck Product Vision

Agents make production faster; keeping up becomes the constraint. Workdeck is the place where a developer follows commits, pull requests, CI, and artifacts across every active project without reconstructing context from a dozen browser tabs.

## Product promise

Workdeck makes activity legible. A commit branch or pull request is unread when its source revision has advanced beyond the user’s durable read cursor. Opening or explicitly marking it read advances that cursor. A later push, comment, or status change makes it unread again.

## Daily loop

1. Updates presents unread commit branches and pull-request activity.
2. The developer opens the branch in Git or the PR in its dense master/detail view.
3. Workdeck marks the exact activity revision read and reopens it only when the source advances.
4. Changes chooses the useful representation: diff, source, Markdown, calls, structure, or AST.
5. CI and artifacts stay attached as evidence without becoming separate review chores.

## Portfolio scale

People work across repositories, linked worktrees, and agent branches. Workdeck treats Project → Repository → Checkout → Worktree identity as first-class. Unavailable checkouts retain history. Commit bursts are grouped by branch, so five fast agent commits are one catch-up item rather than five artificial tasks.

## Evidence before mutation

Git history, branches, refs, WIP, pull requests, CI, logs, and artifacts are Workdeck’s core objects. The current desktop surface is read-only. The TUI and future desktop actions may perform explicit user-selected Git operations, but agent output never silently stages, commits, rebases, merges, or publishes.

Herder runs agent sessions; Workdeck reviews their repository effects. Aya may direct higher-level missions and experiments without becoming either the session runtime or the Git authority.

## Platform direction

The initial product is a high-quality macOS desktop app. The same Rust/Dioxus UI and renderer-independent protocol also support deterministic browser development today and preserve seams for a hosted read-only instance or future mobile companion. Authentication, tenancy, sync, and mobile releases are separate products, not hidden scope in the desktop rewrite.

## Success

Workdeck succeeds when a developer can return after five commits and two pull requests, immediately see what is unread, inspect the relevant changes, and leave without maintaining a parallel review taxonomy.
