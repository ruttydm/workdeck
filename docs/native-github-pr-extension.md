# Native GitHub pull-request review

The `github-pr` example is Workdeck's executable parity port of Hunk's generic `gh` extension. It
also exercises a lifecycle guarantee needed by any native extension that delegates to a reloadable
temporary input.

## Command and ownership flow

1. The extension parses one PR number, `owner/repo#number`, or an unmodified `github.com` PR URL.
2. A bare number resolves `origin` through a bounded, no-shell Git child process.
3. A native Rust HTTP client requests the diff from the fixed GitHub API origin. Authentication,
   redirects, response size, errors, cancellation, and time are bounded before bytes become review
   input.
4. The extension creates a private patch and delegates `patch PATH` plus byte-preserved arguments
   appearing after `--`.
5. The CLI retains the loaded extension registry for the whole delegated review. On return, the
   host sends `workdeck/shutdown` to extensions that registered the `shutdown` lifecycle event,
   gives them a bounded grace period, then force-terminates only non-cooperating children.
6. The extension removes retained patches when its final live registry retires. Replacing a
   registry cannot invalidate a patch still used by the replacement.

The handler reports that it never started or consumed stdin. Workdeck can therefore preserve its
normal terminal ownership rules during delegation.

## Trust and transport

The executable uses extension API v1 JSON-RPC over newline-delimited stdio and declares only
`cli-commands` and `events`. Standard error remains reserved for native-process logs; command output
is transported as byte arrays in `workdeck/cli/output` notifications. The host validates that the
delegate targets a built-in Workdeck command and rejects recursive extension loading.

The executable is a trusted native program. The transport provides capability validation, bounded
requests, cancellation, and crash isolation; it is not a filesystem or network sandbox.

## Verification

`examples/tests/github_pr.rs` translates every baseline source test and adds Rust-specific checks
for streamed body limits, POSIX modes, native manifest/handshake behavior, and graceful shutdown.
`port/hunk/oracles/github-pr.json` records the outputs observed by executing the pinned baseline and
stable Hunk implementations through injected Git, network, CLI-stream, and lifecycle boundaries.
