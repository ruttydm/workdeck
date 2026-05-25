# Workdeck

Workdeck is a terminal-native sidecar for agentic coding. It runs as a narrow pane beside Codex, an editor, lazygit, or another terminal workflow and keeps a structured mental map of repo changes, files, local tasks, and agent sessions.

## Current Features

- Rust single-binary CLI
- Ratatui + crossterm terminal UI
- Default `Changes` tab with dirty Git files grouped by directory
- Changes grouping toggle for directory view or status/intent view
- Compact change glyphs: `+` untracked, `M` modified, `A` added, `D` deleted, `R` renamed, `S` staged, `S+` staged plus unstaged
- Dirstat weight toggle for file counts and compact churn like `+12`, `-4`, or `+12/-4`
- Folder-shaped `Files` tab with `.gitignore` support
- File and diff preview with syntax highlighting
- File-backed local issue store under `.agents/workdeck/`
- TOML issues with stable keys like `WD-1`
- Unknown top-level issue and agent-session TOML fields are preserved on save
- Local task status, priority, label, and assignee keyboard actions
- Issue-to-file linking
- Agent session TOML loading
- Fuzzy search across files, changes, issues, projects, cycles, labels, symbols, and agent sessions
- Typed Search previews: change results show diffs, file and symbol results show file previews
- Superseding async refreshes so stale scans do not overwrite newer repo snapshots
- JSON status export for automation

## Install

### Recommended: Cargo

```sh
cargo install --git https://github.com/workdeck/workdeck --package workdeck-cli --locked
workdeck --version
```

Cargo installs the `workdeck` binary into `~/.cargo/bin`. If your shell cannot find it, add Cargo's bin directory to your `PATH`:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
```

For zsh, add that line to `~/.zshrc`. For bash, add it to `~/.bashrc` or `~/.bash_profile`.

### From a local checkout

```sh
cargo install --path crates/workdeck-cli --locked
workdeck
```

### Homebrew

The repo includes a HEAD-only formula in `Formula/workdeck.rb`. Homebrew 5 requires formulae to be installed through a tap, so tap this repo first:

```sh
brew tap workdeck/tap https://github.com/workdeck/workdeck
brew install --HEAD workdeck/tap/workdeck
workdeck --version
```

From a local checkout, you can tap the checkout path:

```sh
brew tap workdeck/tap "$(pwd)"
brew install --HEAD workdeck/tap/workdeck
```

Once the public tap is published as its own repository, the intended stable install command is:

```sh
brew install workdeck/tap/workdeck
```

That stable tap command is reserved for the future `workdeck/tap` repository; use Cargo or the HEAD formula until the tap exists.

### Release Tarballs

Tagged releases build Linux and macOS tarballs named `workdeck-<target>.tar.gz`. Replace `vX.Y.Z` with the release tag you want to install.

```sh
curl -LO https://github.com/workdeck/workdeck/releases/download/vX.Y.Z/workdeck-aarch64-apple-darwin.tar.gz
curl -LO https://github.com/workdeck/workdeck/releases/download/vX.Y.Z/workdeck-aarch64-apple-darwin.tar.gz.sha256
shasum -a 256 -c workdeck-aarch64-apple-darwin.tar.gz.sha256
tar -xzf workdeck-aarch64-apple-darwin.tar.gz
sudo install -m 0755 workdeck-aarch64-apple-darwin/workdeck /usr/local/bin/workdeck
workdeck --version
```

Without sudo, install into a user-local bin directory:

```sh
mkdir -p "$HOME/.local/bin"
install -m 0755 workdeck-aarch64-apple-darwin/workdeck "$HOME/.local/bin/workdeck"
export PATH="$HOME/.local/bin:$PATH"
```

Use the archive matching your platform:

```text
workdeck-x86_64-unknown-linux-gnu.tar.gz
workdeck-x86_64-apple-darwin.tar.gz
workdeck-aarch64-apple-darwin.tar.gz
```

## Quick Start

Run the TUI in a Git repo:

```sh
workdeck
```

Initialize the repo-local Workdeck store:

```sh
workdeck --init
```

Print a read-only Git status snapshot:

```sh
workdeck --status-json
workdeck status --json
workdeck changes list --group status --json
workdeck changes diff crates/workdeck-cli/src/main.rs
workdeck files list crates/workdeck-cli/src --json
workdeck files show README.md --json
workdeck search preview --target files,changes --json
```

Export local Workdeck data without opening the TUI:

```sh
workdeck export
workdeck export --jsonl
```

Validate the current repo/config/data state:

```sh
workdeck doctor
workdeck doctor --json
```

Manage local issues:

```sh
workdeck issue create "Render nested changes" --status todo --priority high --due-at 2026-05-31 --file src/main.rs
workdeck issue create --from-json issue.json --json
workdeck issue update WD-1 --status in-progress --label git,mvp --commit abc123
workdeck issue link-file WD-1 crates/workdeck-cli/src/views/mod.rs
workdeck issue link-commit WD-1 abc123
workdeck issue close WD-1
workdeck issue reopen WD-1
workdeck issue list --status todo --label git --json
workdeck issue show WD-1 --json
```

Manage local projects, cycles, and labels:

```sh
workdeck project save "Workdeck MVP" --description "Initial local release"
workdeck cycle save "MVP" --id mvp --starts-at 2026-05-24
workdeck label save "Git" --color green
workdeck project show workdeck-mvp --json
workdeck cycle list --json
workdeck label list
```

Record agent sessions:

```sh
workdeck agent record "Implement preview cache" \
  --agent codex \
  --status done \
  --goal "Keep preview loading off the render path" \
  --plan "Inspect current preview flow" \
  --plan "Move file/diff loading off render" \
  --file crates/workdeck-cli/src/app.rs \
  --command "cargo test" \
  --test "cargo test"

