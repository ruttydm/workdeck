use std::{fs, path::Path};
use tempfile::TempDir;
use workdeck_pm::{ErrorCode, Repository};

#[test]
fn discovery_does_not_initialize_and_init_uses_git_root() {
    let temp = TempDir::new().unwrap();
    fs::create_dir(temp.path().join(".git")).unwrap();
    let nested = temp.path().join("src/nested");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(
        Repository::discover(&nested).unwrap_err().code,
        ErrorCode::NotInitialized
    );
    assert!(!temp.path().join(".workdeck").exists());
    let repository = Repository::init(&nested, "WD").unwrap();
    assert_eq!(
        repository.root(),
        temp.path().canonicalize().unwrap().join(".workdeck")
    );
    assert_eq!(
        Repository::discover(&nested).unwrap().root(),
        repository.root()
    );
    assert!(!nested.join(".workdeck").exists());
}

#[test]
fn init_is_repeatable_preserves_app_preferences_and_adds_required_ignores() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join(".workdeck");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("config.toml"), "theme = 'dark'\n").unwrap();
    fs::write(root.join(".gitignore"), "# personal additions\nsecrets/\n").unwrap();
    let first = Repository::init(temp.path(), "WD").unwrap();
    let config = first.config().unwrap();
    let bytes = fs::read(root.join("config.yml")).unwrap();
    let second = Repository::init(temp.path(), "WD").unwrap();
    assert_eq!(second.config().unwrap().repository, config.repository);
    assert_eq!(fs::read(root.join("config.yml")).unwrap(), bytes);
    assert_eq!(
        fs::read_to_string(root.join("config.toml")).unwrap(),
        "theme = 'dark'\n"
    );
    let ignore = fs::read_to_string(root.join(".gitignore")).unwrap();
    for line in [
        "# personal additions",
        "secrets/",
        "/.index/",
        "/.tmp/",
        "/.local/",
        "/config.local.yml",
        "/config.local.toml",
    ] {
        assert!(
            ignore.lines().any(|actual| actual == line),
            "missing {line}"
        );
    }
    assert_eq!(ignore.matches("/.tmp/").count(), 1);
    assert_eq!(
        Repository::init(temp.path(), "NEW").unwrap_err().code,
        ErrorCode::Conflict
    );
}

#[test]
fn malformed_and_legacy_stores_are_explicit_and_preserved() {
    let temp = TempDir::new().unwrap();
    let old = temp.path().join(".agents/workdeck");
    fs::create_dir_all(old.join("issues")).unwrap();
    fs::write(old.join("issues/WD-1.toml"), "title = 'original'\n").unwrap();
    assert_eq!(
        Repository::init(temp.path(), "WD").unwrap_err().code,
        ErrorCode::LegacyStore
    );
    assert!(!temp.path().join(".workdeck").exists());
    fs::create_dir(temp.path().join(".workdeck")).unwrap();
    fs::write(temp.path().join(".workdeck/config.yml"), "schema: [\n").unwrap();
    assert_eq!(
        Repository::discover(temp.path()).unwrap_err().code,
        ErrorCode::AmbiguousSource
    );
    let error = Repository::open_source(&temp.path().join(".workdeck")).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidSchema);
    assert!(error.line.is_some());
    assert!(error.path.unwrap().ends_with("config.yml"));
    assert_eq!(
        fs::read_to_string(old.join("issues/WD-1.toml")).unwrap(),
        "title = 'original'\n"
    );
}

#[test]
fn concurrent_initialization_keeps_one_repository_identity() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().to_owned();
    let workers = (0..4)
        .map(|_| {
            let path = path.clone();
            std::thread::spawn(move || {
                Repository::init(&path, "WD")
                    .unwrap()
                    .config()
                    .unwrap()
                    .repository
            })
        })
        .collect::<Vec<_>>();
    let ids = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert!(ids.iter().all(|id| id == &ids[0]));
}

#[cfg(unix)]
#[test]
fn init_and_discovery_reject_symlinked_authoritative_paths() {
    use std::os::unix::fs::symlink;
    let temp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    symlink(outside.path(), temp.path().join(".workdeck")).unwrap();
    assert_eq!(
        Repository::init(temp.path(), "WD").unwrap_err().code,
        ErrorCode::UnsafePath
    );
    assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
}

#[test]
fn unsupported_schema_is_diagnosed_without_rewriting_source() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let path = repository.root().join("config.yml");
    let input = fs::read_to_string(&path)
        .unwrap()
        .replace("schema: 1", "schema: 99");
    fs::write(&path, &input).unwrap();
    assert_eq!(
        Repository::open_source(repository.root()).unwrap_err().code,
        ErrorCode::UnsupportedSchema
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), input);
}

