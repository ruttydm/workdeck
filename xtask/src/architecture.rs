//! Shrink-only architecture enforcement over the native Cargo and Rust module graphs.

use anyhow::{Context, Result, bail, ensure};
use cargo_metadata::{DependencyKind, Metadata, MetadataCommand, Package};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use syn::{Attribute, Expr, Item, Lit, Meta};

/// Hunk's pinned dependency-cruiser baseline is the empty JSON array. Keep the
/// native equivalent explicit: a future exception must never silently become a
/// new baseline entry.
const KNOWN_ARCHITECTURE_VIOLATIONS: &[&str] = &[];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ArchitectureViolation {
    rule: &'static str,
    subject: String,
    detail: String,
}

impl ArchitectureViolation {
    fn id(&self) -> String {
        format!("{}:{}", self.rule, self.subject)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageShape {
    name: String,
    dependencies: BTreeSet<String>,
    binaries: BTreeSet<String>,
}

/// Run every architecture gate against the live workspace rather than a copied
/// or hand-maintained dependency inventory.
pub(crate) fn check(repo: &Path) -> Result<()> {
    verify_legacy_launcher(repo)?;
    let metadata = MetadataCommand::new()
        .current_dir(repo)
        .no_deps()
        .exec()
        .context("resolve the live Cargo workspace for architecture validation")?;
    let violations = inspect_workspace(repo, &metadata)?;
    validate_shrink_only_baseline(&violations)?;
    println!(
        "Workdeck architecture check passed: {} production crates, one shipped executable, zero dependency or source-reachability violations.",
        allowed_dependencies().len()
    );
    Ok(())
}

/// Account for Hunk's JavaScript `bin/hunk.cjs` launcher without retaining a
/// Node/Bun runtime mirror.  The native entrypoint owns skill materialization,
/// platform-independent argument parsing, and release installation; the
/// package-manager probing and bundled Bun fallback are deliberately absent
/// because Workdeck ships one Cargo binary rather than an npm wrapper.
fn verify_legacy_launcher(repo: &Path) -> Result<()> {
    const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
    let bytes = crate::git_stdout_bytes(repo, ["show", &format!("{BASELINE}:bin/hunk.cjs")])?;
    ensure!(
        bytes.len() == 3_762,
        "pinned bin/hunk.cjs changed size: {} != 3762",
        bytes.len()
    );
    let source = std::str::from_utf8(&bytes)?;
    for marker in [
        "#!/usr/bin/env node",
        "function bundledSkillPath()",
        "function ensureExecutable(target)",
        "function hostCandidates()",
        "function findInstalledBinary(startDir)",
        "function bundledBunRuntime()",
        "const overrideBinary = process.env.HUNK_BIN_PATH",
        "const forwardedArgs = process.argv.slice(2)",
        "forwardedArgs[0] === \"skill\"",
        "spawnSync(target, args",
        "hunk.exe",
        "hunkdiff-darwin-arm64",
        "hunkdiff-linux-x64",
        "hunkdiff-windows-x64",
        "Failed to locate a matching prebuilt Hunk binary",
    ] {
        ensure!(
            source.contains(marker),
            "pinned bin/hunk.cjs lost launcher behavior marker {marker:?}"
        );
    }

    let cli = fs::read_to_string(repo.join("crates/workdeck-cli/src/main.rs"))?;
    for marker in [
        "#[command(name = \"workdeck\")]",
        "enum SkillCommand",
        "fn handle_skill_command(command: Option<SkillCommand>)",
        "include_str!(\"../../../skills/workdeck-review/SKILL.md\")",
        "use workdeck_extension_api::{",
        "fn handle_global_command(",
    ] {
        ensure!(
            cli.contains(marker),
            "native Workdeck launcher replacement is missing {marker:?}"
        );
    }
    ensure!(
        !cli.contains("HUNK_BIN_PATH") && !cli.contains("bundledBunRuntime"),
        "native launcher must not retain Hunk/Bun runtime overrides"
    );

    let migration = fs::read_to_string(repo.join("docs/launcher-migration.md"))?;
    for marker in [
        "bin/hunk.cjs",
        "skill path",
        "HUNK_BIN_PATH",
        "hostCandidates",
        "bundled Bun",
        "Cargo's one `workdeck` binary",
        "not copied into the Workdeck tree or executed",
    ] {
        ensure!(
            migration.contains(marker),
            "launcher migration is missing {marker:?}"
        );
    }
    ensure!(
        !repo.join("bin/hunk.cjs").exists(),
        "the legacy JavaScript launcher must not be retained in the final tree"
    );
    Ok(())
}

fn inspect_workspace(repo: &Path, metadata: &Metadata) -> Result<Vec<ArchitectureViolation>> {
    let workspace_names = metadata
        .workspace_packages()
        .into_iter()
        .map(|package| package.name.as_str())
        .collect::<BTreeSet<_>>();
    let packages = metadata
        .workspace_packages()
        .into_iter()
        .map(|package| package_shape(package, &workspace_names))
        .collect::<Vec<_>>();
    let mut violations = validate_package_graph(&packages);

    for package in metadata.workspace_packages().into_iter().filter(|package| {
        allowed_dependencies().contains_key(package.name.as_str()) || package.name == "xtask"
    }) {
        violations.extend(validate_source_reachability(repo, package)?);
    }
    violations.extend(validate_source_import_boundaries(repo)?);
    violations.extend(validate_startup_lifecycle(repo)?);
    violations.sort();
    violations.dedup();
    Ok(violations)
}

/// Keep command bootstrap and interactive worker ownership explicit.  Rust has
/// no runtime import graph to walk like the pinned TypeScript entrypoint, so
/// the equivalent native contract is the ordering of the composition root:
/// review commands are delegated before repository discovery, and global
/// commands are handled before repository configuration or the TUI can be
/// constructed.  The highlight worker must likewise be retired by the review
/// owner, never by the process entrypoint.
fn validate_startup_lifecycle(repo: &Path) -> Result<Vec<ArchitectureViolation>> {
    let cli_entrypoint = fs::read_to_string(repo.join("crates/workdeck-cli/src/main.rs"))?;
    let mut violations = Vec::new();
    let run_start = cli_entrypoint
        .find("fn run_with_preloaded_extensions")
        .context("native CLI composition root is missing")?;
    let run_source = &cli_entrypoint[run_start..];
    let discovery = run_source
        .find("let repo_root = git::discover_repo_root")
        .context("native CLI composition root lost its repository-discovery boundary")?;
    let before_discovery = &run_source[..discovery];

    for (rule, marker, detail) in [
        (
            "startup-review-before-discovery",
            "is_some_and(Command::is_review_command)",
            "review commands must be delegated before repository discovery",
        ),
        (
            "startup-global-before-discovery",
            "is_some_and(Command::is_global_command)",
            "global commands must be handled before repository discovery",
        ),
        (
            "startup-review-handler-before-discovery",
            "return handle_review_command(",
            "review commands must enter the native review handler before discovery",
        ),
        (
            "startup-global-handler-before-discovery",
            "return handle_global_command(",
            "global commands must enter their handler before discovery",
        ),
    ] {
        if !before_discovery.contains(marker) {
            violations.push(ArchitectureViolation {
                rule,
                subject: "workdeck-cli::run_with_preloaded_extensions".into(),
                detail: detail.into(),
            });
        }
    }

    // The entrypoint may select the owner, but it must not reach into the
    // worker's disposal API.  ReviewApp owns this resource both on normal
    // teardown and while the bootstrap closure returns.
    if cli_entrypoint.contains("dispose_highlight_worker") {
        violations.push(ArchitectureViolation {
            rule: "startup-entrypoint-does-not-dispose-worker",
            subject: "workdeck-cli::main".into(),
            detail: "highlight-worker disposal belongs to the interactive review owner".into(),
        });
    }
    let tui_source = fs::read_to_string(repo.join("crates/workdeck-tui/src/lib.rs"))?;
    let drop_start = tui_source
        .find("impl Drop for ReviewApp")
        .context("ReviewApp drop owner is missing")?;
    let drop_end = tui_source[drop_start..]
        .find("fn rect_contains")
        .map(|offset| drop_start + offset)
        .unwrap_or(tui_source.len());
    if !tui_source[drop_start..drop_end].contains("dispose_highlight_worker") {
        violations.push(ArchitectureViolation {
            rule: "startup-review-owner-disposes-worker",
            subject: "workdeck-tui::ReviewApp".into(),
            detail: "ReviewApp drop must retire the native highlight worker".into(),
        });
    }
    Ok(violations)
}

fn package_shape(package: &Package, workspace_names: &BTreeSet<&str>) -> PackageShape {
    let dependencies = package
        .dependencies
        .iter()
        .filter(|dependency| dependency.kind != DependencyKind::Development)
        .filter(|dependency| workspace_names.contains(dependency.name.as_str()))
        .map(|dependency| dependency.name.clone())
        .collect();
    let binaries = package
        .targets
        .iter()
        .filter(|target| target.is_bin())
        .map(|target| target.name.clone())
        .collect();
    PackageShape {
        name: package.name.clone(),
        dependencies,
        binaries,
    }
}

fn allowed_dependencies() -> BTreeMap<&'static str, BTreeSet<&'static str>> {
    BTreeMap::from([
        ("workdeck-core", BTreeSet::new()),
        ("workdeck-diff", BTreeSet::from(["workdeck-core"])),
        ("workdeck-extension-api", BTreeSet::from(["workdeck-core"])),
        (
            "workdeck-review",
            BTreeSet::from(["workdeck-core", "workdeck-extension-api"]),
        ),
        (
            "workdeck-vcs",
            BTreeSet::from(["workdeck-core", "workdeck-diff"]),
        ),
        (
            "workdeck-extension-host",
            BTreeSet::from([
                "workdeck-core",
                "workdeck-diff",
                "workdeck-extension-api",
                "workdeck-review",
                "workdeck-vcs",
            ]),
        ),
        (
            "workdeck-session",
            BTreeSet::from([
                "workdeck-core",
                "workdeck-diff",
                "workdeck-review",
                "workdeck-vcs",
            ]),
        ),
        ("workdeck-markup", BTreeSet::new()),
        ("workdeck-migration", BTreeSet::new()),
        ("workdeck-store", BTreeSet::new()),
        (
            "workdeck-tui",
            BTreeSet::from([
                "workdeck-core",
                "workdeck-diff",
                "workdeck-extension-api",
                "workdeck-extension-host",
                "workdeck-markup",
                "workdeck-review",
                "workdeck-session",
                "workdeck-vcs",
            ]),
        ),
        (
            "workdeck-cli",
            BTreeSet::from([
                "workdeck-core",
                "workdeck-diff",
                "workdeck-extension-api",
                "workdeck-extension-host",
                "workdeck-markup",
                "workdeck-migration",
                "workdeck-review",
                "workdeck-session",
                "workdeck-store",
                "workdeck-tui",
                "workdeck-vcs",
            ]),
        ),
    ])
}

fn validate_package_graph(packages: &[PackageShape]) -> Vec<ArchitectureViolation> {
    let allowed = allowed_dependencies();
    let package_by_name = packages
        .iter()
        .map(|package| (package.name.as_str(), package))
        .collect::<BTreeMap<_, _>>();
    let mut violations = Vec::new();

    for (name, allowed_targets) in &allowed {
        let Some(package) = package_by_name.get(name) else {
            violations.push(ArchitectureViolation {
                rule: "required-boundary-is-present",
                subject: (*name).into(),
                detail: "the architecture-owned crate is absent from the workspace".into(),
            });
            continue;
        };
        for dependency in &package.dependencies {
            if !allowed_targets.contains(dependency.as_str()) {
                violations.push(ArchitectureViolation {
                    rule: boundary_rule(name),
                    subject: format!("{name}->{dependency}"),
                    detail: "workspace dependency crosses its declared ownership boundary".into(),
                });
            }
        }
    }

    let production_names = allowed.keys().copied().collect::<BTreeSet<_>>();
    let graph = allowed
        .keys()
        .map(|name| {
            let edges = package_by_name
                .get(name)
                .map_or_else(BTreeSet::new, |package| {
                    package
                        .dependencies
                        .iter()
                        .filter(|dependency| production_names.contains(dependency.as_str()))
                        .cloned()
                        .collect()
                });
            ((*name).to_owned(), edges)
        })
        .collect::<BTreeMap<_, _>>();
    for cycle in dependency_cycles(&graph) {
        violations.push(ArchitectureViolation {
            rule: "no-circular",
            subject: cycle.join("->"),
            detail: "production crates form a dependency cycle".into(),
        });
    }

    let shipped = allowed
        .keys()
        .filter_map(|name| package_by_name.get(name))
        .flat_map(|package| {
            package
                .binaries
                .iter()
                .map(move |binary| (package.name.as_str(), binary.as_str()))
        })
        .collect::<Vec<_>>();
    if shipped != [("workdeck-cli", "workdeck")] {
        violations.push(ArchitectureViolation {
            rule: "one-shipped-executable",
            subject: shipped
                .iter()
                .map(|(package, binary)| format!("{package}:{binary}"))
                .collect::<Vec<_>>()
                .join(","),
            detail: "production crates must expose exactly the workdeck binary".into(),
        });
    }

    violations
}

fn boundary_rule(package: &str) -> &'static str {
    match package {
        "workdeck-core" => "core-stays-domain",
        "workdeck-diff" => "diff-stays-below-review",
        "workdeck-extension-api" => "extension-api-is-contract-only",
        "workdeck-extension-host" => "extensions-host-stays-below-surfaces",
        "workdeck-review" => "review-stays-below-host",
        "workdeck-session" => "session-stays-below-cli-and-ui",
        "workdeck-store" => "store-is-independent",
        "workdeck-tui" => "ui-stays-below-composition-root",
        "workdeck-vcs" => "vcs-stays-provider-neutral",
        _ => "workspace-boundary",
    }
}

fn dependency_cycles(graph: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
    fn visit(
        node: &str,
        graph: &BTreeMap<String, BTreeSet<String>>,
        marks: &mut BTreeMap<String, u8>,
        stack: &mut Vec<String>,
        cycles: &mut BTreeSet<Vec<String>>,
    ) {
        match marks.get(node).copied().unwrap_or_default() {
            2 => return,
            1 => {
                if let Some(index) = stack.iter().position(|candidate| candidate == node) {
                    let mut cycle = stack[index..].to_vec();
                    if let Some((minimum, _)) =
                        cycle.iter().enumerate().min_by_key(|(_, item)| *item)
                    {
                        cycle.rotate_left(minimum);
                    }
                    cycle.push(cycle[0].clone());
                    cycles.insert(cycle);
                }
                return;
            }
            _ => {}
        }
        marks.insert(node.to_owned(), 1);
        stack.push(node.to_owned());
        if let Some(edges) = graph.get(node) {
            for edge in edges {
                visit(edge, graph, marks, stack, cycles);
            }
        }
        stack.pop();
        marks.insert(node.to_owned(), 2);
    }

    let mut marks = BTreeMap::new();
    let mut stack = Vec::new();
    let mut cycles = BTreeSet::new();
    for node in graph.keys() {
        visit(node, graph, &mut marks, &mut stack, &mut cycles);
    }
    cycles.into_iter().collect()
}

fn validate_source_reachability(
    repo: &Path,
    package: &Package,
) -> Result<Vec<ArchitectureViolation>> {
    let manifest = package.manifest_path.as_std_path();
    let source_root = manifest
        .parent()
        .context("package manifest has no parent")?
        .join("src");
    if !source_root.is_dir() {
        return Ok(Vec::new());
    }
    let roots = package
        .targets
        .iter()
        .filter(|target| target.is_lib() || target.is_bin())
        .map(|target| target.src_path.as_std_path().to_path_buf())
        .filter(|path| path.starts_with(&source_root))
        .collect::<Vec<_>>();
    let scan = scan_rust_module_tree(&source_root, &roots)?;
    Ok(scan
        .orphaned
        .into_iter()
        .map(|path| ArchitectureViolation {
            rule: "no-dead-modules",
            subject: format!(
                "{}:{}",
                package.name,
                path.strip_prefix(repo).unwrap_or(&path).display()
            ),
            detail: "Rust source is not reachable from a library or binary module root".into(),
        })
        .collect())
}

fn production_mentions_session(source: &str) -> Result<bool> {
    use syn::visit::Visit;

    struct SessionReference(bool);
    impl<'ast> Visit<'ast> for SessionReference {
        fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
            // Only an explicit test-only gate is exempt. In particular,
            // cfg(any(test, feature = "runtime")) remains production-reachable.
            if module.attrs.iter().any(|attribute| {
                attribute.path().is_ident("cfg")
                    && matches!(&attribute.meta, Meta::List(list) if list.tokens.to_string() == "test")
            }) {
                return;
            }
            syn::visit::visit_item_mod(self, module);
        }

        fn visit_ident(&mut self, ident: &'ast syn::Ident) {
            self.0 |= ident == "workdeck_session";
        }

        fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
            // Macro expansion is not available here: conservatively retain
            // references inside token bodies rather than silently overlooking them.
            self.0 |= invocation.tokens.to_string().contains("workdeck_session");
            syn::visit::visit_macro(self, invocation);
        }
    }

    let parsed = syn::parse_file(source).context("parse Rust source for session boundary")?;
    let mut references = SessionReference(false);
    references.visit_file(&parsed);
    Ok(references.0)
}

