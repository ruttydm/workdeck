//! Static output validation for the native Zola website.
//!
//! This is the Rust translation of Hunk's build-time link checker.  It scans
//! generated HTML without a browser or network, validates internal routes and
//! anchors, and keeps the canonical/social metadata contract attached to the
//! output rather than to a hand-maintained route list.

use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use url::Url;

const CANONICAL_ORIGIN: &str = "https://workdeck.dev";
const EDIT_PREFIX: &str = "https://github.com/ruttydm/workdeck/edit/main/";
const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";

/// Verify the native no-JavaScript replacement for Hunk's docs header
/// component. The component itself only composes the brand header, route-aware
/// current state, and Starlight's search slot; the native template owns those
/// same controls and exposes a plain GET form for static deployments.
pub(crate) fn verify_docs_header(repo: &Path) -> Result<()> {
    let source = crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("{BASELINE}:website/src/components/docs/DocsHeader.astro"),
        ],
    )?;
    ensure!(
        source.len() == 638,
        "pinned DocsHeader.astro changed size: {} != 638",
        source.len()
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "import Search from \"@astrojs/starlight/components/Search.astro\"",
        "import BrandHeader from \"../BrandHeader.astro\"",
        "Astro.url.pathname.startsWith(\"/changelog\")",
        "const current =",
        "<BrandHeader current={current} context=\"docs\">",
        "slot=\"tools\" class=\"docs-header-tools print:hidden\"",
        "<Search />",
        "display: flex",
        "min-width: 0",
        "gap: 14px",
    ] {
        ensure!(
            source.contains(marker),
            "pinned DocsHeader.astro lost marker {marker:?}"
        );
    }

    let base = std::fs::read_to_string(repo.join("site/templates/base.html"))?;
    for marker in [
        "class=\"brand-tools\"",
        "class=\"docs-header-tools print:hidden\"",
        "action=\"https://github.com/ruttydm/workdeck/search\"",
        "method=\"get\" role=\"search\"",
        "type=\"search\" name=\"q\"",
        "current_path is starting_with(pat=\"/docs/\")",
        "current_path is starting_with(pat=\"/changelog\")",
    ] {
        ensure!(
            base.contains(marker),
            "native docs header is missing {marker:?}"
        );
    }
    let css = std::fs::read_to_string(repo.join("site/static/main.css"))?;
    for marker in [
        ".docs-header-tools",
        "align-items: center",
        "min-width: 0",
        "gap: 14px",
        "input[type=\"search\"]",
    ] {
        ensure!(
            css.contains(marker),
            "native docs header styles are missing {marker:?}"
        );
    }
    let migration = std::fs::read_to_string(repo.join("docs/docs-header-migration.md"))?;
    for marker in [
        "DocsHeader.astro",
        "BrandHeader",
        "Starlight Search",
        "plain GET form",
        "no application JavaScript",
    ] {
        ensure!(
            migration.contains(marker),
            "docs-header migration is missing {marker:?}"
        );
    }
    Ok(())
}

/// Verify the native Rust/Zola replacement for Hunk's website workflow. The
/// source workflow is inspected from Git and every build, route, export,
/// preview, and browser-check responsibility is routed to deterministic Rust
/// tooling; Bun/Node/Playwright execution is not retained.
pub(crate) fn verify_website_workflow(repo: &Path) -> Result<()> {
    let source = crate::git_stdout_bytes(
        repo,
        ["show", &format!("{BASELINE}:.github/workflows/website.yml")],
    )?;
    ensure!(
        source.len() == 1_396,
        "pinned website workflow changed size: {} != 1396",
        source.len()
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "name: Website",
        "SKIP_INSTALL_SIMPLE_GIT_HOOKS",
        "pull_request:",
        "push:",
        "branches:",
        "website-${{ github.workflow }}-${{ github.ref }}",
        "jobs:",
        "build:",
        "Check and build website",
        "Set up Bun",
        "bun install --frozen-lockfile",
        "bun install --cwd website --frozen-lockfile",
        "scripts/generate-docs.test.ts",
        "scripts/check-website-links.test.ts",
        "scripts/check-extension-catalog.test.ts",
        "bun run check:docs",
        "bun run website:check",
        "bun run website:build",
        "bun run website:links",
        "playwright install --with-deps chromium",
        "bun run website:test:browser",
    ] {
        ensure!(
            source.contains(marker),
            "pinned website workflow lost marker {marker:?}"
        );
    }
    let native = std::fs::read_to_string(repo.join(".github/workflows/website.yml"))?;
    for marker in [
        "name: Website",
        "SKIP_INSTALL_SIMPLE_GIT_HOOKS",
        "pull_request:",
        "push:",
        "website-${{ github.workflow }}-${{ github.ref }}",
        "Check and build native website",
        "uses: actions/checkout@v5",
        "uses: dtolnay/rust-toolchain@stable",
        "tool: zola@0.23.4",
        "cargo xtask site check",
        "cargo xtask site build",
        "cargo xtask site preview-check",
        "cargo test --locked -p xtask site_links site_markdown site_preview",
    ] {
        ensure!(
            native.contains(marker),
            "native website workflow is missing {marker:?}"
        );
    }
    for forbidden in ["bun", "node", "npm", "opentui", "wasm", "playwright"] {
        let pattern = regex::Regex::new(&format!(
            r"(?i)(?:^|[^a-z]){}(?:$|[^a-z])",
            regex::escape(forbidden)
        ))
        .expect("forbidden runtime token pattern is valid");
        ensure!(
            !pattern.is_match(&native),
            "native website workflow retains forbidden runtime token {forbidden:?}"
        );
    }
    let migration = std::fs::read_to_string(repo.join("docs/website-workflow-migration.md"))?;
    for marker in [
        ".github/workflows/website.yml",
        "Zola",
        "static links",
        "preview isolation",
        "browser",
        "not retained",
    ] {
        ensure!(
            migration.contains(marker),
            "website workflow migration is missing {marker:?}"
        );
    }
    Ok(())
}

