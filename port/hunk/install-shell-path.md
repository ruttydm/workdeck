# Native shell PATH planning

`install::shell_path::plan` is a read-only translation of the installer PATH
selection and append rules. `--no-modify-path` takes precedence over `GITHUB_PATH`;
otherwise GitHub Actions receives a raw directory line. zsh honors nonempty
`ZDOTDIR`, Bash selects the first existing `.bashrc`, `.bash_profile` or `.profile`
(defaulting to `.bashrc`), fish uses `config.fish`, and other shells use `.profile`.
Single quotes in directory values use the source shell-quoting escape.

The planner returns skipped, already-present or an edit containing exact original
bytes and proposed replacement bytes. It preserves non-UTF-8 profile content and
the source `grep -Fq` substring semantics, including matches inside comments.
GitHub Actions appends are not deduplicated, matching the source. Proposed profile
edits use Workdeck attribution; planning never creates a file or directory.

Two native tests pass across shell selection, quoting, binary original content,
GitHub Actions, no-modify precedence and substring idempotence. Transactional
profile application and backup work is described below; end-to-end shell startup
execution and full source diagnostic parity remain open. No installer interval is
mapped complete.

Expanded tests cover redirected and empty `ZDOTDIR`, Bash's complete startup-file
precedence, missing-profile defaults and duplicate GitHub Actions appends. Shell
selection now strips trailing slashes before selecting the basename, matching
the pinned `basename "$SHELL"` behavior; `/bin/bash///` selects Bash rather than
falling through to `.profile`. Four native PATH-planner tests pass without writing
the proposed edits or creating the redirected zsh directory.

## Explicit application and recovery

`install::shell_path::apply` applies an edit only when the existing bytes still
match the plan. Before replacement it creates a new, non-overwriting recovery
JSON file containing the absolute profile path, exact original bytes (or null
for an absent profile), and the original Unix permission mode. The recovery path
must differ from the profile path, including when the profile does not yet exist.
Skipped and already-present plans do not write anything.

Replacement uses a synchronized temporary file in the destination directory.
Existing permissions are preserved and rechecked alongside content immediately
before replacement. Missing profiles use non-overwriting creation; on Unix new
profiles inherit the temporary file's private mode, rather than shell umask
defaults. Failures after recovery creation retain the recovery record.

Tests use temporary directories only: original non-UTF-8 bytes, stale plans,
idempotent replanning, preexisting recovery files, missing fish directories and
recovery/profile path collisions. These do not execute user shell startup files.
Parent-directory races, a write after the final content check, crash-durable
directory synchronization, multi-file rollback and installer command integration
remain open. Late failures may leave newly created parent directories behind.
This API does not establish complete installer parity or change ledger coverage.

Verification: `cargo test -p workdeck-cli --lib install::shell_path` passes all
nine tests on this macOS host. This is scoped native coverage, not cross-platform
or source-oracle evidence for the application transaction.

Two additional execution tests source the applied temporary profile using real
`/bin/sh`, `/bin/bash` and `/bin/zsh` on macOS, with a cleared environment and
temporary HOME/ZDOTDIR. They assert the exact resulting PATH and empty stderr for
a directory containing whitespace, a single quote, dollar expansion, backticks
and command-substitution syntax. Those characters remain literal directory data;
replanning recognizes the applied line. The POSIX shell test runs on Unix; the
Bash/zsh test is macOS-only and does not silently skip a missing interpreter.
These source a selected profile explicitly rather than simulating login startup.
Fish execution remains unverified because fish is not installed on this host.
