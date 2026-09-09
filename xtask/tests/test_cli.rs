use std::process::{Command, Stdio};

#[test]
fn workspace_test_rejects_arguments_before_repository_or_cargo_access() {
    let directory = tempfile::tempdir().unwrap();
    for argument in ["--help", "--package", "--", "unexpected"] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .current_dir(directory.path())
            .env("PATH", directory.path())
            .args(["test", argument])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap().trim(),
            "xtask: test accepts no arguments"
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}
