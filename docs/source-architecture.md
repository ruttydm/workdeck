# Source architecture

This maintainer map records source ownership and dependency direction for the Rust
port. Use it when adding a module or deciding where a responsibility belongs. It
complements [Architecture](ARCHITECTURE.md) and the feature-specific native
extension documents linked there. Ownership is not a claim that every behavior in
that subsystem has reached parity; the [port ledger](../port/hunk/README.md) is the
completion record.

## Ownership

The source-role column preserves every ownership boundary from the pinned Hunk
map. Those names identify provenance, not directories to recreate or runtime
imports to retain. Native owners may split a former directory across crates.

| Source role | Responsibility and native owner |
| --- | --- |
| `src/app/` | Executable composition: CLI parsing, startup plans, and shared session bootstrap. [workdeck-cli](../crates/workdeck-cli/src/) composes the product; [bootstrap contracts](../crates/workdeck-core/src/bootstrap.rs) carry the prepared handoff. |
| `src/app/session/` | Mounted-review registration, bridge, and reload authorization. [workdeck-session](../crates/workdeck-session/src/) owns registration and bridge contracts; [workdeck-tui](../crates/workdeck-tui/src/) owns mounted terminal integration. |
| `src/core/` | Shared review models, patch handling, VCS contracts, configuration, and runtime primitives. These have named owners in [core](../crates/workdeck-core/src/), [diff](../crates/workdeck-diff/src/), [review](../crates/workdeck-review/src/), and [VCS](../crates/workdeck-vcs/src/), rather than a catch-all crate. |
| `src/core/changeset/` | Changeset model and acquisition pipeline: loaders, per-file construction, sidecar/source reads, and hunk formatting. [core](../crates/workdeck-core/src/) owns value models, [diff](../crates/workdeck-diff/src/) parses and formats patches, and [VCS](../crates/workdeck-vcs/src/) acquires and materializes sources. |
| `src/core/run/` | How a run is requested: command inputs, layered configuration, command catalog, user-facing errors, paths, and version. [core](../crates/workdeck-core/src/) owns input/error/path primitives; [CLI](../crates/workdeck-cli/src/) owns parsing, configuration loading, command composition, and version reporting. |
| `src/core/process/` | Process and terminal lifetime: TTY capabilities, pager, job control, shutdown, project-root discovery, persisted application state, and startup/update notices. [CLI](../crates/workdeck-cli/src/), [terminal runtime](../crates/workdeck-tui/src/terminal_runtime.rs), and [VCS](../crates/workdeck-vcs/src/) own their respective process, terminal, and repository operations; [core](../crates/workdeck-core/src/) supplies shared notices and paths. |
| `src/core/theme/` | Bundled metadata, custom-theme rules, and terminal theme detection. [core theme rules](../crates/workdeck-core/src/theme.rs), [TUI themes](../crates/workdeck-tui/src/theme.rs), and [terminal detection](../crates/workdeck-tui/src/theme_detection.rs) separate configuration from paint and terminal I/O. |
| `src/core/watch/` | Input signatures, observation plans/backends, and refresh coordination. [VCS watch modules](../crates/workdeck-vcs/src/) own observation; [TUI](../crates/workdeck-tui/src/) coordinates mounted review refresh. |
| `src/core/vcs/` | Provider-neutral catalog, contracts, operation dispatch, and host support. [workdeck-vcs](../crates/workdeck-vcs/src/) owns providers and catalog; the [extension host](../crates/workdeck-extension-host/src/) adapts native extension providers. |
| `src/extensions/` | Host, registry, trust, lifecycle, and bundled extensions. [workdeck-extension-host](../crates/workdeck-extension-host/src/) owns host services; bundled VCS implementations live in [VCS](../crates/workdeck-vcs/src/), and declarative examples live in [extensions](../examples/extensions/). |
| `src/session/` | Shared protocol, schemas, types, agent surface, and broker transport. [workdeck-session](../crates/workdeck-session/src/) owns these contracts and services. |
| `src/session/client/` | Shared daemon HTTP and compatibility client support. [workdeck-session](../crates/workdeck-session/src/) owns broker client and HTTP protocol modules. |
| `src/session/agent/` | Agent-facing session CLI, command manifest, errors, and formatting. [session agent modules](../crates/workdeck-session/src/) own this surface; [CLI](../crates/workdeck-cli/src/) dispatches it. |
| `src/session/broker/` | Local daemon transport, launcher, Workdeck broker state, wire parsing, and projections. [session broker modules](../crates/workdeck-session/src/) own this boundary. |
| `src/ui/` | Interactive review application, rendering, interaction, and chrome. [workdeck-tui](../crates/workdeck-tui/src/) owns the Ratatui shell and terminal paint. |
| `src/extension-api/` | Public extension declaration and runtime boundary. [workdeck-extension-api](../crates/workdeck-extension-api/src/) exposes native contracts; [extension host](../crates/workdeck-extension-host/src/) owns subprocess execution. The former JavaScript package export is not a supported runtime entrypoint. |
| `src/opentui/` | Former public component boundary. [workdeck-tui](../crates/workdeck-tui/src/) owns native rendering and [extension API](../crates/workdeck-extension-api/src/) exposes declarative UI contracts. There is no OpenTUI package or compatibility runtime. |
| `src/lib/` | Small product-wide utilities without feature ownership. Keep them in an appropriate named native crate, using [core](../crates/workdeck-core/src/) only for genuinely shared domain primitives; do not create a generic dumping ground. |

