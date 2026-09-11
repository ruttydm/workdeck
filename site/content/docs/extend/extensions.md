+++
title = "Extensions"
description = "Load trusted native extensions, understand discovery and trust, and configure them."
template = "docs.html"
+++

A Workdeck extension is a compiled native executable that declares its API
version and capabilities in `workdeck-extension.toml`. Workdeck starts it only
after discovery, trust, and manifest validation, then communicates over JSON-RPC
2.0 on newline-delimited standard input/output. No JavaScript engine, package
manager, or build step is involved at runtime.

```rust
// examples/extensions/hello/src/main.rs
// The host owns Ratatui rendering; the child returns declarative notifications.
use workdeck_extension_api::{ExtensionServer, Notification};

fn main() -> anyhow::Result<()> {
    ExtensionServer::stdio(|api| {
        api.on("startup", |_event, ctx| {
            ctx.notify(Notification::info("Hello from my extension"));
            Ok(())
        })
    })
}
```

The API is versioned. A manifest's `api_version` identifies the surface against
which the executable was compiled, and an incompatible extension is rejected
before it can claim an ID. See the companion [extension API](/docs/extend/extension-api/),
[file previews](/docs/extend/file-previews/), [VCS adapters](/docs/extend/vcs-adapters/),
and [custom panes](/docs/extend/custom-sidebars/) pages for individual
capabilities.

Writing one with a coding agent? `workdeck skill path workdeck-extensions`
prints the bundled native-extension skill, just as `workdeck skill path`
prints the review skill.

## Where Workdeck looks

