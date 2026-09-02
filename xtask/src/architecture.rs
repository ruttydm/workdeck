//! Shrink-only architecture enforcement over the native Cargo and Rust module graphs.

use anyhow::{Context, Result, bail};
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
    violations.sort();
    violations.dedup();
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

fn validate_source_import_boundaries(repo: &Path) -> Result<Vec<ArchitectureViolation>> {
    let mut violations = Vec::new();
    let tui_root = repo.join("crates/workdeck-tui/src");
    let allowed_session_adapters = BTreeSet::from([
        PathBuf::from("crates/workdeck-tui/src/current_review_controller.rs"),
        PathBuf::from("crates/workdeck-tui/src/current_review_refresh.rs"),
        PathBuf::from("crates/workdeck-tui/src/lib.rs"),
        PathBuf::from("crates/workdeck-tui/src/review_state_helpers.rs"),
    ]);
    let mut tui_files = BTreeSet::new();
    collect_rust_files(&tui_root, &mut tui_files)?;
    for path in tui_files {
        let source = fs::read_to_string(&path)?;
        let relative = path.strip_prefix(repo).unwrap_or(&path).to_path_buf();
        if source.contains("workdeck_session") && !allowed_session_adapters.contains(&relative) {
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
}
