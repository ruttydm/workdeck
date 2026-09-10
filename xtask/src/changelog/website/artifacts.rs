//! Native artifact composition from MIT-licensed Hunk generate-changelog.ts.
//! Copyright (c) Modem Labs Inc. See THIRD_PARTY_NOTICES.
use super::*;
use anyhow::Context;
use std::collections::BTreeMap;
mod application;

fn generate(
    markdown: &str,
    recorded: &BTreeMap<String, String>,
    notes: serde_json::Value,
    lookup: impl FnMut(&str) -> Option<String>,
) -> Result<BTreeMap<String, String>> {
    let releases = parse_changelog(markdown);
    let resolved = resolve_dates(&releases, recorded, lookup);
    let dates: BTreeMap<String, String> =
        serde_json::from_value(serde_json::Value::Object(resolved.clone()))?;
    let latest_notes = serde_json::from_value(notes.clone())?;
    let latest_value = latest::latest(parse_changelog(markdown), &dates, &latest_notes, "Workdeck");
    let latest_version = latest_value["version"].as_str();
    let releases = releases
        .into_iter()
        .filter(|r| !r.prerelease || is_published(r, &dates))
        .collect();
    let series = group_into_series(releases);
    let index_notes = serde_json::from_value(notes.clone())?;
    let page_notes: BTreeMap<String, pages::Notes> = serde_json::from_value(notes.clone())?;
    #[derive(serde::Deserialize)]
    struct Summary {
        summary: Option<String>,
    }
    let summaries: BTreeMap<String, Summary> = serde_json::from_value(notes)?;
    let summaries: BTreeMap<String, String> = summaries
        .into_iter()
        .filter_map(|(k, v)| v.summary.map(|s| (k, s)))
        .collect();
    let mut cards = vec![index_card(&series, &dates, "Workdeck")];
    let mut output = BTreeMap::from([
        (
            "site/data/releases/dates.json".into(),
            format!("{}\n", json::format(&serde_json::Value::Object(resolved))),
        ),
        (
            "site/data/releases/latest.json".into(),
            format!("{}\n", json::format(&latest_value)),
        ),
        (
            "site/content/changelog/index.md".into(),
            index::render(&series, &index_notes, &dates),
        ),
        (
            "site/static/changelog/rss.xml".into(),
            feed::render(
                &series,
                &summaries,
                &dates,
                "Workdeck",
                "https://workdeck.dev",
            ),
        ),
    ]);
    for (i, item) in series.iter().enumerate() {
        cards.push(series_card(
            item,
            summaries.get(&item.minor).map(String::as_str),
            &dates,
            latest_value["minor"].as_str() == Some(&item.minor),
            "Workdeck",
        ));
        output.insert(
            format!("site/content/changelog/{}.md", item.minor),
            pages::render_page(
                item,
                page_notes
                    .get(&item.minor)
                    .unwrap_or(&pages::Notes::default()),
                &dates,
                i.checked_sub(1).map(|j| series[j].minor.as_str()),
                series.get(i + 1).map(|s| s.minor.as_str()),
                latest_version,
            )?,
        );
    }
    output.insert(
        "site/data/releases/cards.json".into(),
        format!("{}\n", json::format(&cards.into())),
    );
    Ok(output)
}

