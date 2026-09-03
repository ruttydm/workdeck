# Native extension VCS adapters

Workdeck ports Hunk's in-process VCS extension surface to the same trusted native subprocess used
by every other extension capability. One `vcs-adapter` handshake registration owns an ID, display
name, detection priority, and an explicit map of `working-tree-diff`, `revision-show`, and
`stash-show` operations. Each operation always has `load` and may declare `watchSignature` and
`watchPlan`. A declaration is published only after the complete handshake validates.

The host translates registrations into `workdeck-vcs::VcsAdapter` values and extends the bundled
Jujutsu, Sapling, and Git catalog in declaration order. Bundled IDs remain reserved. Extension IDs
are first-wins, use Hunk's default priority of `-100` when unspecified, and can be selected with
`workdeck diff --vcs ID`. Automatic selection asks adapters in priority order and still chooses the
nearest detected checkout. A detection without a usable `repoRoot` is a miss. A returned ID that
does not match the registration is repaired to the registered ID and reported once.

## Protocol

All messages are JSON-RPC 2.0 over newline-delimited stdio. The host enforces the ordinary extension
message limit, request deadline, cancellation, crash isolation, and one in-flight request per
process.

| Method | Purpose |
| --- | --- |
| `workdeck/vcs/detect` | Detect one registered adapter at a canonical current directory. |
| `workdeck/vcs/load` | Produce a patch result for one declared operation. |
| `workdeck/vcs/source/read` | Read one exact old or new file side from a load-owned token. |
| `workdeck/vcs/watch-signature` | Compute the operation's cheap change fingerprint. |
| `workdeck/vcs/watch-plan` | Return filesystem targets or an explicit polling-only plan. |

The load input contains only content-affecting review options: explicit range or separate range
endpoints, staged state, pathspecs, `excludeUntracked`, and `colorMoved`. Renderer choices never
cross into a backend.

A patch result carries `repoRoot`, `sourceLabel`, `title`, unified patch text, untracked paths,
optional extra patch/skipped files, and an optional source cache key. Native source callbacks also
return a non-empty opaque `loadToken`; it keeps the extension's immutable revision pair private.
Source answers use Hunk's public wire shape: text, `null`, or `{ "kind": "too-large",
"maxBytes": N }`.

The host caches successful and structural too-large reads per file and side. Ordinary failures are
not cached, so a later expansion can retry. Binary and skipped-large files never invoke the source
callback. Extra one-file patches retain declared paths, former paths, untracked flags, order, and
source capability; skipped entries retain change type, statistics, truncation, and no source
capability.

## Runtime ownership

`LoadedExtension` clones share one locked process connection, monotonic request-ID stream,
notification hub, and authority registry. The CLI composes the catalog before resolving the initial
review, then passes that same catalog to native watch planning while the TUI retains another handle.
Reload transforms and VCS calls therefore observe one extension instance rather than duplicate
processes. Explicit retirement revokes every clone. The final connection owner also force-reaps the
child as a race-safe fallback.

The executable example in `examples/extensions/native-vcs/` implements every method. Its integration
test proves detection normalization, patch and exact-source loading, successful and too-large
caching, failed-read retry, watch calls, process sharing, diagnostics, and final retirement. The
untouched Hunk baseline and stable test results are frozen in
`port/hunk/oracles/extension-vcs-patch-result.json`.
