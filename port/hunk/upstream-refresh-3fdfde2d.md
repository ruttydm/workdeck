# Interim upstream refresh at 3fdfde2d

`cargo xtask port fetch`'s already-built executable (`target/debug/xtask port fetch`)
successfully refreshed Hunk through main `3fdfde2dd77eb93802c43304c8edd34d79ce81de`
(`chore(release): prepare v0.22.0 (#1081)`). This is an interim refresh, not the
final release-time fetch required by the semantic-rebase plan.

The append-only receipt file gained 84 entries. `port audit-upstream` verified
696 archived refs after the fetch. The fetch pruned one deleted tracking branch
and observed force-updated branches; the independently archived histories remain
recoverable. Both annotated anchor tags still peel to the original main
2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd commits. Workdeck ancestry was not merged
with Hunk, and nothing was pushed or published.

Strict port audit still exits 1: 1,257 baseline files, 1,415 ledger intervals,
275 unmapped intervals, five stable-only commits tracked, and **92 pending
post-baseline commits**. Earlier reports of eleven pending commits were based
on stale tracking refs and are superseded by this refresh.

The topological catch-up sequence still starts with c828427b (session lifecycle
clock), 9b5d4190 (late lifecycle settlements), be35bb59 (runtime exit fixtures)
and 034796a9 (pane activation callback). No catch-up commit is marked ported
by fetching or archiving it. Baseline parity remains incomplete.