fn validate_source_import_boundaries(repo: &Path) -> Result<Vec<ArchitectureViolation>> {
    let mut violations = Vec::new();
    let tui_root = repo.join("crates/workdeck-tui/src");
    let allowed_session_adapters = BTreeSet::from([
        PathBuf::from("crates/workdeck-tui/src/app_host.rs"),
        PathBuf::from("crates/workdeck-tui/src/current_review_controller.rs"),
        PathBuf::from("crates/workdeck-tui/src/current_review_refresh.rs"),
        PathBuf::from("crates/workdeck-tui/src/interactive_session_adapter.rs"),
        PathBuf::from("crates/workdeck-tui/src/lib.rs"),
        PathBuf::from("crates/workdeck-tui/src/review_state_helpers.rs"),
        PathBuf::from("crates/workdeck-tui/src/session_review_controller.rs"),
    ]);
    let mut tui_files = BTreeSet::new();
    collect_rust_files(&tui_root, &mut tui_files)?;
    for path in tui_files {
        let source = fs::read_to_string(&path)?;
        let relative = path.strip_prefix(repo).unwrap_or(&path).to_path_buf();
        if production_mentions_session(&source)? && !allowed_session_adapters.contains(&relative) {
            violations.push(ArchitectureViolation {
                rule: "ui-couples-to-session-via-adapters",
                subject: relative.display().to_string(),
                detail:
                    "only the composition shell and named review adapters may use workdeck-session"
                        .into(),
            });
        }
    }

    let extension_root = repo.join("examples/extensions");
    if extension_root.is_dir() {
        let mut extension_files = BTreeSet::new();
        collect_rust_files(&extension_root, &mut extension_files)?;
        for path in extension_files {
            let source = fs::read_to_string(&path)?;
            if ["workdeck_cli", "workdeck_session", "workdeck_tui"]
                .iter()
                .any(|forbidden| source.contains(forbidden))
            {
                violations.push(ArchitectureViolation {
                    rule: "bundled-ui-extensions-render-only",
                    subject: path
                        .strip_prefix(repo)
                        .unwrap_or(&path)
                        .display()
                        .to_string(),
                    detail:
                        "native extension implementations must use declarative API data and actions"
                            .into(),
                });
            }
        }
    }

    let review_root = repo.join("crates/workdeck-review/src");
    let review_library = fs::read_to_string(review_root.join("lib.rs"))?;
    let reducer = fs::read_to_string(review_root.join("semantic_reducer.rs"))?;
    if review_library.contains("pub use semantic_reducer")
        || reducer.contains("pub fn reduce_semantic_review_state")
    {
        violations.push(ArchitectureViolation {
            rule: "review-reducer-is-module-internal",
            subject: "workdeck-review::semantic_reducer".into(),
            detail: "callers must dispatch intents through SemanticReviewStore".into(),
        });
    }
    let diff_library = fs::read_to_string(repo.join("crates/workdeck-diff/src/lib.rs"))?;
    if diff_library
        .lines()
        .any(|line| line.trim_start().starts_with("pub mod "))
    {
        violations.push(ArchitectureViolation {
            rule: "changeset-internals-stay-in-module",
            subject: "workdeck-diff::module-namespace".into(),
            detail:
                "implementation modules stay private; only deliberate facade items are re-exported"
                    .into(),
        });
    }
    Ok(violations)
}

