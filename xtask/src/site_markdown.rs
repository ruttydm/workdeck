//! Native Markdown exports for the migrated website. Hunk MIT astro.config.mjs
//! supplies the overview-first ordering and compact-corpus exclusion policy.
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Component, Path},
};

pub(crate) fn emit(repo: &Path, output: &Path) -> Result<()> {
    emit_plan(&plan(repo)?, output)
}

/// Exercise native Zola routing with a disposable site, independently of the
/// current documentation tree. This runs as part of `xtask site check`.
pub(crate) fn check_zola_routes() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    fs::write(
        root.join("config.toml"),
        "base_url = 'https://example.invalid'\n",
    )?;
    fs::create_dir(root.join("templates"))?;
    fs::write(
        root.join("templates/page.html"),
        "{{ page.content | safe }}",
    )?;
    fs::write(
        root.join("templates/section.html"),
        "{{ section.content | safe }}",
    )?;
    let sources = BTreeMap::from([
        ("docs/bundle/index.md".into(), "+++\ntitle = 'Bundle'\n+++\nBundle body\n".into()),
        ("docs/renamed/index.md".into(), "+++\ntitle = 'Renamed bundle'\nslug = 'new-name'\n+++\nRenamed body\n".into()),
        ("docs/original.md".into(), "+++\ntitle = 'Renamed page'\nslug = 'different'\n+++\nPage body\n".into()),
        ("docs/override.md".into(), "+++\ntitle = 'Override'\nslug = 'ignored'\npath = 'elsewhere/custom/'\n+++\nOverride body\n".into()),
        ("docs/section/_index.md".into(), "+++\ntitle = 'Section'\n+++\nSection body\n".into()),
    ]);
    for (path, source) in &sources {
        let destination = root.join("content").join(path);
        fs::create_dir_all(destination.parent().context("fixture parent")?)?;
        fs::write(destination, source)?;
    }
    crate::run_checked(root, "zola", &["build"])?;
    let exports = render(&sources)?;
    let expected = [
        "docs/bundle.md",
        "docs/different.md",
        "docs/new-name.md",
        "docs/section.md",
        "elsewhere/custom.md",
    ];
    let routes: Vec<_> = exports
        .keys()
        .filter(|name| name.ends_with(".md"))
        .map(String::as_str)
        .collect();
    ensure!(routes == expected, "native routing fixture export mismatch");
    let public = root.join("public");
    // emit_plan independently requires every corresponding HTML file to exist.
    emit_plan(&exports, &public)?;
    for absent in [
        "docs/bundle/index/index.html",
        "docs/renamed/index.html",
        "docs/original/index.html",
        "docs/ignored/index.html",
    ] {
        ensure!(
            !public.join(absent).exists(),
            "Zola routing changed: {absent}"
        );
    }
    Ok(())
}

fn emit_plan(exports: &BTreeMap<String, String>, output: &Path) -> Result<()> {
    ensure!(
        fs::symlink_metadata(output)?.file_type().is_dir(),
        "export output must be a real directory"
    );
    // Preflight all destinations before writing any export. Zola owns this output;
    // do not overwrite a static asset that claims one of our generated routes.
    for name in exports.keys() {
        let relative = Path::new(name);
        ensure!(
            relative
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
            "unsafe export destination"
        );
        let mut parent = output.to_path_buf();
        if let Some(directory) = relative.parent() {
            for component in directory.components() {
                parent.push(component);
                ensure!(
                    fs::symlink_metadata(&parent)?.file_type().is_dir(),
                    "export parent must be a real directory"
                );
            }
        }
        ensure!(
            !output.join(name).try_exists()? && fs::symlink_metadata(output.join(name)).is_err(),
            "export destination already exists: {name}"
        );
        if let Some(route) = name.strip_suffix(".md") {
            ensure!(
                fs::symlink_metadata(output.join(route).join("index.html"))?
                    .file_type()
                    .is_file(),
                "Markdown export has no rendered page: {route}"
            );
        }
    }
    for (name, content) in exports {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(name))?;
        file.write_all(content.as_bytes())?;
        ensure!(
            fs::read_to_string(output.join(name))? == *content,
            "export verification failed: {name}"
        );
    }
    Ok(())
}

pub(crate) fn plan(repo: &Path) -> Result<BTreeMap<String, String>> {
    let root = repo.join("site/content");
    let mut sources = BTreeMap::new();
    collect(&root, &root, &mut sources)?;
    render(&sources)
}

