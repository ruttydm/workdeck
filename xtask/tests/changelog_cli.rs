use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn pages_cli_composes_overlays_and_never_writes_site_files() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let inputs = [
        (
            "history.md",
            "## 1.0.0\n### Fixed\n- Fix.\n## 2.0.0-beta.1\n## 3.0.0\n",
        ),
        (
            "dates.json",
            r#"{"1.0.0":"2026-08-01","2.0.0-beta.1":"2026-08-02"}"#,
        ),
        (
            "notes.json",
            r#"{"1.0":{"summary":"Editorial lead.","video":{"mp4":"/v.mp4"},"links":[{"label":"Docs","href":"/docs/"}]}}"#,
        ),
    ];
    for (name, content) in inputs {
        std::fs::write(repo.path().join(name), content).unwrap();
    }
    let output = run(
        repo.path(),
        &[
            "changelog",
            "pages",
            "history.md",
            "dates.json",
            "notes.json",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let pages: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let pages = pages.as_array().unwrap();
    assert_eq!(
        pages
            .iter()
            .map(|p| p["minor"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["3.0", "2.0", "1.0"]
    );
    for page in &pages[..2] {
        let markdown = page["markdown"].as_str().unwrap();
        assert!(!markdown.contains("cargo install"));
        assert!(!markdown.contains("workdeck update"));
    }
    assert!(
        pages[1]["markdown"]
            .as_str()
            .unwrap()
            .contains("Prerelease · August 2, 2026")
    );
    let stable = pages[2]["markdown"].as_str().unwrap();
    assert!(stable.contains("Editorial lead."));
    assert!(stable.contains("--tag v1.0.0 --package workdeck-cli --locked"));
    assert!(stable.contains("workdeck update"));
    assert!(stable.contains("<source src=\"/v.mp4\""));
    assert!(stable.contains("- [Docs](/docs/)"));
    assert!(stable.contains("[Newer: Workdeck 2.0]"));
    let no_notes = run(
        repo.path(),
        &["changelog", "pages", "history.md", "dates.json"],
    );
    assert!(no_notes.status.success());
    assert!(
        !String::from_utf8(no_notes.stdout)
            .unwrap()
            .contains("Editorial lead.")
    );
    for args in [
        vec!["changelog", "pages"],
        vec!["changelog", "pages", "history.md"],
        vec!["changelog", "pages", "missing.md", "dates.json"],
        vec!["changelog", "pages", "history.md", "missing.json"],
        vec![
            "changelog",
            "pages",
            "history.md",
            "dates.json",
            "missing.json",
        ],
        vec![
            "changelog",
            "pages",
            "history.md",
            "dates.json",
            "notes.json",
            "extra",
        ],
    ] {
        let output = run(repo.path(), &args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    for (name, content) in inputs {
        assert_eq!(
            std::fs::read_to_string(repo.path().join(name)).unwrap(),
            content
        );
    }
    for invalid in ["{broken", r#"{"1.0":{"video":{"mp4":42}}}"#] {
        std::fs::write(repo.path().join("notes.json"), invalid).unwrap();
        let output = run(
            repo.path(),
            &[
                "changelog",
                "pages",
                "history.md",
                "dates.json",
                "notes.json",
            ],
        );
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(
            std::fs::read_to_string(repo.path().join("notes.json")).unwrap(),
            invalid
        );
    }
    let mut entries = std::fs::read_dir(repo.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        [".git", "dates.json", "history.md", "notes.json"].map(std::ffi::OsString::from)
    );
}

#[test]
fn video_cli_escapes_markup_and_preserves_input() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let path = repo.path().join("video.json");
    let input = serde_json::json!({"minor":"1.2", "summary":"Summary.", "video":{"mp4":"/v?x=\"&y=<", "poster":"/poster.png", "title":"</script><script>bad</script>\u{2028}"}}).to_string();
    std::fs::write(&path, &input).unwrap();
    let output = run(repo.path(), &["changelog", "video", "video.json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let html = String::from_utf8(output.stdout).unwrap();
    assert!(html.contains("src=\"/v?x=&quot;&amp;y=&lt;\""));
    assert!(!html.contains("<script>bad"));
    assert_eq!(html.matches("</script>").count(), 1);
    let body = html
        .split("is:inline>")
        .nth(1)
        .unwrap()
        .split("</script>")
        .next()
        .unwrap();
    assert!(!body.contains('\u{2028}'));
    let schema: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(schema["name"], "</script><script>bad</script>\u{2028}");
    assert_eq!(schema["thumbnailUrl"], "https://workdeck.dev/poster.png");
    assert_eq!(schema["contentUrl"], "/v?x=\"&y=<");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), input);
    let minimal = r#"{"minor":"1.2","summary":"Summary.","video":{"mp4":"/v.mp4"}}"#;
    std::fs::write(&path, minimal).unwrap();
    let output = run(repo.path(), &["changelog", "video", "video.json"]);
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("What's new in Workdeck 1.2")
    );
    for args in [
        vec!["changelog", "video"],
        vec!["changelog", "video", "missing.json"],
        vec!["changelog", "video", "video.json", "extra"],
    ] {
        let output = run(repo.path(), &args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), minimal);
    for invalid in [
        "{broken",
        "{}",
        r#"{"minor":"1.2","summary":"x","video":{"mp4":42}}"#,
    ] {
        std::fs::write(&path, invalid).unwrap();
        let output = run(repo.path(), &["changelog", "video", "video.json"]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
    }
    let mut entries = std::fs::read_dir(repo.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        [".git", "video.json"].map(std::ffi::OsString::from)
    );
}

#[test]
fn metadata_cli_quotes_and_truncates_without_writes() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let long = "word ".repeat(40);
    let markdown = format!(
        "## 1.0.0\n### Highlights\nUse **\"jj\"** and [docs](/docs/).\n## 2.0.0\n### Highlights\n{long}\n## 3.0.0\n"
    );
    let input = repo.path().join("history.md");
    let dates = repo.path().join("dates.json");
    std::fs::write(&input, &markdown).unwrap();
    std::fs::write(&dates, "{}").unwrap();
    let output = run(
        repo.path(),
        &["changelog", "metadata", "history.md", "dates.json"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let clipped = format!("{}…", "word ".repeat(31).trim_end());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!([
            {"minor":"3.0", "description":"Release notes for Workdeck 3.0: 1 release.", "yamlDescription":"\"Release notes for Workdeck 3.0: 1 release.\""},
            {"minor":"2.0", "description":clipped, "yamlDescription":format!("\"{clipped}\"")},
            {"minor":"1.0", "description":"Use \"jj\" and docs.", "yamlDescription":"'Use \"jj\" and docs.'"}
        ])
    );
    for args in [
        vec!["changelog", "metadata"],
        vec!["changelog", "metadata", "history.md"],
        vec!["changelog", "metadata", "missing.md", "dates.json"],
        vec!["changelog", "metadata", "history.md", "missing.json"],
        vec!["changelog", "metadata", "history.md", "dates.json", "extra"],
    ] {
        let output = run(repo.path(), &args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    assert_eq!(std::fs::read_to_string(&dates).unwrap(), "{}");
    for invalid in ["{broken", r#"{"1.0.0":42}"#] {
        std::fs::write(&dates, invalid).unwrap();
        let output = run(
            repo.path(),
            &["changelog", "metadata", "history.md", "dates.json"],
        );
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(std::fs::read_to_string(&dates).unwrap(), invalid);
    }
    assert_eq!(std::fs::read_to_string(&input).unwrap(), markdown);
    let mut entries = std::fs::read_dir(repo.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        [".git", "dates.json", "history.md"].map(std::ffi::OsString::from)
    );
}

#[test]
fn resolved_summaries_cli_renders_fallback_and_preserves_inputs() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let markdown = "## 1.2.0\n## 1.2.1-rc.1\n## 2.0.0\n";
    let dates = r#"{"1.2.0":"2026-07-01","1.2.1-rc.1":"2026-08-01"}"#;
    let notes = r#"{"1.2":{"summary":"Editorial **unchanged**"}}"#;
    for (name, content) in [
        ("history.md", markdown),
        ("dates.json", dates),
        ("notes.json", notes),
    ] {
        std::fs::write(repo.path().join(name), content).unwrap();
    }
    for (extra, summary) in [
        (
            false,
            "Release notes for Workdeck 1.2: 2 releases, July 1, 2026 – August 1, 2026.",
        ),
        (true, "Editorial **unchanged**"),
    ] {
        let mut args = vec![
            "changelog",
            "resolved-summaries",
            "history.md",
            "dates.json",
        ];
        if extra {
            args.push("notes.json");
        }
        let output = run(repo.path(), &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            serde_json::json!([
                {"minor":"2.0", "summary":"Release notes for Workdeck 2.0: 1 release."},
                {"minor":"1.2", "summary":summary}
            ])
        );
    }
    for args in [
        vec!["changelog", "resolved-summaries"],
        vec!["changelog", "resolved-summaries", "history.md"],
        vec![
            "changelog",
            "resolved-summaries",
            "missing.md",
            "dates.json",
        ],
        vec![
            "changelog",
            "resolved-summaries",
            "history.md",
            "missing.json",
        ],
        vec![
            "changelog",
            "resolved-summaries",
            "history.md",
            "dates.json",
            "missing.json",
        ],
        vec![
            "changelog",
            "resolved-summaries",
            "history.md",
            "dates.json",
            "notes.json",
            "extra",
        ],
    ] {
        let output = run(repo.path(), &args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    for (name, content) in [
        ("history.md", markdown),
        ("dates.json", dates),
        ("notes.json", notes),
    ] {
        assert_eq!(
            std::fs::read_to_string(repo.path().join(name)).unwrap(),
            content
        );
    }
    for invalid in ["{broken", r#"{"1.2.0":42}"#] {
        std::fs::write(repo.path().join("dates.json"), invalid).unwrap();
        let output = run(
            repo.path(),
            &[
                "changelog",
                "resolved-summaries",
                "history.md",
                "dates.json",
            ],
        );
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(
            std::fs::read_to_string(repo.path().join("dates.json")).unwrap(),
            invalid
        );
    }
    let mut entries = std::fs::read_dir(repo.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        [".git", "dates.json", "history.md", "notes.json"].map(std::ffi::OsString::from)
    );
}

#[test]
fn release_notes_cli_renders_workdeck_links_without_writes() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let markdown =
        "## 1.2.0\n### Fixed\n- [#42](https://example.invalid/pr) Native **fix**.\n## 1.2.1\n";
    let dates = r#"{"1.2.0":"2026-08-16"}"#;
    let input = repo.path().join("history.md");
    let dates_path = repo.path().join("dates.json");
    std::fs::write(&input, markdown).unwrap();
    std::fs::write(&dates_path, dates).unwrap();
    let output = run(
        repo.path(),
        &["changelog", "release-notes", "history.md", "dates.json"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!([{
            "minor": "1.2",
            "markdown": "## Releases in this series\n\n<a class=\"release-separator\" id=\"v1-2-1\"></a>\n\n### 1.2.1\n\nUnreleased\n\nNo user-facing changes.\n\n<a class=\"release-separator\" id=\"v1-2-0\"></a>\n\n### 1.2.0\n\nAugust 16, 2026\n\n#### Fixed\n\n- Native **fix**. ([#42](https://github.com/ruttydm/workdeck/pull/42))\n"
        }])
    );
    for args in [
        vec!["changelog", "release-notes"],
        vec!["changelog", "release-notes", "history.md"],
        vec!["changelog", "release-notes", "missing.md", "dates.json"],
        vec!["changelog", "release-notes", "history.md", "missing.json"],
        vec![
            "changelog",
            "release-notes",
            "history.md",
            "dates.json",
            "extra",
        ],
    ] {
        let output = run(repo.path(), &args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    assert_eq!(std::fs::read_to_string(&dates_path).unwrap(), dates);
    for invalid in ["{broken", r#"{"1.2.0":42}"#] {
        std::fs::write(&dates_path, invalid).unwrap();
        let output = run(
            repo.path(),
            &["changelog", "release-notes", "history.md", "dates.json"],
        );
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(std::fs::read_to_string(&dates_path).unwrap(), invalid);
    }
    assert_eq!(std::fs::read_to_string(&input).unwrap(), markdown);
    let mut entries = std::fs::read_dir(repo.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        [".git", "dates.json", "history.md"].map(std::ffi::OsString::from)
    );
}

#[test]
fn publication_cli_never_promotes_unpublished_or_prerelease_versions() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let markdown = "## 1.0.0\n## 3.0.0\n## 2.0.0-rc.1\n";
    let input = repo.path().join("history.md");
    let dates_path = repo.path().join("dates.json");
    std::fs::write(&input, markdown).unwrap();
    for (dates, expected) in [
        (
            "{}",
            serde_json::json!({"published":[],"stable":[],"latestStable":null}),
        ),
        (
            r#"{"2.0.0-rc.1":"2026-08-01"}"#,
            serde_json::json!({"published":["2.0.0-rc.1"],"stable":[],"latestStable":null}),
        ),
        (
            r#"{"1.0.0":"2026-07-01","2.0.0-rc.1":"2026-08-01","9.9.9":"2026-09-01"}"#,
            serde_json::json!({"published":["2.0.0-rc.1","1.0.0"],"stable":["1.0.0"],"latestStable":"1.0.0"}),
        ),
    ] {
        std::fs::write(&dates_path, dates).unwrap();
        let output = run(
            repo.path(),
            &["changelog", "publication", "history.md", "dates.json"],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            expected
        );
        assert_eq!(std::fs::read_to_string(&dates_path).unwrap(), dates);
    }
    for args in [
        vec!["changelog", "publication"],
        vec!["changelog", "publication", "history.md"],
        vec!["changelog", "publication", "missing.md", "dates.json"],
        vec!["changelog", "publication", "history.md", "missing.json"],
        vec![
            "changelog",
            "publication",
            "history.md",
            "dates.json",
            "extra",
        ],
    ] {
        let output = run(repo.path(), &args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    for invalid in ["{broken", r#"{"1.0.0":42}"#, "[]"] {
        std::fs::write(&dates_path, invalid).unwrap();
        let output = run(
            repo.path(),
            &["changelog", "publication", "history.md", "dates.json"],
        );
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(std::fs::read_to_string(&dates_path).unwrap(), invalid);
    }
    assert_eq!(std::fs::read_to_string(&input).unwrap(), markdown);
    let mut entries = std::fs::read_dir(repo.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        [".git", "dates.json", "history.md"].map(std::ffi::OsString::from)
    );
}

#[test]
fn summaries_cli_preserves_inputs_and_checks_overlays() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let markdown = "## 1.2.0\n### Highlights\nOlder **lead** with [link](url).\n\n- old\n## 2.0.0\n### Fixed\n- no summary\n## 1.2.1\n### Highlights\n- newest bullet only\n";
    let path = repo.path().join("history.md");
    std::fs::write(&path, markdown).unwrap();
    let check = |args: &[&str], expected: serde_json::Value| {
        let output = run(repo.path(), args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            expected
        );
    };
    check(
        &["changelog", "summaries", "history.md"],
        serde_json::json!([
            {"minor":"2.0","summary":null}, {"minor":"1.2","summary":"Older lead with link."}
        ]),
    );
    let notes = r#"{"2.0":{"summary":"Editorial **unchanged**"},"1.2":{"summary":""},"9.9":{"summary":"unused"}}"#;
    let notes_path = repo.path().join("notes.json");
    std::fs::write(&notes_path, notes).unwrap();
    check(
        &["changelog", "summaries", "history.md", "notes.json"],
        serde_json::json!([
            {"minor":"2.0","summary":"Editorial **unchanged**"}, {"minor":"1.2","summary":"Older lead with link."}
        ]),
    );
    assert_eq!(std::fs::read_to_string(&notes_path).unwrap(), notes);
    for args in [
        vec!["changelog", "summaries"],
        vec!["changelog", "summaries", "missing.md"],
        vec!["changelog", "summaries", "history.md", "missing.json"],
        vec![
            "changelog",
            "summaries",
            "history.md",
            "notes.json",
            "extra",
        ],
    ] {
        let output = run(repo.path(), &args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    for invalid in ["{broken", r#"{"1.2":{"summary":42}}"#, "[]"] {
        std::fs::write(&notes_path, invalid).unwrap();
        let output = run(
            repo.path(),
            &["changelog", "summaries", "history.md", "notes.json"],
        );
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(std::fs::read_to_string(&notes_path).unwrap(), invalid);
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), markdown);
    let mut entries = std::fs::read_dir(repo.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        [".git", "history.md", "notes.json"].map(std::ffi::OsString::from)
    );
}

#[test]
fn dates_cli_uses_real_tag_dates_and_preserves_recorded_inputs() {
    let repo = tempfile::tempdir().unwrap();
    let git = |args: &[&str], date: &str| {
        let output = Command::new("git")
            .current_dir(repo.path())
            .args(args)
            .env(
                "GIT_CONFIG_GLOBAL",
                repo.path().join("nonexistent-global-config"),
            )
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--quiet"], "2026-08-28T12:00:00+00:00");
    git(
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
        "2026-08-28T12:00:00+00:00",
    );
    git(
        &[
            "-c",
            "tag.gpgSign=false",
            "tag",
            "-a",
            "v1.2.0",
            "-m",
            "published",
        ],
        "2026-08-29T12:00:00+00:00",
    );
    git(
        &["-c", "tag.gpgSign=false", "tag", "v1.1.0"],
        "2026-08-30T12:00:00+00:00",
    );
    let markdown = "## 1.3.0\n## 1.2.0\n## 1.1.0\n## [1.0.0] - 2020-01-01\n";
    let input = repo.path().join("history.md");
    let recorded = repo.path().join("dates.json");
    std::fs::write(&input, markdown).unwrap();
    let output = run(
        repo.path(),
        &["changelog", "dates", "history.md", "dates.json"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!({
            "1.2.0":"2026-08-29", "1.1.0":"2026-08-28", "1.0.0":"2020-01-01"
        })
    );
    assert!(!recorded.exists());
    let original = r#"{"1.2.0":"1999-01-01","9.9.9":"2000-01-01"}"#;
    std::fs::write(&recorded, original).unwrap();
    let output = run(
        repo.path(),
        &["changelog", "dates", "history.md", "dates.json"],
    );
    assert!(output.status.success());
    let dates: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(dates["1.2.0"], "1999-01-01");
    assert!(dates.get("9.9.9").is_none());
    assert_eq!(std::fs::read_to_string(&recorded).unwrap(), original);
    std::fs::write(&recorded, "{broken").unwrap();
    let output = run(
        repo.path(),
        &["changelog", "dates", "history.md", "dates.json"],
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    assert_eq!(std::fs::read_to_string(recorded).unwrap(), "{broken");
    assert_eq!(std::fs::read_to_string(input).unwrap(), markdown);
}

#[test]
fn website_parser_cli_is_read_only_and_rejects_extra_arguments() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let input = "## 1.2.3\n### Fixed\n- Native parser.\n";
    let path = repo.path().join("history.md");
    std::fs::write(&path, input).unwrap();
    let output = run(repo.path(), &["changelog", "parse", "history.md"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!([
            {"version":"1.2.3","prerelease":false,"sections":[{"title":"Fixed","entries":[{"description":"Native parser."}]}]}
        ])
    );
    for args in [
        vec!["changelog", "parse"],
        vec!["changelog", "parse", "history.md", "extra"],
        vec!["changelog", "parse", "missing.md"],
    ] {
        let output = run(repo.path(), &args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), input);
    let mut entries = std::fs::read_dir(repo.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        [
            std::ffi::OsString::from(".git"),
            std::ffi::OsString::from("history.md")
        ]
    );
}

#[test]
fn native_fragment_cli_round_trip_and_read_only_status() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .arg("init")
            .arg("--quiet")
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let empty = run(repo.path(), &["changelog", "status"]);
    assert!(empty.status.success());
    assert!(empty.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&empty.stdout).unwrap(),
        serde_json::json!({"bump":null,"fragments":[]})
    );
    assert!(!repo.path().join("release/fragments").exists());
    let added = run(
        repo.path(),
        &["changelog", "add", "fix-unicode", "patch", "Fix λ."],
    );
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    assert!(added.stderr.is_empty());
    assert_eq!(added.stdout, b"release/fragments/fix-unicode.md\n");
    let path = repo.path().join("release/fragments/fix-unicode.md");
    let original = std::fs::read(&path).unwrap();
    let status = run(repo.path(), &["changelog", "status"]);
    assert!(status.status.success());
    assert!(status.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&status.stdout).unwrap(),
        serde_json::json!({"bump":"patch","fragments":[{"id":"fix-unicode","bump":"patch","body":"Fix λ."}]})
    );
    for args in [
        vec!["changelog", "add", "fix-unicode", "major", "Overwrite"],
        vec!["changelog", "status", "unexpected"],
        vec!["changelog", "plan", "unexpected"],
    ] {
        let rejected = run(repo.path(), &args);
        assert!(!rejected.status.success());
        assert!(rejected.stdout.is_empty());
        assert!(!rejected.stderr.is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    assert_eq!(
        std::fs::read_dir(repo.path().join("release/fragments"))
            .unwrap()
            .count(),
        1
    );
    let help = run(repo.path(), &["help"]);
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for command in ["changelog add", "changelog status", "changelog plan"] {
        assert!(help.contains(command));
    }
}

#[test]
fn version_plan_uses_real_cargo_metadata_without_mutating_inputs() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    std::fs::create_dir(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(repo.path().join("Cargo.toml"), "[package]\nname = \"workdeck-cli\"\nversion = \"1.2.3\"\nedition = \"2024\"\n\n[workspace]\n").unwrap();
    assert!(
        Command::new("cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success()
    );
    for args in [
        vec!["add", "--", "Cargo.toml", "Cargo.lock", "src/main.rs"],
        vec![
            "-c",
            "user.name=Release Test",
            "-c",
            "user.email=release@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture baseline",
        ],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
    }
    for (id, bump, body) in [
        ("minor-feature", "minor", "Add feature."),
        ("patch-fix", "patch", "Fix λ."),
    ] {
        let output = run(repo.path(), &["changelog", "add", id, bump, body]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(
        Command::new("git")
            .args(["add", "--", "release/fragments"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let gate = run(repo.path(), &["release", "status", "--since=HEAD"]);
    assert!(
        gate.status.success(),
        "{}",
        String::from_utf8_lossy(&gate.stderr)
    );
    let gate: serde_json::Value = serde_json::from_slice(&gate.stdout).unwrap();
    assert_eq!(gate["releaseType"], "minor");
    assert_eq!(
        gate["fragments"],
        serde_json::json!(["minor-feature", "patch-fix"])
    );
    let paths = [
        "Cargo.toml",
        "Cargo.lock",
        "release/fragments/minor-feature.md",
        "release/fragments/patch-fix.md",
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|path| std::fs::read(repo.path().join(path)).unwrap())
        .collect();
    let output = run(repo.path(), &["changelog", "plan"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["current"], "1.2.3");
    assert_eq!(plan["next"], "1.3.0");
    assert_eq!(plan["bump"], "minor");
    assert_eq!(plan["applied"], false);
    for path in ["Cargo.toml", "Cargo.lock"] {
        let edited = plan["edits"][path].as_str().unwrap();
        assert!(edited.contains("version = \"1.3.0\""));
        assert!(!edited.contains("version = \"1.2.3\""));
    }
    assert_eq!(plan["fragments"].as_array().unwrap().len(), 2);
    assert_eq!(
        plan["notes"],
        "## 1.3.0\n\n### Minor Changes\n\n- Add feature.\n\n### Patch Changes\n\n- Fix λ.\n\n"
    );
    for (path, expected) in paths.iter().zip(before) {
        use sha2::{Digest, Sha256};
        assert_eq!(
            plan["inputs"][path],
            format!("{:x}", Sha256::digest(&expected))
        );
        assert_eq!(
            std::fs::read(repo.path().join(path)).unwrap(),
            expected,
            "plan changed {path}"
        );
    }
    let saved = repo.path().join("release-plan.json");
    assert_eq!(plan["inputs"]["release/prerelease.json"], "absent");
    let prerelease = repo.path().join("release/prerelease.json");
    for contents in ["{}", "{invalid", "{\"mode\":\"pre\",\"tag\":\"beta\"}"] {
        std::fs::write(&prerelease, contents).unwrap();
        let rejected = run(repo.path(), &["changelog", "plan"]);
        assert!(!rejected.status.success());
        assert!(rejected.stdout.is_empty());
        assert_eq!(std::fs::read_to_string(&prerelease).unwrap(), contents);
        assert!(repo.path().join("release/fragments/patch-fix.md").exists());
    }
    std::fs::remove_file(&prerelease).unwrap();
    assert_eq!(plan["inputs"]["CHANGELOG.md"], "absent");
    assert_eq!(
        plan["edits"]["CHANGELOG.md"],
        format!("# Changelog\n\n{}", plan["notes"].as_str().unwrap())
    );
    assert!(!repo.path().join("CHANGELOG.md").exists());
    std::fs::write(&saved, &output.stdout).unwrap();
    let check = run(
        repo.path(),
        &["changelog", "check-plan", "release-plan.json"],
    );
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(check.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&check.stdout).unwrap(),
        serde_json::json!({"valid":true,"applied":false})
    );
    let history = repo.path().join("CHANGELOG.md");
    std::fs::write(&history, "# History\n\nExisting release.\n").unwrap();
    let stale = run(
        repo.path(),
        &["changelog", "check-plan", "release-plan.json"],
    );
    assert!(!stale.status.success());
    let refreshed = run(repo.path(), &["changelog", "plan"]);
    assert!(refreshed.status.success());
    let refreshed: serde_json::Value = serde_json::from_slice(&refreshed.stdout).unwrap();
    assert!(
        refreshed["edits"]["CHANGELOG.md"]
            .as_str()
            .unwrap()
            .ends_with("Existing release.\n")
    );
    assert_eq!(
        std::fs::read_to_string(&history).unwrap(),
        "# History\n\nExisting release.\n"
    );
    std::fs::remove_file(&history).unwrap();
    let fragment = repo.path().join("release/fragments/patch-fix.md");
    let original = std::fs::read_to_string(&fragment).unwrap();
    let changed = original.replace("Fix λ.", "Different fix.");
    std::fs::write(&fragment, &changed).unwrap();
    let stale = run(
        repo.path(),
        &["changelog", "check-plan", "release-plan.json"],
    );
    assert!(!stale.status.success());
    assert!(stale.stdout.is_empty());
    assert_eq!(std::fs::read_to_string(&fragment).unwrap(), changed);
    std::fs::write(&fragment, original).unwrap();
    assert!(
        run(
            repo.path(),
            &["changelog", "add", "new-fix", "patch", "New note."]
        )
        .status
        .success()
    );
    let stale = run(
        repo.path(),
        &["changelog", "check-plan", "release-plan.json"],
    );
    assert!(!stale.status.success());
    assert!(stale.stdout.is_empty());
    assert_eq!(std::fs::read(saved).unwrap(), output.stdout);
    // Apply only to this disposable fixture to validate the proposed pair.
    // Production plan/check-plan commands remain read-only.
    for path in ["Cargo.toml", "Cargo.lock"] {
        std::fs::write(
            repo.path().join(path),
            plan["edits"][path].as_str().unwrap(),
        )
        .unwrap();
    }
    assert!(
        Command::new("cargo")
            .args(["check", "--locked", "--offline", "--quiet"])
            .current_dir(repo.path())
            .env("CARGO_TARGET_DIR", repo.path().join("target"))
            .status()
            .unwrap()
            .success()
    );
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(repo.path().join("Cargo.toml"))
        .no_deps()
        .other_options(vec!["--offline".into(), "--locked".into()])
        .exec()
        .unwrap();
    let package = metadata
        .packages
        .iter()
        .find(|package| package.name == "workdeck-cli")
        .unwrap();
    assert_eq!(package.version.to_string(), "1.3.0");
    for path in ["Cargo.toml", "Cargo.lock"] {
        assert_eq!(
            std::fs::read_to_string(repo.path().join(path)).unwrap(),
            plan["edits"][path].as_str().unwrap()
        );
    }
    let next = run(repo.path(), &["changelog", "plan"]);
    assert!(next.status.success());
    std::fs::write(repo.path().join("next-plan.json"), &next.stdout).unwrap();
    let backup_parent = tempfile::tempdir().unwrap();
    let backup = backup_parent.path().join("release-backup");
    let applied = run(
        repo.path(),
        &[
            "changelog",
            "apply-plan",
            "next-plan.json",
            backup.to_str().unwrap(),
        ],
    );
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&applied.stdout).unwrap()["applied"],
        true
    );
    assert!(backup.join("recovery.json").is_file());
    assert!(
        backup
            .join("originals/release/fragments/patch-fix.md")
            .is_file()
    );
    assert_eq!(
        std::fs::read_dir(repo.path().join("release/fragments"))
            .unwrap()
            .count(),
        0
    );
}
