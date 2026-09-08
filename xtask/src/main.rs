use anyhow::{Context, Result, bail};
use cargo_metadata::MetadataCommand;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod architecture;
mod benchmark;
mod changelog;
mod ci_changes;
mod ci_host;
mod install;
mod nix;
mod provenance;
mod release_channel;
mod release_notes;
mod release_status;
mod skill;
mod term_video;

const DEFAULT_BASELINE: &str = "hunk-port/main-2c00f435^{}";
const DEFAULT_STABLE: &str = "hunk-port/stable-v0.20.1^{}";
const DEFAULT_LEDGER: &str = "port/hunk/ledger.jsonl";
const DEFAULT_METADATA: &str = "port/hunk/baseline.json";
const DEFAULT_STABLE_FIXES: &str = "port/hunk/stable-fixes.jsonl";
const SHIKI_THEMES_SHA256: &str =
    "3ede8069dbdf87d16534256f06d9ff972fb317cd599c2ebfff4614e723430b0b";
const TM_THEMES_SHA256: &str = "dbcb0b853e04825304bea19a4b255862da211a884eaa86c7644c312d7f02812d";
const PIERRE_THEME_SHA256: &str =
    "47a78fa060c13411bb91ec5b8bfefb64059d89a7b69ebe6aa1d13f93d8a6ed62";
const SOURCE_EXTENSIONS: &[&str] = &[
    ".ts", ".tsx", ".mts", ".js", ".jsx", ".mjs", ".cjs", ".astro", ".css", ".scss", ".sass",
    ".less", ".html", ".htm", ".rs", ".py", ".rb", ".go", ".java", ".kt", ".swift", ".c", ".h",
    ".cc", ".cpp", ".hpp", ".cs", ".sh", ".bash", ".zsh", ".fish", ".ps1",
];
const ASSET_EXTENSIONS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif", ".ico", ".svg", ".woff", ".woff2", ".otf",
    ".ttf", ".mp4", ".webm", ".wav", ".mp3", ".pdf",
];

#[derive(Debug, Clone)]
struct TreeEntry {
    path: String,
    blob: String,
    bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LedgerRecord {
    id: String,
    baseline: String,
    path: String,
    blob: String,
    byte_start: u64,
    byte_end: u64,
    line_start: u64,
    line_end: u64,
    classification: String,
    disposition: String,
    destinations: Vec<String>,
    evidence: Vec<String>,
    provenance: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BaselineMetadata {
    schema_version: u32,
    baseline: String,
    stable: String,
    tree: String,
    file_count: usize,
    byte_count: u64,
    ledger: String,
}

#[derive(Debug, Deserialize)]
struct StableFixRecord {
    commit: String,
    disposition: String,
    destinations: Vec<String>,
    evidence: Vec<String>,
}

#[derive(Debug, Default)]
struct Options {
    baseline: Option<String>,
    ledger: Option<PathBuf>,
    metadata: Option<PathBuf>,
    allow_incomplete: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("port") => {
            let command = args
                .next()
                .context(
                    "port requires fetch, inventory, reclassify, map, materialize-assets, audit, or status",
                )?;
            if !matches!(
                command.as_str(),
                "fetch"
                    | "inventory"
                    | "reclassify"
                    | "map"
                    | "materialize-assets"
                    | "audit"
                    | "status"
            ) {
                bail!("unknown port command {command:?}");
            }
            if command == "fetch" {
                return fetch_hunk();
            }
            if command == "map" {
                return map_records(parse_map_options(args)?);
            }
            if command == "materialize-assets" {
                if args.next().is_some() {
                    bail!("port materialize-assets accepts no options");
                }
                return materialize_assets();
            }
            let options = parse_options(args)?;
            match command.as_str() {
                "inventory" => inventory(options),
                "reclassify" => reclassify(options),
                "audit" => audit(options, true),
                "status" => audit(options, false),
                _ => unreachable!(),
            }
        }
        Some("licenses") => licenses(parse_output_option(args)?),
        Some("benchmark") => benchmark::run(args),
        Some("ci-changes") => ci_changes::run(args),
        Some("ci-host") => ci_host::run(args),
        Some("themes") => match args.next().as_deref() {
            Some("vendor") => vendor_themes(parse_theme_vendor_options(args)?),
            Some("verify") => {
                if args.next().is_some() {
                    bail!("themes verify accepts no options");
                }
                verify_vendored_themes()
            }
            _ => bail!("themes requires the vendor or verify command"),
        },
        Some("verify") => verify(),
        Some("architecture") => match args.next().as_deref() {
            Some("check") => {
                if args.next().is_some() {
                    bail!("architecture check accepts no options");
                }
                architecture::check(&repo_root()?)
            }
            _ => bail!("architecture requires the check command"),
        },
        Some("nix") => match args.next().as_deref() {
            Some("check") => {
                if args.next().is_some() {
                    bail!("nix check accepts no options");
                }
                nix::check(&repo_root()?)
            }
            _ => bail!("nix requires the check command"),
        },
        Some("skill") => match args.next().as_deref() {
            Some("generate") => {
                if args.next().is_some() {
                    bail!("skill generate accepts no options");
                }
                skill::generate(&repo_root()?)
            }
            Some("check") => {
                if args.next().is_some() {
                    bail!("skill check accepts no options");
                }
                skill::check(&repo_root()?)
            }
            _ => bail!("skill requires the generate or check command"),
        },
        Some("extension") => match args.next().as_deref() {
            Some("stage-example") => {
                let name = args
                    .next()
                    .context("extension stage-example requires an example name")?;
                if args.next().is_some() {
                    bail!("extension stage-example accepts exactly one example name");
                }
                stage_extension_example(&name)
            }
            _ => bail!("extension requires the stage-example command"),
        },
        Some("site") => site(args.next().as_deref()),
        Some("install-plan") => install::run(args),
        Some("install-verify") => install::verify(args),
        Some("install-inspect") => install::inspect(args),
        Some("changelog") => changelog::run(&repo_root()?, args),
        Some("media") => match args.next().as_deref() {
            Some("plan") => term_video::plan_file(&repo_root()?, args),
            Some("compose") => term_video::compose_file(&repo_root()?, args),
            Some("capture") => term_video::capture_file(&repo_root()?, args),
            Some("launch") => term_video::launch_file(&repo_root()?, args),
            _ => bail!("media requires the plan, capture, compose, or launch command"),
        },
        Some("release") => match args.next().as_deref() {
            Some("package") => package_release(parse_package_options(args)?),
            Some("provenance-check") => provenance::inspect(args),
            Some("provenance-verify") => provenance::verify(args),
            Some("channel") => release_channel::channel(args),
            Some("check-version") => release_channel::check_version(&repo_root()?, args),
            Some("validate-prerelease") => release_notes::validate_local(&repo_root()?, args),
            Some("status") => release_status::run(&repo_root()?, args),
            Some("verify-pr-notes") => release_notes::verify_pr(&repo_root()?, args),
            _ => bail!(
                "release requires package, channel, check-version, validate-prerelease, status, or verify-pr-notes"
            ),
        },
        _ => {
            print_help();
            Ok(())
        }
    }
}

fn stage_extension_example(name: &str) -> Result<()> {
    let repo = repo_root()?;
    let staged = prepare_extension_example(&repo, name)?;
    println!("staged {}", relative_to(&repo, &staged));
    Ok(())
}

fn extension_example_binary_target(name: &str) -> Result<&'static str> {
    let binary_target = match name {
        "cli-tools" => "workdeck-example-cli-tools-extension",
        "pane-layout" => "workdeck-example-pane-layout-extension",
        "vim-navigation" => "workdeck-example-vim-navigation-extension",
        "review-snapshot-export" => "workdeck-example-review-snapshot-export-extension",
        "review-note-navigator" => "workdeck-example-review-note-navigator-extension",
        "rendered-markdown" => "workdeck-example-rendered-markdown-extension",
        "jsx-file-view" => "workdeck-example-jsx-file-view-extension",
        "inline-edit" => "workdeck-example-inline-edit-extension",
        "review-triage" => "workdeck-example-review-triage-extension",
        "github-pr" => "workdeck-example-github-pr-extension",
        "file-view-gallery" => "workdeck-example-file-view-gallery-extension",
        "native-vcs" => "workdeck-example-native-vcs-extension",
        "startup-lifecycle" => "workdeck-example-startup-lifecycle-extension",
        _ => bail!("unknown native extension example {name:?}"),
    };
    Ok(binary_target)
}

fn prepare_extension_example(repo: &Path, name: &str) -> Result<PathBuf> {
    let binary_target = extension_example_binary_target(name)?;
    run_checked(
        repo,
        "cargo",
        &["build", "-p", "workdeck-examples", "--bin", binary_target],
    )?;
    let metadata = MetadataCommand::new()
        .current_dir(repo)
        .no_deps()
        .exec()
        .context("resolve Cargo target directory")?;
    let binary_name = format!("{binary_target}{}", env::consts::EXE_SUFFIX);
    let binary = metadata.target_directory.join("debug").join(&binary_name);
    if !binary.is_file() {
        bail!("built extension executable is missing: {binary}");
    }
    let staged = metadata
        .target_directory
        .join("workdeck-extension-examples")
        .join(name);
    let staged_binary = staged.join("bin").join(&binary_name);
    fs::create_dir_all(staged_binary.parent().expect("binary has a parent"))?;
    fs::copy(&binary, &staged_binary)
        .with_context(|| format!("stage extension executable {} -> {}", binary, staged_binary))?;
    fs::copy(
        repo.join(format!(
            "examples/extensions/{name}/workdeck-extension.toml"
        )),
        staged.join("workdeck-extension.toml"),
    )?;
    Ok(staged.into_std_path_buf())
}

fn site(command: Option<&str>) -> Result<()> {
    let repo = repo_root()?;
    let site = repo.join("site");
    match command {
        Some("build") => run_checked(&site, "zola", &["build"]),
        Some("check") => run_checked(&site, "zola", &["check"]),
        Some("serve") => run_checked(&site, "zola", &["serve"]),
        _ => bail!("site requires build, check, or serve"),
    }
}

