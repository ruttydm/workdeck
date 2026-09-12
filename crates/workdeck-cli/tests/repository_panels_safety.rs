use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use workdeck_cli::repository_panels::RepositoryPanels;
use workdeck_tui::workbench::*;

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    git2::Repository::init(root.path()).unwrap();
    root
}
fn request(directory: &str) -> PanelRequest {
    PanelRequest {
        page: PanelPage::Files,
        directory: directory.into(),
        query: String::new(),
        limit: 100,
    }
}
fn provider(root: &Path) -> RepositoryPanels {
    RepositoryPanels::new(root, None, 10).unwrap()
}

#[test]
fn unsafe_ignore_worker() {
    let Some(root) = std::env::var_os("WORKDECK_UNSAFE_IGNORE_ROOT") else {
        return;
    };
    let result = provider(Path::new(&root)).load(&request(""));
    assert!(
        result.is_err(),
        "unsafe active ignore source must error: {result:?}"
    );
}

#[cfg(unix)]
#[test]
fn fifo_ignore_sources_fail_promptly_without_blocking_the_files_page() {
    for name in [".gitignore", ".ignore", ".git/info/exclude"] {
        let root = fixture();
        let path = root.path().join(name);
        if path.exists() {
            fs::remove_file(&path).unwrap();
        }
        let path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "unsafe_ignore_worker", "--nocapture"])
            .env("WORKDECK_UNSAFE_IGNORE_ROOT", root.path())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "worker rejected {name} incorrectly");
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("Files blocked on {name}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(unix)]
#[test]
fn symlink_and_oversized_ignore_sources_are_explicit_errors() {
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("rules"), "*.secret\n").unwrap();
    for name in [".gitignore", ".ignore", ".git/info/exclude"] {
        let root = fixture();
        let path = root.path().join(name);
        if path.exists() {
            fs::remove_file(&path).unwrap();
        }
        std::os::unix::fs::symlink(outside.path().join("rules"), &path).unwrap();
        assert!(
            provider(root.path()).load(&request("")).is_err(),
            "accepted symlink {name}"
        );
    }
    let root = fixture();
    fs::write(
        root.path().join(".gitignore"),
        vec![b'#'; 2 * 1024 * 1024 + 1],
    )
    .unwrap();
    assert!(provider(root.path()).load(&request("")).is_err());
}

#[test]
fn lossy_file_previews_remain_within_the_output_byte_limit() {
    let root = fixture();
    fs::write(root.path().join("non-utf8.dat"), vec![0xff; 512 * 1024]).unwrap();
    let preview = provider(root.path())
        .preview(&PanelTarget::File {
            path: "non-utf8.dat".into(),
            line: None,
        })
        .unwrap();
    assert!(
        preview.body.len() <= 512 * 1024,
        "expanded preview length {}",
        preview.body.len()
    );
    assert!(preview.truncated || preview.binary);
}

#[test]
fn nested_ignore_precedence_and_directory_navigation_preserve_visible_scope() {
    let root = fixture();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join(".gitignore"), "*.tmp\nignored/\n").unwrap();
    fs::write(root.path().join(".ignore"), "!visible.tmp\n").unwrap();
    fs::write(root.path().join("src/.gitignore"), "!allowed.tmp\n").unwrap();
    for path in [
        "visible.tmp",
        "hidden.tmp",
        "src/hidden.tmp",
        "src/allowed.tmp",
        "src/ordinary.txt",
    ] {
        fs::write(root.path().join(path), "content").unwrap();
    }
    let provider = provider(root.path());
    let root_list = provider.load(&request("")).unwrap();
    assert!(
        root_list
            .entries
            .iter()
            .any(|row| row.label == "visible.tmp")
    );
    assert!(
        !root_list
            .entries
            .iter()
            .any(|row| row.label == "hidden.tmp")
    );
    let nested = provider.load(&request("src")).unwrap();
    assert!(nested.entries.iter().any(|row| row.label == "allowed.tmp"));
    assert!(!nested.entries.iter().any(|row| row.label == "hidden.tmp"));
    assert!(nested.entries.iter().any(|row| row.label == "ordinary.txt"));
}

#[test]
fn truncated_multibyte_text_remains_text() {
    let root = fixture();
    fs::write(root.path().join("text.txt"), "€".repeat(200_000)).unwrap();
    let preview = provider(root.path())
        .preview(&PanelTarget::File {
            path: "text.txt".into(),
            line: None,
        })
        .unwrap();
    assert!(preview.truncated);
    assert!(!preview.binary);
    assert!(preview.body.len() <= 512 * 1024);
    assert!(preview.body.starts_with('€'));
}

#[test]
fn directory_only_inventory_reports_its_traversal_bound() {
    let root = fixture();
    for index in 0..2050 {
        fs::create_dir(root.path().join(format!("directory-{index:04}"))).unwrap();
    }
    let (_, truncated) = provider(root.path()).search("no-such-content", 10).unwrap();
    assert!(
        truncated,
        "directory scans must be bounded even without file results"
    );
}

#[test]
fn ancestor_ignore_and_nested_repository_boundaries_are_preserved() {
    let enclosing = TempDir::new().unwrap();
    fs::write(enclosing.path().join(".ignore"), "ancestor-hidden\n").unwrap();
    let root = enclosing.path().join("repository");
    git2::Repository::init(&root).unwrap();
    fs::write(root.join(".gitignore"), "outer-hidden\n").unwrap();
    fs::write(root.join("ancestor-hidden"), "hidden").unwrap();
    let nested = root.join("nested");
    git2::Repository::init(&nested).unwrap();
    fs::write(
        nested.join("outer-hidden"),
        "visible in nested Git repository",
    )
    .unwrap();
    fs::write(nested.join("ancestor-hidden"), "still hidden by .ignore").unwrap();
    let provider = provider(&root);
    let list = provider.load(&request("nested")).unwrap();
    assert!(list.entries.iter().any(|row| row.label == "outer-hidden"));
    assert!(
        !list
            .entries
            .iter()
            .any(|row| row.label == "ancestor-hidden")
    );
    let (results, _) = provider.search("nested/outer-hidden", 100).unwrap();
    assert!(
        results
            .iter()
            .any(|row| row.record.label == "nested/outer-hidden")
    );
}

