//! Website-only third-party assets, kept separate from binary dependency SBOMs.
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};

const HUNK_BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const THEME_SHOTS: &[(&str, usize, &str)] = &[
    (
        "shot-catppuccin-mocha.webp",
        293_348,
        "4e985b054b1a0361880c040f769ed6f9099acf5f1242ab3c61d14812349608da",
    ),
    (
        "shot-github-dark.webp",
        296_036,
        "eedfa209c0a39aed6bba42b47b4a24a8bde45626a41536fbe4aa1f6527f4e50f",
    ),
    (
        "shot-github-light.webp",
        291_368,
        "f3427800f45970fce2e750f29ba5ad7108a42d6083d46fff1bbaa11f037cccca",
    ),
    (
        "shot-gruvbox.webp",
        297_806,
        "0eb87ca8f92c5676e082b71848f00d47074607738aff6361d81bdfb3de833092",
    ),
    (
        "shot-nord.webp",
        268_044,
        "8b8dde28892665ff97ed3e62ea4546df4cf2fc7196e23f2ac532b260c4c6be90",
    ),
    (
        "shot-tokyo-night.webp",
        300_264,
        "eb337b5906a8229c054558a15cbe1d6732c7ef0f6a81260e2edd8d6171b427b7",
    ),
];
const VIDEO_THUMBNAILS: &[(&str, usize, &str)] = &[
    (
        "video-devops-toolbox.webp",
        80_718,
        "3f6aede31c6b225688903b5fcb770bf730d2203d6c7827e3e807feea2a25b5b5",
    ),
    (
        "video-jilles.webp",
        32_348,
        "2a9dc5368875d39523c99f8a2172ed64c3add4e1c1b20bc15874ac04ebd5f7c3",
    ),
];
const FEATURE_MEDIA: &[(&str, usize, &str)] = &[
    (
        "feature-agent.mp4",
        292_757,
        "b73d91403aa6c0906b8fd99d0090e254b0d087e94c4a1c041faef1dc0b43f532",
    ),
    (
        "feature-agent.webm",
        251_147,
        "6dd434db6baa48ee9880135b28f3cff377550d276d70f8bccc5aaca1d6e129d9",
    ),
    (
        "feature-layout.mp4",
        408_441,
        "484c094cfa2849b2e664f47fa3056a58bebc7dea0be8b83d81452d78bd5944d7",
    ),
    (
        "feature-layout.webm",
        310_208,
        "26bad21a250c152d0fb05357a2202ba978ee845bfc65a4c54c93c801db6f3853",
    ),
    (
        "feature-mouse.mp4",
        415_855,
        "527458c0b6ca60527703c3a889786ac490c168c207a4512e05cc0e106bb82d37",
    ),
    (
        "feature-mouse.webm",
        362_544,
        "4fbb2b0fc64538607e67f5df2c6097981dc96aaf370fb1d5e65d97b34479ad5b",
    ),
    (
        "feature-stream.webp",
        210_738,
        "b8f7142fb8a8070778087526fd58694d22538d250d4812c5413da3586406f7cf",
    ),
    (
        "feature-themes.mp4",
        1_639_541,
        "f77a1c146bb82a7a545f212dddb481606746da3655ecc632f777c93dd486f81c",
    ),
    (
        "feature-themes.webm",
        1_419_524,
        "a4d17c52f2ac0585e1fe8d3f212785d9fbf033b77d790971860ae957fbf0189f",
    ),
];

