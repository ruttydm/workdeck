//! MIT translation of Hunk's separate patch-normalization, parsing and construction probes.

use super::*;
use std::time::Instant;
use workdeck_diff::{
    build_diff_file, find_patch_chunk, parse_sanitized_patch_metadata, sanitize_patch,
    split_patch_into_file_chunks,
};

const SCENARIOS: [(&str, usize, usize, usize); 3] = [
    ("many_small_files", 240, 48, 8),
    ("balanced_changeset", 96, 220, 48),
    ("large_single_file", 1, 18_000, 2_000),
];

fn patch(files: usize, lines: usize, changed: usize) -> String {
    fixtures::patch(&fixtures::Options {
        file_count: files as f64,
        lines: lines as f64,
        changed_start: None,
        changed_lines: Some(changed as f64),
        extension: "ts".into(),
        prefix: "src/bench".into(),
    })
}

struct Measurement {
    timings: [f64; 4],
    files: Vec<workdeck_core::DiffFile>,
    patch_bytes: usize,
}

fn measure(name: &str, patch: &str) -> Result<Measurement> {
    let start = Instant::now();
    let sanitized = sanitize_patch(patch);
    let normalize_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let parsed = parse_sanitized_patch_metadata(&sanitized)?;
    let parse_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let chunks = split_patch_into_file_chunks(&sanitized);
    let split_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let files = parsed
        .into_iter()
        .enumerate()
        .map(|(index, metadata)| {
            let chunk = find_patch_chunk(
                Some(&metadata.path),
                metadata.previous_path.as_deref(),
                &chunks,
                index,
            );
            build_diff_file(metadata, &chunk, index, name)
        })
        .collect();
    let build_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(Measurement {
        timings: [normalize_ms, parse_ms, split_ms, build_ms],
        files,
        patch_bytes: sanitized.len(),
    })
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark changeset-parse accepts no arguments");
    }
    // Source constructs all patches before timing any scenario.
    let scenarios =
        SCENARIOS.map(|(name, files, lines, changed)| (name, patch(files, lines, changed)));
    for (name, patch) in scenarios {
        let result = measure(name, &patch)?;
        for (suffix, value) in [
            "normalize_patch_ms",
            "parse_patch_ms",
            "split_chunks_ms",
            "build_diff_files_ms",
        ]
        .into_iter()
        .zip(result.timings)
        {
            println!("METRIC {name}_{suffix}={}", fixed(value, 2));
        }
        println!("METRIC {name}_files={}", result.files.len());
        println!("METRIC {name}_patch_bytes={}", result.patch_bytes);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phased_parser_matches_pinned_counts_and_production_models() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-changeset-parse.json"
        ))
        .unwrap();
        let cases = oracle["scenarios"].as_array().unwrap();
        assert_eq!(cases.len(), SCENARIOS.len());
        for ((name, count, lines, changed), expected) in SCENARIOS.into_iter().zip(cases) {
            let patch = patch(count, lines, changed);
            let result = measure(name, &patch).unwrap();
            assert_eq!(expected["name"], name);
            assert_eq!(
                result.files.len() as u64,
                expected["files"].as_u64().unwrap()
            );
            assert_eq!(
                result.patch_bytes as u64,
                expected["patch_bytes"].as_u64().unwrap()
            );
            assert!(
                result
                    .timings
                    .into_iter()
                    .all(|time| time.is_finite() && time >= 0.0)
            );
            let production = workdeck_diff::parse_patch(
                &patch,
                name,
                name,
                workdeck_core::ChangesetSource::Patch { label: name.into() },
            )
            .unwrap();
            let mut rebuilt = production.clone();
            rebuilt.files = result.files;
            rebuilt.refresh_review_identities();
            assert_eq!(rebuilt.files, production.files);
        }
        assert!(run(["extra".into()].into_iter()).is_err());
    }
}
