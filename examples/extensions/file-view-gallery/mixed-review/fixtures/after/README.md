# Review Console

Review Console is a Rust terminal workspace for understanding release changes before they ship.

## Quick start

1. Install Workdeck with `cargo install workdeck`.
2. Run `workdeck diff --watch` in a Git checkout.
3. Review the working-tree changes shown in the terminal.
4. Press `?` at any time to inspect the active command map.

## Review workflow

The default workflow loads a changeset, preserves its narrative file order, and opens the first
hunk. Reviewers can move between hunks, leave notes, preview semantic file views, and export a
Markdown summary.

### Keyboard controls

- `[` selects the previous file.
- `]` selects the next file.
- `p` selects the previous hunk.
- `n` selects the next hunk.
- `/` focuses the file filter.
- `F8` toggles the installed semantic preview for the selected file.
- `q` exits the review.

## Configuration

Configuration is layered from user settings and `.agents/workdeck/config.toml` in the current
repository. The repository file may define a theme, default layout, ignored paths, and extension
folders.

```toml
theme = "github-dark-default"
mode = "stack"
ignored = ["target/**"]
extensions = ["./review-extensions"]
```

## CI integration

Use `workdeck changes diff --jsonl` in CI. The command emits stable records for downstream review
automation. Generated reports can be written to `artifacts/review.jsonl` and retain stable file and
hunk identities.

## Security

Repository extensions run only after an explicit trust decision.
Keep access tokens in the environment rather than committing them to configuration files.

## Support

Open an issue with the terminal type, operating system, active extensions, and a minimal patch that
reproduces the problem.
