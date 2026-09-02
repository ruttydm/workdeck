# GitHub pull request extension

Review a GitHub pull request in Workdeck with a native extension-provided CLI command:

```console
workdeck gh 123
```

The extension fetches the PR diff directly from GitHub's API, writes it to a private temporary
patch, and delegates once to Workdeck's built-in `patch` command. It is compiled Rust, has no npm
or JavaScript runtime dependency, and does not require the `gh` CLI.

## Try it from this checkout

Build and stage the complete native extension folder, then place `--extension` before the
extension-owned command:

```console
cargo xtask extension stage-example github-pr
workdeck --extension target/workdeck-extension-examples/github-pr gh 123
```

A bare number infers `owner/repo` from the current checkout's GitHub `origin`. Explicit forms work
outside a checkout and do not invoke Git:

```console
workdeck gh 123 --repo modem-dev/hunk
workdeck gh 'modem-dev/hunk#123'
workdeck gh https://github.com/modem-dev/hunk/pull/123
```

Quote the `owner/repo#number` form because an unquoted `#` starts a comment in some shells. Use
`--` to pass options to the delegated `workdeck patch` command:

```console
workdeck gh 123 --repo modem-dev/hunk -- --pager --mode stack
```

Run `workdeck gh --help` for extension-owned help. The handler does not read standard input.

## Install it

This folder is a complete Workdeck native extension. Build the binary, place it at
`bin/workdeck-example-github-pr-extension` (plus `.exe` on Windows), and install or explicitly load
the folder through Workdeck's trust-gated extension workflow. The staging command above assembles
that layout reproducibly from the Cargo workspace.

## Authentication and security

Public repositories work anonymously within GitHub's API rate limits. For private repositories or
higher limits, set a token; `GH_TOKEN` takes precedence over `GITHUB_TOKEN`.

The extension only accepts `github.com` PR URLs and only sends credentials to the fixed
`https://api.github.com` endpoint. Redirects are refused, token and response-body details are not
copied into errors, the request has a bounded deadline, and fetched diffs are bounded to 64 MiB.
Origin inference executes `git remote get-url origin` directly without a shell, with a five-second
deadline and a 16 KiB output bound.

PR patches can contain private source. On POSIX systems, the extension creates a mode-`0700`
temporary directory and a mode-`0600` patch. Windows inherits the ACL of the user's system
temporary directory. Workdeck keeps the native extension alive while the delegated review can
reload its patch, then sends a bounded graceful-shutdown notification that removes the patch.
Abrupt process termination may leave cleanup to the operating system's temporary-file policy.

GitHub Enterprise is intentionally outside this example's host and credential policy. Native
extensions are trusted programs with process isolation, not security sandboxes.
