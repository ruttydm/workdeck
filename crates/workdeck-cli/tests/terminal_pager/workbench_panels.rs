//! Repository panels exercise the actual normal-startup composition and terminal.
use super::*;
use workdeck_pm::{CreateIssue, Repository, RequestId};

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn repository(initialized: bool) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    git(root, &["init", "--quiet"]);
    git(root, &["config", "user.name", "Repository panel test"]);
    git(root, &["config", "user.email", "panels@example.test"]);
    fs::create_dir(root.join("src")).unwrap();
    fs::write(
        root.join("root.rs"),
        "pub fn root_function() {\n// root baseline\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/one.rs"),
        "pub fn panel_symbol() {\n// symbol source target\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/two.rs"),
        "pub fn second_file() {\n// second source target\n}\n",
    )
    .unwrap();
    if initialized {
        Repository::init(root, "WD").unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "-m", "Panel initial commit"]);
    git(root, &["branch", "panel-topic"]);
    directory
}
fn launch(root: &std::path::Path, width: u16) -> Session {
    Session::launch_in("", &["--no-extensions"], false, width, 36, None, Some(root))
}
fn page(session: &mut Session, number: u8) {
    session.write(
        format!(
            "\x1b[{}~",
            match number {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                _ => panic!(),
            }
        )
        .as_bytes(),
    );
}
fn query(session: &mut Session, text: &str) {
    session.write(b"/\x15");
    session.write(text.as_bytes());
    session.write(b"\r");
    session.wait(|screen| {
        screen.contains(&format!("/ {text}"))
            && !screen.to_lowercase().contains("loading")
            && !screen.contains("Enter search")
    });
}

#[test]
fn repository_files_keep_native_review_and_planning_drafts() {
    let directory = repository(true);
    let root = directory.path();
    fs::write(
        root.join("root.rs"),
        "pub fn root_function() {\n// dirty panel source\n}\n",
    )
    .unwrap();
    let mut session = launch(root, 110);
    session.wait(|text| text.contains("F5 Changes") && text.contains("F9 Search"));
    session.write(b"\x1bORn");
    session.wait(|text| text.contains("Create issue"));
    session.write(b"Retained panel draft");
    session.click_label("F7 Files");
    session.wait(|text| text.contains("entries") && text.contains("src"));
    query(&mut session, "src");
    session.wait(|text| text.contains("/ src") && text.contains("directory"));
    session.write(b"\r");
    session.wait(|text| text.contains("Parent directory") && text.contains("two.rs"));
    query(&mut session, "two");
    session.wait(|text| text.contains("/ two") && text.contains("two.rs"));
    session.write(b"j");
    session.wait(|text| text.contains("second source target"));
    session.resize(74, 36);
    session.write(b"\r");
    session.wait(|text| text.contains("second source target"));
    session.write(b"v");
    session.wait(|text| {
        text.contains("second source target") && !text.contains("Enter preview/directory")
    });
    page(&mut session, 7);
    session.wait(|text| text.contains("/ two") && text.contains("second source target"));
    session.resize(120, 36);
    session.wait(|text| text.contains("two.rs") && text.contains("second source target"));
    session.write(b"\x1bOR");
    session.wait(|text| text.contains("Retained panel draft") && text.contains("Ctrl-S save"));
    session.write(b"\x1b");
    session.wait(|text| !text.contains("Ctrl-S save"));
    session.quit();
    assert!(!root.join(".agents").exists());
}

#[test]
fn repository_changes_and_git_keep_native_review_return_state() {
    let directory = repository(true);
    let root = directory.path();
    fs::write(
        root.join("root.rs"),
        "pub fn root_function() {\n// dirty panel source\n}\n",
    )
    .unwrap();
    let mut session = launch(root, 120);
    session.wait(|text| text.contains("F6 Git"));
    page(&mut session, 5);
    query(&mut session, "root.rs");
    session.wait(|text| text.contains("root.rs") && text.contains("dirty panel source"));
    session.write(b"v");
    session.wait(|text| {
        text.contains("dirty panel source") && !text.contains("Enter preview/directory")
    });
    page(&mut session, 6);
    query(&mut session, "Panel initial commit");
    session.wait(|text| text.contains("Panel initial commit") && !text.contains("loading"));
    session.write(b"v");
    session.wait(|text| text.contains("show ") && text.contains(".workdeck/config.yml"));
    page(&mut session, 6);
    session.wait(|text| text.contains("/ Panel initial commit"));
    session.quit();
    assert!(!root.join(".agents").exists());
}