fn fetch_hunk() -> Result<()> {
    const URL: &str = "https://github.com/modem-dev/hunk.git";
    const MAIN: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
    const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";
    let repo = repo_root()?;
    let remotes = git_stdout(&repo, ["remote"])?;
    if !remotes.lines().any(|remote| remote == "hunk-upstream") {
        run_checked(&repo, "git", &["remote", "add", "hunk-upstream", URL])?;
    } else {
        let actual = git_stdout(&repo, ["remote", "get-url", "hunk-upstream"])?;
        if actual != URL {
            bail!("hunk-upstream points to {actual:?}, expected {URL:?}");
        }
    }
    run_checked(
        &repo,
        "git",
        &[
            "config",
            "remote.hunk-upstream.fetch",
            "+refs/heads/*:refs/remotes/hunk-upstream/*",
        ],
    )?;
    run_checked(
        &repo,
        "git",
        &[
            "fetch",
            "--prune",
            "hunk-upstream",
            "+refs/heads/*:refs/remotes/hunk-upstream/*",
            "+refs/tags/*:refs/tags/hunk-upstream/*",
        ],
    )?;
    ensure_anchor_tag(&repo, "hunk-port/main-2c00f435", MAIN)?;
    ensure_anchor_tag(&repo, "hunk-port/stable-v0.20.1", STABLE)?;
    println!("updated namespaced Hunk refs and verified both port anchors");
    Ok(())
}

fn ensure_anchor_tag(repo: &Path, name: &str, commit: &str) -> Result<()> {
    let reference = format!("refs/tags/{name}^{{}}");
    let existing = git_output(repo, ["rev-parse", "--verify", &reference])?;
    if existing.status.success() {
        let actual = String::from_utf8(existing.stdout)?.trim().to_owned();
        if actual != commit {
            bail!("anchor tag {name} resolves to {actual}, expected {commit}");
        }
        return Ok(());
    }
    let message = format!("Hunk semantic port anchor {commit}");
    run_checked(repo, "git", &["tag", "-a", name, commit, "-m", &message])
}

#[derive(Debug, Default)]
struct MapOptions {
    ledger: Option<PathBuf>,
    ids: Vec<String>,
    paths: Vec<String>,
    prefixes: Vec<String>,
    disposition: Option<String>,
    destinations: Vec<String>,
    evidence: Vec<String>,
    provenance: Vec<String>,
    replace: bool,
}

fn parse_map_options(mut args: impl Iterator<Item = String>) -> Result<MapOptions> {
    let mut options = MapOptions::default();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--ledger" => {
                options.ledger = Some(PathBuf::from(required_value(&mut args, "--ledger")?))
            }
            "--id" => options.ids.push(required_value(&mut args, "--id")?),
            "--path" => options.paths.push(required_value(&mut args, "--path")?),
            "--prefix" => options
                .prefixes
                .push(required_value(&mut args, "--prefix")?),
            "--disposition" => {
                options.disposition = Some(required_value(&mut args, "--disposition")?)
            }
            "--destination" => options
                .destinations
                .push(required_value(&mut args, "--destination")?),
            "--evidence" => options
                .evidence
                .push(required_value(&mut args, "--evidence")?),
            "--provenance" => options
                .provenance
                .push(required_value(&mut args, "--provenance")?),
            "--replace" => options.replace = true,
            _ => bail!("unknown port map option {argument:?}"),
        }
    }
    if options.ids.is_empty() && options.paths.is_empty() && options.prefixes.is_empty() {
        bail!("port map requires at least one --id, --path, or --prefix");
    }
    if options.destinations.is_empty() || options.evidence.is_empty() {
        bail!("port map requires --destination and --evidence");
    }
    if options.disposition.as_deref() == Some("unmapped") {
        bail!("port map cannot map records back to unmapped");
    }
    Ok(options)
}

fn map_records(options: MapOptions) -> Result<()> {
    let repo = repo_root()?;
    let ledger_path = repo.join(options.ledger.unwrap_or_else(|| DEFAULT_LEDGER.into()));
    let _ledger_lock = lock_ledger_for_write(&ledger_path)?;
    let mut records = read_ledger(&ledger_path)?;
    let disposition = options
        .disposition
        .context("port map requires --disposition")?;
    let mut changed = 0;
    for record in &mut records {
        let selected = options.ids.iter().any(|id| id == &record.id)
            || options.paths.iter().any(|path| path == &record.path)
            || options
                .prefixes
                .iter()
                .any(|prefix| record.path.starts_with(prefix));
        if !selected {
            continue;
        }
        if record.disposition != "unmapped" && !options.replace {
            bail!(
                "{} is already mapped as {}; pass --replace to update it",
                record.path,
                record.disposition
            );
        }
        record.disposition.clone_from(&disposition);
        record.destinations.clone_from(&options.destinations);
        record.evidence.clone_from(&options.evidence);
        for provenance in &options.provenance {
            if !record.provenance.contains(provenance) {
                record.provenance.push(provenance.clone());
            }
        }
        validate_disposition(&repo, record)?;
        validate_test_evidence(&repo, record)?;
        changed += 1;
    }
    if changed == 0 {
        bail!("port map selectors matched no ledger records");
    }
    write_ledger_atomic(&ledger_path, &records)?;
    println!("mapped {changed} ledger records as {disposition}");
    Ok(())
}

#[derive(Debug, Serialize)]
struct RetainedAssetRecord {
    source_path: String,
    source_blob: String,
    destination: String,
    bytes: usize,
    sha256: String,
}

#[derive(Debug)]
struct ThemeVendorOptions {
    shiki_archive: PathBuf,
    tm_themes_archive: PathBuf,
    pierre_archive: PathBuf,
}

fn parse_theme_vendor_options(
    mut args: impl Iterator<Item = String>,
) -> Result<ThemeVendorOptions> {
    let mut shiki_archive = None;
    let mut tm_themes_archive = None;
    let mut pierre_archive = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--shiki-archive" => {
                shiki_archive = Some(PathBuf::from(required_value(&mut args, &argument)?));
            }
            "--tm-themes-archive" => {
                tm_themes_archive = Some(PathBuf::from(required_value(&mut args, &argument)?));
            }
            "--pierre-archive" => {
                pierre_archive = Some(PathBuf::from(required_value(&mut args, &argument)?));
            }
            _ => bail!("unknown themes vendor option {argument:?}"),
        }
    }
    Ok(ThemeVendorOptions {
        shiki_archive: shiki_archive.context("themes vendor requires --shiki-archive")?,
        tm_themes_archive: tm_themes_archive
            .context("themes vendor requires --tm-themes-archive")?,
        pierre_archive: pierre_archive.context("themes vendor requires --pierre-archive")?,
    })
}

fn verify_archive(path: &Path, package: &str, expected_sha256: &str) -> Result<()> {
    if !path.is_file() {
        bail!("{package} archive does not exist: {}", path.display());
    }
    let actual = sha256_file(path)?;
    if actual != expected_sha256 {
        bail!("{package} archive has SHA-256 {actual}, expected {expected_sha256}");
    }
    Ok(())
}

fn tar_gz_files(path: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let decoder = GzDecoder::new(
        File::open(path).with_context(|| format!("open archive {}", path.display()))?,
    );
    let mut archive = tar::Archive::new(decoder);
    let mut files = BTreeMap::new();
    for entry in archive.entries().context("read archive entries")? {
        let mut entry = entry.context("read archive entry")?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().context("read archive path")?;
        if path.components().any(|component| {
            !matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        }) {
            bail!("archive contains unsafe path {}", path.display());
        }
        let path = path.to_string_lossy().replace('\\', "/");
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .with_context(|| format!("read archive member {path}"))?;
        if files.insert(path.clone(), bytes).is_some() {
            bail!("archive contains duplicate path {path}");
        }
    }
    Ok(files)
}

fn required_archive_file<'a>(files: &'a BTreeMap<String, Vec<u8>>, path: &str) -> Result<&'a [u8]> {
    files
        .get(path)
        .map(Vec::as_slice)
        .with_context(|| format!("archive is missing {path}"))
}

fn shiki_module_theme(module: &[u8], path: &str) -> Result<serde_json::Value> {
    let source =
        std::str::from_utf8(module).with_context(|| format!("Shiki module {path} is not UTF-8"))?;
    let marker = "JSON.parse(";
    let start = source
        .find(marker)
        .map(|index| index + marker.len())
        .with_context(|| format!("Shiki module {path} lacks JSON.parse"))?;
    let end = source
        .rfind("))")
        .filter(|end| *end >= start)
        .with_context(|| format!("Shiki module {path} lacks its closing wrapper"))?;
    let json_literal = &source[start..end];
    let embedded: String = serde_json::from_str(json_literal)
        .with_context(|| format!("decode JSON string in Shiki module {path}"))?;
    serde_json::from_str(&embedded).with_context(|| format!("parse theme JSON in {path}"))
}

fn package_version(files: &BTreeMap<String, Vec<u8>>, expected: &str) -> Result<()> {
    let package: serde_json::Value =
        serde_json::from_slice(required_archive_file(files, "package/package.json")?)
            .context("parse package/package.json")?;
    let actual = package
        .get("version")
        .and_then(serde_json::Value::as_str)
        .context("package.json lacks a string version")?;
    if actual != expected {
        bail!("archive contains package version {actual}, expected {expected}");
    }
    Ok(())
}

fn theme_id_from_path<'a>(path: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    path.strip_prefix(prefix)?.strip_suffix(suffix)
}

