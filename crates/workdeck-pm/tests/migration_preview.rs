use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use workdeck_pm::{
    Config, ErrorCode, IssueMetadata,
    migration::{PreviewOptions, preview},
};

fn setup() -> (TempDir, PathBuf, PathBuf, PreviewOptions) {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join(".agents/workdeck");
    fs::create_dir_all(source.join("issues")).unwrap();
    let destination = temp.path().join(".workdeck");
    let options = PreviewOptions {
        config: Config::new("WD").unwrap(),
        imported_at: "2026-09-09T00:00:00Z".parse().unwrap(),
    };
    (temp, source, destination, options)
}

#[test]
fn orphan_retirement_markers_cannot_receive_a_fresh_repository_identity() {
    let (_temp, source, destination, options) = setup();
    fs::create_dir_all(destination.join("tombstones/projects")).unwrap();
    fs::write(
        destination.join("tombstones/projects/retired.yml"),
        "retained retirement authority with missing config",
    )
    .unwrap();
    let result = preview(&source, &destination, &options).unwrap();
    assert!(!result.complete);
    assert!(
        result
            .blockers
            .iter()
            .any(|error| error.code == ErrorCode::RecoveryRequired)
    );
    assert!(!destination.join("config.yml").exists());
}

#[test]
fn orphan_historical_records_cannot_receive_a_fresh_repository_identity() {
    for relative in [
        "imported-sessions/old.toml",
        "imported-history/events.jsonl",
        "imported-handoffs/old.md",
        "commands/old.yml",
        "checks/old.yml",
        "check-profiles/old.yml",
    ] {
        let (_temp, source, destination, options) = setup();
        let path = destination.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "preserve orphan historical bytes").unwrap();
        let result = preview(&source, &destination, &options).unwrap();
        assert!(
            !result.complete,
            "{relative} must block a fresh repository identity"
        );
        assert!(
            result
                .blockers
                .iter()
                .any(|error| error.code == ErrorCode::RecoveryRequired)
        );
        assert!(!destination.join("config.yml").exists());
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "preserve orphan historical bytes"
        );
    }
}

#[test]
fn orphan_destination_authority_cannot_receive_a_fresh_repository_identity() {
    let (_temp, source, destination, options) = setup();
    fs::create_dir_all(destination.join("issues/OTHER-1")).unwrap();
    fs::write(
        destination.join("issues/OTHER-1/item.md"),
        "Existing source record with missing config",
    )
    .unwrap();
    let result = preview(&source, &destination, &options).unwrap();
    assert!(!result.complete);
    assert!(
        result
            .blockers
            .iter()
            .any(|error| error.code == ErrorCode::RecoveryRequired)
    );
    assert!(!destination.join("config.yml").exists());
}

#[test]
fn oversized_draft_is_a_preview_blocker_before_any_bootstrap() {
    let (_temp, source, destination, options) = setup();
    fs::create_dir(source.join("handoffs")).unwrap();
    let file = fs::File::create(source.join("handoffs/large.md")).unwrap();
    file.set_len(32 * 1024 * 1024 + 1).unwrap();
    let result = preview(&source, &destination, &options).unwrap();
    assert!(!result.complete);
    assert!(
        result
            .blockers
            .iter()
            .any(|error| error.code == ErrorCode::Unsupported && error.message.contains("32 MiB"))
    );
    assert!(!destination.exists());
}

fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/legacy-repo/.agents/workdeck")
            .join(name),
    )
    .unwrap()
}

#[test]
fn rich_issue_preview_is_deterministic_preserves_fields_and_never_writes() {
    let (_temp, source, destination, options) = setup();
    let original = fixture("issues/WD-1.toml");
    fs::write(source.join("issues/WD-1.toml"), &original).unwrap();
    let first = preview(&source, &destination, &options).unwrap();
    let second = preview(&source, &destination, &options).unwrap();
    assert_eq!(first, second);
    assert!(!destination.exists());
    assert_eq!(
        fs::read_to_string(source.join("issues/WD-1.toml")).unwrap(),
        original
    );
    let draft = first
        .drafts
        .iter()
        .find(|draft| draft.destination_path == Path::new("issues/WD-1/item.md"))
        .unwrap();
    let text = std::str::from_utf8(&draft.content).unwrap();
    let document =
        workdeck_pm::documents::MarkdownDocument::parse(&draft.destination_path, text).unwrap();
    let issue: IssueMetadata = document.deserialize().unwrap();
    assert_eq!(issue.id.as_str(), "WD-1");
    assert_eq!(issue.status, "in_review");
    assert_eq!(issue.files[1].path, "docs/Auth flow.md");
    assert_eq!(issue.commits[0], "abc123");
    assert_eq!(issue.due_at.as_deref(), Some("2026-09-11"));
    assert!(document.body().contains("résumé / 日本語"));
    assert!(issue.custom.contains_key("legacy"));
}

