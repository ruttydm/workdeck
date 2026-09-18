# Review Console

Review Console is a Rust terminal workspace for inspecting release changes before they ship.

## Quick start

1. Install Workdeck with `cargo install workdeck`.
2. Run `workdeck diff` in a Git checkout.
3. Review the working-tree changes shown in the terminal.

## Review workflow

The default workflow loads a changeset, groups files by directory, and opens the first hunk.
Reviewers can move between hunks, leave notes, and export a text summary.

### Keyboard controls

- `[` selects the previous hunk.
- `]` selects the next hunk.
- `/` focuses the file filter.
- `q` exits the review.

## Configuration

Configuration is loaded from `.agents/workdeck/config.toml` in the current repository.
The file may define a theme, a default layout, and ignored paths.

```toml
theme = "github-dark-default"
mode = "split"
ignored = ["target/**"]
```

## CI integration

Use `workdeck changes diff --json` in CI. The command emits a stable machine-readable envelope.
Generated reports can be written to `artifacts/review.json`.

## Security

Repository configuration is treated as data and does not silently execute extensions.
Keep access tokens in the environment rather than committing them to configuration files.

## Support

Open an issue with the terminal type, operating system, and a minimal patch that reproduces the
problem.
