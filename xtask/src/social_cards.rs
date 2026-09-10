//! Social-card targets translated from Hunk website/scripts/generate-og.ts (MIT).
//! Copyright (c) Modem Labs Inc. See THIRD_PARTY_NOTICES.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Card {
    pub slug: String,
    pub title: String,
    pub tagline: Option<String>,
    pub meta: String,
    pub chips: Option<Vec<String>>,
    #[serde(default)]
    pub latest: bool,
    pub alt: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(super) struct Target {
    pub card: Card,
    pub footer: String,
    pub output_file: String,
}

fn select(cards: Vec<Card>, requested: &[String]) -> Result<Vec<Target>> {
    let mut targets = Vec::new();
    for card in cards {
        // Slugs become staging filenames and published paths; never allow a
        // generated manifest to address a parent or absolute path.
        ensure!(
            !card.slug.is_empty()
                && card.slug != "."
                && card.slug != ".."
                && card
                    .slug
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')),
            "unsafe social-card slug: {:?}",
            card.slug
        );
        targets.push(Target {
            output_file: format!("site/static/changelog/og/{}.png", card.slug),
            footer: "workdeck.dev/changelog".into(),
            card,
        });
    }
    targets.push(Target {
        card: Card {
            slug: "extensions".into(), title: "Extensions".into(),
            tagline: Some("Make Workdeck your own. Community extensions for panes, themes, highlighters, and more.".into()),
            meta: "workdeck extension install <owner>/<repo>".into(),
            alt: "Workdeck extensions: community extensions for panes, themes, highlighters, and more.".into(),
            chips: None, latest: false,
        },
        footer: "workdeck.dev/extensions".into(),
        output_file: "site/static/extensions/og.png".into(),
    });
    let known = targets
        .iter()
        .map(|t| t.card.slug.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let requested_set: BTreeSet<_> = requested.iter().collect();
    let selected: Vec<_> = targets
        .into_iter()
        .filter(|target| requested.is_empty() || requested_set.contains(&target.card.slug))
        .collect();
    let mut seen = BTreeSet::new();
    let unique_requested: Vec<_> = requested
        .iter()
        .filter(|slug| seen.insert(slug.as_str()))
        .map(String::as_str)
        .collect();
    ensure!(
        !selected.is_empty(),
        "No cards matched {}. Known slugs: {known}",
        unique_requested.join(", ")
    );
    Ok(selected)
}

pub(super) fn run(repo: &std::path::Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    use anyhow::Context;
    let input = args
        .next()
        .context("social-cards-plan requires cards.json and optional slugs")?;
    let cards = serde_json::from_slice(&std::fs::read(repo.join(input))?)?;
    let requested: Vec<_> = args.collect();
    let targets = select(cards, &requested)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "width":1200, "height":630, "replaceChangelogDirectory":requested.is_empty(),
            "targets":targets, "rendered":false
        }))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn card(slug: &str) -> Card {
        Card {
            slug: slug.into(),
            title: slug.into(),
            tagline: None,
            meta: "meta".into(),
            chips: None,
            latest: false,
            alt: "alt".into(),
        }
    }
    #[test]
    fn all_and_targeted_selection_preserve_source_order_and_static_page() {
        let targets = select(vec![card("index"), card("0.20")], &[]).unwrap();
        assert_eq!(
            targets
                .iter()
                .map(|t| t.card.slug.as_str())
                .collect::<Vec<_>>(),
            ["index", "0.20", "extensions"]
        );
        assert_eq!(targets[2].output_file, "site/static/extensions/og.png");
        let selected = select(
            vec![card("index"), card("0.20")],
            &[
                "extensions".into(),
                "0.20".into(),
                "0.20".into(),
                "unknown".into(),
            ],
        )
        .unwrap();
        assert_eq!(
            selected
                .iter()
                .map(|t| t.card.slug.as_str())
                .collect::<Vec<_>>(),
            ["0.20", "extensions"]
        );
        assert_eq!(select(vec![], &[]).unwrap().len(), 1);
        assert!(
            select(vec![], &["missing".into(), "missing".into()])
                .unwrap_err()
                .to_string()
                .contains("No cards matched missing. Known slugs: extensions")
        );
    }
    #[test]
    fn manifest_slugs_cannot_escape_staging_or_publication_roots() {
        for slug in ["", ".", "..", "../index", "/tmp/card", "a\\b", "a\0b"] {
            assert!(select(vec![card(slug)], &[]).is_err(), "{slug:?}");
        }
    }
}
