use crate::{DiffFile, FileChangeKind};

const LANE_SEEDS: [u32; 4] = [0x811c_9dc5, 0x0100_0193, 0x9e37_79b9, 0x85eb_ca6b];
const LANE_PRIMES: [u32; 4] = [0x0100_0193, 0x0100_0199, 0x0100_0187, 0x0100_019d];

/// Hunk-compatible renderer-neutral identity hash. This is distinct from the SHA-256 digest used
/// to attest serialized wire resources.
pub fn review_content_digest(parts: &[&str]) -> String {
    let mut lanes = LANE_SEEDS;
    for part in parts {
        let utf16_length = part.encode_utf16().count();
        for code in format!("{utf16_length}:").encode_utf16() {
            mix_lanes(&mut lanes, code);
        }
        for code in part.encode_utf16() {
            mix_lanes(&mut lanes, code);
        }
        mix_lanes(&mut lanes, 0x1f);
    }
    lanes
        .iter()
        .map(|lane| format!("{lane:08x}"))
        .collect::<String>()
}

fn mix_lanes(lanes: &mut [u32; 4], code: u16) {
    for (index, lane) in lanes.iter_mut().enumerate() {
        *lane = (*lane ^ u32::from(code)).wrapping_mul(LANE_PRIMES[index]);
    }
}

pub(crate) fn review_file_content_identity(file: &DiffFile) -> String {
    let mut hunk_signatures = Vec::with_capacity(file.hunks.len());
    let mut old_end = 1_u32;
    let mut new_end = 1_u32;
    let mut addition_line_index = 0_usize;
    let mut deletion_line_index = 0_usize;
    for hunk in &file.hunks {
        let old_gap = hunk.old_start.saturating_sub(old_end);
        let new_gap = hunk.new_start.saturating_sub(new_end);
        let collapsed_before = old_gap.min(new_gap);
        hunk_signatures.push(format!(
            "{collapsed_before},{},{},{addition_line_index},{},{},{deletion_line_index}",
            hunk.new_start, hunk.new_count, hunk.old_start, hunk.old_count
        ));
        addition_line_index += hunk
            .lines
            .iter()
            .filter(|line| line.new_line.is_some())
            .count();
        deletion_line_index += hunk
            .lines
            .iter()
            .filter(|line| line.old_line.is_some())
            .count();
        old_end = hunk.old_start.saturating_add(hunk.old_count);
        new_end = hunk.new_start.saturating_add(hunk.new_count);
    }

    let mut addition_lines = (!file.flags.partial)
        .then_some(file.sources.new.as_ref())
        .flatten()
        .map(|source| rendered_source_lines(&source.content))
        .unwrap_or_default();
    let mut deletion_lines = (!file.flags.partial)
        .then_some(file.sources.old.as_ref())
        .flatten()
        .map(|source| rendered_source_lines(&source.content))
        .unwrap_or_default();
    if addition_lines.is_empty() && deletion_lines.is_empty() {
        for line in file.hunks.iter().flat_map(|hunk| &hunk.lines) {
            let mut rendered = line.content.clone();
            if !line.no_newline_at_eof {
                rendered.push('\n');
            }
            if line.new_line.is_some() {
                addition_lines.push(rendered.clone());
            }
            if line.old_line.is_some() {
                deletion_lines.push(rendered);
            }
        }
    }

    let change_kind = match file.change_kind {
        FileChangeKind::Renamed if file.stats.additions == 0 && file.stats.deletions == 0 => {
            "rename-pure"
        }
        FileChangeKind::Renamed => "rename-changed",
        FileChangeKind::Added | FileChangeKind::Untracked => "new",
        FileChangeKind::Deleted => "deleted",
        FileChangeKind::Modified
        | FileChangeKind::Copied
        | FileChangeKind::TypeChanged
        | FileChangeKind::Conflicted => "change",
    };
    let stats = format!(
        "{}/{}/{}",
        file.stats.additions,
        file.stats.deletions,
        u8::from(file.stats.truncated)
    );
    let flags = format!(
        "{}{}{}{}",
        u8::from(file.flags.untracked),
        u8::from(file.flags.binary),
        u8::from(file.flags.too_large),
        u8::from(file.flags.partial)
    );
    let hunk_signature = hunk_signatures.join(";");
    let mut parts = vec![
        file.path.clone(),
        file.previous_path.clone().unwrap_or_default(),
        change_kind.into(),
        file.language.clone().unwrap_or_default(),
        stats,
        flags,
        hunk_signature,
        file.patch.clone(),
    ];
    parts.extend(addition_lines);
    parts.push("\0deletions".into());
    parts.extend(deletion_lines);
    let borrowed = parts.iter().map(String::as_str).collect::<Vec<_>>();
    review_content_digest(&borrowed)
}

