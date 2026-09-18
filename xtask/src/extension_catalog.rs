//! Partial MIT port of Hunk website/src/data/extensions.ts (Modem Labs Inc.).
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Map, Value};
use sha2::Digest;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;

mod loading;

const EXTENSION_CONSUMER_PATH: &str = "scripts/extension-consumer-check.ts";
const EXTENSION_CONSUMER_BYTES: usize = 5_898;
const EXTENSION_CONSUMER_LINES: usize = 168;
const EXTENSION_CONSUMER_SHA256: &str =
    "efc5cfe468cd1d23f7dffaaf08cd0208873cd1c71e27ea4f155f8e11e78c550d";
const EXTENSION_DOCS_PATH: &str = "scripts/extension-doc-examples.ts";
const EXTENSION_DOCS_BYTES: usize = 5_411;
const EXTENSION_DOCS_LINES: usize = 145;
const EXTENSION_DOCS_SHA256: &str =
    "dbf982d74b673ec46f1eab1903c5806fe092d2528ee780722aac245cad2c954c";
const EXTENSION_DOCS_TEST_PATH: &str = "scripts/extension-doc-examples.test.ts";
const EXTENSION_DOCS_TEST_BYTES: usize = 3_018;
const EXTENSION_DOCS_TEST_LINES: usize = 88;
const EXTENSION_DOCS_TEST_SHA256: &str =
    "3ef479ce20400bf89eb12ab647ea2215ee389987b7484156de4e50d19a6575c5";
const CHECK_PACK_PATH: &str = "scripts/check-pack.ts";
const CHECK_PACK_BYTES: usize = 17_383;
const CHECK_PACK_LINES: usize = 516;
const CHECK_PACK_SHA256: &str = "ee81e1ea3088a833fd1e9b0572f7596e512c7a7c4df095e6c19388105d766c93";

pub fn validate_legacy_catalog(catalog: &Value) -> Result<()> {
    anyhow::ensure!(
        catalog["baseline"] == "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
        "unexpected catalog baseline"
    );
    anyhow::ensure!(
        catalog["blob"] == "23f2196dd3c7a91306e050db352d4b5518a31117",
        "unexpected catalog source blob"
    );
    let entries = catalog["entries"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("catalog entries missing"))?;
    anyhow::ensure!(
        entries.len() == 16,
        "pinned catalog must preserve all 16 listings"
    );
    let mut repositories = std::collections::BTreeSet::new();
    let categorized: Vec<CategorizedListing> = serde_json::from_value(catalog["entries"].clone())?;
    anyhow::ensure!(
        catalog["facets"] == serde_json::to_value(category_facets(&categorized))?,
        "catalog facet counts or order differ from entries"
    );
    let version = regex::Regex::new(r"^\d+\.\d+\.\d+")?;
    for entry in entries {
        let repo = entry["repo"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("repository missing"))?;
        anyhow::ensure!(
            repositories.insert(repo.to_lowercase()),
            "duplicate catalog repository"
        );
        let parts: Vec<_> = repo.split('/').collect();
        anyhow::ensure!(
            parts.len() == 2
                && parts.iter().all(|part| !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))),
            "invalid catalog repository"
        );
        for field in ["name", "summary"] {
            anyhow::ensure!(
                entry[field]
                    .as_str()
                    .is_some_and(|value| !value.trim().is_empty()),
                "catalog {field} missing"
            );
        }
        anyhow::ensure!(
            entry["version"]
                .as_str()
                .is_some_and(|value| version.is_match(value)),
            "invalid recorded version"
        );
        anyhow::ensure!(
            entry["apiVersion"].as_u64().is_some_and(|value| value > 0),
            "recorded source API missing"
        );
        anyhow::ensure!(
            entry["compatibility"] == "requires-rust-rewrite",
            "legacy extension cannot be advertised as a native replacement"
        );
        let categories: Vec<Category> = serde_json::from_value(entry["categories"].clone())?;
        anyhow::ensure!(!categories.is_empty(), "catalog categories missing");
    }
    Ok(())
}