#[test]
fn empty_and_invalid_prefixes_create_nothing() {
    for prefix in ["", "../WD", "wd"] {
        let temp = TempDir::new().unwrap();
        assert!(Repository::init(temp.path(), prefix).is_err());
        assert!(!Path::new(temp.path()).join(".workdeck").exists());
    }
}

#[test]
fn enclosing_git_boundary_wins_over_nested_planning_fixture() {
    let temp = TempDir::new().unwrap();
    let nested = temp.path().join("fixtures/sample");
    fs::create_dir_all(&nested).unwrap();
    let nested_source = Repository::init(&nested, "NEST").unwrap();
    let nested_identity = nested_source.config().unwrap().repository;
    fs::create_dir(temp.path().join(".git")).unwrap();
    let main = Repository::init(temp.path(), "WD").unwrap();
    assert_eq!(Repository::discover(&nested).unwrap().root(), main.root());
    assert_eq!(Repository::init(&nested, "WD").unwrap().root(), main.root());
    assert_eq!(
        Repository::open_source(nested_source.root())
            .unwrap()
            .config()
            .unwrap()
            .repository,
        nested_identity
    );
}

#[test]
fn missing_config_with_authoritative_data_cannot_create_a_new_identity() {
    for relative in [
        "issues/WD-1/item.md",
        "operations/OP-orphan.yml",
        ".tmp/journals/OP-orphan.yml",
        "imported-sessions/orphan.toml",
        "imported-history/events.jsonl",
        "imported-handoffs/orphan.md",
    ] {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join(".workdeck");
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "preserve unexplained existing planning bytes\n").unwrap();
        let error = Repository::init(temp.path(), "WD").unwrap_err();
        assert_eq!(error.code, ErrorCode::RecoveryRequired, "{relative}");
        assert!(!root.join("config.yml").exists(), "{relative}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "preserve unexplained existing planning bytes\n"
        );
    }
}

#[test]
fn interrupted_empty_init_directories_and_preferences_are_reusable() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join(".workdeck");
    for directory in [
        "issues",
        "comments",
        "operations",
        ".local",
        ".index",
        ".tmp/journals",
        ".tmp/writes",
    ] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    fs::write(root.join("config.toml"), "theme = 'dark'\n").unwrap();
    fs::write(root.join(".gitignore"), "# preserve existing rules\n").unwrap();
    fs::write(root.join(".tmp/writer.lock"), "").unwrap();
    fs::write(
        root.join(".tmp/init-OP-01ARZ3NDEKTSV4RRFFQ69G5FAV"),
        "unpublished temporary bytes",
    )
    .unwrap();
    Repository::init(temp.path(), "WD").unwrap();
    assert_eq!(
        fs::read_to_string(root.join("config.toml")).unwrap(),
        "theme = 'dark'\n"
    );
    assert!(
        fs::read_to_string(root.join(".gitignore"))
            .unwrap()
            .starts_with("# preserve existing rules\n")
    );
}

#[test]
fn doctor_validates_canonical_issue_records_without_parsing_child_markdown_as_issues() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let metadata = workdeck_pm::IssueMetadata::new(
        &repository.config().unwrap(),
        "Schema fixture",
        "2026-09-08T10:00:00Z".parse().unwrap(),
    )
    .unwrap();
    let issue_dir = repository.root().join("issues").join(metadata.id.as_str());
    fs::create_dir_all(issue_dir.join("comments")).unwrap();
    fs::create_dir_all(issue_dir.join("attachments")).unwrap();
    fs::write(
        issue_dir.join("item.md"),
        format!(
            "---\n{}---\nIssue body\n",
            serde_yaml_ng::to_string(&metadata).unwrap()
        ),
    )
    .unwrap();
    fs::write(issue_dir.join("comments/COM-01ARZ3NDEKTSV4RRFFQ69G5FAV.md"), format!("---\nschema: 1\nid: COM-01ARZ3NDEKTSV4RRFFQ69G5FAV\nissue: {}\nauthor: reviewer\ncreated_at: 2026-09-08T10:00:00Z\nrevision: 1\n---\nA comment\n", metadata.id)).unwrap();
    repository
        .attach_issue(
            metadata.id.as_str(),
            None,
            &workdeck_pm::AttachmentInput {
                name: "notes.md".into(),
                content: b"# Attachment\nOrdinary Markdown without issue frontmatter.\n".to_vec(),
                media_type: Some("text/markdown".into()),
                actor: "reviewer".into(),
            },
            &workdeck_pm::RequestId::new(),
        )
        .unwrap();
    let report = repository.doctor().unwrap();
    assert!(report.valid, "{:?}", report.errors);
    assert_eq!(report.checked_records, 3);
    let renamed = repository.root().join("issues/WD-1");
    fs::rename(&issue_dir, &renamed).unwrap();
    let report = repository.doctor().unwrap();
    assert!(!report.valid);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("identity"))
    );
}