/// Public files every native site build must ship.
pub(crate) const REQUIRED_ASSETS: &[&str] = &[
    "favicon.svg",
    "brand.css",
    "main.css",
    "fonts/jetbrains-mono.css",
    "docs/workdeck-review-skill.md",
    "docs/images/review-stream-native.png",
    "og.svg",
    "extensions/og.svg",
    "shots/shot-catppuccin-mocha.webp",
    "shots/shot-github-dark.webp",
    "shots/shot-github-light.webp",
    "shots/shot-gruvbox.webp",
    "shots/shot-nord.webp",
    "shots/shot-tokyo-night.webp",
    "videos/video-devops-toolbox.webp",
    "videos/video-jilles.webp",
    "features/feature-agent.mp4",
    "features/feature-agent.webm",
    "features/feature-layout.mp4",
    "features/feature-layout.webm",
    "features/feature-mouse.mp4",
    "features/feature-mouse.webm",
    "features/feature-stream.webp",
    "features/feature-themes.mp4",
    "features/feature-themes.webm",
    "robots.txt",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CheckSummary {
    pub pages: usize,
    pub canonical_pages: usize,
}

/// Validate one generated Zola output directory.
pub(crate) fn check(dist: &Path, repo: &Path) -> Result<CheckSummary> {
    ensure!(
        dist.is_dir(),
        "site output directory is missing: {}",
        dist.display()
    );
    let html_paths = collect_files(dist, "html")?;
    let html_by_path = html_paths
        .iter()
        .map(|path| {
            fs::read_to_string(path)
                .map(|html| (path.clone(), html))
                .with_context(|| format!("read generated page {}", path.display()))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let ids_by_path = html_by_path
        .iter()
        .map(|(path, html)| (path.clone(), collect_attributes(html, "id")))
        .collect::<BTreeMap<_, _>>();
    let mut errors = Vec::new();
    let mut canonical_urls = BTreeSet::new();

    for (html_path, html) in &html_by_path {
        let label = html_path
            .strip_prefix(dist)
            .with_context(|| format!("page escaped output directory: {}", html_path.display()))?
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/")
            .trim_start_matches('/')
            .to_owned();
        if !has_nonempty_title(html) {
            errors.push(format!("{label}: missing title"));
        }
        if !has_nonempty_meta(html, "description") {
            errors.push(format!("{label}: missing meta description"));
        }

        let canonical = attribute_value(html, "link", "rel", "canonical", "href");
        if label != "404.html" {
            let expected = canonical_url_for_output(&label);
            if canonical.as_deref() != Some(expected.as_str()) {
                errors.push(format!(
                    "{label}: expected canonical {expected}, found {}",
                    canonical.as_deref().unwrap_or("none")
                ));
            } else if let Some(value) = canonical.as_deref() {
                canonical_urls.insert(value.trim_end_matches('/').to_owned());
            }
            for required in required_head_tags(&label) {
                if !html.contains(&required) {
                    errors.push(format!("{label}: missing head metadata: {required}"));
                }
            }
            if html.contains("<script") && !contains_only_json_ld_scripts(html) {
                errors.push(format!("{label}: application JavaScript is not allowed"));
            }
        }

        let references = collect_attributes(html, "href")
            .into_iter()
            .map(|value| ("href", value))
            .chain(
                collect_attributes(html, "src")
                    .into_iter()
                    .map(|value| ("src", value)),
            )
            .collect::<Vec<_>>();
        for (attribute, value) in references {
            if value.is_empty()
                || value.starts_with("data:")
                || value.starts_with("mailto:")
                || value.starts_with("tel:")
                || value.starts_with("javascript:")
            {
                continue;
            }
            if let Some(source) = value.strip_prefix(EDIT_PREFIX) {
                let source_path = repo.join(percent_decode(source));
                if !source_path.starts_with(repo) || !source_path.is_file() {
                    errors.push(format!("{label}: edit link target does not exist: {value}"));
                }
                continue;
            }
            if value.starts_with("http://")
                || value.starts_with("https://")
                || value.starts_with("//")
            {
                continue;
            }

            let page_url = canonical
                .clone()
                .unwrap_or_else(|| format!("{CANONICAL_ORIGIN}/{label}"));
            let parsed = match Url::parse(&page_url).and_then(|base| base.join(&value)) {
                Ok(parsed) => parsed,
                Err(error) => {
                    errors.push(format!(
                        "{label}: invalid {attribute} target {value}: {error}"
                    ));
                    continue;
                }
            };
            if parsed.path().starts_with("/_vercel/") {
                continue;
            }
            let target_path = match output_path_for_url(dist, parsed.path()) {
                Ok(path) => path,
                Err(error) => {
                    errors.push(format!(
                        "{label}: invalid {attribute} target {value}: {error}"
                    ));
                    continue;
                }
            };
            if !target_path.is_file() {
                errors.push(format!("{label}: missing {attribute} target: {value}"));
                continue;
            }
            if !parsed.fragment().unwrap_or_default().is_empty()
                && target_path
                    .extension()
                    .is_some_and(|extension| extension == "html")
            {
                let target_ids = ids_by_path.get(&target_path).cloned().unwrap_or_else(|| {
                    fs::read_to_string(&target_path)
                        .ok()
                        .map(|html| collect_attributes(&html, "id"))
                        .unwrap_or_default()
                });
                let anchor = percent_decode(parsed.fragment().unwrap_or_default());
                if !target_ids.contains(&anchor) {
                    errors.push(format!("{label}: missing anchor #{anchor} in {value}"));
                }
            }
        }
    }

    for asset in REQUIRED_ASSETS {
        if !dist.join(asset).is_file() {
            errors.push(format!("missing required public asset: {asset}"));
        }
    }

    let sitemap = ["sitemap.xml", "sitemap-0.xml"]
        .iter()
        .map(|name| dist.join(name))
        .find(|path| path.is_file());
    let Some(sitemap) = sitemap else {
        errors.push("missing sitemap.xml".to_owned());
        return finish(errors, html_paths.len(), canonical_urls.len());
    };
    let sitemap = fs::read_to_string(&sitemap).context("read generated sitemap")?;
    for canonical in &canonical_urls {
        ensure!(
            sitemap.contains(&format!("<loc>{canonical}</loc>"))
                || sitemap.contains(&format!("<loc>{canonical}/</loc>")),
            "sitemap omits canonical URL: {canonical}"
        );
    }

    finish(errors, html_paths.len(), canonical_urls.len())
}

fn finish(errors: Vec<String>, pages: usize, canonical_pages: usize) -> Result<CheckSummary> {
    if errors.is_empty() {
        Ok(CheckSummary {
            pages,
            canonical_pages,
        })
    } else {
        bail!(
            "Website link check failed:\n{}",
            errors
                .iter()
                .map(|error| format!("- {error}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

fn collect_files(directory: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)
        .with_context(|| format!("read website output {}", directory.display()))?
    {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            files.extend(collect_files(&path, extension)?);
        } else if kind.is_file()
            && path
                .extension()
                .is_some_and(|value| value == extension.trim_start_matches('.'))
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn collect_attributes(html: &str, attribute: &str) -> BTreeSet<String> {
    let pattern = Regex::new(&format!(r#"(?i)\s{attribute}\s*=\s*[\"']([^\"']+)[\"']"#))
        .expect("attribute pattern is valid");
    pattern
        .captures_iter(html)
        .filter_map(|capture| capture.get(1).map(|value| decode_attribute(value.as_str())))
        .collect()
}

fn attribute_value(
    html: &str,
    element: &str,
    key: &str,
    expected: &str,
    value: &str,
) -> Option<String> {
    let pattern = Regex::new(&format!(
        r#"(?is)<{element}\b[^>]*\b{key}\s*=\s*[\"']{expected}[\"'][^>]*\b{value}\s*=\s*[\"']([^\"']+)[\"'][^>]*>"#
    ))
    .expect("element attribute pattern is valid");
    pattern
        .captures_iter(html)
        .find_map(|capture| capture.get(1).map(|value| decode_attribute(value.as_str())))
}

fn has_nonempty_title(html: &str) -> bool {
    Regex::new(r"(?is)<title>\s*[^<\s][^<]*</title>")
        .expect("title pattern is valid")
        .is_match(html)
}

fn has_nonempty_meta(html: &str, name: &str) -> bool {
    Regex::new(&format!(
        r#"(?is)<meta\b[^>]*\bname\s*=\s*[\"']{name}[\"'][^>]*\bcontent\s*=\s*[\"']([^\"']+)[\"']"#
    ))
    .expect("meta pattern is valid")
    .captures_iter(html)
    .any(|capture| {
        capture
            .get(1)
            .is_some_and(|value| !value.as_str().trim().is_empty())
    })
}

fn required_head_tags(label: &str) -> Vec<String> {
    let image = if label == "index.html" {
        format!("{CANONICAL_ORIGIN}/og.svg")
    } else if label == "extensions/index.html" {
        format!("{CANONICAL_ORIGIN}/extensions/og.svg")
    } else if label.starts_with("docs/") {
        format!("{CANONICAL_ORIGIN}/docs/images/review-stream-native.png")
    } else {
        return vec![
            r#"<link rel="icon" href="/favicon.svg""#.to_owned(),
            r#"<meta property="og:type" content="website""#.to_owned(),
            r#"<meta name="twitter:card" content="summary_large_image""#.to_owned(),
        ];
    };
    vec![
        r#"<link rel="icon" href="/favicon.svg""#.to_owned(),
        r#"<meta property="og:type" content="website""#.to_owned(),
        format!(r#"<meta property="og:image" content="{image}""#),
        r#"<meta name="twitter:card" content="summary_large_image""#.to_owned(),
        format!(r#"<meta name="twitter:image" content="{image}""#),
    ]
}

fn contains_only_json_ld_scripts(html: &str) -> bool {
    let script = Regex::new(r"(?is)<script\b([^>]*)>").expect("script pattern is valid");
    script.captures_iter(html).all(|capture| {
        capture
            .get(1)
            .is_some_and(|attributes| attributes.as_str().contains("application/ld+json"))
    })
}

fn canonical_url_for_output(label: &str) -> String {
    if label == "index.html" {
        format!("{CANONICAL_ORIGIN}/")
    } else {
        format!("{CANONICAL_ORIGIN}/{}", label.replace("index.html", ""))
    }
}

fn output_path_for_url(dist: &Path, pathname: &str) -> Result<PathBuf> {
    let decoded = percent_decode(pathname).trim_start_matches('/').to_owned();
    let route = decoded.trim_end_matches('/');
    ensure!(
        route.is_empty()
            || !route
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == ".."),
        "path escapes the static output"
    );
    let direct = dist.join(route);
    let target = if route.is_empty() || decoded.ends_with('/') {
        direct.join("index.html")
    } else if direct.is_file() {
        direct
    } else {
        direct.join("index.html")
    };
    ensure!(target.starts_with(dist), "path escapes the static output");
    Ok(target)
}

fn decode_attribute(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x2F;", "/")
        .replace("&#47;", "/")
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2]))
        {
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn native_docs_header_replaces_the_complete_starlight_composition() {
        let repo = super::super::repo_root().unwrap();
        super::verify_docs_header(&repo).unwrap();
    }

    #[test]
    fn native_website_workflow_replaces_the_complete_bun_site_pipeline() {
        let repo = super::super::repo_root().unwrap();
        super::verify_website_workflow(&repo).unwrap();
    }

    fn write_fixture() -> tempfile::TempDir {
        let dist = tempfile::tempdir().unwrap();
        for asset in REQUIRED_ASSETS {
            let path = dist.path().join(asset);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "asset").unwrap();
        }
        fs::create_dir_all(dist.path().join("docs/guide")).unwrap();
        fs::create_dir_all(dist.path().join("extensions")).unwrap();
        let marketing = |image: &str| {
            format!(
                r#"<link rel="icon" href="/favicon.svg"><meta property="og:type" content="website"><meta property="og:image" content="{image}"><meta name="twitter:card" content="summary_large_image"><meta name="twitter:image" content="{image}"/>"#
            )
        };
        let docs = marketing("https://workdeck.dev/docs/images/review-stream-native.png");
        fs::write(
            dist.path().join("index.html"),
            format!(
                r#"<html><head><title>Workdeck</title><meta name="description" content="Workdeck"><link rel="canonical" href="https://workdeck.dev/">{}</head><body><main id="install"><a href="/docs/">Docs</a></main></body></html>"#,
                marketing("https://workdeck.dev/og.svg")
            ),
        )
        .unwrap();
        fs::write(
            dist.path().join("extensions/index.html"),
            format!(
                r#"<html><head><title>Extensions</title><meta name="description" content="Extensions"><link rel="canonical" href="https://workdeck.dev/extensions/">{}</head><body><a href="/docs/">Docs</a></body></html>"#,
                marketing("https://workdeck.dev/extensions/og.svg")
            ),
        )
        .unwrap();
        fs::write(
            dist.path().join("docs/index.html"),
            format!(
                r#"<html><head><title>Docs</title><meta name="description" content="Docs"><link rel="canonical" href="https://workdeck.dev/docs/">{}</head><body><a href="/docs/guide/#step">Guide</a><a href="/">Home</a></body></html>"#,
                docs
            ),
        )
        .unwrap();
        fs::write(
            dist.path().join("docs/guide/index.html"),
            format!(
                r#"<html><head><title>Guide</title><meta name="description" content="Guide"><link rel="canonical" href="https://workdeck.dev/docs/guide/">{}</head><body><h1 id="step">Step</h1><a href="/docs/">Docs</a></body></html>"#,
                docs
            ),
        )
        .unwrap();
        fs::write(
            dist.path().join("sitemap.xml"),
            "<urlset><url><loc>https://workdeck.dev</loc></url><url><loc>https://workdeck.dev/extensions</loc></url><url><loc>https://workdeck.dev/docs</loc></url><url><loc>https://workdeck.dev/docs/guide</loc></url></urlset>",
        )
        .unwrap();
        dist
    }

    #[test]
    fn accepts_routes_anchors_metadata_assets_and_sitemap() {
        let dist = write_fixture();
        let summary = check(dist.path(), dist.path()).unwrap();
        assert_eq!(
            summary,
            CheckSummary {
                pages: 4,
                canonical_pages: 4
            }
        );
    }

    #[test]
    fn reports_missing_internal_anchor_without_network_access() {
        let dist = write_fixture();
        let path = dist.path().join("docs/index.html");
        let mut html = fs::read_to_string(&path).unwrap();
        html = html.replace("#step", "#missing");
        fs::write(path, html).unwrap();
        let error = check(dist.path(), dist.path()).unwrap_err().to_string();
        assert!(error.contains("missing anchor #missing"), "{error}");
    }

    #[test]
    fn requires_route_specific_extension_social_card() {
        let dist = write_fixture();
        let path = dist.path().join("extensions/index.html");
        let mut html = fs::read_to_string(&path).unwrap();
        html = html.replace(
            "https://workdeck.dev/extensions/og.svg",
            "https://workdeck.dev/og.svg",
        );
        fs::write(path, html).unwrap();
        let error = check(dist.path(), dist.path()).unwrap_err().to_string();
        assert!(
            error.contains("extensions/index.html: missing head metadata"),
            "{error}"
        );
    }

    #[test]
    fn rejects_canonical_route_and_social_metadata_drift() {
        let dist = write_fixture();
        let path = dist.path().join("docs/guide/index.html");
        let mut html = fs::read_to_string(&path).unwrap();
        html = html
            .replace(
                "https://workdeck.dev/docs/guide/",
                "https://workdeck.dev/docs/wrong/",
            )
            .replace(
                r#"<meta name="twitter:card" content="summary_large_image">"#,
                "",
            );
        fs::write(path, html).unwrap();
        let error = check(dist.path(), dist.path()).unwrap_err().to_string();
        assert!(error.contains("expected canonical"), "{error}");
        assert!(error.contains("missing head metadata"), "{error}");
    }

    #[test]
    fn rejects_non_json_application_scripts() {
        let dist = write_fixture();
        let path = dist.path().join("index.html");
        let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
        writeln!(file, "<script src=\"/app.js\"></script>").unwrap();
        let error = check(dist.path(), dist.path()).unwrap_err().to_string();
        assert!(
            error.contains("application JavaScript is not allowed"),
            "{error}"
        );
    }
}