/// Verify the complete pinned website catalog through the native projection.
/// The TypeScript module is read as data from Git and its literal is decoded
/// by the restrictive parser below; it is never executed.
pub(crate) fn verify_pinned_source(repo: &Path, baseline: &str) -> Result<()> {
    if baseline != "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2" {
        return Ok(());
    }
    let catalog: Value = serde_json::from_slice(
        &std::fs::read(repo.join("site/data/legacy-extensions.json"))
            .context("read native legacy extension catalog")?,
    )?;
    let source_bytes = crate::git_stdout_bytes(
        repo,
        [
            "show",
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2:website/src/data/extensions.ts",
        ],
    )?;
    ensure!(
        source_bytes.len() == 13_852,
        "pinned extension catalog source changed size"
    );
    let source = String::from_utf8(source_bytes)?;
    verify_legacy_source(&catalog, &source)
}

struct SourcePair<'a> {
    path: &'a str,
    baseline_bytes: usize,
    baseline_lines: usize,
    baseline_sha: &'a str,
    stable_bytes: usize,
    stable_lines: usize,
    stable_sha: &'a str,
}

fn verify_source_pair(repo: &Path, spec: SourcePair<'_>) -> Result<()> {
    let SourcePair {
        path,
        baseline_bytes,
        baseline_lines,
        baseline_sha,
        stable_bytes,
        stable_lines,
        stable_sha,
    } = spec;
    for (pin, bytes_expected, lines_expected, sha_expected) in [
        (
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
            baseline_bytes,
            baseline_lines,
            baseline_sha,
        ),
        (
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd",
            stable_bytes,
            stable_lines,
            stable_sha,
        ),
    ] {
        let source = crate::git_stdout_bytes(repo, ["show", &format!("{pin}:{path}")])?;
        ensure!(
            source.len() == bytes_expected,
            "pinned {path} {pin} changed size: {} != {bytes_expected}",
            source.len()
        );
        ensure!(
            source.split(|byte| *byte == b'\n').count() == lines_expected + 1,
            "pinned {path} {pin} changed line count"
        );
        ensure!(
            format!("{:x}", sha2::Sha256::digest(&source)) == sha_expected,
            "pinned {path} {pin} changed SHA-256"
        );
    }
    Ok(())
}

/// Verify the source-level extension consumer, documentation-example, and
/// package-surface checks through the native Rust extension API and examples.
/// The original TypeScript helpers are inspected from protected Git refs only;
/// none is copied into or executed by Workdeck.
pub(crate) fn verify_extension_tooling(repo: &Path, baseline: &str) -> Result<()> {
    if baseline != "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2" {
        return Ok(());
    }
    verify_source_pair(
        repo,
        SourcePair {
            path: EXTENSION_CONSUMER_PATH,
            baseline_bytes: EXTENSION_CONSUMER_BYTES,
            baseline_lines: EXTENSION_CONSUMER_LINES,
            baseline_sha: EXTENSION_CONSUMER_SHA256,
            stable_bytes: EXTENSION_CONSUMER_BYTES,
            stable_lines: EXTENSION_CONSUMER_LINES,
            stable_sha: EXTENSION_CONSUMER_SHA256,
        },
    )?;
    verify_source_pair(
        repo,
        SourcePair {
            path: EXTENSION_DOCS_PATH,
            baseline_bytes: EXTENSION_DOCS_BYTES,
            baseline_lines: EXTENSION_DOCS_LINES,
            baseline_sha: EXTENSION_DOCS_SHA256,
            stable_bytes: EXTENSION_DOCS_BYTES,
            stable_lines: EXTENSION_DOCS_LINES,
            stable_sha: EXTENSION_DOCS_SHA256,
        },
    )?;
    verify_source_pair(
        repo,
        SourcePair {
            path: EXTENSION_DOCS_TEST_PATH,
            baseline_bytes: EXTENSION_DOCS_TEST_BYTES,
            baseline_lines: EXTENSION_DOCS_TEST_LINES,
            baseline_sha: EXTENSION_DOCS_TEST_SHA256,
            stable_bytes: EXTENSION_DOCS_TEST_BYTES,
            stable_lines: EXTENSION_DOCS_TEST_LINES,
            stable_sha: EXTENSION_DOCS_TEST_SHA256,
        },
    )?;
    verify_source_pair(
        repo,
        SourcePair {
            path: CHECK_PACK_PATH,
            baseline_bytes: CHECK_PACK_BYTES,
            baseline_lines: CHECK_PACK_LINES,
            baseline_sha: CHECK_PACK_SHA256,
            stable_bytes: 16_313,
            stable_lines: 485,
            stable_sha: "af8b7773487a84a79645347fa6449f45345a58f03c4364fee044daeb716e68fa",
        },
    )?;
    let examples = fs::read_to_string(repo.join("examples/Cargo.toml"))?;
    let binary_count = examples
        .lines()
        .filter(|line| line.trim_start().starts_with("name = \"workdeck-example-"))
        .count();
    ensure!(
        binary_count >= 13,
        "native extension examples unexpectedly shrank to {binary_count} binaries"
    );
    for (path, marker) in [
        (
            "crates/workdeck-extension-api/src/lib.rs",
            "pub struct ExtensionManifest",
        ),
        ("crates/workdeck-extension-host/src/lib.rs", "HostError"),
        ("xtask/src/main.rs", "prepare_extension_example"),
        ("docs/extensions.md", "Rust/Ratatui extension API"),
    ] {
        let contents = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read extension tooling native surface {path}"))?;
        ensure!(
            contents.contains(marker),
            "extension tooling native surface {path} is missing {marker:?}"
        );
    }
    Ok(())
}