#[test]
fn repository_search_opens_symbols_and_issues() {
    let directory = repository(true);
    let root = directory.path();
    let repository = Repository::discover(root).unwrap();
    repository
        .create_issue(
            &CreateIssue::new("Search planning target", "Search issue body detail"),
            &RequestId::new(),
        )
        .unwrap();
    let mut session = launch(root, 120);
    session.wait(|text| text.contains("F9 Search"));
    page(&mut session, 9);
    query(&mut session, "panel_symbol");
    session.wait(|text| text.contains("panel_symbol") && !text.contains("loading"));
    session.write(b"v");
    session.wait(|text| {
        text.contains("symbol source target") && !text.contains("Enter preview/directory")
    });
    page(&mut session, 9);
    query(&mut session, "Search planning target");
    session.wait(|text| text.contains("Search planning target") && !text.contains("loading"));
    session.write(b"v");
    session.wait(|text| {
        text.contains("Search issue body detail") && !text.contains("Enter preview/directory")
    });
    session.quit();
    assert!(!root.join(".agents").exists());
}

#[test]
fn repository_agents_remain_historical() {
    let directory = repository(true);
    let root = directory.path();
    fs::create_dir_all(root.join(".workdeck/imported-sessions")).unwrap();
    let path = root.join(".workdeck/imported-sessions/session-1.toml");
    let record = "id = \"session-1\"\ntitle = \"Historical panel session\"\nagent = \"recorded-agent\"\nstatus = \"completed\"\ngoal = \"Historical goal preview\"\nsummary = \"Historical summary preview\"\n";
    fs::write(&path, record).unwrap();
    let mut session = launch(root, 120);
    session.wait(|text| text.contains("F8 Agents"));
    page(&mut session, 8);
    session.wait(|text| {
        text.contains("Recorded agents")
            && text.contains("Historical panel session")
            && text.contains("Historical summary preview")
            && !text.to_lowercase().contains("loading")
    });
    session.write(b"v");
    session.wait(|text| text.contains("read-only preview"));
    session.resize(76, 36);
    session.write(b"\r");
    session.wait(|text| text.contains("Historical summary preview"));
    session.quit();
    assert_eq!(fs::read_to_string(path).unwrap(), record);
    assert!(!root.join(".agents").exists());
}

#[test]
fn repository_panels_work_without_initializing_planning_and_bad_preview_is_actionable() {
    let directory = repository(false);
    let root = directory.path();
    let outside = tempfile::tempdir().unwrap();
    fs::write(
        outside.path().join("external.txt"),
        "Outside preview sentinel",
    )
    .unwrap();
    let mut session = launch(root, 120);
    session.wait(|text| text.contains("source unavailable") && text.contains("F7 Files"));
    std::os::unix::fs::symlink(
        outside.path().join("external.txt"),
        root.join("outside-link"),
    )
    .unwrap();
    page(&mut session, 7);
    query(&mut session, "outside-link");
    session.wait(|text| text.contains("symbolic link") && !text.contains("loading"));
    assert!(
        !session
            .parser
            .terminal()
            .plain_string()
            .contains("Outside preview sentinel")
    );
    session.write(b"\x1bOQ");
    session.wait(|text| text.contains("F2 Review") && !text.contains("Enter preview/directory"));
    page(&mut session, 6);
    session.wait(|text| text.contains("Panel initial commit"));
    page(&mut session, 9);
    query(&mut session, "panel_symbol");
    session.wait(|text| text.contains("panel_symbol") && !text.contains("loading"));
    session.write(b"\x1bOR");
    session.wait(|text| text.contains("source unavailable"));
    session.quit();
    assert!(!root.join(".workdeck").exists());
    assert!(!root.join(".agents").exists());
}