#[test]
fn missing_timestamps_and_malformed_toml_are_blockers_without_partial_writes() {
    let (_temp, source, destination, options) = setup();
    fs::write(source.join("issues/WD-2.toml"), fixture("issues/WD-2.toml")).unwrap();
    fs::write(source.join("issues/WD-8.toml"), "key = [broken").unwrap();
    let result = preview(&source, &destination, &options).unwrap();
    assert!(!result.complete);
    assert_eq!(result.inventory.len(), 2);
    assert!(
        result
            .blockers
            .iter()
            .any(|error| error.message.contains("created_at"))
    );
    assert!(result.blockers.iter().any(|error| {
        error
            .path
            .as_deref()
            .is_some_and(|path| path.ends_with("WD-8.toml"))
    }));
    assert!(!destination.exists());
}

#[test]
fn historical_done_does_not_invent_a_completion_timestamp_or_acceptance() {
    let (_temp, source, destination, options) = setup();
    fs::write(source.join("issues/WD-3.toml"), fixture("issues/WD-3.toml")).unwrap();
    let result = preview(&source, &destination, &options).unwrap();
    let draft = result
        .drafts
        .iter()
        .find(|draft| draft.destination_path == Path::new("issues/WD-3/item.md"))
        .unwrap();
    let document = workdeck_pm::documents::MarkdownDocument::parse(
        &draft.destination_path,
        std::str::from_utf8(&draft.content).unwrap(),
    )
    .unwrap();
    let metadata = document.metadata();
    assert_eq!(
        metadata
            .get("status")
            .and_then(serde_yaml_ng::Value::as_str),
        Some("done")
    );
    assert!(metadata.get("completed_at").is_none());
    assert!(metadata.get("manual_acceptance").is_none());
    assert_eq!(
        metadata["imported_completion"]["imported_at"].as_str(),
        Some("2026-09-09T00:00:00Z")
    );
}

#[test]
fn destination_collisions_and_unsafe_source_identities_block_conversion() {
    let (_temp, source, destination, options) = setup();
    fs::write(source.join("issues/WD-4.toml"), fixture("issues/WD-4.toml")).unwrap();
    fs::write(
        source.join("issues/WD-8.toml"),
        fixture("issues/WD-4.toml").replace("WD-4", "../escape"),
    )
    .unwrap();
    fs::create_dir_all(destination.join("issues/WD-4")).unwrap();
    let target = destination.join("issues/WD-4/item.md");
    fs::write(&target, "User-owned destination").unwrap();
    let result = preview(&source, &destination, &options).unwrap();
    assert!(!result.complete);
    assert!(
        result
            .blockers
            .iter()
            .any(|error| error.code == ErrorCode::Conflict)
    );
    assert!(result.blockers.iter().any(|error| {
        error
            .path
            .as_deref()
            .is_some_and(|path| path.ends_with("WD-8.toml"))
    }));
    assert_eq!(
        fs::read_to_string(target).unwrap(),
        "User-owned destination"
    );
    assert!(!destination.join("config.yml").exists());
}