/// Verify the complete public extension-directory page. Astro's interactive
/// catalog is projected into a static, no-JavaScript migration directory: all
/// listings, facets, provenance, and trust warnings remain in document order,
/// while TypeScript install/copy actions are intentionally replaced by an
/// explicit native-Rust rewrite boundary.
pub(crate) fn verify_extensions_page(repo: &Path) -> Result<()> {
    const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
    let source = crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("{BASELINE}:website/src/pages/extensions.astro"),
        ],
    )?;
    ensure!(
        source.len() == 13_408,
        "pinned extensions.astro changed size: {} != 13408",
        source.len()
    );
    let source = String::from_utf8(source)?;
    for marker in [
        "import BrandFooter from \"../components/BrandFooter.astro\"",
        "import BrandHeader from \"../components/BrandHeader.astro\"",
        "loadExtensionEntries",
        "categoryFacets",
        "formatUpdated",
        "installCommand",
        "toJsonLdScriptBody",
        "const title = \"hunk extensions — the community directory\"",
        "const description =",
        "const entries = await loadExtensionEntries();",
        "const facets = categoryFacets(entries);",
        "const PAGE_SIZE = 24;",
        "\"@type\": \"ItemList\"",
        "numberOfItems: entries.length",
        "data-categories=",
        "data-search=",
        "data-stars=",
        "data-pushed=",
        "data-created=",
        "placeholder=\"search name, owner, description\"",
        "id=\"x-sort\"",
        "data-category",
        "aria-live=\"polite\"",
        "No extensions match that search.",
        "function render()",
        "navigator.clipboard.writeText",
    ] {
        ensure!(
            source.contains(marker),
            "pinned extensions.astro lost marker {marker:?}"
        );
    }

    let catalog: Value = serde_json::from_slice(&std::fs::read(
        repo.join("site/data/legacy-extensions.json"),
    )?)?;
    validate_legacy_catalog(&catalog)?;
    let catalog_source = String::from_utf8(crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("{BASELINE}:website/src/data/extensions.ts"),
        ],
    )?)?;
    verify_legacy_source(&catalog, &catalog_source)?;
    let entries = catalog["entries"]
        .as_array()
        .context("legacy catalog entries missing")?;
    ensure!(
        entries.len() == 16,
        "native extension catalog count changed"
    );
    let html = std::fs::read_to_string(repo.join("site/templates/extensions.html"))?;
    for marker in [
        "Extension migration directory",
        "class=\"extension-browser\"",
        "aria-label=\"Legacy extension directory\"",
        "Filter by capability",
        "type=\"radio\"",
        "data-categories=",
        "legacy-extensions.json",
        "requiring a Rust rewrite",
        "Requires Rust rewrite",
        "Source repository:",
        "Recorded version:",
        "aria-label=\"Extension capabilities\"",
    ] {
        ensure!(
            html.contains(marker),
            "native extension directory is missing {marker:?}"
        );
    }
    ensure!(
        html.contains("{% for entry in catalog.entries %}")
            && html.matches("class=\"extension-card\"").count() == 1,
        "native extension directory does not loop over every catalog listing"
    );
    ensure!(
        html.matches("<strong>Requires Rust rewrite</strong>")
            .count()
            == 1,
        "native extension directory does not warn for every rendered listing"
    );
    ensure!(
        !html.contains("<script")
            && !html.contains("hunk.dev")
            && !html.contains("npmjs.com")
            && !html.contains("extension install"),
        "native extension directory reintroduced a legacy runtime or installer"
    );
    let migration = std::fs::read_to_string(repo.join("docs/extensions-page-migration.md"))?;
    for marker in [
        "extensions.astro",
        "16 listings",
        "facets",
        "no-JavaScript",
        "Rust rewrite",
        "full permissions",
    ] {
        ensure!(
            migration.contains(marker),
            "extension page migration is missing {marker:?}"
        );
    }
    Ok(())
}

