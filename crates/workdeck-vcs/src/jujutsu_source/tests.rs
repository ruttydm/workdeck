use super::*;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn spec(repo_root: &Path, commit_id: &str, path: &str) -> JujutsuFileSourceSpec {
    JujutsuFileSourceSpec {
        repo_root: repo_root.into(),
        commit_id: commit_id.into(),
        path: path.into(),
    }
}

fn capturing_options() -> (JujutsuFileSourceOptions, Arc<Mutex<Vec<String>>>) {
    let diagnostics = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&diagnostics);
    (
        JujutsuFileSourceOptions {
            diagnostic: Some(Arc::new(move |message| {
                captured.lock().unwrap().push(message);
            })),
            ..JujutsuFileSourceOptions::default()
        },
        diagnostics,
    )
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
fn reads_exact_commit_contents_through_jj_file_show() {
    let fixture = TempDir::new().unwrap();
    let executable = fixture.path().join("jj");
    write_executable(
        &executable,
        "#!/bin/sh\ncase \"$8\" in first) printf 'first revision\\n' ;; second) printf 'second revision\\n' ;; esac\n",
    );
    let options = JujutsuFileSourceOptions {
        jj_executable: executable,
        ..JujutsuFileSourceOptions::default()
    };
    let path = "-note [exact] file.txt";
    assert_eq!(
        read_jj_file_source(&spec(fixture.path(), "first", path), &options),
        LimitedSourceTextResult::Text("first revision\n".into())
    );
    assert_eq!(
        read_jj_file_source(&spec(fixture.path(), "second", path), &options),
        LimitedSourceTextResult::Text("second revision\n".into())
    );
}

