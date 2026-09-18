//! Reproduce the entire pinned contributor guide through explicit native adaptations.
use anyhow::{Result, ensure};
use std::{
    fs,
    path::{Component, Path},
};

const ADAPTATIONS: &[(&str, &str, usize)] = &[
    ("Hunk", "Workdeck", 8),
    (
        "[Modem Discord](https://discord.gg/WZFjaP6Gt8)",
        "[existing Workdeck issues](https://github.com/ruttydm/workdeck/issues) before opening a contribution proposal",
        1,
    ),
    (
        "docs/extensions.md",
        "docs/native-extension-application.md",
        2,
    ),
    (
        "skills/hunk-extensions/SKILL.md",
        "skills/workdeck-extensions/SKILL.md",
        2,
    ),
    ("AGENTS.md", "docs/ARCHITECTURE.md", 6),
    (
        "- Bun 1.3.14+\n- Node.js 22+ for npm package verification and release tasks",
        "- Rust and Cargo (use the repository's pinned toolchain)\n- Native prerequisites for the Cargo dependencies; Zola, Chromium/WebDriver and FFmpeg for their respective website/media checks",
        1,
    ),
    (
        "bun install\nbun run src/main.tsx -- diff",
        "cargo build --locked -p workdeck-cli --bin workdeck\ncargo run --locked -p workdeck-cli --bin workdeck -- diff",
        1,
    ),
    (
        "skills/launch-video/SKILL.md",
        "skills/workdeck-launch-video/SKILL.md",
        2,
    ),
    ("test/README.md", "README.md#validate", 2),
    ("`.hunk/latest.json`", "`.agents/workdeck/`", 1),
    (
        "For a user-visible change, add a Changeset targeting `hunkdiff`:",
        "For a user-visible change, add a native release fragment targeting `workdeck-cli`:",
        1,
    ),
    (
        "bun run changeset\n",
        "cargo xtask changelog add fix-example patch \"Fix the user-visible problem.\"\n",
        1,
    ),
    (
        "Use `patch` for fixes, `minor` for features, and `major` for breaking changes. Keep non-empty Changeset summaries to one user-facing sentence. For maintenance-only work, create an empty Changeset with `bun run changeset -- --empty`, and do not edit `CHANGELOG.md` directly.",
        "Use `patch` for fixes, `minor` for features, and `major` for breaking changes. Keep non-empty fragment summaries to one user-facing sentence. For maintenance-only work, create an empty fragment with `cargo xtask changelog add maintenance-example empty`, and do not edit `CHANGELOG.md` directly. See [native release fragments](docs/native-release-fragments.md) for IDs, storage, preparation and recovery.",
        1,
    ),
    (
        "[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) contains the repository's architecture rules, canonical commands, test expectations, cross-platform guidance, and release process. Read the relevant sections before making substantial changes rather than duplicating those instructions here.",
        "Read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for ownership and architecture rules, [README validation](README.md#validate) for canonical checks, and [native release policy](docs/native-release-policy.md) for the release gates. Consult the relevant guidance before making substantial changes.",
        1,
    ),
    (
        "Use the validation guidance in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and the test placement guidance in [`README.md#validate`](README.md#validate).",
        "Use the ownership guidance in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and the workspace validation commands in [README](README.md#validate). Colocate Rust unit tests with their implementation; use the owning crate's `tests/` directory for integration contracts and the existing PTY, oracle and examples harnesses for user-visible behavior.",
        1,
    ),
];
const SCOPE: &str = "\n## Verification and port scope\n\nRun the exact checks appropriate to the change from [README validation](README.md#validate), including formatting, Clippy and workspace tests. The full semantic port also requires strict `cargo xtask port audit`, dependency/license checks, native platform gates, oracle comparisons and release evidence. A scoped passing test is not full parity. Keep incomplete source intervals unmapped and record accurate `Hunk-Port:` and `Hunk-Upstream:` commit trailers. Do not push, tag, publish, or change external installations without explicit authorization.\n\nThis guide adapts the complete MIT-licensed upstream contributor guide from Hunk `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`, copyright Modem Labs Inc. See [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES). Native links and commands replace upstream runtime-specific tooling; this document does not claim the full repository port is complete.\n";

pub(crate) fn render(source: &str) -> Result<String> {
    let mut result = source.to_owned();
    for &(from, to, expected_count) in ADAPTATIONS {
        ensure!(
            result.matches(from).count() == expected_count,
            "contributor adaptation source/count changed: {from}"
        );
        result = result.replace(from, to);
    }
    result.push_str(SCOPE);
    Ok(result)
}

pub(crate) fn check_links(repo: &Path, document: &str) -> Result<()> {
    let links = regex::Regex::new(r"\]\(([^)]+)\)")?;
    for capture in links.captures_iter(document) {
        let target = &capture[1];
        if target.starts_with("https://") {
            continue;
        }
        let (path, fragment) = target
            .split_once('#')
            .map_or((target, None), |(path, fragment)| (path, Some(fragment)));
        ensure!(
            !path.is_empty()
                && Path::new(path)
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
            "unsafe contributor link: {target}"
        );
        ensure!(
            repo.join(path).exists(),
            "missing contributor link: {target}"
        );
        if let Some(fragment) = fragment {
            // The generated guide has exactly one fragment target. Keep its
            // contract explicit instead of pretending to parse arbitrary Markdown.
            ensure!(
                path == "README.md" && fragment == "validate",
                "unsupported contributor anchor: {target}"
            );
            ensure!(
                fs::read_to_string(repo.join(path))?
                    .lines()
                    .any(|line| line == "## Validate"),
                "missing contributor anchor: {target}"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_pinned_guide_reconstructs_and_every_local_link_resolves() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let source = String::from_utf8(
            crate::git_stdout_bytes(
                repo,
                [
                    "show",
                    "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2:CONTRIBUTING.md",
                ],
            )
            .unwrap(),
        )
        .unwrap();
        let rendered = render(&source).unwrap();
        assert_eq!(rendered, include_str!("../../CONTRIBUTING.md"));
        check_links(repo, &rendered).unwrap();
        assert!(render(&source.replace("Bun 1.3.14+", "Bun changed")).is_err());
        assert!(render(&format!("{source}\n- Bun 1.3.14+\n- Node.js 22+ for npm package verification and release tasks\n")).is_err());
    }

    #[test]
    fn missing_unsafe_and_changed_anchor_links_fail() {
        let repo = tempfile::tempdir().unwrap();
        assert!(check_links(repo.path(), "[missing](guide.md)").is_err());
        assert!(check_links(repo.path(), "[unsafe](../guide.md)").is_err());
        fs::write(repo.path().join("README.md"), "# README\n").unwrap();
        assert!(check_links(repo.path(), "[validate](README.md#validate)").is_err());
        fs::write(repo.path().join("README.md"), "# README\n\n## Validate\n").unwrap();
        check_links(repo.path(), "[validate](README.md#validate)").unwrap();
        assert!(check_links(repo.path(), "[unknown](README.md#unknown)").is_err());
    }
}