workdeck agent import sessions.jsonl
workdeck agent append-plan 20260524123000-implement-preview-cache "Run clippy"
workdeck agent add-file 20260524123000-implement-preview-cache crates/workdeck-cli/src/app.rs
workdeck agent finish 20260524123000-implement-preview-cache --summary "Preview cache landed"
workdeck agent list
workdeck agent show 20260524123000-implement-preview-cache --json
```

Manage config, events, and imports headlessly:

```sh
workdeck config path
workdeck config set ui.preview false
workdeck config validate
workdeck events list --json
workdeck import export.json --dry-run --json
```

Check an installation:

```sh
workdeck --version
workdeck --help
workdeck doctor
```

Headless commands use stable non-zero exit classes where possible: `2` for validation, `3` for not found, `4` for conflicts, and `5` for config/store parse errors.
Commands with `--json` return an envelope:

```json
{
  "ok": true,
  "kind": "issue",
  "action": "create",
  "data": {}
}
```

Runtime failures for `--json` commands return:

```json
{
  "ok": false,
  "error": {
    "code": "not_found",
    "message": "issue WD-404 does not exist"
  }
}
```

## Data Layout

Workdeck stores repo-local data in `.agents/workdeck/`:

```text
.agents/workdeck/
  config.toml
  issues/
    WD-1.toml
  projects.toml
  cycles.toml
  labels.toml
  agents/
    session-id.toml
  events.jsonl
```

Opening the TUI reads existing data but does not create `.agents/workdeck/` by itself. Use `workdeck --init` or create an issue from the TUI when you want the local store created.

## Config

The repo-local config lives at `.agents/workdeck/config.toml` after `workdeck --init`.
User-level defaults can also live at `~/.config/workdeck/config.toml`; repo-local config overrides those values.

```toml
[ui]
theme = "auto" # auto, light, or dark
preview = true

[paths]
data_dir = ".agents/workdeck"

[keys]
quit = "q"
refresh = "r"
search = "/"
help = "?"
changes = "c"
files = "f"
tasks = "i"
agents = "a"
toggle_preview = "t"
group_changes = "g"
toggle_dirstat = "w"
open_editor = "o"
copy = "y"
new_issue = "n"
edit_issue = "e"
status = "s"
priority = "p"
labels = "l"
assign = "A"
jump = "space"
link_file = "L"
```

Workdeck validates configured keybindings at startup. Empty bindings, unsupported multi-character bindings, and duplicate action bindings fail before the TUI enters raw mode.

## Keys

```text
j/k or arrows      move tree selection; scroll preview when preview is focused
h                  collapse tree directory, parent folder in narrow Files, or return from preview
l                  expand tree directory, enter folder in narrow Files, or focus preview
Tab / Shift-Tab    switch tabs
Enter              enter narrow Files folder, focus preview, or open selected issue
g/G                jump preview top/bottom when preview is focused
c                  changes
f                  files
i                  tasks
a                  agents
/                  search
?                  help
t                  toggle preview
g                  group changes by directory/status
w                  toggle dirstat weight
n                  create issue from selection
e                  edit selected issue in $EDITOR
s                  cycle issue status
p                  cycle issue priority
l                  toggle next configured label on selected issue
A                  assign/unassign selected issue to $WORKDECK_ASSIGNEE or $USER
Space              jump issue <-> linked file
L                  link selected file to selected issue
o                  open selected file in $EDITOR
y                  copy selected path or issue key
r                  refresh
q                  quit
```

## Verify

```sh
cargo fmt --all --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
cargo package --allow-dirty -p workdeck-cli
```

CI runs the same format, test, clippy, and release-build gates on Linux and macOS. Tagged releases matching `v*` build tarballs for Linux x86_64, macOS x86_64, and macOS arm64.

Run the local soak gate:

```sh
scripts/soak.sh
```

The soak gate runs formatting, tests, clippy, release build, package verification, temp-root install, TUI quit/Ctrl-C pseudo-terminal smokes, and a synthetic 300-change Git status performance check.