#[cfg(unix)]
#[test]
fn reads_unix_filenames_containing_backslashes_as_literal_filesets() {
    let fixture = TempDir::new().unwrap();
    let executable = fixture.path().join("jj");
    write_executable(&executable, "#!/bin/sh\nprintf 'backslash source\\n'\n");
    let options = JujutsuFileSourceOptions {
        jj_executable: executable,
        ..JujutsuFileSourceOptions::default()
    };
    assert_eq!(jj_file_path_fileset("a\\b.txt"), r#""a[\\\\]b.txt""#);
    assert_eq!(
        read_jj_file_source(&spec(fixture.path(), "abc123", "a\\b.txt"), &options),
        LimitedSourceTextResult::Text("backslash source\n".into())
    );
}

#[cfg(unix)]
#[test]
fn ignores_fileset_aliases_and_file_show_templates() {
    let fixture = TempDir::new().unwrap();
    let executable = fixture.path().join("jj");
    write_executable(&executable, "#!/bin/sh\nprintf 'one\\n'\n");
    let options = JujutsuFileSourceOptions {
        jj_executable: executable,
        ..JujutsuFileSourceOptions::default()
    };
    let source_spec = spec(fixture.path(), "abc123", "a*?[x]{y}.txt");
    let arguments = jj_source_arguments(&source_spec);
    assert_eq!(arguments[9], "\"\"");
    assert_eq!(arguments[10], "--");
    assert_eq!(arguments[11], r#""a[*][?][[]x[]][{]y[}].txt""#);
    assert_eq!(
        read_jj_file_source(&source_spec, &options),
        LimitedSourceTextResult::Text("one\n".into())
    );
}

#[cfg(unix)]
#[test]
fn reports_source_reads_above_the_configured_byte_cap() {
    let fixture = TempDir::new().unwrap();
    let executable = fixture.path().join("jj");
    write_executable(&executable, "#!/bin/sh\nprintf 'committed source\\n'\n");
    let options = JujutsuFileSourceOptions {
        jj_executable: executable,
        max_source_bytes: 5,
        diagnostic: None,
    };
    assert_eq!(
        read_jj_file_source(&spec(fixture.path(), "abc123", "note.txt"), &options),
        LimitedSourceTextResult::TooLarge { max_bytes: 5 }
    );
}

#[cfg(unix)]
#[test]
fn absent_paths_are_missing_without_diagnostics() {
    let fixture = TempDir::new().unwrap();
    let executable = fixture.path().join("jj");
    write_executable(
        &executable,
        "#!/bin/sh\nprintf 'Error: No such path: missing-from-history.txt\\n' >&2\nexit 1\n",
    );
    let (mut options, diagnostics) = capturing_options();
    options.jj_executable = executable;
    assert_eq!(
        read_jj_file_source(
            &spec(fixture.path(), "abc123", "missing-from-history.txt"),
            &options
        ),
        LimitedSourceTextResult::Missing
    );
    assert!(diagnostics.lock().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn oversized_diagnostics_force_terminate_a_child_that_ignores_term() {
    let fixture = TempDir::new().unwrap();
    let executable = fixture.path().join("jj");
    write_executable(
        &executable,
        "#!/bin/sh\ntrap '' TERM\nprintf 'small source\\n'\nhead -c 70000 /dev/zero | tr '\\000' x >&2\nwhile :; do sleep 1; done\n",
    );
    let (mut options, diagnostics) = capturing_options();
    options.jj_executable = executable;
    let started = Instant::now();
    assert_eq!(
        read_jj_file_source(
            &spec(fixture.path(), "0123456789abcdef", "note.txt"),
            &options
        ),
        LimitedSourceTextResult::Missing
    );
    // Cleanup is bounded; allow parallel test-load headroom for the shell's inherited pipe.
    assert!(started.elapsed() < Duration::from_secs(5));
    let diagnostics = diagnostics.lock().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].contains("failed to collect Jujutsu source"));
    assert!(diagnostics[0].contains("diagnostics exceeded"));
}

#[cfg(unix)]
#[test]
fn passes_revisions_and_files_as_separate_arguments_to_custom_executable() {
    let fixture = TempDir::new().unwrap();
    let executable = fixture.path().join("custom-jj");
    write_executable(&executable, "#!/bin/sh\nprintf 'safe source\\n'\n");
    let source_spec = spec(
        fixture.path(),
        "0123456789abcdef",
        "-leading *?[fileset]{path} \"file\".txt",
    );
    let options = JujutsuFileSourceOptions {
        jj_executable: executable.clone(),
        ..JujutsuFileSourceOptions::default()
    };
    let arguments = jj_source_arguments(&source_spec);
    assert_eq!(
        arguments,
        [
            "--no-pager",
            "--color",
            "never",
            "file",
            "show",
            "--ignore-working-copy",
            "-r",
            "0123456789abcdef",
            "-T",
            "\"\"",
            "--",
            r#""-leading [*][?][[]fileset[]][{]path[}] \"file\".txt""#,
        ]
    );
    let command = jj_source_command(&options, &source_spec, &arguments);
    assert_eq!(command.get_program(), executable.as_os_str());
    assert_eq!(command.get_current_dir(), Some(fixture.path()));
    assert_eq!(
        read_jj_file_source(&source_spec, &options),
        LimitedSourceTextResult::Text("safe source\n".into())
    );
}

#[cfg(unix)]
#[test]
fn unexpected_source_failures_include_revision_path_and_repository_context() {
    let fixture = TempDir::new().unwrap();
    let executable = fixture.path().join("jj");
    write_executable(
        &executable,
        "#!/bin/sh\nprintf 'Error: not in a workspace\\n' >&2\nexit 1\n",
    );
    let (mut options, diagnostics) = capturing_options();
    options.jj_executable = executable;
    assert_eq!(
        read_jj_file_source(
            &spec(fixture.path(), "0123456789abcdef", "note.txt"),
            &options
        ),
        LimitedSourceTextResult::Missing
    );
    let diagnostics = diagnostics.lock().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].contains("0123456789abcdef:note.txt"));
    assert!(diagnostics[0].contains(&fixture.path().display().to_string()));
}

#[test]
fn spawn_failures_are_missing_and_name_the_requested_source() {
    let fixture = TempDir::new().unwrap();
    let (mut options, diagnostics) = capturing_options();
    options.jj_executable = "definitely-not-a-real-jj-binary".into();
    assert_eq!(
        read_jj_file_source(&spec(fixture.path(), "abc123", "note.txt"), &options),
        LimitedSourceTextResult::Missing
    );
    assert!(diagnostics.lock().unwrap()[0].contains("abc123:note.txt"));
}
