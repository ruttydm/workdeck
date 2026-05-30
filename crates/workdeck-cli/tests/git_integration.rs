use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;
use workdeck_cli::git::{self, ChangeKind};

#[test]
fn scanner_reports_core_statuses_and_churn() {
    let dir = tempdir().unwrap();
    init_repo(dir.path());

    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(dir.path().join("deleted.txt"), "remove me\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "initial"]);

    fs::write(
        dir.path().join("src/main.rs"),
        "fn main() {\n    println!(\"hi\");\n}\n",
    )
    .unwrap();
    fs::write(dir.path().join("new.txt"), "new\nfile\n").unwrap();
    fs::remove_file(dir.path().join("deleted.txt")).unwrap();

    let snapshot = git::scan_repo(dir.path()).unwrap();

    let modified = snapshot
        .changes
        .iter()
        .find(|change| change.path == Path::new("src/main.rs"))
        .unwrap();
    assert_eq!(modified.kind, ChangeKind::Modified);
    assert!(modified.additions > 0);
    assert!(modified.deletions > 0);

    let deleted = snapshot
        .changes
        .iter()
        .find(|change| change.path == Path::new("deleted.txt"))
        .unwrap();
    assert_eq!(deleted.kind, ChangeKind::Deleted);

    let untracked = snapshot
        .changes
        .iter()
        .find(|change| change.path == Path::new("new.txt"))
        .unwrap();
    assert_eq!(untracked.kind, ChangeKind::Untracked);
    assert_eq!(untracked.additions, 2);
}

#[test]
fn scanner_respects_gitignore() {
    let dir = tempdir().unwrap();
    init_repo(dir.path());
    fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
    fs::create_dir_all(dir.path().join("target")).unwrap();
    fs::write(dir.path().join("target/generated.txt"), "ignored\n").unwrap();
    fs::write(dir.path().join("visible.txt"), "visible\n").unwrap();

    let snapshot = git::scan_repo(dir.path()).unwrap();
    let paths = snapshot
        .changes
        .iter()
        .map(|change| change.path.to_string_lossy().to_string())
        .collect::<Vec<_>>();

    assert!(paths.contains(&".gitignore".to_string()));
    assert!(paths.contains(&"visible.txt".to_string()));
    assert!(!paths.iter().any(|path| path.starts_with("target/")));
}

#[test]
fn diff_preview_contains_staged_and_unstaged_sections() {
    let dir = tempdir().unwrap();
    init_repo(dir.path());
    fs::write(dir.path().join("file.txt"), "one\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "initial"]);

    fs::write(dir.path().join("file.txt"), "one\ntwo\n").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    fs::write(dir.path().join("file.txt"), "one\ntwo\nthree\n").unwrap();

    let preview = git::diff_for_path(dir.path(), Path::new("file.txt")).unwrap();

    assert!(preview.content.contains("# staged"));
    assert!(preview.content.contains("# unstaged"));
    assert!(preview.content.contains("+two"));
    assert!(preview.content.contains("+three"));
}

#[test]
fn diff_preview_bounds_large_staged_and_unstaged_outputs() {
    let dir = tempdir().unwrap();
    init_repo(dir.path());
    fs::write(dir.path().join("large.txt"), "base\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "initial"]);

    let staged = (0..50_000)
        .map(|index| format!("staged-{index}\n"))
        .collect::<String>();
    fs::write(dir.path().join("large.txt"), staged).unwrap();
    git(dir.path(), &["add", "large.txt"]);

    let unstaged = (0..50_000)
        .map(|index| format!("unstaged-{index}\n"))
        .collect::<String>();
    fs::write(dir.path().join("large.txt"), unstaged).unwrap();

    let preview = git::diff_for_path(dir.path(), Path::new("large.txt")).unwrap();

    assert!(preview.truncated);
    assert!(preview.content.contains("# staged"));
    assert!(preview.content.contains("# unstaged"));
    assert!(preview.content.contains("staged diff truncated"));
    assert!(preview.content.contains("unstaged diff truncated"));
    assert!(preview.content.len() < 1_300_000);
}

#[test]
fn file_preview_uses_metadata_for_images_archives_and_binary_control_bytes() {
    let dir = tempdir().unwrap();
    init_repo(dir.path());
    fs::write(dir.path().join("image.png"), b"not really a png").unwrap();
    fs::write(dir.path().join("bundle.zip"), b"PK").unwrap();
    fs::write(
        dir.path().join("control.bin"),
        [1, 2, 3, 4, 5, 6, b'a', b'b'],
    )
    .unwrap();

    let image = git::read_file_preview(dir.path(), Path::new("image.png"), 80_000).unwrap();
    let archive = git::read_file_preview(dir.path(), Path::new("bundle.zip"), 80_000).unwrap();
    let binary = git::read_file_preview(dir.path(), Path::new("control.bin"), 80_000).unwrap();

    assert!(image.binary);
    assert!(image.content.contains("image file"));
    assert!(archive.binary);
    assert!(archive.content.contains("archive file"));
    assert!(binary.binary);
    assert!(binary.content.contains("binary file"));
}

#[test]
fn file_preview_keeps_text_preview_for_plain_files() {
    let dir = tempdir().unwrap();
    init_repo(dir.path());
    fs::write(dir.path().join("README.md"), "hello\nworld\n").unwrap();

    let preview = git::read_file_preview(dir.path(), Path::new("README.md"), 80_000).unwrap();

    assert!(!preview.binary);
    assert_eq!(preview.content, "hello\nworld\n");
}

#[test]
fn file_preview_truncates_large_unknown_files_with_bounded_output() {
    let dir = tempdir().unwrap();
    init_repo(dir.path());
    let mut content = "a".repeat(120_000);
    content.push_str("tail-marker");
    fs::write(dir.path().join("large.txt"), content).unwrap();

    let preview = git::read_file_preview(dir.path(), Path::new("large.txt"), 1_024).unwrap();

    assert!(!preview.binary);
    assert!(preview.truncated);
    assert!(preview.content.contains("... truncated ..."));
    assert!(!preview.content.contains("tail-marker"));
    assert!(preview.content.len() < 2_000);
}

#[test]
fn file_preview_detects_binary_from_bounded_sniff_window() {
    let dir = tempdir().unwrap();
    init_repo(dir.path());
    let mut bytes = vec![b'a'; 512];
    bytes.extend((0..64).map(|index| (index % 8 + 1) as u8));
    fs::write(dir.path().join("unknown.dat"), bytes).unwrap();

    let preview = git::read_file_preview(dir.path(), Path::new("unknown.dat"), 64).unwrap();

    assert!(preview.binary);
    assert!(preview.content.contains("binary file"));
}

#[test]
fn git_overview_reports_repo_without_upstream() {
    let dir = tempdir().unwrap();
    init_repo(dir.path());
    fs::write(dir.path().join("README.md"), "hello\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "initial"]);

    let overview = git::scan_git_overview(dir.path(), None, 30).unwrap();

    assert!(!overview.current_branch.is_empty());
    assert_eq!(overview.upstream, None);
    assert_eq!(overview.ahead, 0);
    assert_eq!(overview.behind, 0);
}

#[test]
fn git_overview_discovers_branch_tag_and_stash() {
    let dir = tempdir().unwrap();
    init_repo(dir.path());
    fs::write(dir.path().join("README.md"), "hello\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "initial"]);
    git(dir.path(), &["checkout", "-b", "feature/git-tab"]);
    git(dir.path(), &["tag", "v0.1.0"]);
    fs::write(dir.path().join("README.md"), "hello\nstash\n").unwrap();
    git(dir.path(), &["stash", "push", "-m", "save work"]);

    let overview = git::scan_git_overview(dir.path(), None, 30).unwrap();

    assert_eq!(overview.current_branch, "feature/git-tab");
    assert!(
        overview
            .branches
            .iter()
            .any(|branch| branch.name == "feature/git-tab" && branch.is_current)
    );
    assert!(overview.tags.iter().any(|tag| tag.name == "v0.1.0"));
    assert!(
        overview
            .stashes
            .iter()
            .any(|stash| stash.name == "stash@{0}")
    );
}

#[test]
fn git_commit_preview_contains_stat_and_patch() {
    let dir = tempdir().unwrap();
    init_repo(dir.path());
    fs::write(dir.path().join("README.md"), "hello\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "initial"]);
    let sha = git_stdout(dir.path(), &["rev-parse", "HEAD"]);

    let preview = git::git_commit_preview(dir.path(), sha.trim()).unwrap();

    assert!(preview.content.contains("README.md"));
    assert!(preview.content.contains("diff --git"));
    assert!(preview.content.contains("+hello"));
}

#[test]
fn git_summary_preview_handles_missing_base() {
    let dir = tempdir().unwrap();
    init_repo(dir.path());

    let preview = git::git_summary_preview(dir.path(), None).unwrap();

    assert!(
        preview
            .content
            .contains("No base branch configured or detected")
    );
}

fn init_repo(path: &Path) {
    git(path, &["init"]);
    git(path, &["config", "user.email", "workdeck@example.test"]);
    git(path, &["config", "user.name", "Workdeck Test"]);
}

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}