// Decode only the pinned declarative catalog literal, never execute source.
// Reject syntax outside JSON strings, integers, punctuation and known keys.
pub fn verify_legacy_source(catalog: &Value, source: &str) -> Result<()> {
    validate_legacy_catalog(catalog)?;
    let expected = decode_legacy_source(source)?;
    let mut actual = catalog["entries"].clone();
    for entry in actual.as_array_mut().unwrap() {
        entry.as_object_mut().unwrap().remove("compatibility");
    }
    anyhow::ensure!(
        actual == expected,
        "migrated catalog differs from pinned source fields"
    );
    Ok(())
}

fn decode_legacy_source(source: &str) -> Result<Value> {
    let literal = source
        .split_once("export const EXTENSION_CATALOG: readonly ExtensionListing[] = [")
        .and_then(|(_, tail)| {
            tail.split_once("\n];")
                .map(|(body, _)| format!("[{body}\n]"))
        })
        .ok_or_else(|| anyhow::anyhow!("pinned catalog literal not found"))?;
    let token_pattern =
        regex::Regex::new(r#""(?:[^"\\]|\\.)*"|[A-Za-z_][A-Za-z_0-9]*|[0-9]+|[\[\]{},:]"#)?;
    let tokens: Vec<_> = token_pattern.find_iter(&literal).collect();
    let mut end = 0;
    let mut json = String::new();
    for (index, token) in tokens.iter().enumerate() {
        anyhow::ensure!(
            literal[end..token.start()].trim().is_empty(),
            "unsupported catalog literal syntax"
        );
        end = token.end();
        let text = token.as_str();
        if text == ","
            && tokens
                .get(index + 1)
                .is_some_and(|next| matches!(next.as_str(), "]" | "}"))
        {
            continue;
        }
        if text.as_bytes()[0].is_ascii_alphabetic() || text.starts_with('_') {
            anyhow::ensure!(
                matches!(
                    text,
                    "repo" | "name" | "summary" | "categories" | "version" | "apiVersion"
                ),
                "unsupported catalog identifier"
            );
            json.push_str(&serde_json::to_string(text)?);
        } else {
            json.push_str(text);
        }
    }
    anyhow::ensure!(
        literal[end..].trim().is_empty(),
        "unsupported trailing catalog syntax"
    );
    Ok(serde_json::from_str(&json)?)
}

fn generate_legacy_catalog() -> Result<Value> {
    let repo = super::repo_root()?;
    let source = super::git_stdout(
        &repo,
        [
            "show",
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2:website/src/data/extensions.ts",
        ],
    )?;
    let mut entries = decode_legacy_source(&source)?;
    let categorized: Vec<CategorizedListing> = serde_json::from_value(entries.clone())?;
    for entry in entries
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("catalog is not an array"))?
    {
        entry
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("listing is not an object"))?
            .insert(
                "compatibility".into(),
                serde_json::json!("requires-rust-rewrite"),
            );
    }
    let catalog = serde_json::json!({
        "source":"Hunk extension directory",
        "baseline":"2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
        "blob":"23f2196dd3c7a91306e050db352d4b5518a31117",
        "license":"MIT; Copyright Modem Labs Inc.; see THIRD_PARTY_NOTICES",
        "facets":category_facets(&categorized),
        "entries":entries
    });
    verify_legacy_source(&catalog, &source)?;
    Ok(catalog)
}

