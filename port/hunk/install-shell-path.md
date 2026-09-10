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
profile application, backups, end-to-end shell startup execution and full source
diagnostic parity remain open. No installer interval is mapped complete.

Expanded tests cover redirected and empty `ZDOTDIR`, Bash's complete startup-file
precedence, missing-profile defaults and duplicate GitHub Actions appends. Shell
selection now strips trailing slashes before selecting the basename, matching
the pinned `basename "$SHELL"` behavior; `/bin/bash///` selects Bash rather than
falling through to `.profile`. Four native PATH-planner tests pass without writing
the proposed edits or creating the redirected zsh directory.