/// Vendor the exact TextMate theme payloads used by the Hunk-pinned dependency graph.
///
/// The Shiki npm archive is used only as the byte-authenticated oracle. The semantically identical
/// `tm-themes` JSON is retained so the Rust product contains data rather than JavaScript modules,
/// together with the complete upstream per-theme NOTICE. Pierre's two default themes are copied
/// from its own JSON distribution. No Node/Bun/JavaScript runtime participates in this process.
fn vendor_themes(options: ThemeVendorOptions) -> Result<()> {
    verify_archive(
        &options.shiki_archive,
        "@shikijs/themes 3.23.0",
        SHIKI_THEMES_SHA256,
    )?;
    verify_archive(
        &options.tm_themes_archive,
        "tm-themes 1.12.0",
        TM_THEMES_SHA256,
    )?;
    verify_archive(
        &options.pierre_archive,
        "@pierre/theme 2.0.0",
        PIERRE_THEME_SHA256,
    )?;

    let shiki = tar_gz_files(&options.shiki_archive)?;
    let tm_themes = tar_gz_files(&options.tm_themes_archive)?;
    let pierre = tar_gz_files(&options.pierre_archive)?;
    package_version(&shiki, "3.23.0")?;
    package_version(&tm_themes, "1.12.0")?;
    package_version(&pierre, "2.0.0")?;

    let mut bundled = BTreeMap::<String, Vec<u8>>::new();
    for (path, bytes) in &tm_themes {
        let Some(theme_id) = theme_id_from_path(path, "package/themes/", ".json") else {
            continue;
        };
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .with_context(|| format!("parse tm-themes payload {path}"))?;
        let shiki_path = format!("package/dist/{theme_id}.mjs");
        let shiki_value =
            shiki_module_theme(required_archive_file(&shiki, &shiki_path)?, &shiki_path)?;
        if value != shiki_value {
            bail!("{path} is not semantically identical to {shiki_path}");
        }
        if value.get("name").and_then(serde_json::Value::as_str) != Some(theme_id) {
            bail!("{path} does not declare theme name {theme_id}");
        }
        bundled.insert(theme_id.to_owned(), bytes.clone());
    }
    if bundled.len() != 65 {
        bail!(
            "tm-themes archive produced {} verified Shiki themes, expected 65",
            bundled.len()
        );
    }

    for theme_id in ["pierre-dark", "pierre-light"] {
        let path = format!("package/themes/{theme_id}.json");
        let bytes = required_archive_file(&pierre, &path)?;
        let value: serde_json::Value =
            serde_json::from_slice(bytes).with_context(|| format!("parse Pierre theme {path}"))?;
        if value.get("name").and_then(serde_json::Value::as_str) != Some(theme_id) {
            bail!("{path} does not declare theme name {theme_id}");
        }
        bundled.insert(theme_id.to_owned(), bytes.to_vec());
    }

    let repo = repo_root()?;
    let assets = repo.join("crates/workdeck-diff/assets/themes");
    fs::create_dir_all(&assets).context("create bundled theme asset directory")?;
    let expected_files = bundled
        .keys()
        .map(|theme_id| format!("{theme_id}.json"))
        .collect::<BTreeSet<_>>();
    for entry in fs::read_dir(&assets).context("read bundled theme asset directory")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != "manifest.json" && !expected_files.contains(&name) {
            bail!(
                "unexpected bundled theme asset {}; remove it deliberately",
                entry.path().display()
            );
        }
    }

    let mut manifest_themes = Vec::with_capacity(bundled.len());
    for (theme_id, bytes) in &bundled {
        let path = assets.join(format!("{theme_id}.json"));
        fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
        manifest_themes.push(serde_json::json!({
            "id": theme_id,
            "path": relative_to(&repo, &path),
            "bytes": bytes.len(),
            "sha256": format!("{:x}", Sha256::digest(bytes)),
        }));
    }
    let notice_payloads = vec![
        (
            "third_party/themes/tm-themes-LICENSE".to_owned(),
            required_archive_file(&tm_themes, "package/LICENSE")?.to_vec(),
        ),
        (
            "third_party/themes/tm-themes-NOTICE".to_owned(),
            required_archive_file(&tm_themes, "package/NOTICE")?.to_vec(),
        ),
        (
            "third_party/themes/pierre-theme-LICENSE".to_owned(),
            required_archive_file(&pierre, "package/LICENSE")?.to_vec(),
        ),
        (
            "third_party/themes/pierre-theme-NOTICE.md".to_owned(),
            required_archive_file(&pierre, "package/NOTICE.md")?.to_vec(),
        ),
    ];
    let manifest_notices = notice_payloads
        .iter()
        .map(|(path, bytes)| {
            serde_json::json!({
                "path": path,
                "bytes": bytes.len(),
                "sha256": format!("{:x}", Sha256::digest(bytes)),
            })
        })
        .collect::<Vec<_>>();

    let manifest = serde_json::json!({
        "schema_version": 1,
        "generated_by": "cargo xtask themes vendor",
        "packages": [
            {
                "name": "@shikijs/themes",
                "version": "3.23.0",
                "sha256": SHIKI_THEMES_SHA256,
                "role": "semantic oracle for all 65 Shiki module payloads"
            },
            {
                "name": "tm-themes",
                "version": "1.12.0",
                "sha256": TM_THEMES_SHA256,
                "role": "byte-retained JSON and per-theme notices"
            },
            {
                "name": "@pierre/theme",
                "version": "2.0.0",
                "sha256": PIERRE_THEME_SHA256,
                "role": "byte-retained Pierre default themes"
            }
        ],
        "themes": manifest_themes,
        "notices": manifest_notices,
    });
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    manifest_bytes.push(b'\n');
    fs::write(assets.join("manifest.json"), manifest_bytes)?;

    let generated_module = repo.join("crates/workdeck-diff/src/bundled_theme_assets.rs");
    let mut generated = String::from(
        "// @generated by `cargo xtask themes vendor`; do not edit by hand.\n\n\
         pub(crate) const BUNDLED_THEME_ASSETS: &[(&str, &str)] = &[\n",
    );
    for theme_id in bundled.keys() {
        generated.push_str(&format!(
            "    (\n        {theme_id:?},\n        include_str!(\"../assets/themes/{theme_id}.json\"),\n    ),\n"
        ));
    }
    generated.push_str("];\n");
    fs::write(&generated_module, generated)
        .with_context(|| format!("write {}", generated_module.display()))?;
    let generated_relative = relative_to(&repo, &generated_module);
    run_checked(
        &repo,
        "rustfmt",
        &["--edition", "2024", generated_relative.as_str()],
    )?;

    let notices = repo.join("third_party/themes");
    fs::create_dir_all(&notices).context("create theme notice directory")?;
    for (path, bytes) in notice_payloads {
        fs::write(repo.join(path), bytes)?;
    }

    println!(
        "vendored {} authenticated TextMate themes and complete notices",
        bundled.len()
    );
    println!(
        "manifest: {}",
        relative_to(&repo, &assets.join("manifest.json"))
    );
    Ok(())
}

#[derive(Debug, Deserialize)]
struct BundledThemeManifest {
    schema_version: u32,
    generated_by: String,
    packages: Vec<BundledThemePackage>,
    themes: Vec<BundledThemeFile>,
    notices: Vec<BundledThemeNotice>,
}

#[derive(Debug, Deserialize)]
struct BundledThemePackage {
    name: String,
    version: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct BundledThemeFile {
    id: String,
    path: String,
    bytes: usize,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct BundledThemeNotice {
    path: String,
    bytes: usize,
    sha256: String,
}

fn verify_vendored_themes() -> Result<()> {
    let repo = repo_root()?;
    let assets = repo.join("crates/workdeck-diff/assets/themes");
    let manifest_path = assets.join("manifest.json");
    let manifest: BundledThemeManifest = serde_json::from_slice(
        &fs::read(&manifest_path).with_context(|| format!("read {}", manifest_path.display()))?,
    )
    .context("parse bundled theme manifest")?;
    if manifest.schema_version != 1 || manifest.generated_by != "cargo xtask themes vendor" {
        bail!("bundled theme manifest has unsupported provenance metadata");
    }

    let packages = manifest
        .packages
        .iter()
        .map(|package| {
            (
                package.name.as_str(),
                (package.version.as_str(), package.sha256.as_str()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let expected_packages = BTreeMap::from([
        ("@pierre/theme", ("2.0.0", PIERRE_THEME_SHA256)),
        ("@shikijs/themes", ("3.23.0", SHIKI_THEMES_SHA256)),
        ("tm-themes", ("1.12.0", TM_THEMES_SHA256)),
    ]);
    if packages != expected_packages {
        bail!("bundled theme manifest package provenance does not match pinned archives");
    }
    if manifest.themes.len() != 67 {
        bail!(
            "bundled theme manifest contains {} themes, expected 67",
            manifest.themes.len()
        );
    }

    let generated_module =
        fs::read_to_string(repo.join("crates/workdeck-diff/src/bundled_theme_assets.rs"))
            .context("read generated bundled-theme module")?;
    if generated_module.matches("include_str!(").count() != 67 {
        bail!("generated bundled-theme module does not contain exactly 67 assets");
    }
    let mut ids = BTreeSet::new();
    let mut expected_files = BTreeSet::new();
    for theme in &manifest.themes {
        if !ids.insert(theme.id.as_str()) {
            bail!("bundled theme manifest repeats {}", theme.id);
        }
        let expected_path = format!("crates/workdeck-diff/assets/themes/{}.json", theme.id);
        if theme.path != expected_path {
            bail!(
                "bundled theme {} has path {}, expected {}",
                theme.id,
                theme.path,
                expected_path
            );
        }
        let path = repo.join(&theme.path);
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        if bytes.len() != theme.bytes {
            bail!(
                "bundled theme {} has {} bytes, manifest records {}",
                theme.id,
                bytes.len(),
                theme.bytes
            );
        }
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if actual != theme.sha256 {
            bail!(
                "bundled theme {} has SHA-256 {}, manifest records {}",
                theme.id,
                actual,
                theme.sha256
            );
        }
        let id_literal = format!("{:?}", theme.id);
        let include_literal = format!("include_str!(\"../assets/themes/{}.json\")", theme.id);
        if !generated_module.contains(&id_literal) || !generated_module.contains(&include_literal) {
            bail!("generated bundled-theme module omits {}", theme.id);
        }
        expected_files.insert(format!("{}.json", theme.id));
    }

    for entry in fs::read_dir(&assets).context("read bundled theme directory")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != "manifest.json" && !expected_files.contains(&name) {
            bail!(
                "unmanifested bundled theme asset: {}",
                entry.path().display()
            );
        }
    }
    let expected_notices = BTreeSet::from([
        "third_party/themes/tm-themes-LICENSE",
        "third_party/themes/tm-themes-NOTICE",
        "third_party/themes/pierre-theme-LICENSE",
        "third_party/themes/pierre-theme-NOTICE.md",
    ]);
    let mut notices = BTreeSet::new();
    for notice in &manifest.notices {
        if !expected_notices.contains(notice.path.as_str()) {
            bail!(
                "bundled theme manifest names unexpected notice {}",
                notice.path
            );
        }
        if !notices.insert(notice.path.as_str()) {
            bail!("bundled theme manifest repeats notice {}", notice.path);
        }
        let bytes = fs::read(repo.join(&notice.path))
            .with_context(|| format!("read bundled theme notice {}", notice.path))?;
        if bytes.len() != notice.bytes {
            bail!(
                "bundled theme notice {} has {} bytes, manifest records {}",
                notice.path,
                bytes.len(),
                notice.bytes
            );
        }
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if actual != notice.sha256 {
            bail!(
                "bundled theme notice {} has SHA-256 {}, manifest records {}",
                notice.path,
                actual,
                notice.sha256
            );
        }
    }
    if notices != expected_notices {
        bail!("bundled theme manifest does not cover all four complete notices");
    }
    if !repo.join("third_party/themes/README.md").is_file() {
        bail!("bundled theme provenance README is missing");
    }
    println!("verified 67 pinned TextMate theme assets and complete notices");
    Ok(())
}

/// Materialize byte-exact, non-executable media from the pinned Hunk tree.
///
/// Retained media lives under a third-party namespace and is never loaded by the Workdeck
/// executable or website. Keeping it in the checkout makes its ledger disposition independently
/// inspectable without introducing a source mirror or retaining the upstream application runtime.
fn materialize_assets() -> Result<()> {
    const MEDIA_EXTENSIONS: &[&str] = &[
        "gif", "ico", "jpeg", "jpg", "mp4", "png", "webm", "webp", "woff", "woff2",
    ];

    let repo = repo_root()?;
    let ledger_path = repo.join(DEFAULT_LEDGER);
    let _ledger_lock = lock_ledger_for_write(&ledger_path)?;
    let mut records = read_ledger(&ledger_path)?;
    let retained_root = repo.join("third_party/hunk/assets");
    fs::create_dir_all(&retained_root).context("create retained Hunk asset directory")?;

    let mut manifest = Vec::new();
    let mut changed = 0;
    for record in &mut records {
        let extension = Path::new(&record.path)
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase);
        if !matches!(record.disposition.as_str(), "unmapped" | "retained-asset")
            || !extension
                .as_deref()
                .is_some_and(|value| MEDIA_EXTENSIONS.contains(&value))
        {
            continue;
        }

        let relative = Path::new(&record.path);
        if relative.components().any(|component| {
            !matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        }) {
            bail!("refusing unsafe retained asset path {}", record.path);
        }
        let bytes = git_stdout_bytes(&repo, ["cat-file", "blob", record.blob.as_str()])?;
        if bytes.len() as u64 != record.byte_end - record.byte_start {
            bail!(
                "retained asset {} has {} bytes, expected {}",
                record.path,
                bytes.len(),
                record.byte_end - record.byte_start
            );
        }

        let destination_path = retained_root.join(relative);
        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&destination_path, &bytes)
            .with_context(|| format!("write {}", destination_path.display()))?;

        let destination = relative_to(&repo, &destination_path);
        manifest.push(RetainedAssetRecord {
            source_path: record.path.clone(),
            source_blob: record.blob.clone(),
            destination: destination.clone(),
            bytes: bytes.len(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        });
        record.disposition = "retained-asset".to_owned();
        record.destinations = vec![destination];
        record.evidence = vec![
            "xtask/src/main.rs#materialize_assets".to_owned(),
            "third_party/hunk/README.md".to_owned(),
        ];
        changed += 1;
    }

    let manifest_path = repo.join("third_party/hunk/assets.jsonl");
    let mut manifest_writer = BufWriter::new(File::create(&manifest_path)?);
    for record in &manifest {
        serde_json::to_writer(&mut manifest_writer, record)?;
        manifest_writer.write_all(b"\n")?;
    }
    manifest_writer.flush()?;

    write_ledger_atomic(&ledger_path, &records)?;

    println!("materialized {changed} byte-exact Hunk media assets");
    println!("manifest: {}", relative_to(&repo, &manifest_path));
    Ok(())
}

#[derive(Debug)]
struct PackageOptions {
    target: String,
    binary: Option<PathBuf>,
    provenance: PathBuf,
    verify_ci: bool,
    output: PathBuf,
}

fn parse_output_option(mut args: impl Iterator<Item = String>) -> Result<PathBuf> {
    let mut output = PathBuf::from("dist/licenses.json");
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--output" => output = PathBuf::from(required_value(&mut args, "--output")?),
            _ => bail!("unknown licenses option {argument:?}"),
        }
    }
    Ok(output)
}

fn parse_package_options(mut args: impl Iterator<Item = String>) -> Result<PackageOptions> {
    let mut target = None;
    let mut binary = None;
    let mut provenance = None;
    let mut verify_ci = false;
    let mut output = PathBuf::from("dist");
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--target" => target = Some(required_value(&mut args, "--target")?),
            "--binary" => binary = Some(PathBuf::from(required_value(&mut args, "--binary")?)),
            "--provenance" => {
                provenance = Some(PathBuf::from(required_value(&mut args, "--provenance")?))
            }
            "--verify-ci" => verify_ci = true,
            "--output" => output = PathBuf::from(required_value(&mut args, "--output")?),
            _ => bail!("unknown release package option {argument:?}"),
        }
    }
    Ok(PackageOptions {
        target: target.context("release package requires --target")?,
        binary,
        provenance: provenance.context("release package requires --provenance STATEMENT")?,
        verify_ci,
        output,
    })
}

