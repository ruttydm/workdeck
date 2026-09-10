+++
title = "Native extensions"
description = "Install and load trusted native extension executables."
template = "docs.html"
+++

Workdeck extensions are compiled executables described by
`workdeck-extension.toml`. They communicate with the host using JSON-RPC 2.0 over
newline-delimited standard input/output. Standard error is for logs. Ratatui
rendering remains in the host; extensions return declarative content and actions.

This replaces Hunk's TypeScript default-export factories, `package.json` entry
discovery and npm dependency installation. Workdeck does not execute TypeScript
extensions or provide a JavaScript compatibility runtime. Existing extensions
require a rewrite, not a renamed entry file.

## A manifest

The checked-in startup lifecycle example uses:

```toml
id = "startup-lifecycle"
name = "Startup lifecycle fixture"
version = "0.1.0"
api_version = 1
executable = "workdeck-example-startup-lifecycle-extension"
capabilities = ["configuration", "themes", "events"]
```

The executable must actually exist at its declared location. A manifest alone is
not an implementation. The compiled example and its protocol implementation live
under `examples/extensions/startup-lifecycle/` in the repository.

## Load and manage

```sh
workdeck extension validate ./my-extension/workdeck-extension.toml
workdeck diff --extension ./my-extension
workdeck diff --no-extensions
workdeck extension install /path/to/native-extension
workdeck extension list
workdeck extension update
workdeck extension remove <name>
```

The management commands change local installation state. `remove` deletes the
selected managed installation; inspect `list` before choosing its name. Explicit
`--extension` paths express execution intent: never pass unreviewed code merely
because it came with a repository. Disabling user extensions does not remove
host-compiled providers and capabilities.

## Trust and ownership

Native extensions run with your full user permissions. Subprocess isolation,
timeouts and payload limits are failure containment, not a security sandbox.
Repository discovery is trust-gated; do not turn repository content into trusted
code automatically. The manager exposes an explicit trust decision:

```sh
workdeck extension trust --repo /path/to/repository --allow --yes
workdeck extension trust --repo /path/to/repository --deny --yes
```

Only use the allow command after reviewing the native code and its origin.
Imported legacy trust does not authorize a rewritten native replacement.

## Configuration and further migration

User and repository configuration can supply `[extensions]` loading settings and
opaque `[extension.<id>]` configuration. The native discovery model distinguishes
explicit paths, user-config entries, global discovery and repository discovery;
its source is `crates/workdeck-extension-host/src/extension_discovery.rs`.

See the [legacy extension inventory](/extensions/) for migration status. The
complete discovery rules, authoring SDK guide, publication recipe and upstream
hello/collapse-generated examples still need their full native documentation and
executable migration evidence. No registry publication or package installation
is performed by reading this guide.

Adapted in part from Hunk's MIT extension guide, Copyright Modem Labs Inc.
Its source interval remains unmapped: this page documents verified native
boundaries and commands, not a completed port of the whole upstream guide.
