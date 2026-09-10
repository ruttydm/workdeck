//! Social-card targets translated from Hunk website/scripts/generate-og.ts (MIT).
//! Copyright (c) Modem Labs Inc. See THIRD_PARTY_NOTICES.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
mod publication;

pub(super) fn run_publish(
    repo: &std::path::Path,
    mut args: impl Iterator<Item = String>,
) -> Result<()> {
    use anyhow::Context;
    let saved = args
        .next()
        .context("social-cards-publish requires plan, backup, staging directory and cards.json")?;
    let backup = args.next().context("backup directory required")?;
    let staging = args.next().context("staging directory required")?;
    let cards = args.next().context("cards.json required")?;
    let requested: Vec<_> = args.collect();
    let _lock = crate::changelog::publication_lock(repo)?;
    let saved: publication::Plan = serde_json::from_slice(&std::fs::read(repo.join(saved))?)?;
    let targets = select(
        serde_json::from_slice(&std::fs::read(repo.join(cards))?)?,
        &requested,
    )?;
    let current = publication::plan(repo, &repo.join(staging), &targets, requested.is_empty())?;
    ensure!(saved == current, "publication plan is stale or modified");
    let changed = !current.replacements.is_empty() || !current.remove_directories.is_empty();
    if changed {
        publication::apply(repo, &current, &repo.join(backup), |_| Ok(()))?;
    }
    println!(
        "{}",
        serde_json::json!({"applied": changed, "files": current.replacements.len()})
    );
    Ok(())
}

pub(super) fn run_publication_plan(
    repo: &std::path::Path,
    mut args: impl Iterator<Item = String>,
) -> Result<()> {
    use anyhow::Context;
    let staging = args
        .next()
        .context("social-cards-publication-plan requires staging directory and cards.json")?;
    let cards = args.next().context("cards.json required")?;
    let requested: Vec<_> = args.collect();
    let targets = select(
        serde_json::from_slice(&std::fs::read(repo.join(cards))?)?,
        &requested,
    )?;
    let plan = publication::plan(repo, &repo.join(staging), &targets, requested.is_empty())?;
    println!("{}", serde_json::to_string_pretty(&plan)?);
    Ok(())
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(super) fn run_check(
    repo: &std::path::Path,
    mut args: impl Iterator<Item = String>,
) -> Result<()> {
    use anyhow::Context;
    let staging = args
        .next()
        .context("social-cards-check requires staging directory and cards.json")?;
    let cards = args.next().context("cards.json required")?;
    let requested: Vec<_> = args.collect();
    let targets = select(
        serde_json::from_slice(&std::fs::read(repo.join(cards))?)?,
        &requested,
    )?;
    check_capture(&repo.join(staging), &targets, requested.is_empty())?;
    println!(
        "{}",
        serde_json::json!({"valid":true,"published":false,"images":targets.len()})
    );
    Ok(())
}

fn check_capture(
    staging: &std::path::Path,
    targets: &[Target],
    full: bool,
) -> Result<Vec<Vec<u8>>> {
    let metadata = std::fs::symlink_metadata(staging)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "capture staging must be a real directory"
    );
    let manifest = staging.join("capture.json");
    let metadata = std::fs::symlink_metadata(&manifest)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "capture manifest must be a regular file"
    );
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(manifest)?)?;
    let (current, images) = capture_snapshot(&staging.canonicalize()?, targets, full)?;
    ensure!(
        saved == current,
        "capture manifest is stale, modified or belongs to another staging directory"
    );
    for bytes in &images {
        crate::term_video::validate_card_png_bytes(bytes)?;
    }
    Ok(images)
}