#[derive(Debug, Serialize)]
struct LicenseInventory {
    schema_version: u32,
    generated_by: &'static str,
    packages: Vec<LicensePackage>,
}

#[derive(Debug, Serialize)]
struct LicensePackage {
    name: String,
    version: String,
    license: Option<String>,
    repository: Option<String>,
    source: Option<String>,
}

fn dependency_inventory() -> Result<LicenseInventory> {
    let metadata = MetadataCommand::new()
        .other_options(vec!["--locked".into(), "--offline".into()])
        .exec()
        .context("read Cargo dependency metadata")?;
    let mut packages = metadata
        .packages
        .into_iter()
        .map(|package| LicensePackage {
            name: package.name.to_string(),
            version: package.version.to_string(),
            license: package.license.map(|license| license.to_string()),
            repository: package.repository.map(|repository| repository.to_string()),
            source: package.source.map(|source| source.to_string()),
        })
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| {
        (&left.name, &left.version, &left.source).cmp(&(&right.name, &right.version, &right.source))
    });
    Ok(LicenseInventory {
        schema_version: 1,
        generated_by: "cargo xtask licenses",
        packages,
    })
}

fn licenses(output: PathBuf) -> Result<()> {
    let repo = repo_root()?;
    let output = repo.join(output);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let inventory = dependency_inventory()?;
    let mut encoded = serde_json::to_string_pretty(&inventory)?;
    encoded.push('\n');
    fs::write(&output, encoded).with_context(|| format!("write {}", output.display()))?;
    println!(
        "wrote {} dependency license records to {}",
        inventory.packages.len(),
        relative_to(&repo, &output)
    );
    Ok(())
}

fn package_release(options: PackageOptions) -> Result<()> {
    let repo = repo_root()?;
    let executable_name = if options.target.contains("windows") {
        "workdeck.exe"
    } else {
        "workdeck"
    };
    let binary = options.binary.unwrap_or_else(|| {
        repo.join("target")
            .join(&options.target)
            .join("release")
            .join(executable_name)
    });
    if !binary.is_file() {
        bail!("release binary does not exist: {}", binary.display());
    }
    let output = repo.join(options.output);
    let inventory = dependency_inventory()?;
    let inventory_bytes = serde_json::to_vec_pretty(&inventory)?;
    let sbom = cyclonedx_sbom(&inventory);
    let root = format!("workdeck-{}", options.target);
    let mut entries = release_entries(
        &root,
        &binary,
        executable_name,
        &repo,
        &inventory_bytes,
        &sbom,
    )?;
    let mut provenance_bytes = Vec::new();
    File::open(&options.provenance)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut provenance_bytes)?;
    attach_release_provenance(&mut entries, &root, executable_name, provenance_bytes)?;
    if options.verify_ci {
        let binary = entries
            .iter()
            .find(|entry| entry.0 == format!("{root}/{executable_name}"))
            .context("missing archive executable")?;
        let bundle = entries
            .iter()
            .find(|entry| entry.0 == format!("{root}/provenance.sigstore.json"))
            .context("verified packaging requires a signed Sigstore bundle")?;
        provenance::verify_ci_snapshot(&binary.1, &bundle.1, executable_name)?;
    }
    fs::create_dir_all(&output).with_context(|| format!("create {}", output.display()))?;
    let archive = if options.target.contains("windows") {
        let path = output.join(format!("{root}.zip"));
        write_zip_archive(&path, &entries)?;
        path
    } else {
        let path = output.join(format!("{root}.tar.gz"));
        write_tar_archive(&path, &entries)?;
        path
    };
    let digest = sha256_file(&archive)?;
    let checksum = archive.with_extension(format!(
        "{}sha256",
        archive
            .extension()
            .and_then(|extension| extension.to_str())
            .map_or(String::new(), |extension| format!("{extension}."))
    ));
    fs::write(
        &checksum,
        format!(
            "{digest}  {}\n",
            archive.file_name().unwrap_or_default().to_string_lossy()
        ),
    )?;
    println!("packaged {}", relative_to(&repo, &archive));
    println!("checksum {}", relative_to(&repo, &checksum));
    Ok(())
}

fn release_entries<'a>(
    root: &'a str,
    binary: &'a Path,
    executable_name: &'a str,
    repo: &'a Path,
    inventory: &'a [u8],
    sbom: &'a [u8],
) -> Result<Vec<(String, Vec<u8>, u32)>> {
    let mut entries = vec![
        (
            format!("{root}/{executable_name}"),
            fs::read(binary).with_context(|| format!("read {}", binary.display()))?,
            0o755,
        ),
        (
            format!("{root}/LICENSE"),
            fs::read(repo.join("LICENSE"))?,
            0o644,
        ),
        (
            format!("{root}/THIRD_PARTY_NOTICES"),
            fs::read(repo.join("THIRD_PARTY_NOTICES"))?,
            0o644,
        ),
        (format!("{root}/licenses.json"), inventory.to_vec(), 0o644),
        (format!("{root}/sbom.cdx.json"), sbom.to_vec(), 0o644),
    ];
    for (archive_name, source) in [
        (
            "third-party/themes/tm-themes-LICENSE",
            "third_party/themes/tm-themes-LICENSE",
        ),
        (
            "third-party/themes/tm-themes-NOTICE",
            "third_party/themes/tm-themes-NOTICE",
        ),
        (
            "third-party/themes/pierre-theme-LICENSE",
            "third_party/themes/pierre-theme-LICENSE",
        ),
        (
            "third-party/themes/pierre-theme-NOTICE.md",
            "third_party/themes/pierre-theme-NOTICE.md",
        ),
        (
            "third-party/grammars/shikijs-langs-LICENSE",
            "third_party/grammars/shikijs-langs-LICENSE",
        ),
    ] {
        entries.push((
            format!("{root}/{archive_name}"),
            fs::read(repo.join(source)).with_context(|| format!("read {source}"))?,
            0o644,
        ));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries)
}

fn attach_release_provenance(
    entries: &mut Vec<(String, Vec<u8>, u32)>,
    root: &str,
    executable_name: &str,
    bytes: Vec<u8>,
) -> Result<()> {
    if bytes.len() > 1024 * 1024 {
        bail!("provenance input exceeds 1 MiB");
    }
    let binary_name = format!("{root}/{executable_name}");
    let binary = entries
        .iter()
        .find(|entry| entry.0 == binary_name)
        .context("release entries lack binary")?;
    let digest = format!("{:x}", Sha256::digest(&binary.1));
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let statement = if value.get("mediaType").is_some() {
        provenance::decode_bundle(&bytes)?
    } else {
        bytes.clone()
    };
    provenance::check_binary_subject(&statement, executable_name, &digest)?;
    let name = format!("{root}/provenance.json");
    let bundle_name = format!("{root}/provenance.sigstore.json");
    if entries
        .iter()
        .any(|entry| entry.0 == name || entry.0 == bundle_name)
    {
        bail!("release entries already contain provenance");
    }
    if value.get("mediaType").is_some() {
        entries.push((bundle_name, bytes, 0o644));
    }
    entries.push((name, statement, 0o644));
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(())
}