/// Validate that the served theme screenshots are byte-identical to the
/// retained, licensed Hunk baseline assets and remain independently inventoried.
pub(crate) fn verify_theme_shots(repo: &Path) -> Result<()> {
    for (name, expected_bytes, expected_sha) in THEME_SHOTS {
        let retained = repo
            .join("third_party/hunk/assets/website/public")
            .join(name);
        let served = repo.join("site/static/shots").join(name);
        let retained_bytes = fs::read(&retained)
            .with_context(|| format!("read retained theme shot {}", retained.display()))?;
        let served_bytes = fs::read(&served)
            .with_context(|| format!("read served theme shot {}", served.display()))?;
        ensure!(
            retained_bytes.len() == *expected_bytes && served_bytes.len() == *expected_bytes,
            "theme shot {name} has unexpected byte count"
        );
        ensure!(
            format!("{:x}", Sha256::digest(&retained_bytes)) == *expected_sha
                && format!("{:x}", Sha256::digest(&served_bytes)) == *expected_sha,
            "theme shot {name} hash mismatch"
        );
        let source = crate::git_stdout_bytes(
            repo,
            [
                "show",
                &format!("{HUNK_BASELINE}:website/public/{name}"),
            ],
        )?;
        ensure!(
            source == retained_bytes && source == served_bytes,
            "theme shot {name} differs from the pinned Hunk blob"
        );
    }
    let inventory: Vec<Asset> = serde_json::from_slice(&fs::read(
        repo.join("site/data/third-party-assets.json"),
    )?)?;
    let files = inventory
        .iter()
        .flat_map(|asset| asset.files.iter())
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    for (name, _, _) in THEME_SHOTS {
        ensure!(
            files.contains(format!("site/static/shots/{name}").as_str()),
            "theme shot {name} missing from the website asset inventory"
        );
    }
    Ok(())
}

/// Verify the no-JavaScript, CSS-radio translation of Hunk's theme picker.
pub(crate) fn verify_theme_shot_component(repo: &Path) -> Result<()> {
    let source = crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("{HUNK_BASELINE}:website/src/components/marketing/ThemeShot.astro"),
        ],
    )?;
    ensure!(
        source.len() == 4_272,
        "pinned ThemeShot.astro changed size: {} != 4272",
        source.len()
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "BUNDLED_SHIKI_THEME_IDS",
        "const themes = [",
        "GitHub Dark",
        "Tokyo Night",
        "Catppuccin Mocha",
        "Gruvbox",
        "Nord",
        "GitHub Light",
        "const remainingThemes = BUNDLED_SHIKI_THEME_IDS.length - themes.length",
        "role=\"group\" aria-label=\"Preview theme\"",
        "aria-pressed",
        "aria-controls",
        "data-theme-index",
        "requestIdleCallback",
        "shot.decode()",
        "pointerenter",
    ] {
        ensure!(
            source.contains(marker),
            "pinned ThemeShot.astro lost marker {marker:?}"
        );
    }
    let index = std::fs::read_to_string(repo.join("site/templates/index.html"))?;
    for marker in [
        "class=\"theme-showcase\"",
        "class=\"theme-frame\"",
        "name=\"theme-shot\" checked",
        "id=\"theme-shot-0\"",
        "id=\"theme-shot-1\"",
        "id=\"theme-shot-2\"",
        "id=\"theme-shot-3\"",
        "id=\"theme-shot-4\"",
        "id=\"theme-shot-5\"",
        "shot-github-dark.webp",
        "shot-tokyo-night.webp",
        "shot-catppuccin-mocha.webp",
        "shot-gruvbox.webp",
        "shot-nord.webp",
        "shot-github-light.webp",
        "role=\"group\" aria-label=\"Preview theme\"",
        "and 61 more",
    ] {
        ensure!(
            index.contains(marker),
            "native theme picker is missing {marker:?}"
        );
    }
    ensure!(
        !index.contains("<script"),
        "native theme picker must not reintroduce application JavaScript"
    );
    let css = std::fs::read_to_string(repo.join("site/static/main.css"))?;
    for marker in [
        ".theme-showcase",
        ".theme-shots",
        ".theme-shot",
        "#theme-shot-0:checked",
        "#theme-shot-5:checked",
        ".tpill",
        ".tmore",
    ] {
        ensure!(
            css.contains(marker),
            "native theme picker styles are missing {marker:?}"
        );
    }
    let migration = std::fs::read_to_string(repo.join("docs/theme-shot-migration.md"))?;
    for marker in [
        "ThemeShot.astro",
        "CSS radio",
        "aria-pressed",
        "lazy",
        "requestIdleCallback",
        "no application JavaScript",
    ] {
        ensure!(
            migration.contains(marker),
            "theme picker migration is missing {marker:?}"
        );
    }
    Ok(())
}