fn rendered_source_lines(source: &str) -> Vec<String> {
    source
        .replace("\r\n", "\n")
        .split_inclusive('\n')
        .map(str::to_owned)
        .collect()
}

pub fn review_file_key(
    source_label: &str,
    path: &str,
    previous_path: Option<&str>,
    duplicate_index: usize,
) -> String {
    let duplicate_index = duplicate_index.to_string();
    format!(
        "file:{}",
        review_content_digest(&[
            source_label,
            path,
            previous_path.unwrap_or_default(),
            &duplicate_index,
        ])
    )
}

pub fn review_source_identity(
    path: &str,
    content_identity: &str,
    fetcher_cache_key: Option<&str>,
) -> String {
    format!(
        "source:{}",
        review_content_digest(&[
            path,
            content_identity,
            fetcher_cache_key.unwrap_or_default(),
        ])
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiffHunk, DiffLine, DiffLineKind, FileFlags, FileStats};

    fn file() -> DiffFile {
        DiffFile {
            key: String::new(),
            runtime_id: "runtime".into(),
            path: "src/alpha.ts".into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: Some("typescript".into()),
            stats: FileStats {
                additions: 1,
                deletions: 1,
                truncated: false,
            },
            flags: FileFlags::default(),
            patch: "@@ -1 +1 @@\n-a\n+b\n".into(),
            split_row_count: 1,
            stack_row_count: 2,
            hunks: vec![DiffHunk {
                index: 0,
                header: "@@ -1 +1 @@".into(),
                context: None,
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
                split_row_start: 0,
                split_row_count: 1,
                stack_row_start: 0,
                stack_row_count: 2,
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Deletion,
                        content: "a".into(),
                        old_line: Some(1),
                        new_line: None,
                        moved: false,
                        no_newline_at_eof: false,
                    },
                    DiffLine {
                        kind: DiffLineKind::Addition,
                        content: "b".into(),
                        old_line: None,
                        new_line: Some(1),
                        moved: false,
                        no_newline_at_eof: false,
                    },
                ],
            }],
            content_identity: String::new(),
            sources: crate::FileSourceSnapshots::default(),
            source_identity: None,
            source_attested: false,
            agent: None,
        }
    }

    #[test]
    fn content_digest_is_utf16_stable_framed_ordered_and_fixed_width() {
        let digest = review_content_digest(&["alpha", "🧪"]);
        assert_eq!(digest, "6d73e458208f9c4cc690752047740040");
        assert_eq!(digest.len(), 32);
        assert_eq!(digest, review_content_digest(&["alpha", "🧪"]));
        assert_ne!(
            review_content_digest(&["ab", "c"]),
            review_content_digest(&["a", "bc"])
        );
        assert_ne!(
            review_content_digest(&["a", "b"]),
            review_content_digest(&["b", "a"])
        );
    }

    #[test]
    fn file_content_identity_covers_rendered_lines_geometry_and_flags() {
        let base = file();
        let identity = review_file_content_identity(&base);
        let mut changed = base.clone();
        changed.hunks[0].lines[1].content = "c".into();
        assert_ne!(identity, review_file_content_identity(&changed));
        let mut flagged = base.clone();
        flagged.flags.untracked = true;
        assert_ne!(identity, review_file_content_identity(&flagged));
        let mut geometry = base.clone();
        geometry.hunks[0].new_start = 3;
        assert_ne!(identity, review_file_content_identity(&geometry));
    }

    #[test]
    fn file_keys_address_reviews_not_content_and_source_identity_names_snapshots() {
        let key = review_file_key("HEAD", "src/alpha.ts", None, 0);
        assert_eq!(key, review_file_key("HEAD", "src/alpha.ts", None, 0));
        assert_ne!(key, review_file_key("HEAD~1", "src/alpha.ts", None, 0));
        assert_ne!(key, review_file_key("HEAD", "src/alpha.ts", None, 1));
        assert_ne!(
            review_source_identity("src/alpha.ts", "content", Some("tree:1")),
            review_source_identity("src/alpha.ts", "content", Some("tree:2"))
        );
    }
}
