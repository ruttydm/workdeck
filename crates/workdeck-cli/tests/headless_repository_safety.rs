use assert_cmd::prelude::*;
use git2::{Repository, Signature};
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn fixture() -> (TempDir, Repository) {
    let root = TempDir::new().unwrap();
    let repo = Repository::init(root.path()).unwrap();
    (root, repo)
}
fn commit(repo: &Repository) {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = Signature::now("Synthetic", "test@example.invalid").unwrap();
    repo.commit(Some("HEAD"), &signature, &signature, "Fixture", &tree, &[])
        .unwrap();
}
fn run(root: &Path, args: &[&str]) -> Output {
    let mut command = Command::cargo_bin("workdeck").unwrap();
    command
        .current_dir(root)
        .args(args)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    // Allow concurrent Cargo jobs and macOS's first launch after relinking;
    // an actual FIFO open still cannot outlive this subprocess deadline.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut blocked = false;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            blocked = true;
            child.kill().unwrap();
            break child.wait().unwrap();
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = Output {
        status,
        stdout: out.join().unwrap(),
        stderr: err.join().unwrap(),
    };
    assert!(!blocked, "command blocked: {args:?}");
    output
}
fn success(root: &Path, args: &[&str]) -> Value {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[cfg(unix)]
#[test]
fn files_show_rejects_leaf_and_parent_symlinks_and_fifo_without_init() {
    let (root, _) = fixture();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("secret"), "outside-content").unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret"), root.path().join("link")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("parent")).unwrap();
    let pipe = std::ffi::CString::new(root.path().join("pipe").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
    for path in ["link", "parent/secret", "pipe", "../outside"] {
        let output = run(root.path(), &["files", "show", path, "--json"]);
        assert!(!output.status.success(), "accepted {path}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("outside-content"));
    }
    assert!(!root.path().join(".workdeck").exists());
    assert!(!root.path().join(".agents").exists());
}

#[cfg(unix)]
#[test]
fn files_list_rejects_active_fifo_ignore_sources_promptly() {
    let (root, _) = fixture();
    let path = std::ffi::CString::new(root.path().join(".gitignore").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let output = run(root.path(), &["files", "list", "--json"]);
    assert!(!output.status.success());
}

#[test]
fn files_preserve_shapes_directory_browsing_and_truthful_size_bounds() {
    let (root, _) = fixture();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/file.txt"), "hello\n").unwrap();
    fs::write(root.path().join("large.txt"), vec![b'a'; 200_000]).unwrap();
    fs::write(root.path().join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(root.path().join("ignored.txt"), "hidden").unwrap();
    let list = success(root.path(), &["files", "list", "--json"]);
    assert_eq!(list["kind"], "file_list");
    assert!(list["data"].is_array());
    assert!(
        list["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["kind"] == "directory" && row["path"] == "src")
    );
    assert!(!list.to_string().contains("ignored.txt"));
    let nested = success(root.path(), &["files", "list", "src", "--json"]);
    assert_eq!(nested["data"][0]["path"], "src/file.txt");
    let text = success(root.path(), &["files", "show", "src/file.txt", "--json"]);
    assert_eq!(text["data"]["content"], "hello\n");
    assert_eq!(text["data"]["title"], "src/file.txt");
    let directory = success(root.path(), &["files", "show", "src", "--json"]);
    assert_eq!(directory["data"]["content"], "directory");
    let large = success(root.path(), &["files", "show", "large.txt", "--json"]);
    assert_eq!(large["data"]["truncated"], true);
    assert!(large["data"]["content"].as_str().unwrap().len() <= 80_100);
    assert!(!root.path().join(".workdeck").exists());
    assert!(!root.path().join(".agents").exists());
}

#[test]
fn changes_preserve_merged_stage_counts_and_literal_diff_paths() {
    let (root, repo) = fixture();
    fs::write(root.path().join("[literal].txt"), "one\n").unwrap();
    fs::write(root.path().join("l.txt"), "unrelated before\n").unwrap();
    commit(&repo);
    fs::write(root.path().join("[literal].txt"), "one\nstaged\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("[literal].txt")).unwrap();
    index.write().unwrap();
    fs::write(root.path().join("[literal].txt"), "one\nstaged\nunstaged\n").unwrap();
    fs::write(root.path().join("l.txt"), "unrelated after\n").unwrap();
    let before = fs::read(repo.path().join("index")).unwrap();
    let list = success(root.path(), &["changes", "list", "--json"]);
    let changes = list["data"]["changes"].as_array().unwrap();
    let row = changes
        .iter()
        .find(|row| row["path"] == "[literal].txt")
        .unwrap();
    assert_eq!(row["stage"], "staged+unstaged");
    assert_eq!(row["additions"], 2);
    assert_eq!(row["deletions"], 0);
    assert_eq!(
        changes
            .iter()
            .filter(|row| row["path"] == "[literal].txt")
            .count(),
        1
    );
    let preview = success(root.path(), &["changes", "diff", "[literal].txt", "--json"]);
    let content = preview["data"]["content"].as_str().unwrap();
    assert!(content.contains("# staged\n"));
    assert!(content.contains("# unstaged\n"));
    assert!(content.contains("+staged"));
    assert!(content.contains("+unstaged"));
    assert!(!content.contains("unrelated after"));
    assert_eq!(fs::read(repo.path().join("index")).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn changes_never_execute_external_diff_or_textconv_helpers() {
    let (root, repo) = fixture();
    fs::write(root.path().join("tracked.txt"), "before\n").unwrap();
    commit(&repo);
    fs::write(root.path().join("tracked.txt"), "after\n").unwrap();
    fs::write(root.path().join(".gitattributes"), "*.txt diff=unsafe\n").unwrap();
    let marker = root.path().join("helper-executed");
    let script = root.path().join("unsafe-helper.sh");
    fs::write(
        &script,
        format!("#!/bin/sh\ntouch '{}'\ncat \"$1\"\n", marker.display()),
    )
    .unwrap();
    let mut config = repo.config().unwrap();
    config
        .set_str(
            "diff.unsafe.textconv",
            &format!("sh '{}'", script.display()),
        )
        .unwrap();
    config
        .set_str("diff.external", &format!("sh '{}'", script.display()))
        .unwrap();
    success(root.path(), &["changes", "list", "--json"]);
    success(root.path(), &["changes", "diff", "tracked.txt", "--json"]);
    assert!(
        !marker.exists(),
        "headless inspection executed a configured helper"
    );
}

#[test]
fn directory_diff_scopes_remain_literal_and_include_tracked_children() {
    let (root, repo) = fixture();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/a.txt"), "before a\n").unwrap();
    fs::write(root.path().join("top.txt"), "before top\n").unwrap();
    commit(&repo);
    fs::write(root.path().join("src/a.txt"), "after a\n").unwrap();
    fs::write(root.path().join("top.txt"), "after top\n").unwrap();
    let subtree = success(root.path(), &["changes", "diff", "src", "--json"]);
    let subtree = subtree["data"]["content"].as_str().unwrap();
    assert!(subtree.contains("+after a"));
    assert!(!subtree.contains("+after top"));
    let all = success(root.path(), &["changes", "diff", ".", "--json"]);
    let all = all["data"]["content"].as_str().unwrap();
    assert!(all.contains("+after a"));
    assert!(all.contains("+after top"));
    let text = success(root.path(), &["files", "show", "./top.txt", "--json"]);
    assert!(
        text["data"]["content"]
            .as_str()
            .unwrap()
            .contains("after top")
    );
}

#[cfg(unix)]
#[test]
fn changes_reject_special_file_content_without_hanging_or_following_links() {
    let (root, repo) = fixture();
    fs::write(root.path().join("tracked.txt"), "before\n").unwrap();
    commit(&repo);
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("secret"), "EXTERNAL_SECRET_CONTENT").unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret"), root.path().join("link")).unwrap();
    let pipe = std::ffi::CString::new(root.path().join("pipe").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
    let output = run(root.path(), &["changes", "list", "--json"]);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("EXTERNAL_SECRET_CONTENT"));
    for path in ["pipe", "link"] {
        let output = run(root.path(), &["changes", "diff", path, "--json"]);
        assert!(!output.status.success(), "accepted special file {path}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("EXTERNAL_SECRET_CONTENT"));
    }
    fs::write(root.path().join("tracked.txt"), "after\n").unwrap();
    for relative in [".gitattributes", ".git/info/attributes"] {
        let path = root.path().join(relative);
        let pipe = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
        run(root.path(), &["changes", "list", "--json"]);
        run(root.path(), &["changes", "diff", "tracked.txt", "--json"]);
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn changes_rename_origin_and_bounded_large_diffs_are_truthful() {
    let (root, repo) = fixture();
    fs::write(root.path().join("old.txt"), "before\n").unwrap();
    fs::write(root.path().join("large.txt"), "before\n").unwrap();
    commit(&repo);
    fs::rename(root.path().join("old.txt"), root.path().join("renamed.txt")).unwrap();
    let mut index = repo.index().unwrap();
    index.remove_path(Path::new("old.txt")).unwrap();
    index.add_path(Path::new("renamed.txt")).unwrap();
    index.write().unwrap();
    let list = success(root.path(), &["changes", "list", "--json"]);
    let row = list["data"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["path"] == "renamed.txt")
        .unwrap();
    assert_eq!(row["old_path"], "old.txt");
    for path in ["old.txt", "renamed.txt"] {
        let diff = success(root.path(), &["changes", "diff", path, "--json"]);
        let content = diff["data"]["content"].as_str().unwrap();
        assert!(content.contains("rename from old.txt"));
        assert!(content.contains("rename to renamed.txt"));
    }
    fs::write(
        root.path().join("large.txt"),
        "long changed line\n".repeat(70_000),
    )
    .unwrap();
    let diff = success(root.path(), &["changes", "diff", "large.txt", "--json"]);
    assert_eq!(diff["data"]["truncated"], true);
    assert!(diff["data"]["content"].as_str().unwrap().len() < 530_000);
    fs::write(root.path().join("large.txt"), vec![b'x'; 3 * 1024 * 1024]).unwrap();
    let oversized = success(root.path(), &["changes", "diff", "large.txt", "--json"]);
    assert_eq!(oversized["data"]["truncated"], true);
    assert!(oversized["data"]["content"].as_str().unwrap().len() < 530_000);
    let list = success(root.path(), &["changes", "list", "--json"]);
    assert_eq!(list["data"]["truncated"], true);
    fs::write(root.path().join("binary.dat"), b"\0binary\xff").unwrap();
    let binary = success(root.path(), &["files", "show", "binary.dat", "--json"]);
    assert_eq!(binary["data"]["binary"], true);
    assert!(
        binary["data"]["content"]
            .as_str()
            .unwrap()
            .contains("size: 8 bytes")
    );
}

#[cfg(unix)]
#[test]
fn changes_reject_unsafe_active_attribute_sources_with_bounded_subprocesses() {
    let (root, repo) = fixture();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/tracked.txt"), "before\n").unwrap();
    commit(&repo);
    fs::write(root.path().join("src/tracked.txt"), "after\n").unwrap();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("attrs"), "*.txt -diff\n").unwrap();
    for relative in [
        ".gitattributes",
        "src/.gitattributes",
        ".git/info/attributes",
    ] {
        let path = root.path().join(relative);
        let pipe = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
        assert!(
            !run(root.path(), &["changes", "list", "--json"])
                .status
                .success()
        );
        assert!(
            !run(
                root.path(),
                &["changes", "diff", "src/tracked.txt", "--json"]
            )
            .status
            .success()
        );
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(outside.path().join("attrs"), &path).unwrap();
        assert!(
            !run(root.path(), &["changes", "list", "--json"])
                .status
                .success()
        );
        fs::remove_file(&path).unwrap();
        fs::write(&path, vec![b'#'; 2 * 1024 * 1024 + 1]).unwrap();
        assert!(
            !run(
                root.path(),
                &["changes", "diff", "src/tracked.txt", "--json"]
            )
            .status
            .success()
        );
        fs::remove_file(&path).unwrap();
    }
    let global = outside.path().join("global-attributes");
    repo.config()
        .unwrap()
        .set_str("core.attributesFile", global.to_str().unwrap())
        .unwrap();
    let pipe = std::ffi::CString::new(global.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
    assert!(
        !run(root.path(), &["changes", "list", "--json"])
            .status
            .success()
    );
    fs::remove_file(&global).unwrap();
    fs::write(&global, "*.txt -diff\n").unwrap();
    let diff = success(
        root.path(),
        &["changes", "diff", "src/tracked.txt", "--json"],
    );
    assert_eq!(diff["data"]["binary"], true);
    assert!(!diff["data"]["content"].as_str().unwrap().contains("+after"));
}

#[cfg(unix)]
#[test]
fn changes_preflight_git_ignores_without_using_dot_ignore_or_scanning_ignored_trees() {
    let (root, repo) = fixture();
    let fifo = |path: &Path| {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    };
    fifo(&root.path().join(".gitignore"));
    assert!(
        !run(root.path(), &["changes", "list", "--json"])
            .status
            .success()
    );
    fs::remove_file(root.path().join(".gitignore")).unwrap();
    fs::write(root.path().join(".ignore"), "new/\n").unwrap();
    fifo(&root.path().join("new/.gitignore"));
    assert!(
        !run(root.path(), &["changes", "list", "--json"])
            .status
            .success()
    );
    fs::remove_file(root.path().join("new/.gitignore")).unwrap();
    fs::write(root.path().join(".gitignore"), "ignored/\n").unwrap();
    fifo(&root.path().join("ignored/.gitignore"));
    let global = root.path().join("git-excludes");
    fs::write(&global, "ignored-global/\n").unwrap();
    repo.config()
        .unwrap()
        .set_str("core.excludesFile", global.to_str().unwrap())
        .unwrap();
    fifo(&root.path().join("ignored-global/.gitignore"));
    success(root.path(), &["changes", "list", "--json"]);
    fs::remove_file(&global).unwrap();
    fifo(&global);
    assert!(
        !run(root.path(), &["changes", "list", "--json"])
            .status
            .success()
    );
}

#[test]
fn incomplete_change_coverage_is_not_reported_as_a_clean_worktree() {
    let (root, _) = fixture();
    for index in 0..2050 {
        fs::create_dir(root.path().join(format!("directory-{index}"))).unwrap();
    }
    let list = success(root.path(), &["changes", "list", "--json"]);
    assert_eq!(list["data"]["truncated"], true);
    let plain = run(root.path(), &["changes", "list"]);
    assert!(plain.status.success());
    assert!(!String::from_utf8_lossy(&plain.stdout).contains("clean worktree"));
    assert!(
        !run(root.path(), &["changes", "diff", ".", "--json"])
            .status
            .success()
    );
    assert!(!root.path().join(".workdeck").exists());
}