#[test]
fn global_ignore_worker() {
    let Some(root) = std::env::var_os("WORKDECK_GLOBAL_IGNORE_ROOT") else {
        return;
    };
    let list = provider(Path::new(&root)).load(&request("")).unwrap();
    for hidden in ["globally-hidden", "info-hidden"] {
        assert!(
            !list.entries.iter().any(|row| row.label == hidden),
            "{list:?}"
        );
    }
    assert!(list.entries.iter().any(|row| row.label == "visible"));
}

#[test]
fn explicit_global_and_repository_excludes_keep_their_precedence() {
    let root = fixture();
    let external = TempDir::new().unwrap();
    fs::write(external.path().join("ignore"), "globally-hidden\nvisible\n").unwrap();
    fs::write(
        external.path().join("config"),
        format!(
            "[core]\n excludesFile = {}\n",
            external.path().join("ignore").display()
        ),
    )
    .unwrap();
    fs::write(
        root.path().join(".git/info/exclude"),
        "info-hidden\n!visible\n",
    )
    .unwrap();
    for name in ["globally-hidden", "info-hidden", "visible"] {
        fs::write(root.path().join(name), "content").unwrap();
    }
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "global_ignore_worker", "--nocapture"])
        .env("WORKDECK_GLOBAL_IGNORE_ROOT", root.path())
        .env("GIT_CONFIG_GLOBAL", external.path().join("config"))
        .status()
        .unwrap();
    assert!(status.success());
}

#[cfg(unix)]
#[test]
fn ignore_parent_symlinks_cannot_redirect_rule_reads() {
    let root = fixture();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("exclude"), "private rule\n").unwrap();
    fs::remove_dir_all(root.path().join(".git/info")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join(".git/info")).unwrap();
    assert!(provider(root.path()).load(&request("")).is_err());
}

#[test]
fn worktree_common_excludes_are_loaded_from_the_selected_git_metadata() {
    let common = fixture();
    fs::write(common.path().join(".git/info/exclude"), "common-hidden\n").unwrap();
    let metadata = common.path().join(".git/worktrees/synthetic");
    fs::create_dir_all(&metadata).unwrap();
    fs::write(metadata.join("commondir"), "../..\n").unwrap();
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".git"),
        format!("gitdir: {}\n", metadata.display()),
    )
    .unwrap();
    fs::write(root.path().join("common-hidden"), "hidden").unwrap();
    fs::write(root.path().join("visible"), "shown").unwrap();
    let list = provider(root.path()).load(&request("")).unwrap();
    assert!(!list.entries.iter().any(|row| row.label == "common-hidden"));
    assert!(list.entries.iter().any(|row| row.label == "visible"));
}

#[cfg(unix)]
#[test]
fn fifo_global_ignore_configuration_cannot_block_discovery() {
    let root = fixture();
    let external = TempDir::new().unwrap();
    let fifo = external.path().join("config");
    let fifo_c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "unsafe_ignore_worker", "--nocapture"])
        .env("WORKDECK_UNSAFE_IGNORE_ROOT", root.path())
        .env("GIT_CONFIG_GLOBAL", &fifo)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("global ignore config blocked Files");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn concurrent_parent_replacement_never_redirects_content_or_directory_entries() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let root = fixture();
    let outside = TempDir::new().unwrap();
    fs::create_dir(root.path().join("parent")).unwrap();
    fs::write(root.path().join("parent/inside.txt"), "inside content").unwrap();
    fs::write(
        outside.path().join("inside.txt"),
        "outside content must not appear",
    )
    .unwrap();
    fs::write(outside.path().join("outside-only.txt"), "outside name").unwrap();
    let provider = provider(root.path());
    assert_eq!(
        provider.load(&request("parent")).unwrap().entries[0].label,
        "inside.txt"
    );
    let stop = Arc::new(AtomicBool::new(false));
    let task_stop = stop.clone();
    let root_path = root.path().to_owned();
    let outside_path = outside.path().to_owned();
    let worker = std::thread::spawn(move || {
        while !task_stop.load(Ordering::SeqCst) {
            fs::rename(root_path.join("parent"), root_path.join("held-parent")).unwrap();
            std::os::unix::fs::symlink(&outside_path, root_path.join("parent")).unwrap();
            std::thread::yield_now();
            fs::remove_file(root_path.join("parent")).unwrap();
            fs::rename(root_path.join("held-parent"), root_path.join("parent")).unwrap();
        }
    });
    let mut redirected = false;
    for _ in 0..100 {
        if let Ok(preview) = provider.preview(&PanelTarget::File {
            path: "parent/inside.txt".into(),
            line: None,
        }) {
            redirected |= preview.body.contains("outside content");
        }
        if let Ok(list) = provider.load(&request("parent")) {
            redirected |= list
                .entries
                .iter()
                .any(|row| row.label == "outside-only.txt");
        }
    }
    stop.store(true, Ordering::SeqCst);
    worker.join().unwrap();
    assert!(
        !redirected,
        "parent swap redirected a descriptor-bound read"
    );
}