#[test]
fn legacy_source_comparison_rejects_field_drift_and_executable_syntax() {
    let catalog: Value =
        serde_json::from_str(include_str!("../../site/data/legacy-extensions.json")).unwrap();
    let mut entries = catalog["entries"].clone();
    for entry in entries.as_array_mut().unwrap() {
        entry.as_object_mut().unwrap().remove("compatibility");
    }
    let source = format!(
        "export const EXTENSION_CATALOG: readonly ExtensionListing[] = {};",
        serde_json::to_string_pretty(&entries).unwrap()
    );
    verify_legacy_source(&catalog, &source).unwrap();
    let mut changed = catalog.clone();
    changed["entries"][0]["summary"] =
        serde_json::json!("Different but structurally valid summary");
    assert!(validate_legacy_catalog(&changed).is_ok());
    assert!(verify_legacy_source(&changed, &source).is_err());
    let injected = source.replacen("= [", "= [compute(),", 1);
    assert!(verify_legacy_source(&catalog, &injected).is_err());
    assert!(verify_legacy_source(&catalog, "").is_err());
}

#[test]
fn native_extension_directory_replaces_the_complete_pinned_page() {
    let repo = super::repo_root().unwrap();
    verify_extensions_page(&repo).unwrap();
}

#[test]
fn pinned_extension_tooling_sources_are_verified_from_both_anchors() {
    let repo = super::repo_root().unwrap();
    verify_extension_tooling(&repo, "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2").unwrap();
}

#[test]
fn native_extension_examples_and_docs_are_checked_by_rust() {
    let repo = super::repo_root().unwrap();
    let examples = fs::read_to_string(repo.join("examples/Cargo.toml")).unwrap();
    let binary_count = examples
        .lines()
        .filter(|line| line.trim_start().starts_with("name = \"workdeck-example-"))
        .count();
    assert!(binary_count >= 13, "expected native extension examples");
    let docs = fs::read_to_string(repo.join("docs/extensions.md")).unwrap();
    assert!(docs.contains("Rust/Ratatui extension API"));
    assert!(docs.contains("native API reference"));
}

#[test]
fn legacy_literal_decoder_preserves_quoted_syntax_and_trailing_commas() {
    let mut catalog: Value =
        serde_json::from_str(include_str!("../../site/data/legacy-extensions.json")).unwrap();
    catalog["entries"][0]["summary"] =
        serde_json::json!("Quoted \"repo\": [text, ] and braces { }, backslash \\, Unicode λ 🦀");
    let mut entries = catalog["entries"].clone();
    for entry in entries.as_array_mut().unwrap() {
        entry.as_object_mut().unwrap().remove("compatibility");
    }
    // Only object boundaries outside strings receive an extra comma.
    let body = serde_json::to_string_pretty(&entries)
        .unwrap()
        .replace("\n  }", ",\n  }")
        .replace("\n]", ",\n]");
    let source = format!("export const EXTENSION_CATALOG: readonly ExtensionListing[] = {body};");
    verify_legacy_source(&catalog, &source).unwrap();
    for expression in ["compute()", "undefined", "-1", "1.5", "/* comment */"] {
        let unsupported = source.replacen("= [", &format!("= [{expression},"), 1);
        assert!(
            verify_legacy_source(&catalog, &unsupported).is_err(),
            "accepted {expression}"
        );
    }
}

