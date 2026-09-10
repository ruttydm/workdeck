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