fn write_tar_archive(path: &Path, entries: &[(String, Vec<u8>, u32)]) -> Result<()> {
    let writer = BufWriter::new(File::create(path)?);
    let encoder = GzEncoder::new(writer, Compression::best());
    let mut archive = tar::Builder::new(encoder);
    archive.mode(tar::HeaderMode::Deterministic);
    for (name, bytes, mode) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(*mode);
        header.set_mtime(0);
        header.set_cksum();
        archive.append_data(&mut header, name, bytes.as_slice())?;
    }
    archive.into_inner()?.finish()?.flush()?;
    Ok(())
}

fn write_zip_archive(path: &Path, entries: &[(String, Vec<u8>, u32)]) -> Result<()> {
    let file = File::create(path)?;
    let mut archive = zip::ZipWriter::new(file);
    for (name, bytes, mode) in entries {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(*mode);
        archive.start_file(name, options)?;
        archive.write_all(bytes)?;
    }
    archive.finish()?;
    Ok(())
}

fn cyclonedx_sbom(inventory: &LicenseInventory) -> Vec<u8> {
    let components = inventory
        .packages
        .iter()
        .map(|package| {
            serde_json::json!({
                "type": "library",
                "name": package.name,
                "version": package.version,
                "purl": format!("pkg:cargo/{}@{}", package.name, package.version),
                "licenses": package.license.as_ref().map(|license| vec![serde_json::json!({ "expression": license })]).unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();
    let mut encoded = serde_json::to_vec_pretty(&serde_json::json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": { "component": { "type": "application", "name": "workdeck" } },
        "components": components,
    }))
    .expect("SBOM JSON is serializable");
    encoded.push(b'\n');
    encoded
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify() -> Result<()> {
    let repo = repo_root()?;
    verify_vendored_themes()?;
    skill::check(&repo)?;
    architecture::check(&repo)?;
    changelog::run(
        &repo,
        ["upstream-history".into(), "--check".into()].into_iter(),
    )?;
    run_checked(&repo, "cargo", &["fmt", "--all", "--check"])?;
    run_checked(
        &repo,
        "cargo",
        &["test", "--locked", "--workspace", "--all-targets"],
    )?;
    run_checked(
        &repo,
        "cargo",
        &[
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run_checked(
        &repo,
        "cargo",
        &[
            "build",
            "--locked",
            "--release",
            "--package",
            "workdeck-cli",
            "--bin",
            "workdeck",
        ],
    )?;

    let scratch = tempfile::tempdir().context("create verification directory")?;
    let fixture = scratch.path().join("repository");
    fs::create_dir_all(fixture.join("src"))?;
    fs::create_dir_all(fixture.join("resources/js/pages"))?;
    run_checked(&fixture, "git", &["init", "-q"])?;
    run_checked(
        &fixture,
        "git",
        &["config", "user.email", "workdeck@example.test"],
    )?;
    run_checked(&fixture, "git", &["config", "user.name", "Workdeck Test"])?;
    for index in 1..=600 {
        fs::write(
            fixture.join(format!("src/file_{index}.rs")),
            format!("line {index}\n"),
        )?;
    }
    run_checked(&fixture, "git", &["add", "."])?;
    run_checked(&fixture, "git", &["commit", "-qm", "initial"])?;
    for index in 1..=200 {
        fs::write(
            fixture.join(format!("src/file_{index}.rs")),
            format!("line {index}\nchanged\n"),
        )?;
    }
    for index in 1..=100 {
        fs::write(
            fixture.join(format!("resources/js/pages/page_{index}.vue")),
            format!("new {index}\n"),
        )?;
    }
    let binary = repo.join("target/release").join(if cfg!(windows) {
        "workdeck.exe"
    } else {
        "workdeck"
    });
    let help = Command::new(&binary)
        .arg("--help")
        .env_remove("HOME")
        .output()
        .context("run installed-style help smoke")?;
    if !help.status.success() {
        bail!("release binary help smoke failed");
    }
    let status = Command::new(&binary)
        .args(["--cwd", fixture.to_string_lossy().as_ref(), "--status-json"])
        .env_remove("HOME")
        .output()
        .context("run large repository smoke")?;
    if !status.status.success() {
        bail!(
            "large repository smoke failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }
    let payload: serde_json::Value = serde_json::from_slice(&status.stdout)?;
    let changes = payload
        .get("data")
        .unwrap_or(&payload)
        .get("changes")
        .and_then(serde_json::Value::as_array)
        .context("status smoke did not contain a changes array")?;
    if changes.len() != 300 {
        bail!(
            "large repository smoke returned {} changes, expected 300",
            changes.len()
        );
    }
    println!("Workdeck Rust verification and large-repository smoke passed.");
    Ok(())
}

fn run_checked(cwd: &Path, program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .status()
        .with_context(|| format!("run {program}"))?;
    if !status.success() {
        bail!("{program} {} failed with {status}", args.join(" "));
    }
    Ok(())
}

fn parse_options(mut args: impl Iterator<Item = String>) -> Result<Options> {
    let mut options = Options::default();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--baseline" => options.baseline = Some(required_value(&mut args, "--baseline")?),
            "--ledger" => {
                options.ledger = Some(PathBuf::from(required_value(&mut args, "--ledger")?));
            }
            "--metadata" => {
                options.metadata = Some(PathBuf::from(required_value(&mut args, "--metadata")?));
            }
            "--allow-incomplete" => options.allow_incomplete = true,
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            _ => bail!("unknown option {argument:?}"),
        }
    }
    Ok(options)
}

fn required_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String> {
    args.next()
        .with_context(|| format!("{option} requires a value"))
}

fn inventory(options: Options) -> Result<()> {
    let repo = repo_root()?;
    let baseline_ref = options.baseline.as_deref().unwrap_or(DEFAULT_BASELINE);
    let baseline = resolve_commit(&repo, baseline_ref)?;
    let stable = resolve_commit(&repo, DEFAULT_STABLE)?;
    let tree = resolve_tree(&repo, &baseline)?;
    let entries = read_tree(&repo, &baseline)?;
    let ledger_path = repo.join(options.ledger.unwrap_or_else(|| DEFAULT_LEDGER.into()));
    let metadata_path = repo.join(options.metadata.unwrap_or_else(|| DEFAULT_METADATA.into()));
    let _ledger_lock = lock_ledger_for_write(&ledger_path)?;

    if ledger_path.exists() {
        bail!(
            "{} already exists; inventory never overwrites an audited ledger",
            ledger_path.display()
        );
    }

    if let Some(parent) = ledger_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create inventory directory {}", parent.display()))?;
    }

    let mut blob_metadata = HashMap::<String, (u64, bool)>::new();
    let mut records = Vec::with_capacity(entries.len());

    for entry in &entries {
        let (line_count, binary) = match blob_metadata.get(&entry.blob) {
            Some(metadata) => *metadata,
            None => {
                let contents = git_stdout_bytes(&repo, ["cat-file", "blob", entry.blob.as_str()])?;
                let metadata = (source_line_count(&contents), contents.contains(&0));
                blob_metadata.insert(entry.blob.clone(), metadata);
                metadata
            }
        };
        let record = LedgerRecord {
            id: format!(
                "{}:{}:0-{}",
                short_commit(&baseline),
                entry.path,
                entry.bytes
            ),
            baseline: baseline.clone(),
            path: entry.path.clone(),
            blob: entry.blob.clone(),
            byte_start: 0,
            byte_end: entry.bytes,
            line_start: if entry.bytes == 0 { 0 } else { 1 },
            line_end: line_count,
            classification: classify(&entry.path, binary).to_owned(),
            disposition: "unmapped".to_owned(),
            destinations: Vec::new(),
            evidence: Vec::new(),
            provenance: vec![baseline.clone()],
        };
        records.push(record);
    }
    write_ledger_atomic(&ledger_path, &records)?;

    let metadata = BaselineMetadata {
        schema_version: 1,
        baseline,
        stable,
        tree,
        file_count: entries.len(),
        byte_count: entries.iter().map(|entry| entry.bytes).sum(),
        ledger: relative_to(&repo, &ledger_path),
    };
    let mut encoded = serde_json::to_string_pretty(&metadata).context("serialize metadata")?;
    encoded.push('\n');
    fs::write(&metadata_path, encoded)
        .with_context(|| format!("write {}", metadata_path.display()))?;

    println!(
        "inventoried {} files ({} bytes) from {}",
        metadata.file_count, metadata.byte_count, metadata.baseline
    );
    println!("ledger: {}", relative_to(&repo, &ledger_path));
    Ok(())
}

/// Recompute classifications from the pinned blobs without changing port dispositions.
///
/// This is intentionally separate from `inventory`: the inventory is immutable after creation,
/// while classification rules can become stricter as ambiguous upstream path shapes are found.
fn reclassify(options: Options) -> Result<()> {
    let repo = repo_root()?;
    let ledger_path = repo.join(options.ledger.unwrap_or_else(|| DEFAULT_LEDGER.into()));
    let _ledger_lock = lock_ledger_for_write(&ledger_path)?;
    let mut records = read_ledger(&ledger_path)?;
    if records.is_empty() {
        bail!("ledger is empty: {}", ledger_path.display());
    }
    let baseline = options
        .baseline
        .as_deref()
        .map(|value| resolve_commit(&repo, value))
        .transpose()?
        .unwrap_or_else(|| records[0].baseline.clone());
    let entries = read_tree(&repo, &baseline)?;
    let expected = entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let binary_by_blob = binary_blob_flags(&repo, entries.iter().map(|entry| entry.blob.as_str()))?;
    let mut changed = 0;

    for record in &mut records {
        if record.baseline != baseline {
            bail!(
                "record {} uses baseline {}, expected {}",
                record.id,
                record.baseline,
                baseline
            );
        }
        let entry = expected
            .get(record.path.as_str())
            .with_context(|| format!("ledger path absent from baseline: {}", record.path))?;
        if record.blob != entry.blob {
            bail!(
                "{} has blob {}, expected {}",
                record.id,
                record.blob,
                entry.blob
            );
        }
        let binary = *binary_by_blob
            .get(&entry.blob)
            .with_context(|| format!("missing binary metadata for blob {}", entry.blob))?;
        let classification = classify(&record.path, binary);
        if record.classification != classification {
            record.classification = classification.to_owned();
            changed += 1;
        }
    }

    write_ledger_atomic(&ledger_path, &records)?;
    println!("reclassified {changed} ledger records from pinned baseline blobs");
    Ok(())
}

fn audit(options: Options, strict: bool) -> Result<()> {
    let repo = repo_root()?;
    let ledger_path = repo.join(options.ledger.unwrap_or_else(|| DEFAULT_LEDGER.into()));
    let records = read_ledger(&ledger_path)?;
    if records.is_empty() {
        bail!("ledger is empty: {}", ledger_path.display());
    }

    let requested_baseline = options
        .baseline
        .as_deref()
        .map(|value| resolve_commit(&repo, value))
        .transpose()?;
    let baseline = requested_baseline.unwrap_or_else(|| records[0].baseline.clone());
    let entries = read_tree(&repo, &baseline)?;
    let expected = entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut by_path = BTreeMap::<&str, Vec<&LedgerRecord>>::new();
    let mut disposition_counts = BTreeMap::<&str, usize>::new();
    let binary_by_blob = binary_blob_flags(&repo, entries.iter().map(|entry| entry.blob.as_str()))?;

    for record in &records {
        if record.baseline != baseline {
            bail!(
                "record {} uses baseline {}, expected {}",
                record.id,
                record.baseline,
                baseline
            );
        }
        by_path.entry(&record.path).or_default().push(record);
        *disposition_counts.entry(&record.disposition).or_default() += 1;
    }

    for (path, entry) in &expected {
        let binary = binary_by_blob
            .get(&entry.blob)
            .with_context(|| format!("missing binary metadata for blob {}", entry.blob))?;
        let expected_classification = classify(path, *binary);
        let mut path_records = by_path
            .remove(path)
            .with_context(|| format!("missing ledger coverage for {path}"))?;
        path_records.sort_by_key(|record| record.byte_start);
        let mut cursor = 0;
        for record in path_records {
            if record.classification != expected_classification {
                bail!(
                    "{} is classified as {}, expected {}",
                    record.id,
                    record.classification,
                    expected_classification
                );
            }
            if record.blob != entry.blob {
                bail!(
                    "{} has blob {}, expected {}",
                    record.id,
                    record.blob,
                    entry.blob
                );
            }
            if record.byte_start != cursor {
                bail!(
                    "{} has a byte gap or overlap at {}; expected {}",
                    record.id,
                    record.byte_start,
                    cursor
                );
            }
            if record.byte_end < record.byte_start || record.byte_end > entry.bytes {
                bail!("{} has invalid byte bounds", record.id);
            }
            validate_disposition(&repo, record)?;
            validate_test_evidence(&repo, record)?;
            cursor = record.byte_end;
        }
        if cursor != entry.bytes {
            bail!("{path} coverage ends at {cursor}, expected {}", entry.bytes);
        }
    }

    if let Some((path, _)) = by_path.first_key_value() {
        bail!("ledger contains path absent from baseline: {path}");
    }

    let unmapped = disposition_counts.get("unmapped").copied().unwrap_or(0);
    let stable_fixes = validate_stable_fixes(&repo)?;
    let upstream_delta = upstream_delta_commits(&repo, &baseline)?;
    println!("Hunk semantic-port ledger");
    println!("  baseline: {baseline}");
    println!("  files: {}", entries.len());
    println!("  records: {}", records.len());
    for (disposition, count) in disposition_counts {
        println!("  {disposition}: {count}");
    }
    println!("  stable-only commits: {stable_fixes}");
    match &upstream_delta {
        Some(commits) => {
            println!("  upstream delta commits: {}", commits.len());
            for commit in commits {
                println!("    pending upstream: {commit}");
            }
        }
        None => println!("  upstream delta commits: unknown (fetch hunk-upstream)"),
    }

    if strict && unmapped > 0 && !options.allow_incomplete {
        bail!("{unmapped} ledger records remain unmapped");
    }
    if strict {
        validate_upstream_delta(upstream_delta.as_deref())?;
    }
    Ok(())
}

fn validate_stable_fixes(repo: &Path) -> Result<usize> {
    let path = repo.join(DEFAULT_STABLE_FIXES);
    let reader =
        BufReader::new(File::open(&path).with_context(|| format!("open {}", path.display()))?);
    let records = reader
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let line = line?;
            serde_json::from_str::<StableFixRecord>(&line)
                .with_context(|| format!("parse {} line {}", path.display(), index + 1))
        })
        .collect::<Result<Vec<_>>>()?;
    if records.len() != 5 {
        bail!(
            "stable-fix ledger has {} records, expected 5",
            records.len()
        );
    }
    for record in &records {
        if !matches!(
            record.disposition.as_str(),
            "rust-reimplementation" | "branding-adapted"
        ) {
            bail!("stable fix {} has invalid disposition", record.commit);
        }
        let object = format!("{}^{{commit}}", record.commit);
        resolve_commit(repo, &object)
            .with_context(|| format!("missing stable fix commit {}", record.commit))?;
        if record.destinations.is_empty() || record.evidence.is_empty() {
            bail!(
                "stable fix {} lacks destinations or evidence",
                record.commit
            );
        }
        for item in record.destinations.iter().chain(&record.evidence) {
            let item = item.split_once('#').map_or(item.as_str(), |(path, _)| path);
            if !repo.join(item).exists() {
                bail!(
                    "stable fix {} references missing path {item}",
                    record.commit
                );
            }
        }
    }
    Ok(records.len())
}

fn validate_upstream_delta(commits: Option<&[String]>) -> Result<()> {
    match commits {
        None => bail!("Hunk upstream state is unknown; fetch hunk-upstream before strict audit"),
        Some(commits) if !commits.is_empty() => bail!("the Hunk upstream-delta queue is not empty"),
        Some(_) => Ok(()),
    }
}

fn upstream_delta_commits(repo: &Path, baseline: &str) -> Result<Option<Vec<String>>> {
    let upstream = "refs/remotes/hunk-upstream/main";
    let probe = git_output(repo, ["rev-parse", "--verify", upstream])?;
    if !probe.status.success() {
        return Ok(None);
    }
    let ancestry = git_output(repo, ["merge-base", "--is-ancestor", baseline, upstream])?;
    if !ancestry.status.success() {
        bail!(
            "tracked Hunk upstream must descend from the pinned baseline; refusing a truncated or unrelated delta"
        );
    }
    let range = format!("{baseline}..{upstream}");
    let output = git_stdout(repo, ["rev-list", "--reverse", "--topo-order", &range])?;
    Ok(Some(output.lines().map(str::to_owned).collect()))
}

fn validate_disposition(repo: &Path, record: &LedgerRecord) -> Result<()> {
    const VALID: &[&str] = &[
        "unmapped",
        "rust-reimplementation",
        "translated-test",
        "migrated-content",
        "retained-asset",
        "rust-generated-replacement",
        "license-retained",
    ];
    if !VALID.contains(&record.disposition.as_str()) {
        bail!(
            "{} has unknown disposition {}",
            record.id,
            record.disposition
        );
    }
    if record.disposition == "unmapped" {
        if !record.destinations.is_empty() || !record.evidence.is_empty() {
            bail!(
                "{} is unmapped but already names destinations/evidence",
                record.id
            );
        }
        return Ok(());
    }
    if record.destinations.is_empty() || record.evidence.is_empty() {
        bail!("{} is mapped without destinations and evidence", record.id);
    }
    let compatible = match record.classification.as_str() {
        "license" => matches!(record.disposition.as_str(), "license-retained"),
        "asset" => matches!(
            record.disposition.as_str(),
            "retained-asset" | "rust-generated-replacement"
        ),
        "documentation" => matches!(
            record.disposition.as_str(),
            "migrated-content" | "rust-generated-replacement"
        ),
        "test" => {
            matches!(
                record.disposition.as_str(),
                "translated-test" | "rust-generated-replacement"
            ) || (record.disposition == "retained-asset"
                && !has_extension(&record.path, SOURCE_EXTENSIONS))
        }
        "source" | "tooling" => matches!(
            record.disposition.as_str(),
            "rust-reimplementation" | "rust-generated-replacement"
        ),
        "configuration" => matches!(
            record.disposition.as_str(),
            "rust-reimplementation" | "rust-generated-replacement" | "migrated-content"
        ),
        other => bail!("{} has unknown classification {other}", record.id),
    };
    if !compatible {
        bail!(
            "{} classification {} is incompatible with disposition {}",
            record.id,
            record.classification,
            record.disposition
        );
    }
    for item in record.destinations.iter().chain(&record.evidence) {
        let path = item.split_once('#').map_or(item.as_str(), |(path, _)| path);
        if !repo.join(path).exists() {
            bail!("{} references missing repository path {path}", record.id);
        }
    }
    Ok(())
}

fn validate_test_evidence(repo: &Path, record: &LedgerRecord) -> Result<()> {
    if record.classification != "test" || record.disposition == "unmapped" {
        return Ok(());
    }
    for item in &record.evidence {
        let path = item.split_once('#').map_or(item.as_str(), |(path, _)| path);
        if !path.ends_with(".rs") {
            continue;
        }
        let source = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read test evidence {path}"))?;
        if source.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("#[test]")
                || line.starts_with("#[test(")
                || line.starts_with("#[rstest")
                || (line.starts_with("#[") && line.contains("::test"))
                || line.starts_with("proptest!")
        }) {
            return Ok(());
        }
    }
    bail!(
        "{} is a mapped Hunk test without executable Rust test evidence",
        record.id
    )
}