#[test]
fn legacy_catalog_rejects_missing_duplicate_or_misrepresented_listings() {
    let catalog: Value =
        serde_json::from_str(include_str!("../../site/data/legacy-extensions.json")).unwrap();
    validate_legacy_catalog(&catalog).unwrap();
    for (field, value) in [
        ("repo", catalog["entries"][1]["repo"].clone()),
        ("repo", serde_json::json!("owner/repo\" onclick=bad")),
        ("name", serde_json::json!("")),
        ("summary", serde_json::json!(" ")),
        ("version", serde_json::json!("unknown")),
        ("apiVersion", serde_json::json!(null)),
        ("categories", serde_json::json!([])),
        ("categories", serde_json::json!(["Unknown"])),
        ("compatibility", serde_json::json!("native")),
    ] {
        let mut changed = catalog.clone();
        changed["entries"][0][field] = value;
        assert!(
            validate_legacy_catalog(&changed).is_err(),
            "accepted changed {field}"
        );
    }
    let mut wrong_count = catalog.clone();
    wrong_count["facets"][0]["count"] = serde_json::json!(999);
    assert!(validate_legacy_catalog(&wrong_count).is_err());
    let mut wrong_order = catalog.clone();
    wrong_order["facets"].as_array_mut().unwrap().swap(0, 1);
    assert!(validate_legacy_catalog(&wrong_order).is_err());
    let mut missing_facet = catalog.clone();
    missing_facet["facets"].as_array_mut().unwrap().pop();
    assert!(validate_legacy_catalog(&missing_facet).is_err());
    let mut shortened = catalog;
    shortened["entries"].as_array_mut().unwrap().pop();
    assert!(validate_legacy_catalog(&shortened).is_err());
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize,
)]
enum Category {
    #[serde(rename = "Changeset transform")]
    ChangesetTransform,
    Command,
    #[serde(rename = "File view")]
    FileView,
    #[serde(rename = "Keyboard mode")]
    KeyboardMode,
    #[serde(rename = "Line highlighter")]
    LineHighlighter,
    Pane,
    Theme,
    #[serde(rename = "VCS backend")]
    VcsBackend,
}

#[derive(serde::Deserialize)]
struct CategorizedListing {
    categories: Vec<Category>,
}

#[derive(serde::Serialize)]
struct CategoryFacet {
    category: Category,
    count: usize,
}

fn category_facets(entries: &[CategorizedListing]) -> Vec<CategoryFacet> {
    let mut counts = BTreeMap::new();
    for entry in entries {
        for &category in &entry.categories {
            *counts.entry(category).or_insert(0) += 1;
        }
    }
    let mut facets: Vec<_> = counts
        .into_iter()
        .map(|(category, count)| CategoryFacet { category, count })
        .collect();
    // The closed source category vocabulary has the same alphabetical order
    // under localeCompare and this enum; do not extend it without an oracle.
    facets.sort_by_key(|facet| (std::cmp::Reverse(facet.count), facet.category));
    facets
}

fn activity(value: &Value) -> Value {
    let mut result = Map::new();
    for (source, destination, numeric) in [
        ("stargazers_count", "stars", true),
        ("pushed_at", "pushedAt", false),
        ("created_at", "createdAt", false),
    ] {
        if let Some(value) = value.get(source)
            && if numeric {
                value.is_number()
            } else {
                value.is_string()
            }
        {
            result.insert(destination.into(), value.clone());
        }
    }
    Value::Object(result)
}

fn index_activity(payload: &Value) -> BTreeMap<String, Value> {
    let mut result = BTreeMap::new();
    if let Some(items) = payload.get("items").and_then(Value::as_array) {
        for item in items {
            if let Some(name) = item.get("full_name").and_then(Value::as_str) {
                result.insert(name.to_lowercase(), activity(item));
            }
        }
    }
    result
}

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let command = args.next();
    if command.as_deref() == Some("seed") {
        anyhow::ensure!(args.next().is_none(), "seed does not accept arguments");
        println!(
            "{}",
            serde_json::to_string_pretty(&generate_legacy_catalog()?)?
        );
        return Ok(());
    }
    if !matches!(
        command.as_deref(),
        Some("activity-index" | "json-ld" | "format-updated" | "category-facets" | "load")
    ) || args.next().is_some()
    {
        bail!(
            "extension-catalog requires activity-index, json-ld, format-updated, category-facets or load (JSON on stdin)"
        );
    }
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let payload: Value = serde_json::from_str(&input)?;
    if command.as_deref() == Some("load") {
        let entries = loading::load(&payload)?;
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }
    if command.as_deref() == Some("category-facets") {
        let entries: Vec<CategorizedListing> = serde_json::from_value(payload)?;
        println!("{}", serde_json::to_string(&category_facets(&entries))?);
        return Ok(());
    }
    if command.as_deref() == Some("format-updated") {
        let pushed = payload
            .get("pushedAt")
            .and_then(Value::as_str)
            .and_then(parse_catalog_timestamp);
        let now = match payload.get("now") {
            None => Some(chrono::Utc::now().timestamp_millis()),
            Some(value) => value.as_str().and_then(parse_catalog_timestamp),
        };
        let formatted = pushed
            .zip(now)
            .and_then(|(pushed, now)| format_updated_millis(pushed, now));
        println!("{}", serde_json::to_string(&formatted)?);
        return Ok(());
    }
    if command.as_deref() == Some("json-ld") {
        println!("{}", json_ld_script_body(&payload)?);
        return Ok(());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&index_activity(&payload))?
    );
    Ok(())
}