pub(super) fn run_capture(
    repo: &std::path::Path,
    mut args: impl Iterator<Item = String>,
) -> Result<()> {
    use anyhow::Context;
    let cards = args
        .next()
        .context("social-cards-capture requires cards, font, WebDriver and Chromium paths")?;
    let font = args.next().context("font path required")?;
    let driver = args.next().context("WebDriver path required")?;
    let chromium = args.next().context("Chromium path required")?;
    let requested: Vec<_> = args.collect();
    let targets = select(
        serde_json::from_slice(&std::fs::read(repo.join(cards))?)?,
        &requested,
    )?;
    let font = std::fs::read(repo.join(font))?;
    let documents: Vec<_> = targets.iter().map(|t| render_html(t, &font)).collect();
    let staged = crate::term_video::capture_card_documents(
        &repo.join(driver),
        &repo.join(chromium),
        &documents,
    )?;
    let report = capture_report(staged.path(), &targets, requested.is_empty())?;
    let encoded = serde_json::to_string_pretty(&report)?;
    save_capture_manifest(staged.path(), &encoded)?;
    let _retained_staging_directory = staged.keep();
    println!("{encoded}");
    Ok(())
}

fn capture_report(
    staging: &std::path::Path,
    targets: &[Target],
    full: bool,
) -> Result<serde_json::Value> {
    Ok(capture_snapshot(staging, targets, full)?.0)
}

fn capture_snapshot(
    staging: &std::path::Path,
    targets: &[Target],
    full: bool,
) -> Result<(serde_json::Value, Vec<Vec<u8>>)> {
    use sha2::{Digest, Sha256};
    let staging = staging.canonicalize()?;
    let mut images = Vec::new();
    let mut contents = Vec::new();
    for (index, target) in targets.iter().enumerate() {
        let file = format!("{index:04}.png");
        let path = staging.join(&file);
        let metadata = std::fs::symlink_metadata(&path)?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "staged social card must be a regular file"
        );
        let bytes = std::fs::read(&path)?;
        images.push(serde_json::json!({"stagedFile":file,"target":target,
            "bytes":bytes.len(),"sha256":format!("{:x}", Sha256::digest(&bytes))}));
        contents.push(bytes);
    }
    Ok((
        serde_json::json!({"schema":1,"stagingDirectory":staging,"rendered":true,
        "published":false,"replaceChangelogDirectory":full,"images":images}),
        contents,
    ))
}

