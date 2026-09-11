# Website documentation migrations

The native Zola pages below replace the pinned Astro/Starlight Markdown pages.
Each source blob is read from the pinned Git tree, hashed, and checked for the
complete heading and fenced-command surface. Hunk command names are normalized
to Workdeck names; native pages may add Rust-specific installation and trust
guidance, but they may not silently drop a documented section or command.

The verifier is `xtask::website_docs`; its test is executable evidence for every
ledger interval listed here. Runtime behavior described by a page still has the
corresponding CLI/TUI test gate; documenting a command is not used as a
substitute for those behavioral tests.

Migrated sources:

- `website/src/content/docs/docs/start/quick-start.md`
- `website/src/content/docs/docs/start/keyboard-and-mouse.md`
- `website/src/content/docs/docs/configure/keybindings.md`
- `website/src/content/docs/docs/configure/configuration.md`
- `website/src/content/docs/docs/configure/layout-and-display.md`
- `website/src/content/docs/docs/configure/themes.md`
- `website/src/content/docs/docs/workflows/files-and-patches.md`
- `website/src/content/docs/docs/workflows/watch-mode.md`
- `website/src/content/docs/docs/workflows/jujutsu-and-sapling.md`
- `website/src/content/docs/docs/workflows/working-trees-and-commits.md`
- `website/src/content/docs/docs/help/compatibility.md`
- `website/src/content/docs/docs/help/troubleshooting.md`
- `website/src/content/docs/docs/agents/review-with-an-agent.md`
- `website/src/content/docs/docs/agents/live-session-control.md`
- `website/src/content/docs/docs/agents/comments-and-annotations.md`
- `website/src/content/docs/docs/agents/agent-context-and-stml.md`
- `website/src/content/docs/docs/workflows/git-pager-and-difftool.md`
- `website/src/content/docs/docs/agents/review-skill.md`
- `website/src/content/docs/docs/extend/extensions.md`
