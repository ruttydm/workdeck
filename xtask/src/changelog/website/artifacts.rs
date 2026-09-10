//! Native artifact composition from MIT-licensed Hunk generate-changelog.ts.
//! Copyright (c) Modem Labs Inc. See THIRD_PARTY_NOTICES.
use super::*;
use std::collections::BTreeMap;

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
) -> Result<()> {
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
    let markdown = std::fs::read_to_string(repo.join(markdown))?;
    let recorded = serde_json::from_str(&std::fs::read_to_string(repo.join(recorded))?)?;
    let notes = match notes {
        Some(path) => serde_json::from_str(&std::fs::read_to_string(repo.join(path))?)?,
        None => serde_json::json!({}),
    };
    let output = generate(&markdown, &recorded, notes, |v| tag_date(repo, v))?;
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../../../port/hunk/website-changelog-test-sample.md");

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
