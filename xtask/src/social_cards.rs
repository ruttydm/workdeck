//! Social-card targets translated from Hunk website/scripts/generate-og.ts (MIT).
//! Copyright (c) Modem Labs Inc. See THIRD_PARTY_NOTICES.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn render_html(target: &Target, font: &[u8]) -> String {
    use base64::Engine;
    let card = &target.card;
    // JavaScript counts UTF-16 code units, not Unicode scalar values.
    let title_size = if card.title.encode_utf16().count() > 12 {
        "82"
    } else {
        "104"
    };
    let font = format!(
        "data:font/woff2;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(font)
    );
    let style = include_str!("social_cards.css")
        .replace("${WIDTH}", "1200")
        .replace("${HEIGHT}", "630")
        .replace("${titleSize}", title_size)
        .replace("${fontDataUri}", &font);
    let latest = if card.latest {
        "<span class=\"pill\">Latest</span>"
    } else {
        ""
    };
    let tagline = card
        .tagline
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| format!("<div class=\"tagline\">{}</div>", escape_html(s)))
        .unwrap_or_default();
    let chips = card
        .chips
        .as_ref()
        .filter(|v| !v.is_empty())
        .map(|values| {
            let chips = values
                .iter()
                .map(|s| format!("<span class=\"chip\">{}</span>", escape_html(s)))
                .collect::<String>();
            format!("<div class=\"chips\">{chips}</div>")
        })
        .unwrap_or_default();
    format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\" />\n<style>\n{style}</style>\n</head>\n<body>\n  <div class=\"mark\">workdeck</div>\n  <div class=\"mid\">\n    <div class=\"vrow\">\n      <span class=\"title\">{}</span>\n      {latest}\n    </div>\n    {tagline}\n    {chips}\n  </div>\n  <div class=\"foot\"><span>{}</span><span>{}</span></div>\n</body>\n</html>",
        escape_html(&card.title),
        escape_html(&card.meta),
        escape_html(&target.footer)
    )
}

pub(super) fn run_html(
    repo: &std::path::Path,
    mut args: impl Iterator<Item = String>,
) -> Result<()> {
    use anyhow::Context;
    let cards = args
        .next()
        .context("social-cards-html requires cards.json and a WOFF2 font path")?;
    let font = args
        .next()
        .context("social-cards-html requires a WOFF2 font path")?;
    let cards = serde_json::from_slice(&std::fs::read(repo.join(cards))?)?;
    let targets = select(cards, &args.collect::<Vec<_>>())?;
    let font = std::fs::read(repo.join(font))?;
    let documents: std::collections::BTreeMap<_, _> = targets
        .iter()
        .map(|target| (target.output_file.clone(), render_html(target, &font)))
        .collect();
    println!("{}", serde_json::to_string_pretty(&documents)?);
    Ok(())
}

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
    #[test]
    fn html_preserves_pinned_geometry_escaping_and_utf16_title_threshold() {
        let mut target = select(vec![], &[]).unwrap().remove(0);
        target.card.title = "😀".repeat(6);
        target.card.tagline = Some("<&\"'".into());
        target.card.chips = Some(vec!["<script>".into()]);
        target.card.latest = true;
        target.card.meta = "&metadata".into();
        target.footer = "<footer>".into();
        let html = render_html(&target, b"font");
        for expected in [
            "width: 1200px",
            "height: 630px",
            "font-size: 104px",
            "data:font/woff2;base64,Zm9udA==",
            "&lt;&amp;&quot;'",
            "&lt;script&gt;",
            "&amp;metadata",
            "&lt;footer&gt;",
            "class=\"pill\">Latest",
        ] {
            assert!(html.contains(expected), "{expected}");
        }
        assert!(!html.contains("${"));
        target.card.title.push('x');
        target.card.latest = false;
        target.card.tagline = Some(String::new());
        target.card.chips = Some(vec![]);
        let html = render_html(&target, b"font");
        assert!(html.contains("font-size: 82px"));
        for absent in ["class=\"pill\"", "class=\"tagline\"", "class=\"chips\""] {
            assert!(!html.contains(absent));
        }
    }
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
