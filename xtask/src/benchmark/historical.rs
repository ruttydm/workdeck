//! Lossless archival of MIT-licensed pinned Hunk release measurements.
use anyhow::{Result, ensure};
use std::path::Path;

const PIN: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const OUTPUT: &str = "port/hunk/historical-release-benchmarks.json";

fn archive(repo: &Path) -> Result<String> {
    let mut reports = Vec::new();
    for entry in super::super::read_tree(repo, PIN)? {
        if !entry.path.starts_with("benchmarks/release/bench-") || !entry.path.ends_with(".json") {
            continue;
        }
        let bytes = super::super::git_stdout_bytes(repo, ["cat-file", "blob", &entry.blob])?;
        ensure!(
            u64::try_from(bytes.len())? == entry.bytes,
            "historical report byte count mismatch"
        );
        let text = String::from_utf8(bytes)?;
        let _: serde_json::Value = serde_json::from_str(&text)?;
        reports.push(serde_json::json!({"path":entry.path,"blob":entry.blob,"bytes":entry.bytes,"sourceText":text}));
    }
    ensure!(
        reports.len() == 22,
        "expected 22 pinned historical release reports"
    );
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({
            "baseline":PIN,
            "license":"MIT; Copyright (c) Modem Labs Inc.",
            "status":"Historical Hunk measurements only; not Workdeck performance acceptance evidence.",
            "reports":reports
        }))?
    ))
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let check = match args.next().as_deref() {
        None => false,
        Some("--check") => true,
        Some(_) => anyhow::bail!("historical-release accepts only --check"),
    };
    ensure!(
        args.next().is_none(),
        "unexpected historical-release argument"
    );
    let repo = super::super::repo_root()?;
    let expected = archive(&repo)?;
    if check {
        ensure!(
            std::fs::read_to_string(repo.join(OUTPUT))? == expected,
            "historical release archive differs from pinned bytes"
        );
    } else {
        std::fs::write(repo.join(OUTPUT), expected)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn historical_archive_reproduces_every_pinned_report_byte() {
        let repo = super::super::super::repo_root().unwrap();
        let expected = archive(&repo).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.join(OUTPUT)).unwrap(),
            expected
        );
    }
}