fn read_ledger(path: &Path) -> Result<Vec<LedgerRecord>> {
    let reader =
        BufReader::new(File::open(path).with_context(|| format!("open {}", path.display()))?);
    reader
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let line =
                line.with_context(|| format!("read {} line {}", path.display(), index + 1))?;
            serde_json::from_str(&line)
                .with_context(|| format!("parse {} line {}", path.display(), index + 1))
        })
        .collect()
}

/// Keep a stable lock inode separate from the atomically replaced ledger inode.
/// Do not unlink this sidecar: doing so lets another writer lock a different inode.
#[derive(Debug)]
struct LedgerWriteGuard(File);

impl Drop for LedgerWriteGuard {
    fn drop(&mut self) {
        // Release explicitly: a concurrent fork may briefly retain the open file description
        // before exec closes its inherited descriptor. Closing only our descriptor can then
        // leave the advisory lock held after the writer has finished.
        let _ = self.0.unlock();
    }
}

fn lock_ledger_for_write(path: &Path) -> Result<LedgerWriteGuard> {
    let parent = path
        .parent()
        .context("ledger requires a parent directory")?;
    fs::create_dir_all(parent)?;
    let lock_path = path.with_extension("jsonl.lock");
    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    lock.try_lock().with_context(|| {
        format!(
            "another port command is editing {}; retry after it finishes",
            path.display()
        )
    })?;
    Ok(LedgerWriteGuard(lock))
}