Composition should stay small: it joins subsystems, not a second application
framework. Shared core is not synonymous with everything outside the renderer.
Prefer a more specific existing owner whenever one already owns the behavior.

## Dependency direction

- The CLI composition root may compose shared core, extension host, sessions, and
  the TUI. The TUI consumes shared models and extension/session contracts and owns
  terminal rendering.
- Extension hosting consumes provider-neutral contracts, not the TUI or CLI.
  Native extension implementations use the public declarative SDK and local
  utilities. They do not acquire host renderer ownership; the host supplies the
  bundled sidebar/rendering boundary.
- Core must not depend on the TUI or extension host. Shared data belongs in a
  structural contract owned below its consumers, never in a reverse dependency.
- The public extension API is a deliberate contract boundary, not an internal
  utility bucket. Unlike the upstream import-free TypeScript declaration file,
  the Rust SDK may use core value types; it must not pull in rendering or process
  hosting. Ratatui internals likewise are not a public extension runtime.
- The upstream bundled-provider rule allowed only the public extension entrypoint,
  provider-local modules, and common utilities. The native adaptation compiles
  bundled providers in the provider-neutral VCS crate, whose workspace dependency
  ceiling is core and diff. Subprocess providers cross the extension API instead.

`cargo xtask architecture check` mechanically checks the native dependency
ceilings, source reachability, and named internal seams. It replaces the relevant
source-boundary/dependency-cruiser checks; [Architecture](ARCHITECTURE.md) records
the precise enforced rules and their limits. This is not evidence that every
upstream architecture test or subsystem is fully ported.

## Bootstrap invariant

Initial launch and live-session reload must share this ordering: apply extension
registrations, resolve extension-aware VCS selection, load a normalized changeset,
apply changeset transforms, and attach session theme/configuration state. Callers
retain their own terminal setup, extension rediscovery, notices, and mounted-app
lifecycle work; they must not recreate a competing bootstrap sequence.

The source centralizes this in `app/sessionBootstrap.ts`. Native composition
currently uses CLI bootstrap preparation and core bootstrap contracts, with
mounted reload integration in the TUI/session adapters. The CLI's
`prepare_app_bootstrap` receives already-loaded input, applies agent context and
extension transforms, and hands configuration to `build_app_bootstrap`. This map
preserves the invariant as a maintenance requirement; it does not certify that
every launch and reload path already satisfies complete upstream parity.

The bundled provider catalog is assembled in the VCS crate and extended by the
native extension host. Provider commands, source readers, and colocated tests
live in the VCS crate, rather than the former `extensions/default/vcs/<provider>/`
tree. CLI composition chooses and wires these services.

## Migration policy

Migration is incremental, not a bulk rename:

1. Move code when a feature changes it or a small cohesive cluster can move with
   its colocated tests.
2. Add the destination and update all consumers together; do not retain permanent
   duplicate implementations.
3. Keep Workdeck's product-visible commands and public native contracts stable.
   The required Rust rewrite replaces upstream JavaScript package exports; it
   does not preserve them as executable compatibility aliases.
4. Prefer a named ownership boundary over a generic utility directory.
5. Update this map and feature architecture documents whenever ownership changes.

## Attribution

Adapted in full from Hunk's MIT-licensed `docs/source-architecture.md` at
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`, with native ownership and explicit port
status qualifications. See [THIRD_PARTY_NOTICES](../THIRD_PARTY_NOTICES).