#[cfg(unix)]
#[test]
fn fifo_probe_child() {
    let Some(root) = std::env::var_os("WORKDECK_PM_FIFO_PROBE_ROOT") else {
        return;
    };
    let error = match std::env::var("WORKDECK_PM_FIFO_PROBE_MODE")
        .unwrap()
        .as_str()
    {
        "open" => Repository::open_source(Path::new(&root)).unwrap_err(),
        "init" => Repository::init(Path::new(&root).parent().unwrap(), "WD").unwrap_err(),
        mode => panic!("unknown FIFO probe mode {mode}"),
    };
    assert_eq!(error.code, ErrorCode::UnsafePath);
}

#[cfg(unix)]
fn assert_fifo_rejected_without_blocking(filename: &str, mode: &str) {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let temp = TempDir::new().unwrap();
    let root = temp.path().join(".workdeck");
    fs::create_dir(&root).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(root.join(filename))
            .status()
            .unwrap()
            .success()
    );
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fifo_probe_child", "--nocapture"])
        .env("WORKDECK_PM_FIFO_PROBE_ROOT", &root)
        .env("WORKDECK_PM_FIFO_PROBE_MODE", mode)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "FIFO {filename} was not rejected as UnsafePath"
            );
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("FIFO {filename} blocked planning inspection beyond the deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn config_fifo_is_rejected_before_open_can_block() {
    assert_fifo_rejected_without_blocking("config.yml", "open");
}

#[cfg(unix)]
#[test]
fn ignore_fifo_is_rejected_before_init_read_can_block() {
    assert_fifo_rejected_without_blocking(".gitignore", "init");
}

fn doctor_fixture_issue(repository: &Repository) -> workdeck_pm::IssueRecord {
    serde_json::from_value(
        repository
            .create_issue(
                &workdeck_pm::CreateIssue::new("Inspect all independent records", "A task body"),
                &workdeck_pm::RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap()
}

fn doctor_fixture_attachment(
    repository: &Repository,
    issue: &workdeck_pm::IssueRecord,
) -> workdeck_pm::AttachmentRecord {
    serde_json::from_value(
        repository
            .attach_issue(
                issue.metadata.id.as_str(),
                None,
                &workdeck_pm::AttachmentInput {
                    name: "report.md".into(),
                    content: vec![0xff, 0xfe, 0x00, 0x42],
                    media_type: Some("text/markdown".into()),
                    actor: "reviewer".into(),
                },
                &workdeck_pm::RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap()
}

fn doctor_fixture_template(
    repository: &Repository,
    id: &str,
    defaults: &str,
) -> std::path::PathBuf {
    let path = repository.root().join(format!("templates/issues/{id}.md"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, format!("---\nschema: 1\nid: {id}\nname: {id} template\ndefaults: {defaults}\n---\nTemplate body\n")).unwrap();
    path
}

#[test]
fn doctor_checks_comments_attachment_metadata_and_templates_without_decoding_binary_payloads() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let issue = doctor_fixture_issue(&repository);
    repository
        .add_comment(
            issue.metadata.id.as_str(),
            &issue.source,
            "reviewer",
            "A comment",
            &workdeck_pm::RequestId::new(),
        )
        .unwrap();
    let attachment = doctor_fixture_attachment(&repository, &issue);
    doctor_fixture_template(&repository, "bug", "{priority: high}");
    let report = repository.doctor().unwrap();
    assert!(report.valid, "{:?}", report.errors);
    assert_eq!(
        report.checked_records, 4,
        "one item, one comment, one attachment descriptor, one template; no payload record"
    );
    assert_eq!(
        fs::read(repository.root().join(&attachment.content_path)).unwrap(),
        [0xff, 0xfe, 0x00, 0x42]
    );
}

#[test]
fn doctor_enumerates_independent_errors_even_when_the_issue_item_is_malformed() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let issue = doctor_fixture_issue(&repository);
    for body in ["First comment", "Second comment"] {
        repository
            .add_comment(
                issue.metadata.id.as_str(),
                &issue.source,
                "reviewer",
                body,
                &workdeck_pm::RequestId::new(),
            )
            .unwrap();
    }
    let comments = repository.comments(issue.metadata.id.as_str()).unwrap();
    let attachment = doctor_fixture_attachment(&repository, &issue);
    let template = doctor_fixture_template(&repository, "bad-template", "{id: WD-1}");
    doctor_fixture_template(&repository, "good-template", "{}");
    fs::write(
        repository.root().join(&issue.path),
        "---\nschema: 1\ntitle: [\n---\nBroken item\n",
    )
    .unwrap();
    for comment in &comments {
        let path = repository.root().join(&comment.path);
        let text = fs::read_to_string(&path).unwrap();
        fs::write(&path, text.replace("author: reviewer", "author: ''")).unwrap();
    }
    fs::remove_file(repository.root().join(&attachment.content_path)).unwrap();
    let report = repository.doctor().unwrap();
    assert!(!report.valid);
    assert_eq!(report.checked_records, 6);
    for expected in [
        issue.path.clone(),
        comments[0].path.clone(),
        comments[1].path.clone(),
        attachment.content_path.clone(),
        template.strip_prefix(repository.root()).unwrap().to_owned(),
    ] {
        assert!(
            report.errors.iter().any(|error| error
                .path
                .as_ref()
                .is_some_and(|path| Path::new(path).ends_with(&expected))),
            "missing diagnostic for {}: {:?}",
            expected.display(),
            report.errors
        );
    }
}

#[test]
fn doctor_reports_orphan_records_without_losing_their_own_diagnostics() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let issue = doctor_fixture_issue(&repository);
    repository
        .add_comment(
            issue.metadata.id.as_str(),
            &issue.source,
            "reviewer",
            "An orphan comment",
            &workdeck_pm::RequestId::new(),
        )
        .unwrap();
    let comments = repository.comments(issue.metadata.id.as_str()).unwrap();
    let path = repository.root().join(&comments[0].path);
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, text.replace("schema: 1", "schema: 99")).unwrap();
    fs::remove_file(repository.root().join(&issue.path)).unwrap();
    let report = repository.doctor().unwrap();
    assert!(!report.valid);
    assert!(report.errors.iter().any(|error| error.code==ErrorCode::CorruptStore&&error.message.contains("item.md")),"{:?}",report.errors);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.code == ErrorCode::UnsupportedSchema
                && error
                    .path
                    .as_ref()
                    .is_some_and(|path| Path::new(path).ends_with(&comments[0].path))),
        "{:?}",
        report.errors
    );
}

