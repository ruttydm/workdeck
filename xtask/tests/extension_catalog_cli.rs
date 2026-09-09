use std::io::Write;
use std::process::{Command, Stdio};

fn run(input: &[u8], args: &[&str]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn activity_index_cli_preserves_metadata_and_rejects_invalid_input() {
    let output = run(br#"{"items":[{"full_name":"Owner/Repo","stargazers_count":0},{"full_name":"Another/Repo","pushed_at":"2026-08-18T09:12:00Z"}]}"#, &["extension-catalog", "activity-index"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!({
            "owner/repo":{"stars":0},
            "another/repo":{"pushedAt":"2026-08-18T09:12:00Z"}
        })
    );
    for input in [b"".as_slice(), b"{broken", b"{} trailing"] {
        let output = run(input, &["extension-catalog", "activity-index"]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    let output = run(b"null", &["extension-catalog", "activity-index"]);
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!({})
    );
    let output = run(b"", &["extension-catalog", "activity-index", "unexpected"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