| Group | Source | Runs |
| ----- | ------ | ----- |
| 1 | `--extension <path>` (repeatable) | immediately |
| 2 | `[extensions] paths` in user config | immediately |
| 3 | `~/.config/workdeck/extensions/` | immediately |
| 4 | `.workdeck/extensions/` in the repository under review | after [trust](#trust) |
| 4 | `[extensions] paths` in repository `workdeck/config.toml` | after [trust](#trust) |

Groups load in order; within a group, entries sort alphabetically by their
canonical path and the first occurrence of a path wins. The two repository
sources form one group and one trust decision. A directory source accepts
manifest directories and compiled executables directly inside it, plus one
level of extension folders. `--no-extensions` disables user extensions for one
run and does not read their manifests. `--extension` is explicit intent and
loads immediately, including a path inside the reviewed repository; only pass
code that has been inspected.

### Folder extensions

A folder is an extension when it contains `workdeck-extension.toml` and the
declared executable. A manifest can use a relative executable path, which is
resolved against the folder and may include several declared capability sets;
each manifest still loads as one extension in manifest order:

```text
~/.config/workdeck/extensions/my-ext/
  workdeck-extension.toml  # id = "my-ext", executable = "bin/my-ext"
  bin/
    my-ext                  # compiled native entry point
  assets/
    helper.json
```

The manifest is deliberately not a package manifest. Native dependencies are
linked at build time and are not installed into an extension directory. The
host checks that the executable exists, is runnable, and matches the manifest
before starting it. Pointing `--extension` or `[extensions] paths` at a
directory works either as a single manifest folder or as a directory scanned
for extension folders.

### Extension ids

The `id` in `workdeck-extension.toml` is the namespace for everything the
extension owns:

- configuration: `[extension.<id>]`;
- commands: `<id>.<command-id>`;
- panes: `<id>:<pane-id>`;
- file previews: `<id>:<view-id>`;
- VCS adapters and highlighters: `<id>:<capability-id>`.

IDs start with a letter or digit, then contain letters, digits, `-`, or `_`.
`workdeck`, `git`, `jj`, and `sl` are reserved. An invalid ID, or a second
source offering an already-loaded ID, is skipped with a startup notice. Native
manifest IDs are never inferred from a filename, although the migration helper
can derive a legacy filename ID while inventorying an old extension.

## Installing shared extensions

Extensions are shared as plain Git repositories. `workdeck extension install`
clones one into the managed directory under
`~/.config/workdeck/extensions/installed/`, verifies its manifest and executable,
and records the source and commit:

```bash
workdeck extension install acme/workdeck-word-diff          # GitHub shorthand
workdeck extension install acme/workdeck-word-diff@v1.2.0   # pin a tag, branch, or commit
workdeck extension install git:codeberg.org/acme/ext        # any host; https:// is assumed
workdeck extension install ~/dev/workdeck-word-diff         # local checkout for testing
```

`workdeck extension list` shows every managed install with its version, commit,
and source. `workdeck extension update [name]` re-clones one install (or all of
them) from its recorded source; an `@ref` pin remains fixed until it is
reinstalled. `workdeck extension remove <name>` removes only the selected
managed install; hand-copied extensions in the global directory are untouched.

Installing is the consent step: extensions run with the user's permissions, so
a fresh install asks for confirmation (or accepts `--yes`) after naming the
repository. Only install repositories whose source and build provenance you
trust. Managed installs then load through the global group with no additional
prompt. Workdeck does not publish an npm package or execute a TypeScript
extension; an existing Hunk extension must be rebuilt against the native SDK.

## Publishing an extension

A publishable native extension repository has a manifest at its root, compiled
code, a README, and reproducible build instructions. To share one:

1. Set `id`, `name`, `version`, and `api_version` in
   `workdeck-extension.toml`; an older Workdeck rejects a newer API cleanly.
2. Declare every capability explicitly and keep the executable path relative to
   the manifest. Build dependencies belong to the Rust project, not to a runtime
   `node_modules` directory.
3. Tag releases so users can pin with `@v1.2.0` and publish checksums and SBOM
   data with each archive.
4. Test the exact layout users receive with
   `workdeck extension install /path/to/checkout`, or load it for one run with
   `workdeck diff --extension /path/to/checkout`.

The host owns rendering, terminal input, and process lifetime. Extensions are
trusted native programs rather than security sandboxes; repository discovery is
trust-gated and every destructive capability must request the host's explicit
consent.

## Bundled extensions

Workdeck's Git, Jujutsu, Sapling, and file-navigation providers use the same
public native extension declarations. Bundled providers differ from user
extensions in three ways:

- they are compiled into the Workdeck distribution and load before config
  resolution selects the session's VCS;
- they are implicitly trusted and do not use an `[extension.<id>]` table;
- they remain available under `--no-extensions` and
  `[extensions] enabled = false`; those switches triage extensions installed by
  the user.

## Trust

Extensions run with full user permissions, and reviewing a repository must never
execute code that arrived with it. Repository-local sources therefore stay inert
until approval, once per repository:

```text
Run this repository's extensions?

  This repository contains extensions in .workdeck/extensions.
  Native extensions run with your user permissions.

  enter/t trust · esc not now · n never
```

**Trust** records the decision and reloads the session; **not now** asks again
next time; **never** suppresses future offers. The prompt is a dialog over the
review stream, not a gate in front of it: dismissing it leaves the review
available. Decisions are stored per repository root in
`~/.config/workdeck/state.json`, keyed by canonical path. A different checkout
later occupying a trusted path inherits the decision, so clear the entry before
reusing a path when that distinction matters. Imported legacy trust provenance
never authorizes a rewritten native executable without fresh confirmation.

## Failure isolation

A broken extension is contained, not fatal. A missing manifest field, invalid
executable, failed spawn or handshake is skipped and rolled back with a startup
notice. A handler or transform that throws later becomes a warning naming the
extension. JSON-RPC frames are bounded, deadlines and cancellation are enforced,
and a crashed child cannot terminate the review process. Event payloads and
changesets are immutable snapshots, so a handler cannot corrupt the active
review.

Native extensions retain their shell permissions. For reviewed files, prefer
the host's workspace/document capability; writes require consent and identify
the extension and file. Subprocess isolation contains failures but is not a
security sandbox.

## CLI flags and config

```bash
workdeck diff --extension ./path/to/compiled-extension   # load one entry (repeatable)
workdeck diff --extension ./my-ext                       # load a manifest folder
workdeck --extension ./my-ext cli-tools status           # run an extension command
workdeck --extension ./examples/extensions/github-pr gh 123
workdeck --no-extensions cli-tools status                # hard-disable lookup/import
workdeck diff --no-extensions                            # disable user extensions for review
```

```toml
# ~/.config/workdeck/config.toml or .workdeck/config.toml
[extensions]
enabled = true
paths = ["~/dev/workdeck-ext/bin/workdeck-ext"]

[extension.my-extension]
some_key = "some value"                 # opaque data handed to the extension
```

`[extensions] enabled` layers like every other option (repository config
overrides user config); `--no-extensions` is a hard off switch that no config
layer can re-enable. Extension-provided CLI trees use the native
`register_cli_command` declaration; bare help stays static while
`workdeck <extension-command> --help` belongs to the extension. The
`github-pr` example demonstrates direct HTTP preprocessing, cancellation,
temporary input ownership, and one-time delegation without a JavaScript
runtime. `[extension.<id>]` tables pass through uninterpreted; see the extension
API page for merge rules and caveats.

## A complete example

Collapse lockfiles and generated output out of every review, and report how many
files were hidden. The compiled example under
`examples/extensions/collapse-generated/` uses the same transform declaration
and newline-delimited protocol as production extensions:

```rust
// examples/extensions/collapse-generated/src/lib.rs
use workdeck_extension_api::{ChangesetTransform, TransformContext};

pub fn transform(
    changeset: workdeck_extension_api::Changeset,
    ctx: &TransformContext,
) -> anyhow::Result<workdeck_extension_api::Changeset> {
    let patterns = ctx.config().get_string_list("patterns").unwrap_or_else(|| {
        vec!["*.lock".into(), "*-lock.json".into(), "dist/*".into()]
    });
    let kept = changeset
        .files
        .iter()
        .filter(|file| !patterns.iter().any(|pattern| workdeck_core::glob_matches(pattern, &file.path)))
        .cloned()
        .collect::<Vec<_>>();
    let hidden = changeset.files.len() - kept.len();
    if hidden > 0 {
        ctx.notify(format!("Collapsed {hidden} generated file{}", if hidden == 1 { "" } else { "s" }));
    }
    Ok(changeset.with_files(kept))
}
```

Configure it without changing the executable:

```toml
# .workdeck/config.toml
[extension.collapse-generated]
patterns = ["*.lock", "bun.lockb", "generated/*"]
```

Try it against the working tree without installing it:

```bash
workdeck diff --extension ./collapse-generated
```

Continue with the [extension API](/docs/extend/extension-api/) for the complete
declarative surface, host capabilities, lifecycle events, and validation rules.

Adapted from Hunk's MIT extension guide, Copyright Modem Labs Inc. Workdeck's
native implementation and this migration preserve the documented discovery,
configuration, trust, failure, and command behavior while replacing the
TypeScript runtime with the Rust SDK.