/// Validate the retained community thumbnails and the no-player marketing
/// cards that replace Hunk's `CommunityVideos.astro` component.
pub(crate) fn verify_community_videos(repo: &Path) -> Result<()> {
    let source = crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("{HUNK_BASELINE}:website/src/components/marketing/CommunityVideos.astro"),
        ],
    )?;
    ensure!(
        source.len() == 1_934,
        "pinned CommunityVideos.astro changed size: {} != 1934",
        source.len()
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "Independent walkthroughs styled as paused YouTube embeds",
        "linked out",
        "const videos = [",
        "https://www.youtube.com/watch?v=FFfz81XM57k",
        "https://www.youtube.com/watch?v=-4fJbIF8WAs",
        "video-jilles.webp",
        "video-devops-toolbox.webp",
        "channel: \"Jilles\"",
        "channel: \"DevOps Toolbox\"",
        "initial: \"J\"",
        "initial: \"D\"",
        "length: \"5:14\"",
        "length: \"13:33\"",
        "<div class=\"vgrid\">",
        "target=\"_blank\" rel=\"noopener noreferrer\"",
        "class=\"vplayer\"",
        "loading=\"lazy\" draggable=\"false\"",
        "class=\"vscrim\"",
        "class=\"vavatar\"",
        "class=\"vtitle\"",
        "class=\"vplay\"",
        "class=\"vlength\"",
        "class=\"vcaption\"",
    ] {
        ensure!(
            source.contains(marker),
            "pinned CommunityVideos.astro lost marker {marker:?}"
        );
    }
    for (name, expected_bytes, expected_sha) in VIDEO_THUMBNAILS {
        let retained = repo
            .join("third_party/hunk/assets/website/public")
            .join(name);
        let served = repo.join("site/static/videos").join(name);
        let retained_bytes = fs::read(&retained)
            .with_context(|| format!("read retained video thumbnail {}", retained.display()))?;
        let served_bytes = fs::read(&served)
            .with_context(|| format!("read served video thumbnail {}", served.display()))?;
        ensure!(
            retained_bytes.len() == *expected_bytes && served_bytes.len() == *expected_bytes,
            "video thumbnail {name} has unexpected byte count"
        );
        ensure!(
            format!("{:x}", Sha256::digest(&retained_bytes)) == *expected_sha
                && format!("{:x}", Sha256::digest(&served_bytes)) == *expected_sha,
            "video thumbnail {name} hash mismatch"
        );
        let source = crate::git_stdout_bytes(
            repo,
            [
                "show",
                &format!("{HUNK_BASELINE}:website/public/{name}"),
            ],
        )?;
        ensure!(
            source == retained_bytes && source == served_bytes,
            "video thumbnail {name} differs from the pinned Hunk blob"
        );
    }
    let index = fs::read_to_string(repo.join("site/templates/index.html"))?;
    for marker in [
        "class=\"community-videos\"",
        "id=\"community-videos-title\"",
        "class=\"vgrid\"",
        "video-jilles.webp",
        "video-devops-toolbox.webp",
        "https://www.youtube.com/watch?v=FFfz81XM57k",
        "https://www.youtube.com/watch?v=-4fJbIF8WAs",
        "target=\"_blank\" rel=\"noopener noreferrer\"",
        "class=\"vplayer\"",
        "class=\"vscrim\"",
        "class=\"vavatar\"",
        "class=\"vtitle\"",
        "class=\"vplay\"",
        "class=\"vlength\"",
        "class=\"vcaption\"",
    ] {
        ensure!(
            index.contains(marker),
            "native community video card is missing {marker:?}"
        );
    }
    ensure!(
        !index.contains("<script"),
        "native community video cards must not add a player script"
    );
    let css = fs::read_to_string(repo.join("site/static/main.css"))?;
    for marker in [
        ".community-videos",
        ".vgrid",
        ".vcard",
        ".vplayer",
        ".vscrim",
        ".vavatar",
        ".vtitle",
        ".vplay",
        ".vlength",
        ".vcaption",
    ] {
        ensure!(
            css.contains(marker),
            "native community video styles are missing {marker:?}"
        );
    }
    let inventory: Vec<Asset> = serde_json::from_slice(&fs::read(
        repo.join("site/data/third-party-assets.json"),
    )?)?;
    let files = inventory
        .iter()
        .flat_map(|asset| asset.files.iter())
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    for (name, _, _) in VIDEO_THUMBNAILS {
        ensure!(
            files.contains(format!("site/static/videos/{name}").as_str()),
            "video thumbnail {name} missing from the website asset inventory"
        );
    }
    let migration = fs::read_to_string(repo.join("docs/community-videos-migration.md"))?;
    for marker in [
        "CommunityVideos.astro",
        "paused YouTube",
        "no-player",
        "loading=\"lazy\"",
        "no application JavaScript",
    ] {
        ensure!(
            migration.contains(marker),
            "community video migration is missing {marker:?}"
        );
    }
    Ok(())
}