fn collect(root: &Path, directory: &Path, sources: &mut BTreeMap<String, String>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "website content symlinks are unsupported"
        );
        let path = entry.path();
        if kind.is_dir() {
            collect(root, &path, sources)?;
        } else if path.extension().is_some_and(|extension| extension == "md") {
            let relative = path
                .strip_prefix(root)?
                .to_str()
                .context("non-UTF8 content path")?
                .replace('\\', "/");
            sources.insert(relative, fs::read_to_string(path)?);
        }
    }
    Ok(())
}

fn render(sources: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>> {
    let mut draft_sections = Vec::new();
    for (path, source) in sources {
        if path == "_index.md" || path.ends_with("/_index.md") {
            let normalized = source.replace("\r\n", "\n");
            let remainder = normalized
                .strip_prefix("+++\n")
                .context("section needs TOML frontmatter")?;
            let (metadata, _) = remainder
                .split_once("\n+++\n")
                .context("unclosed section frontmatter")?;
            let metadata: toml_edit::DocumentMut = metadata.parse()?;
            if metadata.get("draft").and_then(|value| value.as_bool()) == Some(true) {
                draft_sections.push(
                    path.strip_suffix("_index.md")
                        .expect("section suffix")
                        .to_owned(),
                );
            }
        }
    }
    let mut pages = Vec::new();
    for (path, source) in sources {
        if !(path.starts_with("docs/") || path.starts_with("changelog/")) {
            continue;
        }
        if draft_sections.iter().any(|prefix| path.starts_with(prefix)) {
            continue;
        }
        let source = source.replace("\r\n", "\n");
        let remainder = source
            .strip_prefix("+++\n")
            .context("Markdown export needs TOML frontmatter")?;
        let (metadata, body) = remainder
            .split_once("\n+++\n")
            .context("unclosed TOML frontmatter")?;
        let metadata: toml_edit::DocumentMut = metadata.parse()?;
        if metadata.get("draft").and_then(|value| value.as_bool()) == Some(true) {
            continue;
        }
        let title = metadata
            .get("title")
            .and_then(|value| value.as_str())
            .context("export page needs title")?;
        ensure!(
            !title.contains(['\n', '\r']),
            "export title must be one line"
        );
        let route = page_route(path, &metadata)?;
        ensure!(
            !route.is_empty()
                && route.split('/').all(|part| !part.is_empty()
                    && part != "."
                    && part != ".."
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))),
            "unsafe Markdown export route"
        );
        pages.push((
            route,
            title.to_owned(),
            format!("# {title}\n\n{}\n", body.trim()),
        ));
    }
    pages.sort_by_key(|(route, _, _)| {
        (
            if route == "docs" {
                0
            } else if route.starts_with("docs/start/") {
                1
            } else {
                2
            },
            route.clone(),
        )
    });
    let intro = "# Workdeck\n\nNative review documentation. The full semantic port remains in progress.\n\n## Notes for agents\n\n- The TUI belongs to the human operator; use `workdeck session *` for an already-open review.\n- Load the versioned review skill at /docs/workdeck-review-skill.md.\n\n";
    let mut index = intro.to_owned();
    let mut full = intro.to_owned();
    let mut small = intro.to_owned();
    let mut outputs = BTreeMap::new();
    for (route, title, markdown) in pages {
        let destination = format!("{route}.md");
        ensure!(
            outputs
                .insert(destination.clone(), markdown.clone())
                .is_none(),
            "duplicate Markdown export route: {route}"
        );
        index.push_str(&format!("- [{title}](/{destination})\n"));
        let section = format!("\n---\n\nSource: /{destination}\n\n{markdown}");
        full.push_str(&section);
        if !route.starts_with("docs/extend/")
            && route != "docs/reference/opentui-components"
            && route != "changelog"
            && !route.starts_with("changelog/")
        {
            small.push_str(&section);
        }
    }
    outputs.insert("llms.txt".into(), index);
    outputs.insert("llms-small.txt".into(), small);
    outputs.insert("llms-full.txt".into(), full);
    Ok(outputs)
}