fn save_capture_manifest(staging: &std::path::Path, encoded: &str) -> Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(staging.join("capture.json"))?;
    file.write_all(encoded.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
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

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
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
    #[derive(Serialize, Deserialize)]
    struct HtmlOracle {
        commit: String,
        target: Target,
        html: String,
    }

    #[test]
    #[ignore = "requires explicit WORKDECK_ORACLE_DRIVER, WORKDECK_ORACLE_BROWSER and WORKDECK_ORACLE_FONT paths"]
    fn browser_pixels_match_all_frozen_baseline_html() {
        use base64::Engine;
        let driver = std::env::var_os("WORKDECK_ORACLE_DRIVER").expect("driver path");
        let browser = std::env::var_os("WORKDECK_ORACLE_BROWSER").expect("browser path");
        let font =
            std::fs::read(std::env::var_os("WORKDECK_ORACLE_FONT").expect("font path")).unwrap();
        let fixtures: Vec<HtmlOracle> = serde_json::from_slice(
            &std::fs::read(
                crate::repo_root()
                    .unwrap()
                    .join("port/hunk/fixtures/social-card-html.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(fixtures.len(), 108);
        let font_uri = format!(
            "data:font/woff2;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&font)
        );
        let mut documents = Vec::new();
        for fixture in &fixtures {
            documents.push(
                fixture
                    .html
                    .replace(
                        "<div class=\"mark\">hunk</div>",
                        "<div class=\"mark\">workdeck</div>",
                    )
                    .replace("data:font/woff2;base64,Zm9udA==", &font_uri),
            );
            documents.push(render_html(&fixture.target, &font));
        }
        let capture = crate::term_video::capture_card_documents(
            std::path::Path::new(&driver),
            std::path::Path::new(&browser),
            &documents,
        )
        .unwrap();
        let pixels = |index| {
            let bytes = std::fs::read(capture.path().join(format!("{index:04}.png"))).unwrap();
            let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
                .read_info()
                .unwrap();
            let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
            let info = reader.next_frame(&mut buffer).unwrap();
            buffer.truncate(info.buffer_size());
            (
                info.width,
                info.height,
                info.color_type,
                info.bit_depth,
                buffer,
            )
        };
        for pair in 0..fixtures.len() {
            assert!(
                pixels(pair * 2) == pixels(pair * 2 + 1),
                "browser pixel pair {pair}"
            );
        }
    }

    #[test]
    fn frozen_html_oracles_match_rust_without_upstream_runtime() {
        let fixtures: Vec<HtmlOracle> = serde_json::from_slice(
            &std::fs::read(
                crate::repo_root()
                    .unwrap()
                    .join("port/hunk/fixtures/social-card-html.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(fixtures.len(), 108);
        for fixture in fixtures {
            let expected = fixture.html.replace(
                "<div class=\"mark\">hunk</div>",
                "<div class=\"mark\">workdeck</div>",
            );
            let actual = render_html(&fixture.target, b"font").replace(
                "/* Derived from Hunk generate-og.ts, MIT. Copyright Modem Labs Inc. */\n",
                "",
            );
            assert_eq!(actual, expected, "{} {:?}", fixture.commit, fixture.target);
        }
    }

    #[test]
    #[ignore = "executes isolated pinned TypeScript render functions through Bun as an oracle"]
    fn html_matches_both_pinned_source_renderers() {
        let repo = crate::repo_root().unwrap();
        let mut fixtures = Vec::new();
        for commit in [
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd",
        ] {
            let source = std::process::Command::new("git")
                .current_dir(&repo)
                .args(["show", &format!("{commit}:website/scripts/generate-og.ts")])
                .output()
                .unwrap();
            assert!(source.status.success());
            let source = String::from_utf8(source.stdout).unwrap();
            let start = source.find("function escapeHtml(").unwrap();
            let end = source.find("\nconst requested =").unwrap();
            let renderer = &source[start..end];
            let constants = source
                .lines()
                .filter(|line| {
                    line.starts_with("const WIDTH =") || line.starts_with("const HEIGHT =")
                })
                .collect::<Vec<_>>()
                .join("\n");
            for title in ["0.20", "😀😀😀😀😀😀", "😀😀😀😀😀😀x"] {
                for variant in 0..18 {
                    let mut target = select(vec![], &[]).unwrap().remove(0);
                    target.card.title = title.into();
                    target.card.tagline = match variant % 3 {
                        0 => None,
                        1 => Some(String::new()),
                        _ => Some("<&\"' λ".into()),
                    };
                    target.card.chips = match (variant / 3) % 3 {
                        0 => None,
                        1 => Some(vec![]),
                        _ => Some(vec!["one & two".into(), "<three>".into()]),
                    };
                    target.card.latest = variant >= 9;
                    target.card.meta = "<&\"' meta λ".into();
                    target.footer = "<&\"' footer λ".into();
                    let temp = tempfile::tempdir().unwrap();
                    let script = temp.path().join("oracle.ts");
                    std::fs::write(&script, format!("{constants}\n{renderer}\nconsole.log(JSON.stringify(renderCardHtml({}, 'data:font/woff2;base64,Zm9udA==')));", serde_json::to_string(&target).unwrap())).unwrap();
                    let output = std::process::Command::new("bun")
                        .arg(&script)
                        .current_dir(temp.path())
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    assert!(output.stderr.is_empty());
                    let expected: String = serde_json::from_slice(&output.stdout).unwrap();
                    let original_html = expected.clone();
                    let expected = expected.replace(
                        "<div class=\"mark\">hunk</div>",
                        "<div class=\"mark\">workdeck</div>",
                    );
                    let actual = render_html(&target, b"font").replace(
                        "/* Derived from Hunk generate-og.ts, MIT. Copyright Modem Labs Inc. */\n",
                        "",
                    );
                    assert_eq!(actual, expected, "{commit} {title} variant {variant}");
                    fixtures.push(HtmlOracle {
                        commit: commit.into(),
                        target,
                        html: original_html,
                    });
                }
            }
        }
        if std::env::var_os("WORKDECK_CAPTURE_SOCIAL_HTML_ORACLE").is_some() {
            let path = repo.join("port/hunk/fixtures/social-card-html.json");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut encoded = serde_json::to_string_pretty(&fixtures).unwrap();
            encoded.push('\n');
            std::fs::write(path, encoded).unwrap();
        }
    }
    #[test]
    fn saved_capture_check_rejects_image_target_and_scope_changes() {
        let staging = tempfile::tempdir().unwrap();
        let targets = select(vec![], &[]).unwrap();
        let image = staging.path().join("0000.png");
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1200, 630);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![0; 1200 * 630 * 3])
                .unwrap();
        }
        std::fs::write(&image, &bytes).unwrap();
        let report = capture_report(staging.path(), &targets, true).unwrap();
        save_capture_manifest(staging.path(), &serde_json::to_string(&report).unwrap()).unwrap();
        let snapshot = check_capture(staging.path(), &targets, true).unwrap();
        assert_eq!(snapshot, vec![bytes.clone()]);
        assert!(check_capture(staging.path(), &targets, false).is_err());
        std::fs::write(&image, b"modified").unwrap();
        assert!(check_capture(staging.path(), &targets, true).is_err());
        std::fs::write(&image, &bytes).unwrap();
        let mut changed = select(vec![], &[]).unwrap();
        changed[0].card.title = "Changed title".into();
        assert!(check_capture(staging.path(), &changed, true).is_err());
        check_capture(staging.path(), &targets, true).unwrap();
        std::fs::write(&image, b"not a PNG").unwrap();
        let malformed = capture_report(staging.path(), &targets, true).unwrap();
        std::fs::write(
            staging.path().join("capture.json"),
            serde_json::to_vec(&malformed).unwrap(),
        )
        .unwrap();
        assert!(check_capture(staging.path(), &targets, true).is_err());
    }
    #[test]
    fn capture_report_binds_each_ordered_target_to_actual_image_bytes() {
        use sha2::{Digest, Sha256};
        let staging = tempfile::tempdir().unwrap();
        let targets = select(vec![], &[]).unwrap();
        assert!(capture_report(staging.path(), &targets, true).is_err());
        std::fs::write(staging.path().join("0000.png"), b"image bytes").unwrap();
        let report = capture_report(staging.path(), &targets, false).unwrap();
        assert_eq!(report["schema"], 1);
        assert_eq!(report["images"][0]["bytes"], 11);
        assert_eq!(
            report["images"][0]["sha256"],
            format!("{:x}", Sha256::digest(b"image bytes"))
        );
        assert_eq!(
            report["images"][0]["target"]["output_file"],
            "site/static/extensions/og.png"
        );
        std::fs::write(staging.path().join("0000.png"), b"changed bytes").unwrap();
        let changed = capture_report(staging.path(), &targets, false).unwrap();
        assert_ne!(
            report["images"][0]["sha256"],
            changed["images"][0]["sha256"]
        );
    }
    #[test]
    fn capture_manifest_is_durable_exact_and_never_overwritten() {
        let staging = tempfile::tempdir().unwrap();
        let encoded = r#"{"rendered":true,"published":false,"images":[]}"#;
        save_capture_manifest(staging.path(), encoded).unwrap();
        let expected = format!("{encoded}\n");
        assert_eq!(
            std::fs::read_to_string(staging.path().join("capture.json")).unwrap(),
            expected
        );
        assert!(save_capture_manifest(staging.path(), "different").is_err());
        assert_eq!(
            std::fs::read_to_string(staging.path().join("capture.json")).unwrap(),
            expected
        );
    }
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
