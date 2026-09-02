use super::*;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn git(repo_root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repo_root)
        .output()
        .expect("run Git fixture command");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn create_repo() -> TempDir {
    let directory = TempDir::new().unwrap();
    git(directory.path(), &["init", "-q"]);
    git(directory.path(), &["config", "user.name", "Test User"]);
    git(
        directory.path(),
        &["config", "user.email", "test@example.com"],
    );
    git(directory.path(), &["config", "commit.gpgSign", "false"]);
    directory
}

fn capturing_options() -> (GitFileSourceOptions, Arc<Mutex<Vec<String>>>) {
    let diagnostics = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&diagnostics);
    (
        GitFileSourceOptions {
            diagnostic: Some(Arc::new(move |message| {
                captured.lock().unwrap().push(message);
            })),
            ..GitFileSourceOptions::default()
        },
        diagnostics,
    )
}

#[test]
fn maps_every_endpoint_kind_to_a_source_spec() {
    let root = Path::new("/repo");
    let path = Path::new("a.ts");
    assert_eq!(
        git_endpoint_source_spec(&GitDiffEndpoint::None, root, path),
        GitFileSourceSpec::None
    );
    assert_eq!(
        git_endpoint_source_spec(&GitDiffEndpoint::GitRef("HEAD".into()), root, path),
        GitFileSourceSpec::GitBlob {
            repo_root: root.into(),
            reference: "HEAD".into(),
            path: path.into(),
        }
    );
    assert_eq!(
        git_endpoint_source_spec(&GitDiffEndpoint::Index, root, path),
        GitFileSourceSpec::GitIndex {
            repo_root: root.into(),
            path: path.into(),
        }
    );
    assert_eq!(
        git_endpoint_source_spec(&GitDiffEndpoint::Worktree, root, path),
        GitFileSourceSpec::FileSystem {
            absolute_path: root.join(path),
        }
    );
}