#[test]
fn doctor_checks_each_attachment_group_and_reports_undeclared_layout() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let issue = doctor_fixture_issue(&repository);
    let first = doctor_fixture_attachment(&repository, &issue);
    let second = doctor_fixture_attachment(&repository, &issue);
    fs::write(repository.root().join(&first.path), "schema: [\n").unwrap();
    fs::remove_file(repository.root().join(&second.content_path)).unwrap();
    fs::write(
        repository
            .root()
            .join(format!("issues/{}/unexpected.md", issue.metadata.id)),
        "Unexpected file\n",
    )
    .unwrap();
    let report = repository.doctor().unwrap();
    assert!(!report.valid);
    for expected in [&first.path, &second.content_path] {
        assert!(
            report.errors.iter().any(|error| error
                .path
                .as_ref()
                .is_some_and(|path| Path::new(path).ends_with(expected))),
            "missing {}: {:?}",
            expected.display(),
            report.errors
        );
    }
    assert!(
        report.errors.iter().any(|error| error
            .path
            .as_ref()
            .is_some_and(|path| path.ends_with("unexpected.md"))),
        "{:?}",
        report.errors
    );
}

#[test]
fn doctor_reports_multiple_bad_templates_and_unsupported_issue_schema() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let issue = doctor_fixture_issue(&repository);
    let path = repository.root().join(&issue.path);
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("schema: 1", "schema: 99"),
    )
    .unwrap();
    let first = doctor_fixture_template(&repository, "first", "{status: done}");
    let second = doctor_fixture_template(&repository, "second", "{priority: impossible}");
    let report = repository.doctor().unwrap();
    assert!(!report.valid);
    assert_eq!(report.checked_records, 3);
    assert!(report.errors.iter().any(|error| {
        error.code == ErrorCode::UnsupportedSchema
            && error
                .path
                .as_ref()
                .is_some_and(|path| Path::new(path).ends_with(&issue.path))
    }));
    for expected in [&first, &second] {
        assert!(
            report.errors.iter().any(|error| error
                .path
                .as_ref()
                .is_some_and(|path| Path::new(path) == expected)),
            "missing {}: {:?}",
            expected.display(),
            report.errors
        );
    }
}