// Date-only ISO inputs are UTC in the source runtime. Days through 31 roll
// into the following month; larger days and invalid months are rejected.
// This is deliberately not a claim to implement all legacy Date spellings.
fn parse_catalog_timestamp(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        let year = value[..4].parse().ok()?;
        let month = value[5..7].parse().ok()?;
        let day: u64 = value[8..].parse().ok()?;
        if !(1..=31).contains(&day) {
            return None;
        }
        return chrono::NaiveDate::from_ymd_opt(year, month, 1)?
            .checked_add_days(chrono::Days::new(day - 1))?
            .and_hms_opt(0, 0, 0)
            .map(|date| date.and_utc().timestamp_millis());
    }
    let date = chrono::DateTime::parse_from_rfc3339(value).ok()?;
    // Chrono accepts leap seconds, whereas JavaScript Date rejects them.
    (date.timestamp_subsec_nanos() < 1_000_000_000).then(|| date.timestamp_millis())
}

fn format_updated_millis(pushed: i64, now: i64) -> Option<String> {
    let elapsed = i128::from(now) - i128::from(pushed);
    if elapsed < 0 {
        return None;
    }
    let days = elapsed / 86_400_000;
    Some(match days {
        0 => "today".into(),
        1 => "yesterday".into(),
        2..30 => format!("{days} days ago"),
        _ => {
            let months = days / 30;
            if months < 12 {
                format!("{months} month{} ago", if months == 1 { "" } else { "s" })
            } else {
                let years = days / 365;
                format!("{years} year{} ago", if years == 1 { "" } else { "s" })
            }
        }
    })
}

#[test]
fn recency_thresholds_preserve_source_day_month_year_boundaries() {
    for (days, expected) in [
        (0, "today"),
        (1, "yesterday"),
        (14, "14 days ago"),
        (29, "29 days ago"),
        (30, "1 month ago"),
        (59, "1 month ago"),
        (60, "2 months ago"),
        (359, "11 months ago"),
        (360, "0 years ago"),
        (364, "0 years ago"),
        (365, "1 year ago"),
        (730, "2 years ago"),
    ] {
        assert_eq!(
            format_updated_millis(0, days * 86_400_000).as_deref(),
            Some(expected)
        );
    }
    assert_eq!(format_updated_millis(1, 0), None);
    assert_eq!(
        format_updated_millis(0, 86_399_999).as_deref(),
        Some("today")
    );
    assert!(format_updated_millis(i64::MIN, i64::MAX).is_some());
}

fn json_ld_script_body(value: &Value) -> Result<String> {
    let mut output = String::new();
    write_javascript_json(&javascript_property_order(value), &mut output)?;
    Ok(output.replace('<', "\\u003c"))
}

fn write_javascript_json(value: &Value, output: &mut String) -> Result<()> {
    match value {
        Value::Number(number) => {
            let number = number.as_f64().ok_or_else(|| {
                anyhow::anyhow!("JSON number is not representable as a JavaScript number")
            })?;
            if number.is_finite() {
                output.push_str(ryu_js::Buffer::new().format_finite(number));
            } else {
                output.push_str("null");
            }
        }
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_javascript_json(value, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            for (index, (key, value)) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                output.push_str(&serde_json::to_string(key)?);
                output.push(':');
                write_javascript_json(value, output)?;
            }
            output.push('}');
        }
        _ => output.push_str(&serde_json::to_string(value)?),
    }
    Ok(())
}

fn array_index(key: &str) -> Option<u32> {
    let index = key.parse::<u32>().ok()?;
    (index != u32::MAX && index.to_string() == key).then_some(index)
}