/// Validate the complete media-led feature tour and its native, static-video
/// rendering. Playback is user-controlled so the page remains no-script and
/// accessible while retaining every source chapter, quote, and media asset.
pub(crate) fn verify_feature_showcase(repo: &Path) -> Result<()> {
    let source = crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("{HUNK_BASELINE}:website/src/components/marketing/FeatureShowcase.astro"),
        ],
    )?;
    ensure!(
        source.len() == 9_883,
        "pinned FeatureShowcase.astro changed size: {} != 9883",
        source.len()
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "Media-led feature sections captured from the real TUI",
        "interface ShowcaseVideo",
        "interface ShowcaseImage",
        "interface ShowcaseCode",
        "interface ShowcaseFeature",
        "const extensionSample = [",
        "const showcase:",
        "One stream, every file.",
        "Your agent's reasoning, beside the code it explains.",
        "Keys when you're fast, mouse when you're browsing.",
        "Split or stack — or let auto decide.",
        "Real syntax highlighting. Dozens of themes.",
        "Extend it however you want.",
        "Mitchell Hashimoto, creator of Ghostty & Vagrant",
        "DHH, creator of Omarchy and Ruby on Rails",
        "feature-stream.webp",
        "feature-agent",
        "feature-mouse",
        "feature-layout",
        "feature-themes",
        "<div class=\"show\">",
        "class=\"show-item\"",
        "class=\"show-copy\"",
        "show-media",
        "class=\"show-quote\"",
        "preload=\"metadata\"",
        "IntersectionObserver",
        "prefers-reduced-motion",
    ] {
        ensure!(
            source.contains(marker),
            "pinned FeatureShowcase.astro lost marker {marker:?}"
        );
    }
    for (name, expected_bytes, expected_sha) in FEATURE_MEDIA {
        let retained = repo
            .join("third_party/hunk/assets/website/public")
            .join(name);
        let served = repo.join("site/static/features").join(name);
        let retained_bytes = fs::read(&retained)
            .with_context(|| format!("read retained feature media {}", retained.display()))?;
        let served_bytes = fs::read(&served)
            .with_context(|| format!("read served feature media {}", served.display()))?;
        ensure!(
            retained_bytes.len() == *expected_bytes && served_bytes.len() == *expected_bytes,
            "feature media {name} has unexpected byte count"
        );
        ensure!(
            format!("{:x}", Sha256::digest(&retained_bytes)) == *expected_sha
                && format!("{:x}", Sha256::digest(&served_bytes)) == *expected_sha,
            "feature media {name} hash mismatch"
        );
        let source = crate::git_stdout_bytes(
            repo,
            [
                "show",
                &format!("{HUNK_BASELINE}:website/public/{name}"),
            ],
        )?;
        ensure!(
            source == retained_bytes && source == served_bytes,
            "feature media {name} differs from the pinned Hunk blob"
        );
    }
    let index = fs::read_to_string(repo.join("site/templates/index.html"))?;
    for marker in [
        "class=\"feature-showcase\"",
        "id=\"feature-showcase-title\"",
        "class=\"show\"",
        "class=\"show-item\"",
        "One stream, every file.",
        "Your agent's reasoning, beside the code it explains.",
        "Keys when you're fast, mouse when you're browsing.",
        "Split or stack—or let auto decide.",
        "Real syntax highlighting. Dozens of themes.",
        "Extend it however you want.",
        "Mitchell Hashimoto, creator of Ghostty &amp; Vagrant",
        "DHH, creator of Omarchy and Ruby on Rails",
        "feature-stream.webp",
        "feature-agent.webm",
        "feature-agent.mp4",
        "feature-mouse.webm",
        "feature-mouse.mp4",
        "feature-layout.webm",
        "feature-layout.mp4",
        "feature-themes.webm",
        "feature-themes.mp4",
        "class=\"show-quote\"",
        "show-code",
        "controls aria-label=\"Agent notes rendered inline in a Workdeck review\"",
        "preload=\"metadata\"",
    ] {
        ensure!(
            index.contains(marker),
            "native feature showcase is missing {marker:?}"
        );
    }
    ensure!(
        !index.contains("<script"),
        "native feature showcase must not add application JavaScript"
    );
    let css = fs::read_to_string(repo.join("site/static/main.css"))?;
    for marker in [
        ".feature-showcase",
        ".show {",
        ".show-item",
        ".show-copy",
        ".show-media",
        ".show-code",
        ".show-quote",
        "@media (max-width: 48rem)",
    ] {
        ensure!(
            css.contains(marker),
            "native feature showcase styles are missing {marker:?}"
        );
    }
    let inventory: Vec<Asset> = serde_json::from_slice(&fs::read(
        repo.join("site/data/third-party-assets.json"),
    )?)?;
    let files = inventory
        .iter()
        .flat_map(|asset| asset.files.iter())
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    for (name, _, _) in FEATURE_MEDIA {
        ensure!(
            files.contains(format!("site/static/features/{name}").as_str()),
            "feature media {name} missing from the website asset inventory"
        );
    }
    let migration = fs::read_to_string(repo.join("docs/feature-showcase-migration.md"))?;
    for marker in [
        "FeatureShowcase.astro",
        "six feature chapters",
        "two quotes",
        "controls",
        "no application JavaScript",
        "retained",
    ] {
        ensure!(
            migration.contains(marker),
            "feature showcase migration is missing {marker:?}"
        );
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Asset {
    name: String,
    version: String,
    license: String,
    source: String,
    archive_integrity: String,
    files: Vec<AssetFile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetFile {
    path: String,
    sha256: String,
}

pub(super) fn sbom(repo: &Path) -> Result<serde_json::Value> {
    let assets: Vec<Asset> =
        serde_json::from_slice(&fs::read(repo.join("site/data/third-party-assets.json"))?)?;
    ensure!(!assets.is_empty(), "website asset inventory is empty");
    let mut paths = BTreeSet::new();
    let mut components = Vec::new();
    for asset in assets {
        ensure!(!asset.files.is_empty(), "website asset has no files");
        for file in asset.files {
            ensure!(
                file.path.starts_with("site/static/")
                    && Path::new(&file.path)
                        .components()
                        .all(|c| matches!(c, Component::Normal(_))),
                "invalid website asset path"
            );
            ensure!(
                paths.insert(file.path.clone()),
                "duplicate website asset path"
            );
            let mut path = repo.to_owned();
            for component in Path::new(&file.path).components() {
                path.push(component);
                ensure!(
                    !fs::symlink_metadata(&path)?.file_type().is_symlink(),
                    "website asset path is a symlink"
                );
            }
            ensure!(
                fs::metadata(&path)?.is_file(),
                "website asset is not a file"
            );
            let bytes = fs::read(path)?;
            ensure!(
                format!("{:x}", Sha256::digest(&bytes)) == file.sha256,
                "website asset hash mismatch: {}",
                file.path
            );
            components.push(serde_json::json!({
                "type":"file", "bom-ref":file.path, "name":file.path, "version":asset.version,
                "hashes":[{"alg":"SHA-256", "content":file.sha256}],
                "licenses":[{"license":{"id":asset.license}}],
                "externalReferences":[{"type":"distribution", "url":asset.source}],
                "properties":[{"name":"workdeck:asset-name", "value":asset.name},
                    {"name":"workdeck:archive-integrity", "value":asset.archive_integrity}]
            }));
        }
    }
    Ok(
        serde_json::json!({"bomFormat":"CycloneDX", "specVersion":"1.5", "version":1,
        "metadata":{"component":{"type":"application", "name":"workdeck-website-assets"}},
        "components":components}),
    )
}

pub(super) fn run(repo: &Path, args: impl Iterator<Item = String>) -> Result<()> {
    ensure!(args.count() == 0, "site-assets-sbom takes no arguments");
    println!("{}", serde_json::to_string_pretty(&sbom(repo)?)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_shots_match_retained_pinned_blobs_and_inventory() {
        let repo = crate::repo_root().unwrap();
        verify_theme_shots(&repo).unwrap();
    }

    #[test]
    fn native_theme_picker_replaces_the_complete_pinned_component() {
        let repo = crate::repo_root().unwrap();
        verify_theme_shot_component(&repo).unwrap();
    }

    #[test]
    fn native_community_video_cards_replace_the_complete_pinned_component() {
        let repo = crate::repo_root().unwrap();
        verify_community_videos(&repo).unwrap();
    }

    #[test]
    fn native_feature_showcase_replaces_the_complete_pinned_component() {
        let repo = crate::repo_root().unwrap();
        verify_feature_showcase(&repo).unwrap();
    }

    #[test]
    fn duplicate_asset_paths_are_rejected() {
        let repo = tempfile::tempdir().unwrap();
        fs::create_dir_all(repo.path().join("site/data")).unwrap();
        fs::create_dir_all(repo.path().join("site/static")).unwrap();
        fs::write(repo.path().join("site/static/asset"), b"asset").unwrap();
        let file = serde_json::json!({"path":"site/static/asset", "sha256":format!("{:x}", Sha256::digest(b"asset"))});
        let inventory = serde_json::json!([{"name":"fixture", "version":"1", "license":"MIT", "source":"https://example.invalid/asset", "archiveIntegrity":"fixture", "files":[file.clone(), file]}]);
        fs::write(
            repo.path().join("site/data/third-party-assets.json"),
            serde_json::to_vec(&inventory).unwrap(),
        )
        .unwrap();
        assert!(
            sbom(repo.path())
                .unwrap_err()
                .to_string()
                .contains("duplicate website asset path")
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_asset_parents_are_rejected_without_following_them() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(repo.path().join("site/data")).unwrap();
        fs::write(outside.path().join("asset"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), repo.path().join("site/static")).unwrap();
        let inventory = serde_json::json!([{"name":"fixture", "version":"1", "license":"MIT", "source":"https://example.invalid/asset", "archiveIntegrity":"fixture", "files":[{"path":"site/static/asset", "sha256":format!("{:x}",Sha256::digest(b"outside"))}]}]);
        fs::write(
            repo.path().join("site/data/third-party-assets.json"),
            serde_json::to_vec(&inventory).unwrap(),
        )
        .unwrap();
        assert!(
            sbom(repo.path())
                .unwrap_err()
                .to_string()
                .contains("symlink")
        );
        assert_eq!(fs::read(outside.path().join("asset")).unwrap(), b"outside");
    }
    #[test]
    fn shared_brand_styles_preserve_pinned_main_after_explicit_migration() {
        let repo = crate::repo_root().unwrap();
        let actual = fs::read_to_string(repo.join("site/static/brand.css")).unwrap();
        let commit = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
        let source = std::process::Command::new("git")
            .current_dir(&repo)
            .args(["show", &format!("{commit}:website/src/styles/brand.css")])
            .output()
            .unwrap();
        assert!(source.status.success());
        let expected = format!(
            "/* Derived from Hunk brand.css, MIT. Copyright Modem Labs Inc. */\n{}",
            String::from_utf8(source.stdout)
                .unwrap()
                .replace(
                    "@import \"@fontsource-variable/jetbrains-mono/index.css\";\n",
                    ""
                )
                .replace("--hunk-", "--workdeck-")
                .replace("brand-modem", "brand-attribution")
        );
        assert_eq!(actual, expected, "{commit}");
    }
    #[test]
    fn changed_font_or_missing_license_cannot_generate_sbom() {
        let repo = crate::repo_root().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let inventory = "site/data/third-party-assets.json";
        let bytes = fs::read(repo.join(inventory)).unwrap();
        fs::create_dir_all(temp.path().join("site/data")).unwrap();
        fs::write(temp.path().join(inventory), &bytes).unwrap();
        let assets: Vec<Asset> = serde_json::from_slice(&bytes).unwrap();
        for asset in &assets {
            for file in &asset.files {
                let destination = temp.path().join(&file.path);
                fs::create_dir_all(destination.parent().unwrap()).unwrap();
                fs::copy(repo.join(&file.path), destination).unwrap();
            }
        }
        sbom(temp.path()).unwrap();
        let font = &assets[0].files[0].path;
        fs::write(temp.path().join(font), b"changed").unwrap();
        assert!(
            sbom(temp.path())
                .unwrap_err()
                .to_string()
                .contains("hash mismatch")
        );
        assert_eq!(fs::read(temp.path().join(font)).unwrap(), b"changed");
        fs::copy(repo.join(font), temp.path().join(font)).unwrap();
        fs::remove_file(temp.path().join(&assets[0].files[1].path)).unwrap();
        assert!(sbom(temp.path()).is_err());
        assert_eq!(fs::read(temp.path().join(inventory)).unwrap(), bytes);
    }
    #[test]
    fn retained_font_inventory_emits_checked_website_only_components() {
        let output = sbom(&crate::repo_root().unwrap()).unwrap();
        assert_eq!(output["components"].as_array().unwrap().len(), 16);
        assert_eq!(
            output["metadata"]["component"]["name"],
            "workdeck-website-assets"
        );
        let components = output["components"].as_array().unwrap();
        assert_eq!(
            components
                .iter()
                .filter(|component| component["licenses"][0]["license"]["id"] == "OFL-1.1")
                .count(),
            8
        );
        assert_eq!(
            components
                .iter()
                .filter(|component| component["licenses"][0]["license"]["id"] == "MIT")
                .count(),
            8
        );
        for component in components {
            assert_eq!(component["type"], "file");
        }
    }

    #[test]
    fn site_styles_reference_the_retained_font_without_package_imports() {
        let repo = crate::repo_root().unwrap();
        let css = fs::read_to_string(repo.join("site/static/fonts/jetbrains-mono.css")).unwrap();
        assert!(css.contains("url(./jetbrains-mono-latin-wght-normal.woff2)"));
        assert_eq!(css.matches("@font-face").count(), 6);
        assert_eq!(css.matches("unicode-range:").count(), 6);
        let template = fs::read_to_string(repo.join("site/templates/base.html")).unwrap();
        assert!(template.contains("href=\"/fonts/jetbrains-mono.css\""));
        assert!(css.contains("font-weight: 100 800"));
        assert!(css.contains("font-display: swap"));
        assert!(!css.contains("@import"));
        assert!(
            repo.join("site/static/fonts/jetbrains-mono-latin-wght-normal.woff2")
                .is_file()
        );
        sbom(&repo).unwrap();
    }

    #[test]
    fn pinned_upstream_brand_asset_has_a_native_workdeck_replacement() {
        let repo = crate::repo_root().unwrap();
        let source = std::process::Command::new("git")
            .current_dir(&repo)
            .args([
                "show",
                "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2:website/public/modem-light.svg",
            ])
            .output()
            .unwrap();
        assert!(source.status.success());
        assert_eq!(source.stdout.len(), 4244);
        assert_eq!(
            format!("{:x}", Sha256::digest(&source.stdout)),
            "55e0c8d5d2933cb14b808aa70f3cc28dc666dbaf0defac24bd55038a005317f8"
        );

        let replacement = fs::read_to_string(repo.join("site/static/og.svg")).unwrap();
        assert!(replacement.contains("<title id=\"title\">Workdeck</title>"));
        assert!(replacement.contains("Rust · Ratatui · one executable"));
        assert!(!replacement.to_ascii_lowercase().contains("modem"));
        assert!(!replacement.contains("Hunk-first"));
    }

    #[test]
    fn pinned_site_styles_are_translated_to_static_workdeck_css() {
        let repo = crate::repo_root().unwrap();
        for (source_path, destination_path) in [
            (
                "website/src/styles/marketing.css",
                "site/static/marketing.css",
            ),
            (
                "website/src/styles/starlight.css",
                "site/static/starlight.css",
            ),
            (
                "website/src/styles/extensions.css",
                "site/static/extensions.css",
            ),
        ] {
            let source = std::process::Command::new("git")
                .current_dir(&repo)
                .args([
                    "show",
                    &format!("2c00f4358b89cfc0a6b04459ffc538ba601aa3c2:{source_path}"),
                ])
                .output()
                .unwrap();
            assert!(source.status.success(), "{source_path}");
            let expected = String::from_utf8(source.stdout)
                .unwrap()
                .replace("@import \"./brand.css\";", "@import url(\"/brand.css\");")
                .replace("--hunk-", "--workdeck-")
                .replace("Hunk", "Workdeck")
                .replace("hunk", "workdeck");
            let actual = fs::read_to_string(repo.join(destination_path)).unwrap();
            assert_eq!(actual, expected, "{source_path}");
            assert!(!actual.contains("--hunk-") && !actual.contains("Hunk"));
        }
    }
}