fn write_ledger_atomic(path: &Path, records: &[LedgerRecord]) -> Result<()> {
    let parent = path
        .parent()
        .context("ledger requires a parent directory")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).context("create unique ledger temporary file")?;
    if let Ok(metadata) = fs::metadata(path) {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())?;
    }
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        for record in records {
            serde_json::to_writer(&mut writer, record).context("serialize ledger record")?;
            writer.write_all(b"\n").context("write ledger record")?;
        }
        writer.flush().context("flush ledger")?;
    }
    temporary
        .as_file()
        .sync_all()
        .context("sync ledger contents")?;
    temporary
        .persist(path)
        .with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}

fn binary_blob_flags<'a>(
    repo: &Path,
    blobs: impl IntoIterator<Item = &'a str>,
) -> Result<HashMap<String, bool>> {
    let blobs = blobs
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let mut child = Command::new("git")
        .args(["cat-file", "--batch"])
        .current_dir(repo)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .context("start git cat-file --batch")?;
    let mut stdin = BufWriter::new(child.stdin.take().context("open git cat-file stdin")?);
    let mut stdout = BufReader::new(child.stdout.take().context("open git cat-file stdout")?);
    let mut flags = HashMap::with_capacity(blobs.len());

    for blob in blobs {
        writeln!(stdin, "{blob}").context("query git cat-file batch")?;
        stdin.flush().context("flush git cat-file query")?;

        let mut header = String::new();
        stdout
            .read_line(&mut header)
            .context("read git cat-file header")?;
        let mut fields = header.split_whitespace();
        let actual = fields.next().context("missing git cat-file object id")?;
        let kind = fields.next().context("missing git cat-file object type")?;
        let size = fields
            .next()
            .context("missing git cat-file object size")?
            .parse::<usize>()
            .context("invalid git cat-file object size")?;
        if actual != blob || kind != "blob" {
            bail!("git cat-file returned {actual} {kind} for blob {blob}");
        }

        let mut binary = false;
        let mut remaining = size;
        let mut buffer = [0_u8; 8192];
        while remaining > 0 {
            let length = remaining.min(buffer.len());
            stdout
                .read_exact(&mut buffer[..length])
                .context("read git cat-file blob")?;
            binary |= buffer[..length].contains(&0);
            remaining -= length;
        }
        let mut delimiter = [0_u8; 1];
        stdout
            .read_exact(&mut delimiter)
            .context("read git cat-file blob delimiter")?;
        if delimiter[0] != b'\n' {
            bail!("git cat-file returned an invalid blob delimiter");
        }
        flags.insert(blob, binary);
    }

    drop(stdin);
    let status = child.wait().context("wait for git cat-file --batch")?;
    if !status.success() {
        bail!("git cat-file --batch failed with {status}");
    }
    Ok(flags)
}

