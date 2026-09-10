//! Website-only third-party assets, kept separate from binary dependency SBOMs.
use anyhow::{Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};

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
        for file in &assets[0].files {
            let destination = temp.path().join(&file.path);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::copy(repo.join(&file.path), destination).unwrap();
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
        assert_eq!(output["components"].as_array().unwrap().len(), 8);
        assert_eq!(
            output["metadata"]["component"]["name"],
            "workdeck-website-assets"
        );
        for component in output["components"].as_array().unwrap() {
            assert_eq!(component["licenses"][0]["license"]["id"], "OFL-1.1");
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
}