#[test]
fn reads_git_blob_contents_for_both_revisions() {
    let repo = create_repo();
    let source = repo.path().join("note.txt");
    fs::write(&source, "first revision\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "first"]);
    fs::write(&source, "second revision\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "second"]);

    for (reference, expected) in [
        ("HEAD~1", "first revision\n"),
        ("HEAD", "second revision\n"),
    ] {
        assert_eq!(
            read_git_file_source(
                &GitFileSourceSpec::GitBlob {
                    repo_root: repo.path().into(),
                    reference: reference.into(),
                    path: "note.txt".into(),
                },
                &GitFileSourceOptions::default(),
            ),
            LimitedSourceTextResult::Text(expected.into())
        );
    }
}

#[test]
fn reads_index_and_working_tree_as_distinct_sources() {
    let repo = create_repo();
    let source = repo.path().join("note.txt");
    fs::write(&source, "committed\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "first"]);
    fs::write(&source, "staged\n").unwrap();
    git(repo.path(), &["add", "note.txt"]);
    fs::write(&source, "working tree\n").unwrap();

    assert_eq!(
        read_git_file_source(
            &GitFileSourceSpec::GitIndex {
                repo_root: repo.path().into(),
                path: "note.txt".into(),
            },
            &GitFileSourceOptions::default(),
        ),
        LimitedSourceTextResult::Text("staged\n".into())
    );
    assert_eq!(
        read_git_file_source(
            &GitFileSourceSpec::FileSystem {
                absolute_path: source,
            },
            &GitFileSourceOptions::default(),
        ),
        LimitedSourceTextResult::Text("working tree\n".into())
    );
}

#[test]
fn reports_blob_and_index_sources_above_the_configured_limit() {
    let repo = create_repo();
    let source = repo.path().join("note.txt");
    fs::write(&source, "committed source\n").unwrap();
    git(repo.path(), &["add", "note.txt"]);
    git(repo.path(), &["commit", "-q", "-m", "first"]);
    fs::write(&source, "staged source\n").unwrap();
    git(repo.path(), &["add", "note.txt"]);
    let options = GitFileSourceOptions {
        max_source_bytes: 5,
        ..GitFileSourceOptions::default()
    };

    assert_eq!(
        read_git_file_source(
            &GitFileSourceSpec::GitBlob {
                repo_root: repo.path().into(),
                reference: "HEAD".into(),
                path: "note.txt".into(),
            },
            &options,
        ),
        LimitedSourceTextResult::TooLarge { max_bytes: 5 }
    );
    assert_eq!(
        read_git_file_source(
            &GitFileSourceSpec::GitIndex {
                repo_root: repo.path().into(),
                path: "note.txt".into(),
            },
            &options,
        ),
        LimitedSourceTextResult::TooLarge { max_bytes: 5 }
    );
}

#[test]
fn custom_executable_is_used_for_blob_and_index_commands() {
    let options = GitFileSourceOptions {
        git_executable: "custom-git".into(),
        ..GitFileSourceOptions::default()
    };
    let command = git_source_command(&options, Path::new("/repo"), "HEAD:note.txt");
    assert_eq!(command.get_program(), "custom-git");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["show", "HEAD:note.txt"]
    );
    assert_eq!(command.get_current_dir(), Some(Path::new("/repo")));
}

#[cfg(unix)]
fn write_executable(path: &Path, source: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, source).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

#[cfg(unix)]
#[test]
fn passes_custom_executable_through_actual_source_reads() {
    let fixture = TempDir::new().unwrap();
    let executable = fixture.path().join("custom-git");
    write_executable(&executable, "#!/bin/sh\nprintf 'read:%s\\n' \"$2\"\n");
    let options = GitFileSourceOptions {
        git_executable: executable,
        ..GitFileSourceOptions::default()
    };

    assert_eq!(
        read_git_file_source(
            &GitFileSourceSpec::GitBlob {
                repo_root: fixture.path().into(),
                reference: "HEAD".into(),
                path: "note.txt".into(),
            },
            &options,
        ),
        LimitedSourceTextResult::Text("read:HEAD:note.txt\n".into())
    );
    assert_eq!(
        read_git_file_source(
            &GitFileSourceSpec::GitIndex {
                repo_root: fixture.path().into(),
                path: "note.txt".into(),
            },
            &options,
        ),
        LimitedSourceTextResult::Text("read::note.txt\n".into())
    );
}

#[test]
fn unresolved_blob_is_missing_without_a_diagnostic() {
    let repo = create_repo();
    fs::write(repo.path().join("tracked.txt"), "x\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "first"]);
    let (options, diagnostics) = capturing_options();

    assert_eq!(
        read_git_file_source(
            &GitFileSourceSpec::GitBlob {
                repo_root: repo.path().into(),
                reference: "HEAD".into(),
                path: "missing-from-history.txt".into(),
            },
            &options,
        ),
        LimitedSourceTextResult::Missing
    );
    assert!(diagnostics.lock().unwrap().is_empty());
}

#[test]
fn unexpected_git_failure_reports_object_and_repository_context() {
    let directory = TempDir::new().unwrap();
    let (options, diagnostics) = capturing_options();

    assert_eq!(
        read_git_file_source(
            &GitFileSourceSpec::GitBlob {
                repo_root: directory.path().into(),
                reference: "HEAD".into(),
                path: "note.txt".into(),
            },
            &options,
        ),
        LimitedSourceTextResult::Missing
    );
    let diagnostics = diagnostics.lock().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].contains("HEAD:note.txt"));
    assert!(diagnostics[0].contains(&directory.path().display().to_string()));
}

#[cfg(unix)]
#[test]
fn oversized_diagnostics_force_terminate_a_child_that_ignores_term() {
    let fixture = TempDir::new().unwrap();
    let executable = fixture.path().join("overflow-git");
    write_executable(
        &executable,
        "#!/bin/sh\ntrap '' TERM\nprintf 'small source\\n'\nhead -c 70000 /dev/zero | tr '\\000' x >&2\nwhile :; do sleep 1; done\n",
    );
    let (mut options, diagnostics) = capturing_options();
    options.git_executable = executable;
    let started = Instant::now();

    assert_eq!(
        read_git_file_source(
            &GitFileSourceSpec::GitBlob {
                repo_root: fixture.path().into(),
                reference: "HEAD".into(),
                path: "note.txt".into(),
            },
            &options,
        ),
        LimitedSourceTextResult::Missing
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    let diagnostics = diagnostics.lock().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].contains("failed to collect Git source"));
    assert!(diagnostics[0].contains("diagnostics exceeded"));
}