fn read_tree(repo: &Path, commit: &str) -> Result<Vec<TreeEntry>> {
    let output = git_output(repo, ["ls-tree", "-r", "-z", "-l", commit])?;
    if !output.status.success() {
        bail!(
            "git ls-tree failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut entries = Vec::new();
    for raw in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let text = std::str::from_utf8(raw).context("Hunk tree contains a non-UTF-8 path")?;
        let (metadata, path) = text
            .split_once('\t')
            .context("invalid git ls-tree record")?;
        let mut fields = metadata.split_whitespace();
        let _mode = fields.next().context("missing tree mode")?;
        let kind = fields.next().context("missing tree kind")?;
        let blob = fields.next().context("missing blob id")?;
        let bytes = fields.next().context("missing blob size")?;
        if kind != "blob" {
            continue;
        }
        entries.push(TreeEntry {
            path: path.to_owned(),
            blob: blob.to_owned(),
            bytes: bytes.parse().context("invalid blob size")?,
        });
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn repo_root() -> Result<PathBuf> {
    let output = git_output(Path::new("."), ["rev-parse", "--show-toplevel"])?;
    if !output.status.success() {
        bail!("run xtask inside a Git worktree");
    }
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}

fn resolve_commit(repo: &Path, reference: &str) -> Result<String> {
    git_stdout(repo, ["rev-parse", "--verify", reference])
}

fn resolve_tree(repo: &Path, commit: &str) -> Result<String> {
    git_stdout(repo, ["rev-parse", &format!("{commit}^{{tree}}")])
}

fn git_stdout<const N: usize>(repo: &Path, args: [&str; N]) -> Result<String> {
    let output = git_output(repo, args)?;
    if !output.status.success() {
        bail!("git failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn git_stdout_bytes<const N: usize>(repo: &Path, args: [&str; N]) -> Result<Vec<u8>> {
    let output = git_output(repo, args)?;
    if !output.status.success() {
        bail!("git failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(output.stdout)
}

fn git_output<const N: usize>(repo: &Path, args: [&str; N]) -> Result<Output> {
    Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .context("run git")
}

fn source_line_count(contents: &[u8]) -> u64 {
    if contents.is_empty() {
        return 0;
    }
    contents.iter().filter(|byte| **byte == b'\n').count() as u64
        + u64::from(!contents.ends_with(b"\n"))
}

fn classify(path: &str, binary: bool) -> &'static str {
    let lower = path.to_ascii_lowercase();
    let components = lower.split('/').collect::<Vec<_>>();
    let file_name = components.last().copied().unwrap_or_default();
    let license_stem = file_name
        .strip_suffix(".md")
        .or_else(|| file_name.strip_suffix(".txt"))
        .unwrap_or(file_name);
    let is_license = matches!(
        license_stem,
        "license" | "copying" | "copyright" | "notice" | "third_party_notices"
    ) || license_stem.starts_with("license-");
    let is_explicit_test = file_name.contains(".test.")
        || file_name.contains(".spec.")
        || file_name.ends_with(".test")
        || file_name.ends_with(".spec")
        || file_name.ends_with(".snap")
        || components.iter().any(|component| {
            matches!(
                *component,
                "test"
                    | "tests"
                    | "__tests__"
                    | "fixture"
                    | "fixtures"
                    | "__fixtures__"
                    | "snapshot"
                    | "snapshots"
                    | "__snapshots__"
            )
        });
    let is_documentation =
        lower.starts_with("docs/") || lower.ends_with(".md") || lower.ends_with(".mdx");
    let is_tooling = lower.starts_with("scripts/")
        || lower.starts_with("benchmarks/")
        || lower.starts_with(".github/")
        || lower.starts_with("bin/");
    let is_source = has_extension(&lower, SOURCE_EXTENSIONS);
    let is_asset = has_extension(&lower, ASSET_EXTENSIONS);

    if is_license {
        "license"
    } else if is_explicit_test {
        "test"
    } else if is_documentation {
        "documentation"
    } else if is_tooling {
        "tooling"
    } else if is_source {
        "source"
    } else if is_asset || binary {
        "asset"
    } else {
        "configuration"
    }
}

fn has_extension(path: &str, extensions: &[&str]) -> bool {
    extensions.iter().any(|extension| path.ends_with(extension))
}

fn short_commit(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

fn relative_to(repo: &Path, path: &Path) -> String {
    path.strip_prefix(repo)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn print_help() {
    println!("cargo xtask benchmark aggregate SOURCE METRIC SAMPLES_JSON");
    println!("cargo xtask benchmark parse-metrics < METRIC_OUTPUT");
    println!("cargo xtask benchmark run --script WORKLOAD [--samples N] [--out PATH]");
    println!(
        "cargo xtask benchmark runner-plan [--samples N] [--out PATH] [--script NAME] [--include-huge] [--include-competitors]"
    );
    println!("cargo xtask benchmark working-tree");
    println!("cargo xtask benchmark bootstrap-load");
    println!("cargo xtask benchmark changeset-parse");
    println!("cargo xtask benchmark highlight-prefetch");
    println!("cargo xtask benchmark large-stream");
    println!("cargo xtask benchmark memory-snapshot");
    println!("cargo xtask benchmark interaction-diagnostic");
    println!("cargo xtask ci-changes <base-revision> <head-revision>");
    println!("cargo xtask ci-host <expected-native-target>");
    println!("cargo xtask benchmark non-ascii-stream");
    println!("cargo xtask benchmark wrapped-cjk");
    println!("cargo xtask benchmark render-layout");
    println!("cargo xtask benchmark stream-fixture [--huge | --non-ascii]");
    println!("cargo xtask benchmark synthetic-patch OPTIONS_JSON");
    println!("cargo xtask benchmark compare-json BASE_JSON HEAD_JSON");
    println!("cargo xtask benchmark compare-markdown BASE_JSON HEAD_JSON");
    println!(
        "cargo xtask benchmark compare-release [--version VERSION] [--release-dir DIR] [--base PATH] [--head PATH] [--out PATH] [--summary PATH]"
    );
    println!("cargo xtask benchmark previous VERSION RELEASE_DIRECTORY");
    println!("cargo xtask benchmark release-plan [--version VERSION] [--samples N] [--out PATH]");
    println!(
        "cargo xtask port <fetch|inventory|reclassify|map|materialize-assets|audit|status> [port options]"
    );
    println!(
        "cargo xtask themes <vendor --shiki-archive FILE --tm-themes-archive FILE --pierre-archive FILE|verify>"
    );
    println!("cargo xtask licenses [--output PATH]");
    println!("cargo xtask verify");
    println!("cargo xtask architecture check");
    println!("cargo xtask nix check");
    println!("cargo xtask skill <generate|check>");
    println!(
        "cargo xtask extension stage-example <cli-tools|pane-layout|vim-navigation|review-snapshot-export|review-note-navigator|rendered-markdown|jsx-file-view|inline-edit|review-triage|github-pr|file-view-gallery|native-vcs|startup-lifecycle>"
    );
    println!("cargo xtask site <build|check|serve>");
    println!("cargo xtask install-plan [version] [--no-modify-path] [-f|--force]");
    println!("cargo xtask install-verify ARCHIVE CHECKSUM_FILE");
    println!("cargo xtask install-inspect ARCHIVE [--package]");
    println!("cargo xtask changelog upstream-history [--check]");
    println!(
        "cargo xtask release channel --event EVENT --ref REF [--requested-tag CHANNEL] [--current-latest VERSION]"
    );
    println!("cargo xtask release check-version TAG");
    println!("cargo xtask release validate-prerelease");
    println!("cargo xtask release status --since=REVISION");
    println!("cargo xtask release verify-pr-notes BASE_REVISION [HEAD_REVISION]");
    println!(
        "cargo xtask media plan --storyboard FILE --output FILE [--fps N] [--caption-animation-seconds N]"
    );
    println!(
        "cargo xtask media compose --storyboard FILE --work-dir DIR --font FILE [--frames-dir DIR] [--stage FILE] [--webdriver FILE] [--chromium FILE]"
    );
    println!("cargo xtask media capture --script FILE [--scenes NAME,NAME]");
    println!(
        "cargo xtask media launch capture --font FILE [--binary FILE] [--work-dir DIR] [--scenes NAME,NAME]"
    );
    println!(
        "cargo xtask media launch compose [--work-dir DIR] [--font FILE] [--webdriver FILE] [--chromium FILE]"
    );
    println!(
        "cargo xtask media launch encode [--work-dir DIR] [--ffmpeg FILE] [--mp4 FILE] [--webm FILE]"
    );
    println!(
        "cargo xtask release package --target TRIPLE --provenance STATEMENT [--verify-ci] [--binary PATH] [--output DIR]"
    );
    println!("cargo xtask release provenance-check BINARY STATEMENT");
    println!(
        "cargo xtask release provenance-verify BINARY BUNDLE OWNER/REPO SOURCE_COMMIT refs/tags/TAG"
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn upstream_discovery_preserves_merge_order_and_rejects_rewound_refs() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=Port Test",
                    "-c",
                    "user.email=port-test@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        git(&["init", "--quiet"]);
        let tree = git(&["mktree"]);
        let baseline = git(&["commit-tree", &tree, "-m", "baseline"]);
        assert!(
            super::upstream_delta_commits(repo, &baseline)
                .unwrap()
                .is_none()
        );
        let upstream = "refs/remotes/hunk-upstream/main";
        git(&["update-ref", upstream, &baseline]);
        assert_eq!(
            super::upstream_delta_commits(repo, &baseline).unwrap(),
            Some(vec![])
        );
        let left = git(&["commit-tree", &tree, "-p", &baseline, "-m", "left"]);
        let right = git(&["commit-tree", &tree, "-p", &baseline, "-m", "right"]);
        let merge = git(&[
            "commit-tree",
            &tree,
            "-p",
            &left,
            "-p",
            &right,
            "-m",
            "merge",
        ]);
        let tip = git(&["commit-tree", &tree, "-p", &merge, "-m", "tip"]);
        git(&["update-ref", upstream, &tip]);
        let queue = super::upstream_delta_commits(repo, &baseline)
            .unwrap()
            .unwrap();
        assert_eq!(queue.len(), 4);
        let position = |id: &str| queue.iter().position(|value| value == id).unwrap();
        assert!(position(&left) < position(&merge));
        assert!(position(&right) < position(&merge));
        assert!(position(&merge) < position(&tip));
        git(&["update-ref", upstream, &baseline]);
        assert!(super::upstream_delta_commits(repo, &left).is_err());
        let unrelated = git(&["commit-tree", &tree, "-m", "unrelated"]);
        git(&["update-ref", upstream, &unrelated]);
        assert!(super::upstream_delta_commits(repo, &baseline).is_err());
    }

    #[test]
    fn strict_upstream_gate_rejects_unknown_and_pending_state() {
        assert!(super::validate_upstream_delta(None).is_err());
        assert!(super::validate_upstream_delta(Some(&["unported-commit".into()])).is_err());
        assert!(super::validate_upstream_delta(Some(&[])).is_ok());
    }
    use super::{
        LedgerRecord, classify, lock_ledger_for_write, parse_map_options, read_ledger,
        release_entries, source_line_count, validate_test_evidence, write_ledger_atomic,
    };
    use std::collections::BTreeSet;
    use std::fs;

    #[test]
    fn counts_source_lines_without_inventing_an_empty_line() {
        assert_eq!(source_line_count(b""), 0);
        assert_eq!(source_line_count(b"one"), 1);
        assert_eq!(source_line_count(b"one\ntwo\n"), 2);
    }

    #[test]
    fn classifies_port_surfaces() {
        assert_eq!(classify("src/ui/App.tsx", false), "source");
        assert_eq!(classify("src/ui/App.test.tsx", false), "test");
        assert_eq!(classify("docs/extensions.md", false), "documentation");
        assert_eq!(classify("website/public/demo.webp", true), "asset");
        assert_eq!(classify("LICENSE", false), "license");
        assert_eq!(classify("third_party/LICENSE-MIT", false), "license");
        assert_eq!(
            classify("src/core/process/startupNotice.ts", false),
            "source"
        );
        assert_eq!(
            classify("src/core/install/latestRelease.ts", false),
            "source"
        );
        assert_eq!(
            classify("src/extensions/reviewSnapshot.ts", false),
            "source"
        );
        assert_eq!(classify("src/ui/lib/stml/layout.ts", true), "source");
        assert_eq!(
            classify(".changeset/fix-prerelease-version-test.md", false),
            "documentation"
        );
        assert_eq!(
            classify("examples/gallery/fixtures/after/README.md", false),
            "test"
        );
        assert_eq!(classify("scripts/build-bin.test.ts", false), "test");
        assert_eq!(
            classify("scripts/test-session-broker-node.ts", false),
            "tooling"
        );
        assert_eq!(classify("benchmarks/lib/fixtures.ts", false), "tooling");
        assert_eq!(classify("website/src/styles/site.css", false), "source");
    }

    #[test]
    fn parses_exact_ledger_record_selectors_for_split_source_files() {
        let options = parse_map_options(
            [
                "--id",
                "baseline:path:0-10",
                "--disposition",
                "rust-reimplementation",
                "--destination",
                "crates/example/src/lib.rs",
                "--evidence",
                "crates/example/src/lib.rs",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(options.ids, ["baseline:path:0-10"]);
        assert!(options.paths.is_empty());
        assert!(options.prefixes.is_empty());
    }

    #[test]
    fn mapping_still_requires_at_least_one_selector() {
        let error = parse_map_options(
            [
                "--disposition",
                "translated-test",
                "--destination",
                "tests/parity.rs",
                "--evidence",
                "tests/parity.rs",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap_err();
        assert!(error.to_string().contains("--id, --path, or --prefix"));
    }

    #[test]
    fn ledger_write_lock_excludes_competing_writers_and_releases_on_drop() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let first = lock_ledger_for_write(&path).unwrap();
        let error = lock_ledger_for_write(&path).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("another port command is editing")
        );
        assert!(
            !path.exists(),
            "locking must not initialize or truncate the ledger"
        );
        drop(first);
        let second = lock_ledger_for_write(&path).unwrap();
        assert!(path.with_extension("jsonl.lock").exists());
        drop(second);
    }

    #[test]
    fn ledger_guard_releases_lock_even_while_a_duplicate_descriptor_survives() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let guard = lock_ledger_for_write(&path).unwrap();
        let duplicate = guard.0.try_clone().unwrap();
        assert!(lock_ledger_for_write(&path).is_err());
        drop(guard);
        let next = lock_ledger_for_write(&path).unwrap();
        drop(next);
        drop(duplicate);
    }

    #[test]
    fn atomic_ledger_replacement_has_no_shared_temporary_or_trailing_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let _lock = lock_ledger_for_write(&path).unwrap();
        let mut record = LedgerRecord {
            id: "baseline:source:0-1".into(),
            baseline: "baseline".into(),
            path: "source".into(),
            blob: "blob".into(),
            byte_start: 0,
            byte_end: 1,
            line_start: 1,
            line_end: 1,
            classification: "source".into(),
            disposition: "unmapped".into(),
            destinations: Vec::new(),
            evidence: Vec::new(),
            provenance: vec!["x".repeat(8192)],
        };
        write_ledger_atomic(&path, std::slice::from_ref(&record)).unwrap();
        record.provenance.clear();
        write_ledger_atomic(&path, std::slice::from_ref(&record)).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{}\n", serde_json::to_string(&record).unwrap())
        );
        assert_eq!(read_ledger(&path).unwrap().len(), 1);
        assert_eq!(
            fs::read_dir(directory.path()).unwrap().count(),
            2,
            "only the ledger and stable lock sidecar remain"
        );
        assert!(!path.with_extension("jsonl.tmp").exists());
    }

    #[test]
    fn mapped_hunk_tests_require_executable_rust_test_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let evidence = directory.path().join("parity.rs");
        let mut record = LedgerRecord {
            id: "baseline:source.test.ts:0-1".into(),
            baseline: "baseline".into(),
            path: "source.test.ts".into(),
            blob: "blob".into(),
            byte_start: 0,
            byte_end: 1,
            line_start: 1,
            line_end: 1,
            classification: "test".into(),
            disposition: "translated-test".into(),
            destinations: vec!["parity.rs".into()],
            evidence: vec!["parity.rs".into()],
            provenance: vec!["baseline".into()],
        };

        fs::write(&evidence, "// #[test] is not executable evidence\n").unwrap();
        assert!(validate_test_evidence(directory.path(), &record).is_err());

        fs::write(
            &evidence,
            "#[test]\nfn preserves_the_upstream_behavior() {}\n",
        )
        .unwrap();
        assert!(validate_test_evidence(directory.path(), &record).is_ok());

        record.disposition = "unmapped".into();
        record.evidence.clear();
        assert!(validate_test_evidence(directory.path(), &record).is_ok());
    }

    #[test]
    fn release_entries_retain_every_complete_syntax_notice() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        fs::create_dir_all(root.join("third_party/themes")).unwrap();
        fs::create_dir_all(root.join("third_party/grammars")).unwrap();
        fs::write(root.join("LICENSE"), b"license").unwrap();
        fs::write(root.join("THIRD_PARTY_NOTICES"), b"notices").unwrap();
        let binary = root.join("workdeck");
        fs::write(&binary, b"binary").unwrap();
        for notice in [
            "tm-themes-LICENSE",
            "tm-themes-NOTICE",
            "pierre-theme-LICENSE",
            "pierre-theme-NOTICE.md",
        ] {
            fs::write(root.join("third_party/themes").join(notice), notice).unwrap();
        }
        fs::write(
            root.join("third_party/grammars/shikijs-langs-LICENSE"),
            b"shiki license",
        )
        .unwrap();

        let entries = release_entries(
            "workdeck-test",
            &binary,
            "workdeck",
            root,
            b"licenses",
            b"sbom",
        )
        .unwrap();
        let names = entries
            .into_iter()
            .map(|(name, _, _)| name)
            .collect::<BTreeSet<_>>();
        for notice in [
            "tm-themes-LICENSE",
            "tm-themes-NOTICE",
            "pierre-theme-LICENSE",
            "pierre-theme-NOTICE.md",
        ] {
            assert!(names.contains(&format!("workdeck-test/third-party/themes/{notice}")));
        }
        assert!(names.contains("workdeck-test/third-party/grammars/shikijs-langs-LICENSE"));
    }
}