pub(in crate::changelog) fn run_artifacts(
    repo: &Path,
    mut args: impl Iterator<Item = String>,
    mode: &str,
) -> Result<()> {
    let saved_path = if matches!(mode, "plan-check" | "apply") {
        Some(
            args.next()
                .context("artifacts-plan-check requires a saved plan path")?,
        )
    } else {
        None
    };
    let backup = if mode == "apply" {
        Some(
            args.next()
                .context("artifacts-apply requires a new external backup directory")?,
        )
    } else {
        None
    };
    let Some(markdown) = args.next() else {
        bail!("changelog artifacts requires Markdown and recorded dates JSON files");
    };
    let Some(recorded) = args.next() else {
        bail!("changelog artifacts requires Markdown and recorded dates JSON files");
    };
    let notes = args.next();
    if args.next().is_some() {
        bail!(
            "changelog artifacts accepts Markdown, recorded dates JSON and optional notes JSON files"
        );
    }
    // Coordinate with native release preparation before reading generation inputs.
    let _lock = if mode == "apply" {
        Some(crate::changelog::fragments::repository_release_lock(repo)?)
    } else {
        None
    };
    let markdown = std::fs::read_to_string(repo.join(markdown))?;
    let recorded = serde_json::from_str(&std::fs::read_to_string(repo.join(recorded))?)?;
    let notes = match notes {
        Some(path) => serde_json::from_str(&std::fs::read_to_string(repo.join(path))?)?,
        None => serde_json::json!({}),
    };
    let output = generate(&markdown, &recorded, notes, |v| tag_date(repo, v))?;
    if let Some(path) = saved_path {
        let saved: ArtifactPlan = serde_json::from_slice(&std::fs::read(repo.join(path))?)?;
        saved.validate()?;
        let current = write_plan(repo, &output)?;
        anyhow::ensure!(
            serde_json::to_value(&saved)? == current,
            "saved artifact plan is stale or modified"
        );
        if let Some(backup) = backup {
            application::apply(repo, &saved, &repo.join(backup), |_| Ok(()))?;
            println!(
                "{}",
                serde_json::json!({"applied":true,"artifacts":saved.edits.len()})
            );
        }
        return Ok(());
    }
    if mode == "plan" {
        println!(
            "{}",
            serde_json::to_string_pretty(&write_plan(repo, &output)?)?
        );
        return Ok(());
    }
    if mode == "check" {
        let stale = stale_paths(repo, &output)?;
        if !stale.is_empty() {
            bail!(
                "Generated changelog artifacts are stale:\n{}",
                stale.join("\n")
            );
        }
        let missing = missing_card_images(repo, &output)?;
        if !missing.is_empty() {
            bail!(
                "Missing {} social card image(s):\n{}",
                missing.len(),
                missing.join("\n")
            );
        }
        return Ok(());
    }
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn write_plan(repo: &Path, artifacts: &BTreeMap<String, String>) -> Result<serde_json::Value> {
    let stale = stale_paths(repo, artifacts)?;
    let mut edits = BTreeMap::new();
    let mut originals = BTreeMap::<String, Option<Vec<u8>>>::new();
    for path in stale {
        let original = match std::fs::symlink_metadata(repo.join(&path)) {
            Ok(metadata) => {
                anyhow::ensure!(
                    metadata.is_file() && !metadata.file_type().is_symlink(),
                    "artifact target must be a regular file: {path}"
                );
                Some(std::fs::read(repo.join(&path))?)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        edits.insert(path.clone(), artifacts.get(&path).cloned());
        originals.insert(path, original);
    }
    let plan = ArtifactPlan {
        schema: 1,
        edits,
        originals,
    };
    plan.validate()?;
    Ok(serde_json::to_value(plan)?)
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactPlan {
    schema: u32,
    edits: BTreeMap<String, Option<String>>,
    originals: BTreeMap<String, Option<Vec<u8>>>,
}

impl ArtifactPlan {
    fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.schema == 1, "unsupported artifact plan schema");
        anyhow::ensure!(
            self.edits.keys().eq(self.originals.keys()),
            "artifact originals must match edit paths"
        );
        let minor = regex::Regex::new(r"^[0-9]+\.[0-9]+\.md$")?;
        for (path, replacement) in &self.edits {
            let series = path
                .strip_prefix("site/content/changelog/")
                .is_some_and(|name| minor.is_match(name));
            let fixed = matches!(
                path.as_str(),
                "site/content/changelog/index.md"
                    | "site/static/changelog/rss.xml"
                    | "site/data/releases/dates.json"
                    | "site/data/releases/latest.json"
                    | "site/data/releases/cards.json"
            );
            anyhow::ensure!(
                series || fixed,
                "artifact plan target is outside generated release outputs: {path}"
            );
            let original = &self.originals[path];
            if replacement.is_none() {
                anyhow::ensure!(
                    series && original.is_some(),
                    "only an existing generated series page may be removed: {path}"
                );
            }
            anyhow::ensure!(
                replacement.as_ref().map(|s| s.as_bytes()) != original.as_deref(),
                "artifact plan contains unchanged target: {path}"
            );
        }
        Ok(())
    }
}

fn missing_card_images(repo: &Path, artifacts: &BTreeMap<String, String>) -> Result<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct Card {
        slug: String,
    }
    let cards: Vec<Card> = serde_json::from_str(&artifacts["site/data/releases/cards.json"])?;
    Ok(cards
        .into_iter()
        .map(|card| format!("site/static/changelog/og/{}.png", card.slug))
        .filter(|path| !repo.join(path).exists())
        .collect())
}

fn stale_paths(repo: &Path, artifacts: &BTreeMap<String, String>) -> Result<Vec<String>> {
    let directory = repo.join("site/content/changelog");
    let mut pages = Vec::new();
    match std::fs::read_dir(&directory) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let name = entry.file_name();
                if name.to_string_lossy().ends_with(".md") {
                    pages.push(format!("site/content/changelog/{}", name.to_string_lossy()));
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    pages.sort();
    let mut stale: Vec<_> = pages
        .iter()
        .filter(|p| !artifacts.contains_key(*p))
        .cloned()
        .collect();
    if pages.len() > 1 && stale.len() > pages.len() / 2 {
        bail!(
            "Refusing to remove {} of {} generated changelog pages. This usually means CHANGELOG.md failed to parse rather than that releases were removed.",
            stale.len(),
            pages.len()
        );
    }
    for (path, content) in artifacts {
        if std::fs::read(repo.join(path)).ok().as_deref() != Some(content.as_bytes()) {
            stale.push(path.clone());
        }
    }
    Ok(stale)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../../../port/hunk/website-changelog-test-sample.md");

    #[test]
    fn artifact_plan_validation_rejects_unscoped_and_inconsistent_edits() {
        for path in [
            "../outside",
            "/tmp/outside",
            "site/content/changelog/../../outside",
            "site/content/changelog/notes.md",
            "site/data/releases/other.json",
        ] {
            let plan = ArtifactPlan {
                schema: 1,
                edits: BTreeMap::from([(path.into(), Some("new".into()))]),
                originals: BTreeMap::from([(path.into(), None)]),
            };
            assert!(plan.validate().is_err(), "{path}");
        }
        let path = "site/content/changelog/1.0.md".to_owned();
        let mut plan = ArtifactPlan {
            schema: 1,
            edits: BTreeMap::from([(path.clone(), None)]),
            originals: BTreeMap::from([(path.clone(), Some(vec![255]))]),
        };
        plan.validate().unwrap();
        plan.originals.insert(path.clone(), None);
        assert!(plan.validate().is_err());
        plan.edits.insert(path.clone(), Some("same".into()));
        plan.originals.insert(path.clone(), Some(b"same".to_vec()));
        assert!(plan.validate().is_err());
        plan.originals.clear();
        assert!(plan.validate().is_err());
        let invalid = serde_json::json!({"schema":1,"edits":{},"originals":{},"unknown":true});
        assert!(serde_json::from_value::<ArtifactPlan>(invalid).is_err());
    }

    #[test]
    fn artifact_write_plan_records_exact_originals_and_orphans_without_writes() {
        let repo = tempfile::tempdir().unwrap();
        let directory = repo.path().join("site/content/changelog");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("index.md"), "same").unwrap();
        std::fs::write(directory.join("1.0.md"), [0xff, 0, 1]).unwrap();
        let artifacts = BTreeMap::from([
            ("site/content/changelog/index.md".into(), "same".into()),
            ("site/data/releases/latest.json".into(), "null\n".into()),
        ]);
        let plan = write_plan(repo.path(), &artifacts).unwrap();
        assert_eq!(plan["schema"], 1);
        assert_eq!(plan["edits"].as_object().unwrap().len(), 2);
        assert!(plan["edits"]["site/content/changelog/1.0.md"].is_null());
        assert_eq!(
            plan["originals"]["site/content/changelog/1.0.md"],
            serde_json::json!([255, 0, 1])
        );
        assert!(plan["originals"]["site/data/releases/latest.json"].is_null());
        assert_eq!(plan["edits"]["site/data/releases/latest.json"], "null\n");
        assert!(!repo.path().join("site/data").exists());
        assert_eq!(
            std::fs::read(directory.join("1.0.md")).unwrap(),
            [255, 0, 1]
        );
        assert_eq!(write_plan(repo.path(), &artifacts).unwrap(), plan);
    }

    fn pinned_default_artifacts() -> BTreeMap<String, String> {
        let read = |path: &str| {
            let output = std::process::Command::new("git")
                .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap())
                .args([
                    "show",
                    &format!("2c00f4358b89cfc0a6b04459ffc538ba601aa3c2:{path}"),
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap()
        };
        generate(
            &read("CHANGELOG.md"),
            &serde_json::from_str(&read("website/releases/dates.json")).unwrap(),
            serde_json::from_str(&read("website/releases/notes.json")).unwrap(),
            |_| None,
        )
        .unwrap()
    }

    fn install_test_artifacts(repo: &Path, artifacts: &BTreeMap<String, String>) {
        for (path, content) in artifacts {
            let path = repo.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
    }

    #[test]
    fn source_cleanup_refuses_removing_most_generated_pages() {
        let repo = tempfile::tempdir().unwrap();
        let artifacts = pinned_default_artifacts();
        install_test_artifacts(repo.path(), &artifacts);
        assert!(
            stale_paths(repo.path(), &BTreeMap::new())
                .unwrap_err()
                .to_string()
                .contains("Refusing to remove")
        );
        for (path, content) in artifacts {
            assert_eq!(
                std::fs::read_to_string(repo.path().join(path)).unwrap(),
                content
            );
        }
    }

    #[test]
    fn source_cleanup_reports_genuinely_stale_output() {
        let repo = tempfile::tempdir().unwrap();
        let mut artifacts = pinned_default_artifacts();
        install_test_artifacts(repo.path(), &artifacts);
        // The source inserts dates.json first; native map ordering is separate.
        let first = "site/data/releases/dates.json";
        assert!(artifacts.contains_key(first));
        let original = artifacts.insert(first.into(), "stale".into()).unwrap();
        assert_eq!(stale_paths(repo.path(), &artifacts).unwrap(), [first]);
        assert_eq!(
            std::fs::read_to_string(repo.path().join(first)).unwrap(),
            original
        );
    }

    #[test]
    fn missing_card_images_preserve_order_duplicates_and_presence_semantics() {
        let repo = tempfile::tempdir().unwrap();
        let artifacts = BTreeMap::from([(
            "site/data/releases/cards.json".into(),
            r#"[{"slug":"2.0"},{"slug":"index"},{"slug":"2.0"}]"#.into(),
        )]);
        assert_eq!(
            missing_card_images(repo.path(), &artifacts).unwrap(),
            [
                "site/static/changelog/og/2.0.png",
                "site/static/changelog/og/index.png",
                "site/static/changelog/og/2.0.png"
            ]
        );
        assert!(!repo.path().join("site").exists());
        let directory = repo.path().join("site/static/changelog/og");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("2.0.png"),
            "not a PNG: presence-only fixture",
        )
        .unwrap();
        assert_eq!(
            missing_card_images(repo.path(), &artifacts).unwrap(),
            ["site/static/changelog/og/index.png"]
        );
        // Like existsSync, the source presence gate also accepts a directory.
        // A separate image-validation gate must reject invalid image content.
        std::fs::create_dir(directory.join("index.png")).unwrap();
        assert!(
            missing_card_images(repo.path(), &artifacts)
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn missing_card_images_follow_links_and_report_dangling_links() {
        let repo = tempfile::tempdir().unwrap();
        let directory = repo.path().join("site/static/changelog/og");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("present"), "presence fixture").unwrap();
        std::os::unix::fs::symlink("present", directory.join("1.0.png")).unwrap();
        std::os::unix::fs::symlink("absent", directory.join("2.0.png")).unwrap();
        let artifacts = BTreeMap::from([(
            "site/data/releases/cards.json".into(),
            r#"[{"slug":"1.0"},{"slug":"2.0"}]"#.into(),
        )]);
        assert_eq!(
            missing_card_images(repo.path(), &artifacts).unwrap(),
            ["site/static/changelog/og/2.0.png"]
        );
        assert_eq!(
            std::fs::read_link(directory.join("2.0.png")).unwrap(),
            std::path::PathBuf::from("absent")
        );
    }

    #[test]
    fn artifact_check_refuses_collapsed_output_without_deleting_pages() {
        let repo = tempfile::tempdir().unwrap();
        let directory = repo.path().join("site/content/changelog");
        std::fs::create_dir_all(&directory).unwrap();
        for name in ["index.md", "1.0.md", "1.1.md"] {
            std::fs::write(directory.join(name), "original").unwrap();
        }
        let error = stale_paths(repo.path(), &BTreeMap::new()).unwrap_err();
        assert!(error.to_string().contains("Refusing to remove 3 of 3"));
        for name in ["index.md", "1.0.md", "1.1.md"] {
            assert_eq!(
                std::fs::read_to_string(directory.join(name)).unwrap(),
                "original"
            );
        }
    }

    #[test]
    fn artifact_check_reports_missing_changed_and_orphaned_paths_without_writes() {
        let repo = tempfile::tempdir().unwrap();
        let directory = repo.path().join("site/content/changelog");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("index.md"), "same").unwrap();
        std::fs::write(directory.join("old.md"), "old").unwrap();
        let artifacts = BTreeMap::from([
            ("site/content/changelog/index.md".into(), "same".into()),
            ("site/data/releases/latest.json".into(), "null\n".into()),
        ]);
        assert_eq!(
            stale_paths(repo.path(), &artifacts).unwrap(),
            [
                "site/content/changelog/old.md",
                "site/data/releases/latest.json"
            ]
        );
        assert!(!repo.path().join("site/data").exists());
        assert_eq!(
            std::fs::read_to_string(directory.join("old.md")).unwrap(),
            "old"
        );
        let artifacts = BTreeMap::from([
            ("site/content/changelog/index.md".into(), "same".into()),
            ("site/content/changelog/old.md".into(), "old".into()),
        ]);
        assert!(stale_paths(repo.path(), &artifacts).unwrap().is_empty());
        std::fs::write(directory.join("index.md"), "changed").unwrap();
        assert_eq!(
            stale_paths(repo.path(), &artifacts).unwrap(),
            ["site/content/changelog/index.md"]
        );
        assert_eq!(
            std::fs::read_to_string(directory.join("index.md")).unwrap(),
            "changed"
        );
    }

    fn source_beta_artifacts() -> BTreeMap<String, String> {
        generate(
            SAMPLE,
            &BTreeMap::from([
                ("0.19.0".into(), "2026-08-16".into()),
                ("0.19.0-beta.0".into(), "2026-08-10".into()),
            ]),
            serde_json::json!({}),
            |_| None,
        )
        .unwrap()
    }

    #[test]
    fn source_beta_renders_date_and_exact_anchor() {
        let artifacts = source_beta_artifacts();
        let page = &artifacts["site/content/changelog/0.19.md"];
        assert!(page.contains("### 0.19.0-beta.0"));
        assert!(page.contains("id=\"v0-19-0-beta-0\""));
        assert!(page.contains("August 10, 2026"));
    }

    #[test]
    fn source_beta_counts_in_series_dateline() {
        assert!(source_beta_artifacts()["site/content/changelog/0.19.md"].contains("2 releases"));
    }

    #[test]
    fn source_beta_retains_dates_and_legacy_heading() {
        let artifacts = source_beta_artifacts();
        let dates: serde_json::Value =
            serde_json::from_str(&artifacts["site/data/releases/dates.json"]).unwrap();
        assert_eq!(
            dates,
            serde_json::json!({"0.19.0":"2026-08-16","0.19.0-beta.0":"2026-08-10","0.15.3":"2026-06-13"})
        );
    }

    #[test]
    fn source_beta_only_series_is_neither_latest_nor_stable_installable() {
        let artifacts = generate(
            "# Changelog\n\n## 0.20.0-beta.0\n\n### Patch Changes\n\n- 1234567: Early.\n",
            &BTreeMap::from([("0.20.0-beta.0".into(), "2026-09-01".into())]),
            serde_json::json!({}),
            |_| None,
        )
        .unwrap();
        let page = &artifacts["site/content/changelog/0.20.md"];
        let index = &artifacts["site/content/changelog/index.md"];
        assert!(page.contains("Prerelease · September 1, 2026 · 1 release"));
        assert!(page.contains("### 0.20.0-beta.0"));
        assert!(!page.contains("cargo install"));
        assert!(!page.contains("workdeck update"));
        assert!(index.contains("Prerelease · September 1, 2026"));
        assert!(!index.contains("Latest"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&artifacts["site/data/releases/latest.json"])
                .unwrap(),
            serde_json::Value::Null
        );
        assert!(!page.contains("npm i -g"));
    }

    #[test]
    fn source_beta_undated_release_has_no_page() {
        let artifacts = generate(
            "# Changelog\n\n## 0.20.0-beta.0\n\n### Patch Changes\n\n- 1234567: Early.\n",
            &BTreeMap::new(),
            serde_json::json!({}),
            |_| None,
        )
        .unwrap();
        assert!(!artifacts.keys().any(|p| p.ends_with("0.20.md")));
    }

    fn sample_dates(current: bool) -> BTreeMap<String, String> {
        let mut dates = BTreeMap::from([
            ("0.18.0".into(), "2026-08-08".into()),
            ("0.15.3".into(), "2026-06-13".into()),
        ]);
        if current {
            dates.insert("0.19.0".into(), "2026-08-16".into());
        }
        dates
    }

    #[test]
    fn source_artifacts_include_series_index_feed_and_data() {
        let output =
            generate(SAMPLE, &sample_dates(true), serde_json::json!({}), |_| None).unwrap();
        let paths: Vec<_> = output
            .keys()
            .map(|p| {
                p.split('/')
                    .rev()
                    .take(2)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .collect();
        for path in [
            "changelog/0.19.md",
            "changelog/0.18.md",
            "changelog/0.15.md",
            "changelog/index.md",
            "changelog/rss.xml",
            "releases/dates.json",
            "releases/latest.json",
        ] {
            assert!(paths.iter().any(|p| p == path));
        }
    }

    #[test]
    fn source_artifacts_record_newest_published_release() {
        let output =
            generate(SAMPLE, &sample_dates(true), serde_json::json!({}), |_| None).unwrap();
        let latest: serde_json::Value =
            serde_json::from_str(&output["site/data/releases/latest.json"]).unwrap();
        assert_eq!(latest["version"], "0.19.0");
        assert_eq!(latest["minor"], "0.19");
    }

    #[test]
    fn source_artifacts_unpublished_changesets_have_no_latest() {
        let output = generate(
            "# Changelog\n\n## 0.19.0\n\n### Patch Changes\n\n- 1234567: Something.\n",
            &BTreeMap::new(),
            serde_json::json!({}),
            |_| None,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output["site/data/releases/latest.json"])
                .unwrap(),
            serde_json::Value::Null
        );
    }

    #[test]
    fn source_artifacts_are_identical_on_repeated_generation() {
        let first = generate(SAMPLE, &sample_dates(true), serde_json::json!({}), |_| None).unwrap();
        let again = generate(SAMPLE, &sample_dates(true), serde_json::json!({}), |_| None).unwrap();
        assert_eq!(first, again);
    }

    #[test]
    fn source_pretag_index_marks_published_series_latest() {
        let series = group_into_series(parse_changelog(SAMPLE));
        let page = index::render(&series, &BTreeMap::new(), &sample_dates(false));
        let parts: Vec<_> = page.split("## [Workdeck ").skip(1).take(2).collect();
        assert!(parts[0].contains("Unreleased"));
        assert!(!parts[0].contains("Latest"));
        assert!(parts[1].contains("Latest"));
    }

    #[test]
    fn source_pretag_feed_excludes_unreleased_series() {
        let series = group_into_series(parse_changelog(SAMPLE));
        let feed = feed::render(
            &series,
            &BTreeMap::new(),
            &sample_dates(false),
            "Workdeck",
            "https://workdeck.dev",
        );
        assert!(!feed.contains("/changelog/0.19/"));
        assert!(feed.contains("/changelog/0.18/"));
    }

    #[test]
    fn source_pretag_landing_retains_published_release() {
        let output = generate(SAMPLE, &sample_dates(false), serde_json::json!({}), |_| {
            None
        })
        .unwrap();
        let latest: serde_json::Value =
            serde_json::from_str(&output["site/data/releases/latest.json"]).unwrap();
        assert_eq!(latest["version"], "0.18.0");
        assert_eq!(latest["minor"], "0.18");
    }

    #[test]
    fn artifact_data_bytes_match_both_pinned_generators() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../port/hunk/website-changelog-artifact-data-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for result in results {
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 2);
            for case in cases {
                let dates = serde_json::from_value(case["dates"].clone()).unwrap();
                let output = generate(
                    case["input"].as_str().unwrap(),
                    &dates,
                    case["notes"].clone(),
                    |_| None,
                )
                .unwrap();
                assert_eq!(case["artifacts"].as_object().unwrap().len(), 4);
                for name in ["dates.json", "latest.json", "cards.json", "rss.xml"] {
                    let path = if name == "rss.xml" {
                        format!("site/static/changelog/{name}")
                    } else {
                        format!("site/data/releases/{name}")
                    };
                    let expected = case["artifacts"][name]
                        .as_str()
                        .unwrap()
                        .replace("Hunk", "Workdeck")
                        .replace("https://hunk.dev", "https://workdeck.dev");
                    assert_eq!(output[&path], expected, "{} {name}", result["baseline"]);
                }
            }
        }
    }

    #[test]
    fn artifacts_connect_publication_pages_feed_cards_and_latest() {
        let markdown = "## 2.0.0\n## 1.2.0-beta.1\n## 1.1.0\n";
        let dates = BTreeMap::from([("1.1.0".into(), "2026-08-01".into())]);
        let output = generate(markdown, &dates, serde_json::json!({}), |_| None).unwrap();
        assert_eq!(output.len(), 7);
        assert!(!output.contains_key("site/content/changelog/1.2.md"));
        assert!(output.contains_key("site/content/changelog/2.0.md"));
        let latest: serde_json::Value =
            serde_json::from_str(&output["site/data/releases/latest.json"]).unwrap();
        assert_eq!(latest["version"], "1.1.0");
        assert!(output["site/content/changelog/1.1.md"].contains("This is the current release."));
        assert!(output["site/content/changelog/1.1.md"].contains("[Newer: Workdeck 2.0]"));
        assert!(!output["site/static/changelog/rss.xml"].contains("beta.1"));
        let cards: serde_json::Value =
            serde_json::from_str(&output["site/data/releases/cards.json"]).unwrap();
        assert_eq!(cards.as_array().unwrap().len(), 3);
        assert_eq!(cards[2]["latest"], true);
        let output = generate(markdown, &dates, serde_json::json!({}), |v| {
            (v == "1.2.0-beta.1").then(|| "2026-08-02".into())
        })
        .unwrap();
        assert_eq!(output.len(), 8);
        assert!(output.contains_key("site/content/changelog/1.2.md"));
        assert!(output["site/static/changelog/rss.xml"].contains("1.2.0-beta.1 (Prerelease)"));
    }
}
