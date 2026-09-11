//! Verify migrated documentation pages against their pinned Hunk sources.
//!
//! The native site is written in Markdown consumed by Zola.  This checker keeps
//! the migration honest without copying the upstream Astro tree: exact source
//! hashes are checked through Git, every heading and fenced command surface is
//! required in the native page, and the native page must retain the same code
//! fence structure.

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const MIGRATION_DOC: &str = "port/hunk/website-doc-migrations.md";

#[derive(Debug, Clone, Copy)]
struct DocSpec {
    source: &'static str,
    native: &'static str,
    bytes: usize,
    sha256: &'static str,
}

const DOCS: &[DocSpec] = &[
    DocSpec {
        source: "website/src/content/docs/docs/start/keyboard-and-mouse.md",
        native: "site/content/docs/start/keyboard-and-mouse.md",
        bytes: 4103,
        sha256: "db8a47a8941390ed6ddbbcad171143bb48fb942cb9b82979a8532fb846972733",
    },
    DocSpec {
        source: "website/src/content/docs/docs/configure/keybindings.md",
        native: "site/content/docs/configure/keybindings.md",
        bytes: 2621,
        sha256: "a82a1598dbcd696bd66a461e5d91b7e973ca8ccd275def688a87a184d45d9faa",
    },
    DocSpec {
        source: "website/src/content/docs/docs/configure/configuration.md",
        native: "site/content/docs/configure/configuration.md",
        bytes: 2414,
        sha256: "fd76442b0885efdb6d2788c588934cc3d19c09b85678e031840e9c236e60b2e7",
    },
    DocSpec {
        source: "website/src/content/docs/docs/configure/layout-and-display.md",
        native: "site/content/docs/configure/layout-and-display.md",
        bytes: 2066,
        sha256: "ff9c4bd6fefff4c12800c450dcffd5b705374ba368a27ff2502261d393407a35",
    },
    DocSpec {
        source: "website/src/content/docs/docs/configure/themes.md",
        native: "site/content/docs/configure/themes.md",
        bytes: 2109,
        sha256: "dd0b864edc1e60f42f0a3cdd195f73ba3b12a3b1d9174860411b3126323eeb03",
    },
    DocSpec {
        source: "website/src/content/docs/docs/workflows/files-and-patches.md",
        native: "site/content/docs/workflows/files-and-patches.md",
        bytes: 1107,
        sha256: "f952bbde44fb4168e06e028132c9928f85f6f3a3d544166a2336a69c41f98673",
    },
    DocSpec {
        source: "website/src/content/docs/docs/workflows/watch-mode.md",
        native: "site/content/docs/workflows/watch-mode.md",
        bytes: 1040,
        sha256: "5d86d16f2127291806cdd282980ec5adf768fdfac90c6dd44ab5427dea94286c",
    },
    DocSpec {
        source: "website/src/content/docs/docs/workflows/jujutsu-and-sapling.md",
        native: "site/content/docs/workflows/jujutsu-and-sapling.md",
        bytes: 951,
        sha256: "b3d222ece4c744aec2b05afd5273f9484dba140e294781f18e8fb65225b57d19",
    },
    DocSpec {
        source: "website/src/content/docs/docs/workflows/working-trees-and-commits.md",
        native: "site/content/docs/workflows/working-trees-and-commits.md",
        bytes: 1233,
        sha256: "bc4d4a18be4efb0db63444146316305678ea7d723800e6a0347615408b8aa41f",
    },
    DocSpec {
        source: "website/src/content/docs/docs/help/compatibility.md",
        native: "site/content/docs/help/compatibility.md",
        bytes: 1924,
        sha256: "cee541156078f3077a23681367fbe9da94fe032f70131851396898fea86906f3",
    },
    DocSpec {
        source: "website/src/content/docs/docs/help/troubleshooting.md",
        native: "site/content/docs/help/troubleshooting.md",
        bytes: 2825,
        sha256: "b3d5c241aba0d842dee4b523cafe9bfa2030b4b7ff3e21154879820c5d0682f3",
    },
    DocSpec {
        source: "website/src/content/docs/docs/agents/review-with-an-agent.md",
        native: "site/content/docs/agents/review-with-an-agent.md",
        bytes: 2509,
        sha256: "2d72d0408249f0c415ed78ecc89c8cc20fa4f85c6b1f9d13ac3aae89276888b2",
    },
    DocSpec {
        source: "website/src/content/docs/docs/agents/live-session-control.md",
        native: "site/content/docs/agents/live-session-control.md",
        bytes: 1763,
        sha256: "cdd03f34d096a3397bf8f4b3166abbd5bc476e993bc2e19bf44d516a0063a795",
    },
    DocSpec {
        source: "website/src/content/docs/docs/agents/comments-and-annotations.md",
        native: "site/content/docs/agents/comments-and-annotations.md",
        bytes: 1482,
        sha256: "73133342e10d109808a9a6ee762a1e893c7f511eeae1aadbc125c81a610f2049",
    },
    DocSpec {
        source: "website/src/content/docs/docs/agents/agent-context-and-stml.md",
        native: "site/content/docs/agents/agent-context-and-stml.md",
        bytes: 1470,
        sha256: "7d523481403b25cfeb0f8b1681727882313fc9decae9cd6d260719420c4377d3",
    },
    DocSpec {
        source: "website/src/content/docs/docs/start/quick-start.md",
        native: "site/content/docs/start/quick-start.md",
        bytes: 1582,
        sha256: "8a20b04e041858ea0c844aeb8b98a406da1c8e40d79a90a548688c26beed2396",
    },
    DocSpec {
        source: "website/src/content/docs/docs/workflows/git-pager-and-difftool.md",
        native: "site/content/docs/workflows/git-pager-and-difftool.md",
        bytes: 1583,
        sha256: "a0626c5fb870463861ee6ce64ebd642be652f42a6168e675f259613f8f7cb45e",
    },
    DocSpec {
        source: "website/src/content/docs/docs/agents/review-skill.md",
        native: "site/content/docs/agents/review-skill.md",
        bytes: 1630,
        sha256: "713dfc1f5849ece579b128cb744eb724a6d7c7a995b9fbf9614c0019f6be7bfd",
    },
    DocSpec {
        source: "website/src/content/docs/docs/extend/extensions.md",
        native: "site/content/docs/extend/extensions.md",
        bytes: 11430,
        sha256: "32b9e9d8b3e92776117fa16449b90c4fd862797a0e12a16f07173ee4a0539335",
    },
    DocSpec {
        source: "website/src/content/docs/docs/start/install.md",
        native: "site/content/docs/start/install.md",
        bytes: 5438,
        sha256: "71946a9c9d0c476d251df50156f93fc0f8f7841c570ddbf9bd9bfcf6ebdf962e",
    },
    DocSpec {
        source: "website/src/content/docs/docs/index.mdx",
        native: "site/content/docs/_index.md",
        bytes: 2500,
        sha256: "f3562f2e4564b8799d9b5fee504e309ee65b74460bbc6608731ab83db263f249",
    },
    DocSpec {
        source: "website/src/content/docs/docs/extend/vcs-adapters.md",
        native: "site/content/docs/extend/vcs-adapters.md",
        bytes: 9887,
        sha256: "80f36ee8bee371cef3a51f9a80fa7e1be928f7c8f8618ddda8ec4be6491a1b8c",
    },
];

pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    if baseline != BASELINE {
        return Ok(());
    }
    let migration = std::fs::read_to_string(repo.join(MIGRATION_DOC))
        .with_context(|| format!("read {MIGRATION_DOC}"))?;
    for spec in DOCS {
        let source =
            crate::git_stdout_bytes(repo, ["show", &format!("{BASELINE}:{}", spec.source)])?;
        ensure!(
            source.len() == spec.bytes,
            "pinned {} changed size",
            spec.source
        );
        ensure!(
            format!("{:x}", Sha256::digest(&source)) == spec.sha256,
            "pinned {} hash changed",
            spec.source
        );
        let source = std::str::from_utf8(&source)?;
        let native = std::fs::read_to_string(repo.join(spec.native))
            .with_context(|| format!("read {}", spec.native))?;
        ensure!(
            repo.join(spec.native).is_file(),
            "native page is missing: {}",
            spec.native
        );
        ensure!(
            migration.contains(&format!("`{}`", spec.source)),
            "{} is missing a migration entry",
            spec.source
        );

        let source_headings = headings(source);
        let native_headings = headings(&native);
        ensure!(
            !source_headings.is_empty(),
            "{} has no headings",
            spec.source
        );
        for heading in source_headings {
            let normalized = normalize(heading);
            ensure!(
                native_headings.iter().any(|candidate| {
                    let candidate = normalize(candidate);
                    candidate == normalized
                        || candidate.contains(&normalized)
                        || normalized.contains(&candidate)
                }),
                "{} heading is missing from {}: {}",
                spec.source,
                spec.native,
                heading
            );
        }
        ensure!(
            source.matches("```").count() == native.matches("```").count(),
            "{} changed its fenced-code structure",
            spec.native
        );
        for command in commands(source) {
            ensure!(
                normalize(&native).contains(&normalize(&command)),
                "{} command surface is missing from {}: {}",
                spec.source,
                spec.native,
                command
            );
        }
    }
    Ok(())
}

fn headings(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix('#')?;
            let rest = rest.trim_start_matches('#').trim();
            (!rest.is_empty()).then_some(rest)
        })
        .collect()
}

fn commands(text: &str) -> Vec<String> {
    let mut in_fence = false;
    let mut commands = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            continue;
        }
        let command = trimmed.trim_start_matches('$').trim();
        let command = command.split('#').next().unwrap_or_default().trim();
        let mut words = command.split_whitespace();
        let Some(program) = words.next() else {
            continue;
        };
        if !matches!(program, "hunk" | "workdeck" | "git" | "jj" | "sl" | "cargo") {
            continue;
        }
        let operation = words.next().unwrap_or_default();
        if operation.is_empty() {
            continue;
        }
        commands.push(format!("{program} {operation}"));
    }
    commands.sort();
    commands.dedup();
    commands
}

fn normalize(value: &str) -> String {
    let value = value
        .to_ascii_lowercase()
        .replace("hunkdiff", "workdeck")
        .replace("hunk", "workdeck");
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '/' | '.' | ':')
            {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_document_migrations_cover_headings_and_command_surfaces() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        verify(repo, BASELINE).unwrap();
    }

    #[test]
    fn command_extraction_normalizes_hunk_branding() {
        assert_eq!(commands("```sh\nhunk diff --watch\n```"), vec!["hunk diff"]);
        assert!(normalize("Hunk's review stream").contains("workdeck"));
    }
}
