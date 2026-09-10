# Contributing to Workdeck

Thanks for helping improve Workdeck. This guide is here to help you get an issue understood and a pull request merged without unnecessary back-and-forth.

The short version: solve one clear user problem, understand the code you submit, reuse the existing architecture, and show how you verified the change.

## Choose the right place to start

- **Question or early idea:** ask in the [existing Workdeck issues](https://github.com/ruttydm/workdeck/issues) before opening a contribution proposal.
- **Bug:** use the bug report template.
- **Feature or larger change:** open a contribution proposal before investing in an implementation.
- **Small, well-understood fix:** a direct pull request is welcome.

Search existing issues and pull requests first. If related work exists, explain how your report or proposal differs.

## Issues and proposals

A useful bug report lets someone else reproduce the problem without guessing. Include:

- what happened and what you expected instead;
- the smallest command, input, or repository state that reproduces it;
- your Workdeck version or commit;
- operating system, terminal, shell, installation method, and relevant configuration; and
- screenshots, terminal output, logs, or a sanitized fixture when they help.

Workdeck has several input paths (`diff`, `show`, `stash show`, `patch`, `pager`, and `difftool`), so name the exact invocation that failed. For a regression, include the last version or configuration that worked if you know it.

For performance problems, also include the approximate input size, timings or memory use, and a comparison made on the same machine. Keep the actionable summary near the top; deeper investigation can follow.

For a proposed change, describe the user problem, why it matters, the intended behavior, and important non-goals. An implementation idea is useful, but agreement on the problem and product behavior comes first. Say whether you plan to work on it yourself.

Please discuss a change before implementation when it introduces or substantially changes a command, configuration option, public API, dependency, shared review behavior, or broad UI/architecture direction.

## Could this be an extension?

Before adding a built-in workflow, integration, or alternate presentation, check whether it can be implemented as an extension. Extensions are usually the better home for opt-in behavior such as VCS integrations, sidebars, file views, commands, keyboard modes, line highlighters, and repository-specific review workflows.

Start with [`docs/native-extension-application.md`](docs/native-extension-application.md) and the checked-in [`examples/extensions/`](examples/extensions/). If you use a coding agent, [`skills/workdeck-extensions/SKILL.md`](skills/workdeck-extensions/SKILL.md) maps the public API and its implementation. A small prototype is often the fastest way to learn whether the current API is enough.

If the extension API cannot express the idea, do not immediately bypass it with feature-specific core code. Explain:

- where the current API becomes blocked;
- the smallest general capability the host would need to expose;
- whether more than one extension could use it;
- which safety, lifecycle, performance, or review-consistency rules the host must retain; and
- how a real extension or bundled implementation will exercise it.

A good extension-system change enables a class of ideas rather than adding a hook named after one feature. Public extension APIs are long-lived, so discuss new capabilities before implementing them and keep the host contract small.

Core is still the right home for behavior every review surface must share, terminal rendering invariants, and product behavior that should work without an extension. If the boundary is unclear, open a proposal and ask: **could this be an extension, and if not, should we extend the extension system first?**

## Own the work you submit

AI tools are welcome, and you do not need to have written or memorized every line yourself. You are still responsible for understanding the change at the level that matters for review:

- the user-visible behavior and why it is needed;
- the main control and data flow;
- where state, lifecycle, and failure handling live;
- how the change fits Workdeck's architecture without creating a parallel path; and
- what the tests and manual checks actually prove.

Read the final diff, verify its claims, and remove generated boilerplate or code that is not needed. If review uncovers behavior you cannot reason about, pause and investigate it rather than forwarding an agent's answer unchecked.

If you use a coding agent, start it from the repository root so it sees [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md). Never send secrets, private source, or other material you are not allowed to share to an external tool.

Keep the change focused on one user problem. Before adding another helper, state path, renderer path, command path, or protocol, find the existing owner and decide whether it should be extended instead. If a new implementation replaces an old one, remove the obsolete path and tests. Fix the failure class, not only the example that exposed it.

Read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for ownership and architecture rules, [README validation](README.md#validate) for canonical checks, and [native release policy](docs/native-release-policy.md) for the release gates. Consult the relevant guidance before making substantial changes.

## Development setup

Requirements:

- Rust and Cargo (use the repository's pinned toolchain)
- Native prerequisites for the Cargo dependencies; Zola, Chromium/WebDriver and FFmpeg for their respective website/media checks
- Git
- macOS, Linux, or Windows

Install dependencies and run Workdeck from source:

```bash
cargo build --locked -p workdeck-cli --bin workdeck
cargo run --locked -p workdeck-cli --bin workdeck -- diff
```

Nix users can run `nix develop` or use [direnv](https://direnv.net/) to enter the development shell.

## Show UI changes

For user-visible terminal changes, include visual evidence in the pull request.

- A short video is best for interaction, scrolling, resizing, animation, or mouse behavior.
- Before-and-after screenshots are completely acceptable for static visual changes.
- Include the command, terminal dimensions, layout, theme, operating system, and terminal used.
- Demonstrate keyboard and mouse behavior when the change affects an action that supports both.
- Use the real Workdeck TUI rather than a mockup or redirected stdout capture.

The source checkout includes [`skills/workdeck-launch-video/SKILL.md`](skills/workdeck-launch-video/SKILL.md), which generates polished videos from real PTY-driven Workdeck frames. If you use a coding agent, ask it to follow the skill's **single-feature recipe**. The pipeline is Unix-only and requires Chromium and ffmpeg; screenshots are fine when it is not practical to run.

Upload media to the pull request. Do not commit `.video-work/`, captured frames, or encoded videos.

## Pull requests

A helpful pull request description explains:

- the problem and user impact;
- the approach and important non-goals;
- why the solution belongs in core or an extension;
- tests and manual checks performed;
- platforms tested or not tested;
- visual evidence for UI changes; and
- known limitations or follow-up work.

Use the ownership guidance in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and the workspace validation commands in [README](README.md#validate). Colocate Rust unit tests with their implementation; use the owning crate's `tests/` directory for integration contracts and the existing PTY, oracle and examples harnesses for user-visible behavior. List the exact commands you ran. If a relevant check was not run, say why. If a failure also occurs on `main`, reproduce it there and include the command and result rather than only calling it unrelated.

Keep the branch current with `main`. Prefer rebasing over repeatedly merging `main` into the branch. If you force-push after review starts, leave a short note describing what changed.

Stacked pull requests are welcome when each change is reviewable on its own. Name the base pull request and merge order, then rebase after the base lands so each diff contains only its own work.

Use a [Conventional Commit](https://www.conventionalcommits.org/) title such as `fix(ui): keep the selected hunk visible`. Update documentation, examples, and generated artifacts when public behavior or APIs change. Do not commit local review artifacts such as `.agents/workdeck/`.

For a user-visible change, add a native release fragment targeting `workdeck-cli`:

```bash
cargo xtask changelog add fix-example patch "Fix the user-visible problem."
```

Use `patch` for fixes, `minor` for features, and `major` for breaking changes. Keep non-empty fragment summaries to one user-facing sentence. For maintenance-only work, create an empty fragment with `cargo xtask changelog add maintenance-example empty`, and do not edit `CHANGELOG.md` directly. See [native release fragments](docs/native-release-fragments.md) for IDs, storage, preparation and recovery.

Before requesting review, read the final diff once more. Make sure it solves one clear problem, does not duplicate an existing path, removes anything it supersedes, includes relevant evidence, and reports validation honestly.

Thanks for taking the time to make the change easy to understand and review.

## Verification and port scope

Run the exact checks appropriate to the change from [README validation](README.md#validate), including formatting, Clippy and workspace tests. The full semantic port also requires strict `cargo xtask port audit`, dependency/license checks, native platform gates, oracle comparisons and release evidence. A scoped passing test is not full parity. Keep incomplete source intervals unmapped and record accurate `Hunk-Port:` and `Hunk-Upstream:` commit trailers. Do not push, tag, publish, or change external installations without explicit authorization.

This guide adapts the complete MIT-licensed upstream contributor guide from Hunk `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`, copyright Modem Labs Inc. See [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES). Native links and commands replace upstream runtime-specific tooling; this document does not claim the full repository port is complete.
