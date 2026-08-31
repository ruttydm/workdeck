# Workdeck Architecture

## Boundaries

Workdeck separates product meaning, native authority, and rendering.

```text
Dioxus components
      │ WorkdeckRequest / Envelope<WorkdeckResponse> / WorkdeckEvent
      ▼
workdeck-api ───── FixtureWorkdeckClient (web tests)
      │
      ▼
LocalWorkdeckClient (bounded queues, revisions, cancellation)
      │
      ▼
Workdeck runtime owner
  ├── workdeck-db          one SQLite writer
  ├── workdeck-git         read-only repositories
  ├── workdeck-analysis    syntax and semantic evidence
  ├── workdeck-github      read-only provider subprocesses
  └── workdeck-artifacts   isolated import and preview
```

`workdeck-ui` cannot import filesystem, database, Git, provider, process, or platform crates. Its only product-data dependency is `workdeck-api`. Renderer callbacks send typed requests; no component performs blocking I/O.

## Protocol

`WorkdeckClient` is a cloneable transport façade. Every request has a `RequestId`; every response is an `Envelope` containing that ID, a monotonic `Revision`, and a typed `WorkdeckResponse`. Long-running work emits bounded `TaskProgress` events and is cancelled by `OperationId`.

Read-heavy surfaces share a renderer-independent response cache at this façade. Cache identities exclude the transient request ID but include every semantic input (worktree, cursor, repository, range, review, job, or normalized query). A hit is rebound to the caller's request ID while retaining its source revision. Identical in-flight reads share one future; completed responses are capped at 96 entries and abandoned in-flight reads at 24. Explicit refresh and mutations invalidate the relevant identities, and a unique generation token prevents an invalidated older future from repopulating the cache. The monotonic cache clock is `web-time`, so the same implementation is safe in both native and `wasm32-unknown-unknown` renderers.

The protocol models portfolio, unread activity, workspace hierarchy, Git history, pull requests, change evidence, CI, artifacts, search, and UI preference patches. It contains no paths to implementation-only database or provider types.

## Runtime ownership

The native presenter owns a bounded request queue and four request workers. Core service operations flow through a single runtime owner that serializes SQLite writes and durable activity cursors. Git/provider/analysis work is delegated to bounded workers. Caches store immutable snapshots and are refreshed after mutations.

The renderer receives snapshots. A refresh does not redefine the user’s selection, tab, read cursor, or scroll position. Result generations and response revisions prevent an older request from replacing newer state.

Workdeck prewarms only the first available worktree referenced by unread activity after bootstrap. Git, PR, CI, artifact, review, job-log, and search reads use bounded age policies. Git and review surfaces use stale-while-revalidate presentation: expired content stays interactive while a fresh request runs, and failures are reported without replacing useful content. Prepared commit and PR change sets flow directly into the diff surface instead of issuing a redundant `LoadReview` request.

## Syntax pipeline

`workdeck-analysis` is the sole syntax authority. It resolves the file language, parses source with tree-sitter, and applies each grammar's maintained highlight query instead of guessing colors from node names. Nineteen parser-backed languages are supported: Rust, Swift, JavaScript/JSX, TypeScript, TSX, Python, Go, PHP, Bash, C, C++, C#, CSS, HTML, Java, JSON, Ruby, TOML, and YAML. Markdown receives safe structural spans; unknown text stays intentionally uncolored.

Compiled queries are cached once per language. Overlapping grammar captures are normalized by specificity and semantic priority, multi-line captures are split into UTF-8-safe line-local spans, and the presenter converts them to renderer-independent `SyntaxSpan` values. `workdeck-ui` only renders those spans and rejects malformed or overlapping input without changing source text. This keeps tree-sitter, files, and parsing out of Dioxus rendering and preserves the future hosted-client boundary.

## State

Workdeck starts at schema version 2 in a new application directory. Schema 2 adds opaque activity read cursors keyed by commit branch or pull request:

```text
Workdeck/
  workdeck.sqlite3
  objects/
  artifacts/
  logs/
  caches/
  preferences/
```

`WORKDECK_DATA_DIR` replaces the desktop-catalog root for deterministic tests. Desktop application state never enters reviewed repositories and does not replace the TUI's repo-local `.agents/workdeck/` data.

Herder remains authoritative for live agent sessions. Workdeck may consume versioned Herder events for attribution, but no Workdeck runtime owns agent processes, PTYs, or session recovery. See [Product Boundaries](PRODUCT_BOUNDARIES.md).

## Repository safety

All repository access is centralized and read-only. Git subprocesses use bounded arguments, disabled optional locks where appropriate, sanitized output, timeouts/cancellation, and explicit mutation-policy tests. Mutating integration tests create temporary repositories.

## Renderer targets

- Desktop: Dioxus Desktop 0.7.10, Tao/Wry, system WebView, macOS first.
- Web fixture: Dioxus Web with deterministic clients, fonts, clock, provider state, progress, errors, and routes.
- Future hosted/mobile: new `WorkdeckClient` transports and platform hosts; no current authentication, tenancy, sync, iOS, or Android product is implied.

## Native host

The macOS host uses full-size content with a transparent native titlebar, a 900 × 600 minimum, native application/edit/window menus, native file/folder dialogs, and a process-lifetime menu clone for Dioxus issue #5753. Navigation stays internal except exact loopback artifact origins; validated GitHub HTTPS links open externally.

## Shutdown

Dropping the local client closes bounded channels. Artifact preview sessions are owned by the client and UI scope; close, cancel, tab removal, window teardown, and process exit drop the server and join its helper thread. Provider and Git cancellation terminate bounded subprocess work.
