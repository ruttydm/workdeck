//! Publication-set semantics from Hunk generate-og.ts (MIT, Modem Labs Inc.).
//! Planning is read-only; application must revalidate originals before writing.
use super::{Target, check_capture};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{collections::BTreeMap, fs, path::Path};

const CHANGELOG: &str = "site/static/changelog/og";

#[derive(Debug, Serialize)]
pub(super) struct Plan {
    pub originals: BTreeMap<String, Option<Vec<u8>>>,
    pub replacements: BTreeMap<String, Option<Vec<u8>>>,
}

fn read_regular(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "publication entry must be a regular file: {}",
                path.display()
            );
            Ok(Some(fs::read(path)?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn check_parents(repo: &Path, relative: &str) -> Result<()> {
    let mut current = repo.to_owned();
    for component in Path::new(relative).parent().unwrap().components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "publication parent must be a real directory: {}",
                current.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn inventory(
    repo: &Path,
    relative: &str,
    files: &mut BTreeMap<String, Option<Vec<u8>>>,
) -> Result<()> {
    let path = repo.join(relative);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "publication set must be a real directory"
    );
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("non-UTF-8 publication filename"))?;
        let child = format!("{relative}/{name}");
        let kind = entry.file_type()?;
        if kind.is_dir() {
            inventory(repo, &child, files)?;
        } else {
            files.insert(child.clone(), read_regular(&repo.join(child))?);
        }
    }
    Ok(())
}

pub(super) fn plan(repo: &Path, staging: &Path, targets: &[Target], full: bool) -> Result<Plan> {
    check_capture(staging, targets, full)?;
    let mut originals = BTreeMap::new();
    let mut replacements = BTreeMap::new();
    if full {
        check_parents(repo, &format!("{CHANGELOG}/card.png"))?;
        inventory(repo, CHANGELOG, &mut originals)?;
        replacements.extend(originals.keys().map(|name| (name.clone(), None)));
    }
    for (index, target) in targets.iter().enumerate() {
        let name = &target.output_file;
        check_parents(repo, name)?;
        originals.insert(name.clone(), read_regular(&repo.join(name))?);
        ensure!(
            replacements.get(name).is_none_or(Option::is_none),
            "duplicate publication destination: {name}"
        );
        replacements.insert(
            name.clone(),
            read_regular(&staging.join(format!("{index:04}.png")))?,
        );
    }
    replacements.retain(|name, bytes| originals[name] != *bytes);
    originals.retain(|name, _| replacements.contains_key(name));
    Ok(Plan {
        originals,
        replacements,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_sweeps_stale_images_but_targeted_preserves_them() {
        for full in [false, true] {
            let repo = tempfile::tempdir().unwrap();
            let staging = tempfile::tempdir().unwrap();
            let targets = super::super::select(vec![], &[]).unwrap();
            let mut png = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut png, 1200, 630);
                encoder.set_color(png::ColorType::Rgb);
                encoder.set_depth(png::BitDepth::Eight);
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(&vec![0; 1200 * 630 * 3])
                    .unwrap();
            }
            fs::write(staging.path().join("0000.png"), &png).unwrap();
            let report = super::super::capture_report(staging.path(), &targets, full).unwrap();
            super::super::save_capture_manifest(
                staging.path(),
                &serde_json::to_string(&report).unwrap(),
            )
            .unwrap();
            fs::create_dir_all(repo.path().join(CHANGELOG)).unwrap();
            let stale = format!("{CHANGELOG}/old.png");
            fs::write(repo.path().join(&stale), b"old image").unwrap();
            let result = plan(repo.path(), staging.path(), &targets, full).unwrap();
            assert_eq!(result.replacements.len(), if full { 2 } else { 1 });
            assert_eq!(result.replacements.contains_key(&stale), full);
            if full {
                assert_eq!(result.replacements[&stale], None);
                assert_eq!(result.originals[&stale], Some(b"old image".to_vec()));
            }
            assert_eq!(result.replacements[&targets[0].output_file], Some(png));
            assert_eq!(fs::read(repo.path().join(stale)).unwrap(), b"old image");
            assert!(!repo.path().join("site/static/extensions").exists());
        }
    }
}