fn javascript_property_order(value: &Value) -> Value {
    match value {
        Value::Array(values) => {
            Value::Array(values.iter().map(javascript_property_order).collect())
        }
        Value::Object(values) => {
            let mut indexed = values
                .iter()
                .filter_map(|(key, value)| array_index(key).map(|index| (index, key, value)))
                .collect::<Vec<_>>();
            indexed.sort_by_key(|(index, _, _)| *index);
            let mut ordered = Map::new();
            for (_, key, value) in indexed {
                ordered.insert(key.clone(), javascript_property_order(value));
            }
            for (key, value) in values {
                if array_index(key).is_none() {
                    ordered.insert(key.clone(), javascript_property_order(value));
                }
            }
            Value::Object(ordered)
        }
        _ => value.clone(),
    }
}

#[test]
fn json_ld_orders_array_indices_before_insertion_ordered_keys_recursively() {
    let value: Value = serde_json::from_str(r#"{"z":{"10":1,"2":2},"4294967295":3,"01":4,"4294967294":5,"0":6,"-0":7,"a":[{"3":8,"1":9}]}"#).unwrap();
    assert_eq!(
        json_ld_script_body(&value).unwrap(),
        r#"{"0":6,"4294967294":5,"z":{"2":2,"10":1},"4294967295":3,"01":4,"-0":7,"a":[{"1":9,"3":8}]}"#
    );
    let fixture: Value = serde_json::from_str(include_str!(
        "../../port/hunk/oracles/json-ld-serialization-gaps.json"
    ))
    .unwrap();
    for capture in fixture["sourceCaptures"].as_array().unwrap() {
        for case in capture["cases"].as_array().unwrap() {
            let input: Value = serde_json::from_str(case["input"].as_str().unwrap()).unwrap();
            assert_eq!(json_ld_script_body(&input).unwrap(), case["expected"]);
        }
    }
}

#[test]
fn json_ld_neutralizes_markup_without_changing_decoded_values() {
    for value in [
        serde_json::json!({"name":"</script><img src=x onerror=alert(1)>"}),
        serde_json::json!({"<key>":["<!--", "<SCRIPT>", "\\u003c", "雪", null]}),
        Value::Null,
    ] {
        let body = json_ld_script_body(&value).unwrap();
        assert!(!body.contains('<'));
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), value);
    }
}

#[test]
fn activity_index_matches_both_pinned_source_captures() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../port/hunk/oracles/extension-activity-index.json"
    ))
    .unwrap();
    let captures = fixture["captures"].as_array().unwrap();
    assert_eq!(captures.len(), 2);
    for capture in captures {
        assert_eq!(capture["exitCode"], 0);
        let cases = capture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 7);
        for case in cases {
            assert_eq!(
                serde_json::to_value(index_activity(&case["input"])).unwrap(),
                case["expected"],
                "{}: {}",
                capture["kind"],
                case["input"]
            );
        }
    }
}

#[test]
fn activity_index_preserves_missing_fields_and_last_case_insensitive_entry() {
    let result = index_activity(&serde_json::json!({"items":[
        {"full_name":"Owner/Repo", "stargazers_count":12, "pushed_at":"today", "created_at":"earlier"},
        {"full_name":"someone/other", "stargazers_count":4},
        {"stargazers_count":9}, null, 7,
        {"full_name":"OWNER/REPO", "stargazers_count":"wrong", "pushed_at":false, "created_at":"replacement"}
    ]}));
    assert_eq!(result.len(), 2);
    assert_eq!(
        result["owner/repo"],
        serde_json::json!({"createdAt":"replacement"})
    );
    assert_eq!(result["someone/other"], serde_json::json!({"stars":4}));
    assert_eq!(
        activity(&serde_json::json!({"stargazers_count":-1.5})),
        serde_json::json!({"stars":-1.5})
    );
    for payload in [
        Value::Null,
        serde_json::json!({}),
        serde_json::json!({"items":"nope"}),
        serde_json::json!({"items":[null,7]}),
    ] {
        assert!(index_activity(&payload).is_empty());
    }
    assert!(run(["unexpected".into()].into_iter()).is_err());
}