fn copy_fixture(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_fixture(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn complete_fixture_accounts_for_references_settings_sessions_and_history() {
    let (_temp, source, destination, options) = setup();
    copy_fixture(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy-repo/.agents/workdeck"),
        &source,
    );
    fs::remove_file(source.join("issues/WD-2.toml")).unwrap();
    let result = preview(&source, &destination, &options).unwrap();
    assert!(result.complete, "{:?}", result.blockers);
    assert_eq!(result.inventory.len(), 9);
    assert!(
        result
            .inventory
            .iter()
            .all(|file| file.content_hash.is_some() && !file.destinations.is_empty())
    );
    let bytes = |target: &str| {
        result
            .drafts
            .iter()
            .find(|draft| draft.destination_path == Path::new(target))
            .unwrap()
            .content
            .as_slice()
    };
    assert_eq!(
        bytes("imported-sessions/fixture-session.toml"),
        fixture("agents/fixture-session.toml").as_bytes()
    );
    assert_eq!(
        bytes("imported-history/events.jsonl"),
        fixture("events.jsonl").as_bytes()
    );
    let project = workdeck_pm::documents::MarkdownDocument::parse(
        Path::new("project.md"),
        std::str::from_utf8(bytes("projects/account_access/item.md")).unwrap(),
    )
    .unwrap();
    let project: workdeck_pm::PlanningMetadata = project.deserialize().unwrap();
    assert_eq!(project.status.as_deref(), Some("active"));
    assert!(
        project.custom["legacy"]["extra"]["value"]
            .get("fixture_extra")
            .is_some()
    );
    let cycle = workdeck_pm::documents::MarkdownDocument::parse(
        Path::new("cycle.md"),
        std::str::from_utf8(bytes("cycles/week-37/item.md")).unwrap(),
    )
    .unwrap();
    let cycle: workdeck_pm::PlanningMetadata = cycle.deserialize().unwrap();
    assert_eq!(cycle.starts_at.as_deref(), Some("2026-09-07"));
    assert!(cycle.created_at.is_none());
    let labels: workdeck_pm::LabelsMetadata =
        serde_yaml_ng::from_slice(bytes("labels.yml")).unwrap();
    assert_eq!(labels.labels.len(), 2);
    assert_eq!(labels.labels[1].color.as_deref(), Some("#abcdef"));
    let app = std::str::from_utf8(bytes("config.toml")).unwrap();
    assert_eq!(
        app,
        fixture("config.toml").replace(
            "data_dir = \".agents/workdeck\"",
            "data_dir = \".workdeck\""
        )
    );
    assert!(!destination.exists());
}

#[test]
fn typed_extra_toml_values_do_not_lose_datetime_integer_float_or_nested_types() {
    let (_temp, source, destination, options) = setup();
    let original = format!(
        "{}\nobserved = 1979-05-27T07:32:00Z\nquoted_date = \"1979-05-27T07:32:00Z\"\nmaximum = 9223372036854775807\nunbounded = inf\nnot_a_number = nan\nnegative_zero = -0.0\nnested = {{ flags = [true, false], actor = \"é\" }}\n",
        fixture("issues/WD-4.toml")
    );
    fs::write(source.join("issues/WD-4.toml"), original).unwrap();
    let result = preview(&source, &destination, &options).unwrap();
    assert!(result.complete, "{:?}", result.blockers);
    let draft = result
        .drafts
        .iter()
        .find(|draft| draft.kind == workdeck_pm::migration::MigrationKind::Issue)
        .unwrap();
    let document = workdeck_pm::documents::MarkdownDocument::parse(
        &draft.destination_path,
        std::str::from_utf8(&draft.content).unwrap(),
    )
    .unwrap();
    let issue: IssueMetadata = document.deserialize().unwrap();
    let extra = &issue.custom["legacy"]["extra"]["value"];
    assert_eq!(extra["observed"]["type"], "datetime");
    assert_eq!(extra["quoted_date"]["type"], "string");
    assert_eq!(extra["maximum"]["value"], i64::MAX);
    assert_eq!(extra["unbounded"]["value"], "inf");
    assert_eq!(extra["not_a_number"]["value"], "NaN");
    assert_eq!(extra["negative_zero"]["value"], "-0");
    assert_eq!(
        extra["nested"]["value"]["flags"]["value"][0]["type"],
        "boolean"
    );
    let encoded = serde_json::to_vec(&result).unwrap();
    let decoded: workdeck_pm::migration::MigrationPreview =
        serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, result);
}

#[test]
fn config_patch_preserves_comments_and_blocks_unmapped_custom_roots() {
    let (_temp, source, destination, options) = setup();
    let config = "# Keep this note\n[paths] # exact section\ndata_dir = '.agents/workdeck' # why here\n\n[custom]\nkey = 'untouched'\n";
    fs::write(source.join("config.toml"), config).unwrap();
    let first = preview(&source, &destination, &options).unwrap();
    assert!(first.complete);
    let draft = first
        .drafts
        .iter()
        .find(|draft| draft.destination_path == Path::new("config.toml"))
        .unwrap();
    let rendered = std::str::from_utf8(&draft.content).unwrap();
    assert!(rendered.contains("# Keep this note\n[paths] # exact section\n"));
    assert!(rendered.contains("# why here\n\n[custom]\nkey = 'untouched'"));
    fs::write(
        source.join("config.toml"),
        config.replace(".agents/workdeck", "/external/planning"),
    )
    .unwrap();
    let result = preview(&source, &destination, &options).unwrap();
    assert!(!result.complete);
    assert!(
        result
            .blockers
            .iter()
            .any(|error| error.code == ErrorCode::Unsupported)
    );
    assert!(!destination.exists());
}

#[cfg(unix)]
#[test]
fn symlink_sources_and_destinations_are_diagnostics_and_are_never_followed() {
    use std::os::unix::fs::symlink;
    let (temp, source, destination, options) = setup();
    let outside = temp.path().join("outside.toml");
    fs::write(&outside, fixture("issues/WD-4.toml")).unwrap();
    symlink(&outside, source.join("issues/WD-4.toml")).unwrap();
    let result = preview(&source, &destination, &options).unwrap();
    assert!(!result.complete);
    assert!(
        result
            .blockers
            .iter()
            .any(|error| error.code == ErrorCode::UnsafePath)
    );
    assert_eq!(result.inventory[0].content_hash, None);
    fs::remove_file(source.join("issues/WD-4.toml")).unwrap();
    fs::write(source.join("issues/WD-4.toml"), fixture("issues/WD-4.toml")).unwrap();
    fs::create_dir_all(&destination).unwrap();
    symlink(temp.path(), destination.join("issues")).unwrap();
    let result = preview(&source, &destination, &options).unwrap();
    assert!(!result.complete);
    assert!(
        result
            .blockers
            .iter()
            .any(|error| error.code == ErrorCode::UnsafePath)
    );
    assert_eq!(
        fs::read_to_string(&outside).unwrap(),
        fixture("issues/WD-4.toml")
    );
}

#[test]
fn unknown_files_disposable_indexes_and_inert_handoffs_have_explicit_dispositions() {
    let (_temp, source, destination, options) = setup();
    fs::write(source.join("unknown.bin"), [0, 255, 4]).unwrap();
    fs::create_dir(source.join("index")).unwrap();
    fs::write(source.join("index/cache.bin"), [4, 5, 6]).unwrap();
    fs::create_dir(source.join("handoffs")).unwrap();
    let payload = b"#!/bin/sh\ntouch SHOULD_NEVER_EXECUTE\n";
    fs::write(source.join("handoffs/context.sh"), payload).unwrap();
    let result = preview(&source, &destination, &options).unwrap();
    assert_eq!(result.inventory.len(), 3);
    assert!(!result.complete);
    assert!(
        result
            .blockers
            .iter()
            .any(|error| error.message.contains("unclassified"))
    );
    assert!(
        result
            .notices
            .iter()
            .any(|notice| notice.code == "disposable_index")
    );
    assert_eq!(
        result
            .drafts
            .iter()
            .find(|draft| draft.destination_path == Path::new("imported-handoffs/context.sh"))
            .unwrap()
            .content,
        payload
    );
    assert!(!source.join("SHOULD_NEVER_EXECUTE").exists());
    assert!(!destination.exists());
}

#[test]
fn preview_fingerprint_binds_context_source_and_destination_state() {
    let (_temp, source, destination, mut options) = setup();
    fs::write(source.join("issues/WD-4.toml"), fixture("issues/WD-4.toml")).unwrap();
    let first = preview(&source, &destination, &options).unwrap();
    options.imported_at += chrono::Duration::seconds(1);
    let second = preview(&source, &destination, &options).unwrap();
    assert_ne!(first.fingerprint, second.fingerprint);
    fs::write(
        source.join("issues/WD-4.toml"),
        fixture("issues/WD-4.toml").replace("Explicit Todo", "Changed Todo"),
    )
    .unwrap();
    let third = preview(&source, &destination, &options).unwrap();
    assert_ne!(second.fingerprint, third.fingerprint);
    fs::create_dir_all(&destination).unwrap();
    fs::write(
        destination.join("config.yml"),
        serde_yaml_ng::to_string(&options.config).unwrap(),
    )
    .unwrap();
    let fourth = preview(&source, &destination, &options).unwrap();
    assert_ne!(third.fingerprint, fourth.fingerprint);
    assert!(fourth.complete);
}
