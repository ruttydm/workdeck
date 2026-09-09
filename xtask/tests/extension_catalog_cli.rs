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
fn json_ld_cli_matches_frozen_source_number_and_property_formatting() {
    verify_serialization_cases(include_str!(
        "../../port/hunk/oracles/json-ld-serialization-gaps.json"
    ));
    verify_serialization_cases(include_str!(
        "../../port/hunk/oracles/json-ld-number-boundaries.json"
    ));
}

fn verify_serialization_cases(source: &str) {
    let fixture: serde_json::Value = serde_json::from_str(source).unwrap();
    for capture in fixture["sourceCaptures"].as_array().unwrap() {
        for case in capture["cases"].as_array().unwrap() {
            let output = run(
                case["input"].as_str().unwrap().as_bytes(),
                &["extension-catalog", "json-ld"],
            );
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.stderr.is_empty());
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                format!("{}\n", case["expected"].as_str().unwrap()),
                "{}: {}",
                capture["kind"],
                case["input"]
            );
        }
    }
}

#[test]
fn json_ld_command_escapes_script_closers_and_round_trips() {
    let input = br#"{"name":"</script><img src=x onerror=alert(1)>"}"#;
    let output = run(input, &["extension-catalog", "json-ld"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.contains(&b'<'));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::from_slice::<serde_json::Value>(input).unwrap()
    );
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