fn page_route(path: &str, metadata: &toml_edit::DocumentMut) -> Result<String> {
    let section = path.ends_with("/_index.md");
    if section {
        ensure!(
            metadata.get("path").is_none() && metadata.get("slug").is_none(),
            "Zola sections do not support path or slug overrides"
        );
    }
    let inferred = path
        .strip_suffix("/_index.md")
        .or_else(|| path.strip_suffix("/index.md"))
        .unwrap_or_else(|| path.strip_suffix(".md").expect("Markdown path"));
    // An explicit path overrides a page slug in Zola. A bundle's slug replaces
    // the containing directory name, not the literal index.md filename.
    let route = if let Some(value) = metadata.get("path") {
        value
            .as_str()
            .context("export path must be a string")?
            .to_owned()
    } else if let Some(value) = metadata.get("slug") {
        let slug = value.as_str().context("export slug must be a string")?;
        ensure!(
            !slug.is_empty() && !slug.contains('/'),
            "export slug must be one nonempty component"
        );
        match inferred.rsplit_once('/') {
            Some((parent, _)) => format!("{parent}/{slug}"),
            None => slug.to_owned(),
        }
    } else {
        inferred.to_owned()
    };
    Ok(route.trim_matches('/').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_bundle_slug_and_explicit_path_match_zola_0234_routes() {
        // Confirmed by a native Zola 0.23.4 build, not inferred from filenames.
        let sources = BTreeMap::from([
            ("docs/bundle/index.md".into(), page("Bundle", "Bundle body")),
            ("docs/renamed/index.md".into(), "+++\ntitle = 'Renamed bundle'\nslug = 'new-name'\n+++\nRenamed body\n".into()),
            ("docs/original.md".into(), "+++\ntitle = 'Renamed page'\nslug = 'different'\n+++\nPage body\n".into()),
            ("docs/override.md".into(), "+++\ntitle = 'Override'\nslug = 'ignored'\npath = 'elsewhere/custom/'\n+++\nOverride body\n".into()),
        ]);
        let output = render(&sources).unwrap();
        let routes: Vec<_> = output
            .keys()
            .filter(|name| name.ends_with(".md"))
            .map(String::as_str)
            .collect();
        assert_eq!(
            routes,
            [
                "docs/bundle.md",
                "docs/different.md",
                "docs/new-name.md",
                "elsewhere/custom.md"
            ]
        );
        assert!(output["docs/new-name.md"].contains("Renamed body"));
    }

    #[test]
    fn routing_rejects_invalid_types_unsafe_slugs_and_section_overrides() {
        for setting in [
            "slug = '../outside'",
            "slug = ''",
            "slug = '..'",
            "slug = 3",
            "path = 3",
            "slug = 'a/b'",
        ] {
            let source = format!("+++\ntitle = 'Page'\n{setting}\n+++\nbody\n");
            assert!(
                render(&BTreeMap::from([("docs/page.md".into(), source)])).is_err(),
                "{setting}"
            );
        }
        for setting in ["slug = 'renamed'", "path = 'renamed'"] {
            let source = format!("+++\ntitle = 'Section'\n{setting}\n+++\nbody\n");
            assert!(render(&BTreeMap::from([("docs/section/_index.md".into(), source)])).is_err());
        }
    }
    #[test]
    fn emits_only_into_rendered_output_and_rejects_collisions_before_writing() {
        let output = tempfile::tempdir().unwrap();
        let exports = BTreeMap::from([
            ("docs.md".into(), "# Docs\n".into()),
            ("llms.txt".into(), "index".into()),
        ]);
        assert!(emit_plan(&exports, output.path()).is_err());
        assert!(!output.path().join("llms.txt").exists());
        fs::create_dir(output.path().join("docs")).unwrap();
        fs::write(output.path().join("docs/index.html"), "rendered").unwrap();
        fs::write(output.path().join("llms.txt"), "static asset").unwrap();
        assert!(emit_plan(&exports, output.path()).is_err());
        assert!(!output.path().join("docs.md").exists());
        fs::remove_file(output.path().join("llms.txt")).unwrap();
        emit_plan(&exports, output.path()).unwrap();
        assert_eq!(
            fs::read_to_string(output.path().join("docs.md")).unwrap(),
            "# Docs\n"
        );
    }
    fn page(title: &str, body: &str) -> String {
        format!("+++\ntitle = {title:?}\n+++\n\n{body}\n")
    }
    #[test]
    fn exports_source_without_frontmatter_and_keeps_niche_pages_in_full_corpus() {
        let sources = BTreeMap::from([
            (
                "docs/_index.md".into(),
                page("Overview", "Use `--flags` unchanged."),
            ),
            (
                "docs/start/install.md".into(),
                page("Install", "Install body"),
            ),
            ("docs/extend/api.md".into(), page("API", "Native API body")),
            ("changelog/0-1.md".into(), page("Release", "Release body")),
        ]);
        let output = render(&sources).unwrap();
        assert_eq!(
            output["docs.md"],
            "# Overview\n\nUse `--flags` unchanged.\n"
        );
        assert!(output["llms-full.txt"].contains("Native API body"));
        assert!(output["llms-full.txt"].contains("Release body"));
        assert!(!output["llms-small.txt"].contains("Native API body"));
        assert!(!output["llms-small.txt"].contains("Release body"));
        assert!(
            output["llms.txt"].find("[Overview]").unwrap()
                < output["llms.txt"].find("[Install]").unwrap()
        );
    }
    #[test]
    fn rejects_colliding_routes() {
        let sources = BTreeMap::from([
            ("docs/a.md".into(), page("A", "A")),
            ("docs/a/_index.md".into(), page("B", "B")),
        ]);
        assert!(
            render(&sources)
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );
    }

    #[test]
    fn release_routes_accept_series_dots_and_exclude_the_landing_page_from_small() {
        let sources = BTreeMap::from([
            ("changelog/index.md".into(), "+++\ntitle = 'Changelog'\npath = 'changelog/'\n+++\nrelease index body\n".into()),
            ("changelog/0.20.md".into(), "+++\ntitle = 'Workdeck 0.20'\npath = 'changelog/0.20/'\n+++\nrelease series body\n".into()),
        ]);
        let output = render(&sources).unwrap();
        assert!(output.contains_key("changelog.md"));
        assert!(output.contains_key("changelog/0.20.md"));
        for body in ["release index body", "release series body"] {
            assert!(output["llms-full.txt"].contains(body));
            assert!(!output["llms-small.txt"].contains(body));
        }
    }

    #[test]
    fn skips_drafts_and_rejects_unsafe_route_overrides() {
        let draft = "+++\ntitle = 'Unpublished'\ndraft = true\n+++\nprivate draft body\n";
        let output = render(&BTreeMap::from([("docs/draft.md".into(), draft.into())])).unwrap();
        assert!(!output.contains_key("docs/draft.md"));
        assert!(
            output
                .values()
                .all(|value| !value.contains("private draft body"))
        );
        for route in [
            "../escape",
            "docs/../escape",
            "docs//empty",
            "docs/a?query",
            "docs/a\\b",
        ] {
            let source = format!("+++\ntitle = 'Page'\npath = '{route}'\n+++\nbody\n");
            assert!(
                render(&BTreeMap::from([("docs/page.md".into(), source)])).is_err(),
                "{route}"
            );
        }
    }

    #[test]
    fn draft_sections_exclude_descendants_without_hiding_similarly_named_siblings() {
        let sources = BTreeMap::from([
            (
                "docs/private/_index.md".into(),
                "+++\ntitle = 'Draft section'\ndraft = true\n+++\nunpublished section\n".into(),
            ),
            (
                "docs/private/nested/page.md".into(),
                page("Draft child", "unpublished child"),
            ),
            (
                "docs/private-public/page.md".into(),
                page("Public sibling", "public body"),
            ),
        ]);
        let output = render(&sources).unwrap();
        assert!(!output.contains_key("docs/private.md"));
        assert!(!output.contains_key("docs/private/nested/page.md"));
        assert!(output.contains_key("docs/private-public/page.md"));
        assert!(output.values().all(|body| !body.contains("unpublished")));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_export_parent_without_writing_outside_output() {
        let output = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(outside.path().join("page")).unwrap();
        fs::write(outside.path().join("page/index.html"), "rendered").unwrap();
        std::os::unix::fs::symlink(outside.path(), output.path().join("docs")).unwrap();
        let exports = BTreeMap::from([("docs/page.md".into(), "export".into())]);
        assert!(emit_plan(&exports, output.path()).is_err());
        assert!(!outside.path().join("page.md").exists());
        assert_eq!(
            fs::read_to_string(outside.path().join("page/index.html")).unwrap(),
            "rendered"
        );
    }
}
