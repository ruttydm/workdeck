//! Landing release metadata from MIT-licensed Hunk generate-changelog.ts.
//! Copyright (c) Modem Labs Inc. See THIRD_PARTY_NOTICES.
use super::*;
use std::collections::BTreeMap;

#[derive(Default, serde::Deserialize)]
pub(super) struct Note {
    summary: Option<String>,
    tagline: Option<String>,
}

pub(super) fn latest(
    releases: Vec<ReleaseEntry>,
    dates: &BTreeMap<String, String>,
    notes: &BTreeMap<String, Note>,
    product: &str,
) -> serde_json::Value {
    let releases: Vec<_> = releases
        .into_iter()
        .filter(|r| !r.prerelease || is_published(r, dates))
        .collect();
    let Some(release) = releases.iter().find(|r| is_stable_published(r, dates)) else {
        return serde_json::Value::Null;
    };
    let version = release.version.clone();
    let minor = minor_series_of(&version);
    let series = group_into_series(releases)
        .into_iter()
        .find(|s| s.minor == minor)
        .unwrap();
    let note = notes.get(&minor);
    let summary = note.and_then(|n| n.tagline.clone()).unwrap_or_else(|| {
        truncate_description(
            &to_plain_text(&resolve_summary(
                &series,
                note.and_then(|n| n.summary.as_deref()),
                dates,
                product,
            )),
            72,
        )
    });
    serde_json::json!({"version":version,"minor":minor,"date":dates[&version],"summary":summary})
}

pub(in crate::changelog) fn run_latest(
    repo: &Path,
    mut args: impl Iterator<Item = String>,
) -> Result<()> {
    let Some(markdown) = args.next() else {
        bail!("changelog latest requires Markdown and recorded dates JSON files");
    };
    let Some(recorded) = args.next() else {
        bail!("changelog latest requires Markdown and recorded dates JSON files");
    };
    let notes = args.next();
    if args.next().is_some() {
        bail!(
            "changelog latest accepts Markdown, recorded dates JSON and optional notes JSON files"
        );
    }
    let releases = parse_changelog(&std::fs::read_to_string(repo.join(markdown))?);
    let recorded = serde_json::from_str(&std::fs::read_to_string(repo.join(recorded))?)?;
    let notes = match notes {
        Some(path) => serde_json::from_str(&std::fs::read_to_string(repo.join(path))?)?,
        None => BTreeMap::new(),
    };
    let dates = resolve_dates(&releases, &recorded, |version| tag_date(repo, version));
    let dates = serde_json::from_value(serde_json::Value::Object(dates))?;
    println!(
        "{}",
        json::format(&latest(releases, &dates, &notes, "Workdeck"))
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_matches_pinned_artifact_oracles() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../port/hunk/website-changelog-latest-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for result in results {
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 6);
            for case in cases {
                let releases = parse_changelog(case["input"].as_str().unwrap());
                let recorded = serde_json::from_value(case["dates"].clone()).unwrap();
                let dates = resolve_dates(&releases, &recorded, |_| None);
                let dates = serde_json::from_value(serde_json::Value::Object(dates)).unwrap();
                let notes = serde_json::from_value(case["notes"].clone()).unwrap();
                assert_eq!(latest(releases, &dates, &notes, "Hunk"), case["expected"]);
            }
        }
    }

    #[test]
    fn pinned_latest_release_metadata_is_accounted_by_native_generator() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let bytes = crate::git_stdout_bytes(
            repo,
            [
                "show",
                "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2:website/releases/latest.json",
            ],
        )
        .unwrap();
        assert_eq!(bytes.len(), 137);
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["version"].as_str(), Some("0.20.1"));
        assert_eq!(value["minor"].as_str(), Some("0.20"));
        assert_eq!(value["date"].as_str(), Some("2026-08-29"));
        assert!(
            value["summary"]
                .as_str()
                .is_some_and(|summary| !summary.is_empty())
        );
        assert!(
            latest(
                vec![ReleaseEntry {
                    version: "0.20.1".into(),
                    prerelease: false,
                    heading_date: None,
                    highlights: None,
                    sections: vec![],
                }],
                &BTreeMap::from([("0.20.1".into(), "2026-08-29".into())]),
                &BTreeMap::new(),
                "Workdeck",
            )
            .get("version")
            .is_some()
        );
    }
}
