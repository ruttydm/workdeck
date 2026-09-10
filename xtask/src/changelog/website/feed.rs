//! RSS semantics translated from MIT-licensed Hunk generate-changelog.ts.
//! Copyright (c) Modem Labs Inc. See THIRD_PARTY_NOTICES.
use super::*;
use std::collections::BTreeMap;

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn render(
    series: &[ReleaseSeries],
    notes: &BTreeMap<String, String>,
    dates: &BTreeMap<String, String>,
    product: &str,
    origin: &str,
) -> String {
    struct Entry {
        version: String,
        title: String,
        link: String,
        date: String,
        summary: String,
    }
    let mut entries = Vec::new();
    for series in series {
        let summary = truncate_description(
            &to_plain_text(&resolve_summary(
                series,
                notes.get(&series.minor).map(String::as_str),
                dates,
                product,
            )),
            400,
        );
        let series_link = format!("{origin}/changelog/{}/", series.minor);
        let prereleases = series
            .releases
            .iter()
            .filter(|r| r.prerelease && is_published(r, dates))
            .collect::<Vec<_>>();
        if let Some(stable) = series
            .releases
            .iter()
            .find(|r| is_stable_published(r, dates))
        {
            let promotes = prereleases
                .iter()
                .any(|r| r.version.starts_with(&format!("{}-", stable.version)));
            entries.push(Entry {
                version: stable.version.clone(),
                title: format!("{product} {}", series.minor),
                link: if promotes {
                    format!("{series_link}#{}", version_anchor(&stable.version))
                } else {
                    series_link.clone()
                },
                date: dates[&stable.version].clone(),
                summary: summary.clone(),
            });
        }
        for release in prereleases {
            entries.push(Entry {
                version: release.version.clone(),
                title: format!("{product} {} (Prerelease)", release.version),
                link: format!("{series_link}#{}", version_anchor(&release.version)),
                date: dates[&release.version].clone(),
                summary: summary.clone(),
            });
        }
    }
    entries.sort_by(|a, b| {
        b.date
            .cmp(&a.date)
            .then_with(|| compare_versions(&a.version, &b.version))
    });
    let mut lines = vec![
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".into(),
        "<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\">".into(),
        "  <channel>".into(),
        format!("    <title>{product} releases</title>"),
        format!("    <link>{origin}/changelog/</link>"),
        format!(
            "    <description>Release notes for {product}, the review-first terminal diff viewer.</description>"
        ),
        "    <language>en</language>".into(),
        format!(
            "    <atom:link href=\"{origin}/changelog/rss.xml\" rel=\"self\" type=\"application/rss+xml\" />"
        ),
    ];
    for entry in entries {
        let date = chrono::NaiveDate::parse_from_str(&entry.date, "%Y-%m-%d")
            .map(|d| d.format("%a, %d %b %Y 00:00:00 GMT").to_string())
            .unwrap_or_else(|_| "Invalid Date".into());
        lines.extend([
            "    <item>".into(),
            format!("      <title>{}</title>", xml(&entry.title)),
            format!("      <link>{}</link>", entry.link),
            format!("      <guid isPermaLink=\"true\">{}</guid>", entry.link),
            format!("      <pubDate>{date}</pubDate>"),
            format!("      <description>{}</description>", xml(&entry.summary)),
            "    </item>".into(),
        ]);
    }
    lines.extend(["  </channel>".into(), "</rss>".into(), String::new()]);
    lines.join("\n")
}

pub(in crate::changelog) fn run_feed(
    repo: &Path,
    mut args: impl Iterator<Item = String>,
) -> Result<()> {
    let Some(markdown) = args.next() else {
        bail!("changelog feed requires Markdown and dates JSON files");
    };
    let Some(dates) = args.next() else {
        bail!("changelog feed requires Markdown and dates JSON files");
    };
    let notes = args.next();
    if args.next().is_some() {
        bail!("changelog feed accepts Markdown, dates JSON and optional notes JSON files");
    }
    #[derive(serde::Deserialize)]
    struct Note {
        summary: Option<String>,
    }
    let notes: BTreeMap<String, Note> = match notes {
        Some(path) => serde_json::from_str(&std::fs::read_to_string(repo.join(path))?)?,
        None => BTreeMap::new(),
    };
    let notes = notes
        .into_iter()
        .filter_map(|(k, v)| v.summary.map(|v| (k, v)))
        .collect();
    let dates = serde_json::from_str(&std::fs::read_to_string(repo.join(dates))?)?;
    let series = group_into_series(parse_changelog(&std::fs::read_to_string(
        repo.join(markdown),
    )?));
    print!(
        "{}",
        render(&series, &notes, &dates, "Workdeck", "https://workdeck.dev")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn feed_matches_pinned_promotion_order_and_xml_oracles() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../port/hunk/website-changelog-feed-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for (index, result) in results.iter().enumerate() {
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), [8, 2][index]);
            for case in cases {
                let series = group_into_series(parse_changelog(case["input"].as_str().unwrap()));
                let dates = serde_json::from_value(case["dates"].clone()).unwrap();
                let notes =
                    BTreeMap::from([("1.2".into(), case["summary"].as_str().unwrap().into())]);
                assert_eq!(
                    render(&series, &notes, &dates, "Hunk", "https://hunk.dev"),
                    case["expected"]
                );
            }
        }
    }
}