#[derive(Debug, Default)]
struct ModuleScan {
    reached: BTreeSet<PathBuf>,
    test_only: BTreeSet<PathBuf>,
    orphaned: BTreeSet<PathBuf>,
}

fn scan_rust_module_tree(source_root: &Path, roots: &[PathBuf]) -> Result<ModuleScan> {
    let mut scan = ModuleScan::default();
    for root in roots {
        visit_module_file(root, root.parent().unwrap_or(source_root), false, &mut scan)?;
    }
    let mut candidates = BTreeSet::new();
    collect_rust_files(source_root, &mut candidates)?;
    scan.orphaned = candidates
        .difference(&scan.reached)
        .filter(|path| !scan.test_only.contains(*path))
        .cloned()
        .collect();
    Ok(scan)
}

fn collect_rust_files(directory: &Path, files: &mut BTreeSet<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("read Rust source directory {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.insert(path);
        }
    }
    Ok(())
}

fn visit_module_file(
    path: &Path,
    module_directory: &Path,
    test_only: bool,
    scan: &mut ModuleScan,
) -> Result<()> {
    let target = if test_only {
        &mut scan.test_only
    } else {
        &mut scan.reached
    };
    if !target.insert(path.to_path_buf()) {
        return Ok(());
    }
    let source =
        fs::read_to_string(path).with_context(|| format!("read Rust module {}", path.display()))?;
    let syntax = syn::parse_file(&source)
        .with_context(|| format!("parse Rust module {}", path.display()))?;
    visit_module_items(&syntax.items, path, module_directory, test_only, scan)
}

