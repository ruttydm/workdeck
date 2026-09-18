//! Verify configuration-only Hunk artifacts have explicit native replacements.
//!
//! These checks deliberately read the pinned blobs through Git. They do not copy or execute
//! the JavaScript toolchain and fail if an upstream configuration changes shape unexpectedly.

use anyhow::{Context, Result, ensure};
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const MIGRATION_DOC: &str = "port/hunk/tooling-config-migrations.md";

// Native installer context allowlist: only release-controller inputs are copied
// into the disposable VM context; no Docker runtime is shipped by Workdeck.

const CONFIGS: &[(&str, usize, &[&str])] = &[
    ("CLAUDE.md", 9, &["docs/ARCHITECTURE.md", MIGRATION_DOC]),
    (".oxfmtrc.json", 27, &["rust-toolchain.toml", MIGRATION_DOC]),
    (".oxlintrc.json", 51, &["Cargo.toml", MIGRATION_DOC]),
    (
        ".env.test",
        56,
        &["xtask/src/main.rs", "docs/test-git-isolation.md"],
    ),
    (
        "website/tsconfig.json",
        42,
        &["site/config.toml", MIGRATION_DOC],
    ),
    (
        "website/astro.config.mjs",
        9317,
        &[
            "site/config.toml",
            "site/templates/base.html",
            "site/templates/docs.html",
            "site/static/starlight.css",
            "xtask/src/site_markdown.rs",
            "xtask/src/site_links.rs",
            "xtask/src/site_assets.rs",
            "xtask/src/site_preview.rs",
            "xtask/src/website_docs.rs",
            "xtask/src/changelog/website.rs",
            "xtask/src/skill.rs",
            MIGRATION_DOC,
        ],
    ),
    (
        "website/playwright.config.ts",
        793,
        &[
            "xtask/src/site_links.rs",
            "xtask/src/site_preview.rs",
            "xtask/src/main.rs",
            "site/content/docs/help/deployment.md",
            MIGRATION_DOC,
        ],
    ),
    (
        "vercel.json",
        714,
        &[
            "site/config.toml",
            "site/templates/base.html",
            ".github/workflows/ci.yml",
            "xtask/src/site_markdown.rs",
            MIGRATION_DOC,
        ],
    ),
    (
        "website/.gitignore",
        85,
        &[".gitignore", "xtask/src/site_preview.rs", MIGRATION_DOC],
    ),
    (
        "bunfig.toml",
        92,
        &["Cargo.lock", "rust-toolchain.toml", MIGRATION_DOC],
    ),
    (".gitignore", 562, &[".gitignore", MIGRATION_DOC]),
    (".lintstagedrc.json", 142, &["Cargo.toml", MIGRATION_DOC]),
    (
        "knip.json",
        1419,
        &["xtask/src/architecture.rs", MIGRATION_DOC],
    ),
    (
        "tsconfig.examples.json",
        219,
        &[
            "examples/Cargo.toml",
            "port/hunk/tooling-config-migrations.md",
        ],
    ),
    (
        "tsconfig.opentui.json",
        253,
        &["crates/workdeck-tui/Cargo.toml", MIGRATION_DOC],
    ),
    (
        "tsconfig.extension.json",
        269,
        &["crates/workdeck-extension-api/Cargo.toml", MIGRATION_DOC],
    ),
    (
        "tsconfig.json",
        1274,
        &["Cargo.toml", "Cargo.lock", MIGRATION_DOC],
    ),
    (
        "test/cli/install-vm/.dockerignore",
        101,
        &["xtask/src/tooling_configs.rs", MIGRATION_DOC],
    ),
];

pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    if baseline != BASELINE {
        return Ok(());
    }
    let migration = std::fs::read_to_string(repo.join(MIGRATION_DOC))?;
    for (path, expected_bytes, destinations) in CONFIGS {
        let bytes = crate::git_stdout_bytes(repo, ["show", &format!("{BASELINE}:{path}")])?;
        ensure!(
            bytes.len() == *expected_bytes,
            "pinned {path} changed size: {} != {expected_bytes}",
            bytes.len()
        );
        let source = std::str::from_utf8(&bytes)?;
        match *path {
            "CLAUDE.md" => ensure!(source == "AGENTS.md", "CLAUDE.md pointer changed"),
            ".oxfmtrc.json" => {
                let value: serde_json::Value = serde_json::from_str(source)?;
                ensure!(
                    value == serde_json::json!({"ignorePatterns": []}),
                    "Oxfmt configuration changed"
                );
            }
            ".oxlintrc.json" => {
                let value: serde_json::Value = serde_json::from_str(source)?;
                ensure!(
                    value["rules"]["no-control-regex"].as_str() == Some("off"),
                    "Oxlint control-regex rule changed"
                );
            }
            ".env.test" => ensure!(
                source == "GIT_CONFIG_GLOBAL=/dev/null\nGIT_CONFIG_SYSTEM=/dev/null\n",
                "test Git isolation configuration changed"
            ),
            "website/tsconfig.json" => ensure!(
                source == "{\n  \"extends\": \"astro/tsconfigs/strict\"\n}\n",
                "Astro TypeScript configuration changed"
            ),
            "website/astro.config.mjs" => {
                for marker in [
                    "site: \"https://hunk.dev\"",
                    "output: \"static\"",
                    "sitemap()",
                    "starlight({",
                    "starlightDotMd()",
                    "starlightLlmsTxt({",
                    "projectName: \"Hunk\"",
                    "customSelectors: { all: [\"a.sl-anchor-link\"] }",
                    "promote: [\"docs\", \"docs/start/**\"]",
                    "exclude: [\"docs/extend/**\", \"docs/reference/opentui-components\", \"changelog/**\"]",
                    "editLink:",
                    "lastUpdated: true",
                    "pagination: true",
                    "customCss: [\"./src/styles/starlight.css\"]",
                    "components:",
                    "label: \"Start here\"",
                    "label: \"Review workflows\"",
                    "label: \"Working with agents\"",
                    "label: \"Configure\"",
                    "label: \"Extend\"",
                    "label: \"Reference\"",
                    "label: \"Help\"",
                    "{ label: \"Changelog\", link: \"/changelog/\" }",
                    "smartypants: false",
                    "light: \"github-light-default\"",
                    "dark: \"github-dark-default\"",
                    "wrap: true",
                ] {
                    ensure!(
                        source.contains(marker),
                        "Astro site configuration lost {marker}"
                    );
                }
                let migration = std::fs::read_to_string(repo.join(MIGRATION_DOC))?;
                ensure!(
                    migration.contains("`website/astro.config.mjs`")
                        && migration.contains("Zola `site/config.toml`")
                        && migration.contains("native Markdown/LLM exports")
                        && migration.contains("starlight.css"),
                    "Astro site configuration migration is undocumented"
                );
                let native_config = std::fs::read_to_string(repo.join("site/config.toml"))?;
                ensure!(
                    native_config.contains("base_url = \"https://workdeck.dev\"")
                        && native_config.contains("generate_feeds = true"),
                    "native Zola site config is missing the Astro static-site replacement"
                );
                for native in [
                    "site/templates/base.html",
                    "site/templates/docs.html",
                    "site/static/starlight.css",
                    "xtask/src/site_markdown.rs",
                    "xtask/src/site_links.rs",
                    "xtask/src/site_assets.rs",
                    "xtask/src/site_preview.rs",
                    "xtask/src/website_docs.rs",
                    "xtask/src/changelog/website.rs",
                    "xtask/src/skill.rs",
                ] {
                    ensure!(
                        repo.join(native).is_file(),
                        "native site owner is missing: {native}"
                    );
                }
            }
            "website/playwright.config.ts" => {
                for marker in [
                    "testDir: \"./tests\"",
                    "outputDir: \"./test-results\"",
                    "fullyParallel: true",
                    "forbidOnly: Boolean(process.env.CI)",
                    "retries: process.env.CI ? 1 : 0",
                    "reporter: process.env.CI ? \"github\" : \"list\"",
                    "baseURL: \"http://127.0.0.1:4321\"",
                    "trace: \"retain-on-failure\"",
                    "command: \"bun run preview -- --host 127.0.0.1 --port 4321\"",
                    "url: \"http://127.0.0.1:4321/docs/\"",
                    "reuseExistingServer: !process.env.CI",
                    "timeout: 30_000",
                    "name: \"desktop-chromium\"",
                    "width: 1600",
                    "height: 900",
                    "name: \"mobile-chromium\"",
                    "devices[\"Pixel 5\"]",
                ] {
                    ensure!(
                        source.contains(marker),
                        "Playwright configuration lost {marker}"
                    );
                }
                let migration = std::fs::read_to_string(repo.join(MIGRATION_DOC))?;
                ensure!(
                    migration.contains("`website/playwright.config.ts`")
                        && migration.contains("site link/metadata")
                        && migration.contains("preview-check"),
                    "Playwright configuration migration is undocumented"
                );
                for native in [
                    "xtask/src/site_links.rs",
                    "xtask/src/site_preview.rs",
                    "xtask/src/main.rs",
                    "site/content/docs/help/deployment.md",
                ] {
                    ensure!(
                        repo.join(native).is_file(),
                        "native site test owner is missing: {native}"
                    );
                }
            }
            "vercel.json" => {
                let value: serde_json::Value = serde_json::from_str(source)?;
                ensure!(
                    value["framework"] == "astro"
                        && value["outputDirectory"] == "website/dist"
                        && value["buildCommand"] == "bun run website:build",
                    "Vercel framework/build policy changed"
                );
                ensure!(
                    value["rewrites"]
                        == serde_json::json!([{
                            "source": "/install",
                            "destination": "/install.sh"
                        }]),
                    "Vercel installer rewrite changed"
                );
                ensure!(
                    value["headers"]
                        == serde_json::json!([{
                            "source": "/install(\\.sh)?",
                            "headers": [{
                                "key": "Content-Type",
                                "value": "text/plain; charset=utf-8"
                            }]
                        }]),
                    "Vercel installer content-type header changed"
                );
                let site_config = std::fs::read_to_string(repo.join("site/config.toml"))?;
                ensure!(
                    site_config.contains("base_url = \"https://workdeck.dev\"")
                        && site_config.contains("compile_sass = false")
                        && site_config.contains("build_search_index = false")
                        && site_config.contains("generate_feeds = true"),
                    "native Zola site configuration is missing"
                );
                let template = std::fs::read_to_string(repo.join("site/templates/base.html"))?;
                ensure!(
                    template.contains("/docs/start/install/") && template.contains("/#install"),
                    "native installer navigation replacement is missing"
                );
                let ci = std::fs::read_to_string(repo.join(".github/workflows/ci.yml"))?;
                ensure!(
                    ci.contains("cargo xtask site check"),
                    "native site CI replacement is missing"
                );
                ensure!(
                    repo.join("xtask/src/site_markdown.rs").is_file(),
                    "native site export owner is missing"
                );
            }
            "website/.gitignore" => ensure!(
                source.lines().collect::<Vec<_>>()
                    == [
                        "node_modules/",
                        "dist/",
                        ".astro/",
                        ".pagefind/",
                        "test-results/",
                        "playwright-report/",
                        "blob-report/",
                    ],
                "website ignore policy changed"
            ),
            "bunfig.toml" => ensure!(
                source
                    == "[install]\n# Only install packages published at least 7 days ago.\nminimumReleaseAge = 604800\n",
                "Bun install policy changed"
            ),
            ".gitignore" => {
                for marker in [
                    "node_modules",
                    ".hunk/latest.json",
                    ".hunk/config.toml",
                    ".astro/",
                    ".pagefind/",
                ] {
                    ensure!(source.contains(marker), "Hunk ignore policy lost {marker}");
                }
                let native = std::fs::read_to_string(repo.join(".gitignore"))?;
                for marker in ["/target/", "/site/public/", "/artifacts/"] {
                    ensure!(
                        native.contains(marker),
                        "native ignore policy lost {marker}"
                    );
                }
            }
            ".lintstagedrc.json" => {
                let value: serde_json::Value = serde_json::from_str(source)?;
                ensure!(
                    value
                        == serde_json::json!({
                            "*.{ts,tsx,js,jsx,mjs,cjs,mts,cts}": [
                                "oxfmt --write",
                                "oxlint --fix --deny-warnings"
                            ],
                            "*.{json,jsonc,md,yml,yaml}": "oxfmt --write"
                        }),
                    "lint-staged policy changed"
                );
                ensure!(
                    repo.join("Cargo.toml").is_file(),
                    "native Cargo lint owner is missing"
                );
            }
            "knip.json" => {
                let value: serde_json::Value = serde_json::from_str(source)?;
                let workspaces = value["workspaces"]
                    .as_object()
                    .ok_or_else(|| anyhow::anyhow!("Knip workspaces are not an object"))?;
                ensure!(
                    workspaces.keys().collect::<Vec<_>>()
                        == [".", "packages/session-broker*", "packages/term-video"],
                    "Knip workspace set changed"
                );
                ensure!(
                    workspaces["."]["entry"]
                        .as_array()
                        .is_some_and(|entries| entries.len() == 11)
                        && workspaces["."]["project"]
                            .as_array()
                            .is_some_and(|projects| projects.len() == 6),
                    "Knip root entry/project scope changed"
                );
                ensure!(
                    workspaces["packages/session-broker*"]["entry"]
                        .as_array()
                        .is_some_and(|entries| entries.len() == 1)
                        && workspaces["packages/session-broker*"]["project"]
                            .as_array()
                            .is_some_and(|projects| projects.len() == 1),
                    "Knip session package scope changed"
                );
                ensure!(
                    workspaces["packages/term-video"]["entry"]
                        .as_array()
                        .is_some_and(|entries| entries.len() == 1)
                        && workspaces["packages/term-video"]["project"]
                            .as_array()
                            .is_some_and(|projects| projects.len() == 1),
                    "Knip terminal-video package scope changed"
                );
                ensure!(
                    value["ignoreIssues"]
                        == serde_json::json!({
                            "src/app/review/capability.ts": ["exports"],
                            "src/extension-api/types.ts": ["duplicates"]
                        }),
                    "Knip issue exceptions changed"
                );
                ensure!(
                    repo.join("xtask/src/architecture.rs").is_file(),
                    "native module-graph owner is missing"
                );
            }
            "test/cli/install-vm/.dockerignore" => {
                ensure!(
                    source
                        == "**\n!Dockerfile\n!controller.sh\n!pins.json\n!scenarios.json\n!controller-deps/**\n!guest/**\n!scenarios/**\n",
                    "install VM context allowlist changed"
                );
                let native = std::fs::read_to_string(repo.join("xtask/src/tooling_configs.rs"))?;
                ensure!(
                    native.contains("install VM context allowlist")
                        && native.contains("controller-deps/**")
                        && native.contains("scenarios/**"),
                    "native installer context allowlist is missing"
                );
            }
            "tsconfig.examples.json" => {
                let value: serde_json::Value = serde_json::from_str(source)?;
                ensure!(
                    value["extends"] == "./tsconfig.json",
                    "example config base changed"
                );
                ensure!(
                    value["include"] == serde_json::json!([]),
                    "example config include changed"
                );
                ensure!(
                    value["files"]
                        == serde_json::json!([
                            "examples/7-opentui-component/support.tsx",
                            "examples/7-opentui-component/from-files.tsx",
                            "examples/7-opentui-component/from-patch.tsx"
                        ]),
                    "example config file set changed"
                );
                ensure!(
                    repo.join("examples/Cargo.toml").is_file()
                        && repo.join("examples/7-ratatui-component").is_dir(),
                    "native Rust example workspace is missing"
                );
            }
            "tsconfig.opentui.json" => {
                let value: serde_json::Value = serde_json::from_str(source)?;
                ensure!(
                    value["extends"] == "./tsconfig.json",
                    "OpenTUI config base changed"
                );
                ensure!(
                    value["include"] == serde_json::json!([]),
                    "OpenTUI config include changed"
                );
                ensure!(
                    value["files"] == serde_json::json!(["src/opentui/index.ts"]),
                    "OpenTUI declaration entry changed"
                );
                ensure!(
                    value["compilerOptions"]["emitDeclarationOnly"] == true
                        && value["compilerOptions"]["declaration"] == true,
                    "OpenTUI declaration mode changed"
                );
                ensure!(
                    repo.join("crates/workdeck-tui/Cargo.toml").is_file(),
                    "native Ratatui replacement is missing"
                );
            }
            "tsconfig.extension.json" => {
                let value: serde_json::Value = serde_json::from_str(source)?;
                ensure!(
                    value["extends"] == "./tsconfig.json",
                    "extension config base changed"
                );
                ensure!(
                    value["include"] == serde_json::json!([]),
                    "extension config include changed"
                );
                ensure!(
                    value["files"] == serde_json::json!(["src/extension-api/index.ts"]),
                    "extension declaration entry changed"
                );
                ensure!(
                    value["compilerOptions"]["emitDeclarationOnly"] == true
                        && value["compilerOptions"]["declaration"] == true,
                    "extension declaration mode changed"
                );
                ensure!(
                    repo.join("crates/workdeck-extension-api/Cargo.toml")
                        .is_file(),
                    "native extension API replacement is missing"
                );
            }
            "tsconfig.json" => {
                let value: serde_json::Value = serde_json::from_str(source)?;
                ensure!(
                    value["compilerOptions"]["strict"] == true,
                    "TypeScript strict mode changed"
                );
                ensure!(
                    value["compilerOptions"]["noEmit"] == true,
                    "root TypeScript config must remain type-only"
                );
                let include = value["include"]
                    .as_array()
                    .context("TypeScript include list")?;
                ensure!(include.len() == 9, "TypeScript include scope changed");
                ensure!(
                    value["compilerOptions"]["jsxImportSource"] == "@opentui/react",
                    "pinned JSX runtime marker changed"
                );
                ensure!(
                    repo.join("Cargo.toml").is_file()
                        && repo.join("Cargo.lock").is_file()
                        && repo.join("crates/workdeck-tui/Cargo.toml").is_file(),
                    "native Cargo workspace replacement is missing"
                );
            }
            other => ensure!(false, "unknown configuration {other}"),
        }
        ensure!(
            migration.contains(&format!("| `{path}` |")),
            "{path} is missing a dedicated migration entry"
        );
        for destination in *destinations {
            ensure!(
                repo.join(destination).exists(),
                "native replacement for {path} is missing: {destination}"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_configuration_blobs_have_explicit_native_replacements() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        verify(repo, BASELINE).unwrap();
    }
}