fn visit_module_items(
    items: &[Item],
    source_file: &Path,
    module_directory: &Path,
    inherited_test_only: bool,
    scan: &mut ModuleScan,
) -> Result<()> {
    for item in items {
        let Item::Mod(module) = item else {
            continue;
        };
        let test_only = inherited_test_only || cfg_test_only(&module.attrs);
        let name = module.ident.to_string();
        if let Some((_, items)) = &module.content {
            visit_module_items(
                items,
                source_file,
                &module_directory.join(name),
                test_only,
                scan,
            )?;
            continue;
        }
        let module_file = resolve_module_file(source_file, module_directory, &name, &module.attrs)
            .with_context(|| {
                format!(
                    "resolve module {name} declared by {}",
                    source_file.display()
                )
            })?;
        let child_directory = if module_file.file_name().is_some_and(|name| name == "mod.rs") {
            module_file
                .parent()
                .unwrap_or(module_directory)
                .to_path_buf()
        } else {
            module_file.parent().unwrap_or(module_directory).join(&name)
        };
        visit_module_file(&module_file, &child_directory, test_only, scan)?;
    }
    Ok(())
}

fn cfg_test_only(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        if !attribute.path().is_ident("cfg") {
            return false;
        }
        let Meta::List(list) = &attribute.meta else {
            return false;
        };
        let compact = list.tokens.to_string().replace(' ', "");
        compact == "test" || (compact.contains("test") && !compact.contains("not(test)"))
    })
}

fn resolve_module_file(
    source_file: &Path,
    module_directory: &Path,
    name: &str,
    attributes: &[Attribute],
) -> Result<PathBuf> {
    for attribute in attributes {
        if !attribute.path().is_ident("path") {
            continue;
        }
        if let Meta::NameValue(value) = &attribute.meta
            && let Expr::Lit(expression) = &value.value
            && let Lit::Str(path) = &expression.lit
        {
            let candidate = source_file
                .parent()
                .unwrap_or(module_directory)
                .join(path.value());
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        bail!("invalid or missing #[path] module target");
    }

    let flat = module_directory.join(format!("{name}.rs"));
    if flat.is_file() {
        return Ok(flat);
    }
    let nested = module_directory.join(name).join("mod.rs");
    if nested.is_file() {
        return Ok(nested);
    }
    bail!("neither {} nor {} exists", flat.display(), nested.display())
}

fn validate_shrink_only_baseline(violations: &[ArchitectureViolation]) -> Result<()> {
    let actual = violations
        .iter()
        .map(ArchitectureViolation::id)
        .collect::<BTreeSet<_>>();
    let known = KNOWN_ARCHITECTURE_VIOLATIONS
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<BTreeSet<_>>();
    let unexpected = actual.difference(&known).cloned().collect::<Vec<_>>();
    let retired = known.difference(&actual).cloned().collect::<Vec<_>>();
    if unexpected.is_empty() && retired.is_empty() {
        return Ok(());
    }
    let details = violations
        .iter()
        .filter(|violation| unexpected.contains(&violation.id()))
        .map(|violation| format!("{} ({})", violation.id(), violation.detail))
        .chain(
            retired
                .iter()
                .map(|id| format!("retired baseline entry {id} must be removed")),
        )
        .collect::<Vec<_>>()
        .join("; ");
    bail!("architecture violations: {details}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_boundary_excludes_only_explicit_test_modules() {
        for source in [
            "#[cfg(test)] mod tests { use workdeck_session::Session; }",
            "mod live { #[cfg(test)] mod tests { fn check() { workdeck_session::check(); } } }",
            "// workdeck_session is not an import\nfn live() {}",
            "const DOC: &str = \"workdeck_session\";",
        ] {
            assert!(!production_mentions_session(source).unwrap(), "{source}");
        }
        for source in [
            "use workdeck_session::Session;",
            "mod live { fn run() { workdeck_session::run(); } }",
            "#[cfg(not(test))] mod live { use workdeck_session::Session; }",
            "#[cfg(any(test, feature = \"runtime\"))] mod live { use workdeck_session::Session; }",
            "macro_rules! generate { () => { workdeck_session::run() }; }",
            "#[cfg(test)] mod tests {} fn live() { workdeck_session::run(); }",
        ] {
            assert!(production_mentions_session(source).unwrap(), "{source}");
        }
        assert!(production_mentions_session("mod {").is_err());
    }

    fn shape(name: &str, dependencies: &[&str], binaries: &[&str]) -> PackageShape {
        PackageShape {
            name: name.into(),
            dependencies: dependencies.iter().map(|value| (*value).into()).collect(),
            binaries: binaries.iter().map(|value| (*value).into()).collect(),
        }
    }

    #[test]
    fn pinned_empty_violation_baseline_remains_shrink_only() {
        assert!(KNOWN_ARCHITECTURE_VIOLATIONS.is_empty());
        assert!(validate_shrink_only_baseline(&[]).is_ok());
        let violation = ArchitectureViolation {
            rule: "core-stays-domain",
            subject: "workdeck-core->workdeck-tui".into(),
            detail: "test".into(),
        };
        assert!(validate_shrink_only_baseline(&[violation]).is_err());
    }

    #[test]
    fn graph_gate_rejects_cycles_boundary_inversions_and_extra_binaries() {
        let mut packages = allowed_dependencies()
            .keys()
            .map(|name| shape(name, &[], &[]))
            .collect::<Vec<_>>();
        packages
            .iter_mut()
            .find(|package| package.name == "workdeck-cli")
            .unwrap()
            .binaries
            .insert("workdeck".into());
        assert!(validate_package_graph(&packages).is_empty());

        packages
            .iter_mut()
            .find(|package| package.name == "workdeck-core")
            .unwrap()
            .dependencies
            .insert("workdeck-tui".into());
        packages
            .iter_mut()
            .find(|package| package.name == "workdeck-tui")
            .unwrap()
            .dependencies
            .insert("workdeck-core".into());
        packages
            .iter_mut()
            .find(|package| package.name == "workdeck-tui")
            .unwrap()
            .binaries
            .insert("hunk".into());
        let violations = validate_package_graph(&packages);
        assert!(
            violations
                .iter()
                .any(|violation| violation.rule == "core-stays-domain")
        );
        assert!(
            violations
                .iter()
                .any(|violation| violation.rule == "no-circular")
        );
        assert!(
            violations
                .iter()
                .any(|violation| violation.rule == "one-shipped-executable")
        );
    }

    #[test]
    fn module_scan_distinguishes_production_tests_and_orphans() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("src");
        fs::create_dir_all(source.join("tests")).unwrap();
        fs::write(
            source.join("lib.rs"),
            "pub mod live;\n#[cfg(test)]\nmod tests;\n",
        )
        .unwrap();
        fs::write(source.join("live.rs"), "pub fn live() {}\n").unwrap();
        fs::write(source.join("tests.rs"), "mod helper;\n").unwrap();
        fs::write(source.join("tests/helper.rs"), "pub fn helper() {}\n").unwrap();
        fs::write(source.join("orphan.rs"), "pub fn orphan() {}\n").unwrap();

        let scan = scan_rust_module_tree(&source, &[source.join("lib.rs")]).unwrap();
        assert!(scan.reached.contains(&source.join("live.rs")));
        assert!(scan.test_only.contains(&source.join("tests.rs")));
        assert!(scan.test_only.contains(&source.join("tests/helper.rs")));
        assert_eq!(scan.orphaned, BTreeSet::from([source.join("orphan.rs")]));
    }

    #[test]
    fn live_workspace_has_no_hidden_graph_or_source_exceptions() {
        let repo = super::super::repo_root().unwrap();
        let metadata = MetadataCommand::new()
            .current_dir(&repo)
            .no_deps()
            .exec()
            .unwrap();
        let violations = inspect_workspace(&repo, &metadata).unwrap();
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn native_startup_graph_and_worker_lifecycle_preserve_hunk_boundaries() {
        let repo = super::super::repo_root().unwrap();
        let violations = validate_startup_lifecycle(&repo).unwrap();
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn native_review_vocabulary_and_transport_limits_have_one_authority() {
        use workdeck_review::{REVIEW_INTENT_TYPES, REVIEW_RESOURCE_CHUNK_BYTES};
        use workdeck_session::{
            MAX_REVIEW_EVENT_CHUNKS, MAX_REVIEW_EVENT_PAYLOAD_BYTES,
            MAX_WORKDECK_REVIEW_ENVELOPE_BYTES, MAX_WS_MESSAGE_BYTES, REVIEW_EVENT_CHUNK_BYTES,
            WORKDECK_REVIEW_ACTION_TYPES, WorkdeckReviewParseFailureReason,
            WorkdeckReviewParseResult, parse_workdeck_review_action,
        };

        assert_eq!(WORKDECK_REVIEW_ACTION_TYPES, REVIEW_INTENT_TYPES);
        let unique = WORKDECK_REVIEW_ACTION_TYPES.iter().collect::<BTreeSet<_>>();
        assert_eq!(unique.len(), WORKDECK_REVIEW_ACTION_TYPES.len());
        for action_type in WORKDECK_REVIEW_ACTION_TYPES {
            assert_eq!(
                parse_workdeck_review_action(&serde_json::json!({
                    "type": action_type,
                    "unexpectedField": 1
                })),
                WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Invalid)
            );
        }
        assert_eq!(
            parse_workdeck_review_action(&serde_json::json!({
                "type": "notes/archive-user",
                "noteId": "n"
            })),
            WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Unsupported)
        );
        let transport_limit = std::hint::black_box(MAX_WS_MESSAGE_BYTES);
        assert!(transport_limit >= MAX_WORKDECK_REVIEW_ENVELOPE_BYTES);
        assert_eq!(
            MAX_REVIEW_EVENT_PAYLOAD_BYTES,
            MAX_WORKDECK_REVIEW_ENVELOPE_BYTES
        );
        assert_eq!(REVIEW_EVENT_CHUNK_BYTES, REVIEW_RESOURCE_CHUNK_BYTES);
        assert_eq!(
            MAX_REVIEW_EVENT_CHUNKS,
            MAX_REVIEW_EVENT_PAYLOAD_BYTES.div_ceil(REVIEW_RESOURCE_CHUNK_BYTES)
        );

        let repo = super::super::repo_root().unwrap();
        let mut rust_files = BTreeSet::new();
        collect_rust_files(&repo.join("crates/workdeck-review/src"), &mut rust_files).unwrap();
        collect_rust_files(&repo.join("crates/workdeck-session/src"), &mut rust_files).unwrap();
        let canonical_digest_helper = repo.join("crates/workdeck-core/src/identity.rs");
        for path in rust_files {
            let source = fs::read_to_string(&path).unwrap();
            if path != canonical_digest_helper {
                assert!(
                    !source.contains("{ 64 }") && !source.contains("{64}"),
                    "digest width was re-declared outside the shared identity helper: {}",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn native_boundary_gate_replaces_the_legacy_source_tree_without_a_runtime_mirror() {
        let repo = super::super::repo_root().unwrap();
        for forbidden in [
            "src",
            "website",
            "package.json",
            "bun.lock",
            "node_modules",
            "hunk",
        ] {
            assert!(
                !repo.join(forbidden).exists(),
                "legacy source/runtime path unexpectedly remains: {forbidden}"
            );
        }
        let metadata = MetadataCommand::new()
            .current_dir(&repo)
            .no_deps()
            .exec()
            .unwrap();
        let workspace_names = metadata
            .workspace_packages()
            .into_iter()
            .map(|package| package.name.as_str())
            .collect::<BTreeSet<_>>();
        let packages = metadata
            .workspace_packages()
            .into_iter()
            .map(|package| package_shape(package, &workspace_names))
            .collect::<Vec<_>>();
        assert!(validate_package_graph(&packages).is_empty());
        assert!(validate_source_import_boundaries(&repo).unwrap().is_empty());
    }

    #[test]
    fn migrated_source_ownership_map_retains_every_role_and_resolves_native_links() {
        let repo = super::super::repo_root().unwrap();
        let document = fs::read_to_string(repo.join("docs/source-architecture.md")).unwrap();
        let roles = document
            .lines()
            .filter_map(|line| line.strip_prefix("| `src/"))
            .map(|line| line.split_once('`').unwrap().0)
            .collect::<Vec<_>>();
        assert_eq!(
            roles,
            [
                "app/",
                "app/session/",
                "core/",
                "core/changeset/",
                "core/run/",
                "core/process/",
                "core/theme/",
                "core/watch/",
                "core/vcs/",
                "extensions/",
                "session/",
                "session/client/",
                "session/agent/",
                "session/broker/",
                "ui/",
                "extension-api/",
                "opentui/",
                "lib/",
            ]
        );
        // These are documentation links, not claims of source implementation parity.
        for suffix in document.split("](").skip(1) {
            let (target, _) = suffix.split_once(')').unwrap();
            assert!(
                repo.join("docs").join(target).exists(),
                "missing link: {target}"
            );
        }
        for section in [
            "## Dependency direction",
            "## Bootstrap invariant",
            "## Migration policy",
            "## Attribution",
        ] {
            assert!(document.contains(section), "missing section: {section}");
        }
    }

    #[test]
    fn pinned_hunk_launcher_is_replaced_by_one_native_workdeck_entrypoint() {
        let repo = super::super::repo_root().unwrap();
        super::verify_legacy_launcher(&repo).unwrap();
    }
}
