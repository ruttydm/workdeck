//! Native MIT port of Hunk's optional competitor benchmark.
//!
//! The workload is informational: it measures the same deterministic fixture
//! against Git and whichever optional diff viewers are installed.  Tool
//! versions and availability are reported in the metric stream; no external
//! tool is required for the benchmark to succeed.

use super::*;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

const SOURCE_PATH: &str = "benchmarks/competitors.ts";
const SOURCE_BYTES: usize = 3_432;
const SOURCE_LINES: usize = 112;
const SOURCE_SHA256: &str = "5b09707e6ee9916d3c44ce6f559ddb2e16973fb7fe0e1eac8e20ca0302ce74e0";
const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";

fn command_exists(command: &str) -> bool {
    #[cfg(windows)]
    {
        Command::new("where.exe")
            .arg(command)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
    #[cfg(not(windows))]
    {
        Command::new("sh")
            .args([
                "-c",
                "command -v \"$1\" >/dev/null 2>&1",
                "workdeck",
                command,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}

fn process_exit_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    -1
}

fn tool_command(
    command: &[String],
    cwd: Option<&Path>,
    stdin: Option<&str>,
) -> Result<(i32, String)> {
    let Some(program) = command.first() else {
        bail!("competitor command cannot be empty");
    };
    let mut process = Command::new(program);
    process.args(&command[1..]);
    if let Some(cwd) = cwd {
        process.current_dir(cwd);
    }
    process
        .env("NO_COLOR", "1")
        .env("TERM", "xterm-256color")
        .stdin(match stdin {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        })
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = process.spawn()?;
    if let Some(input) = stdin {
        use std::io::Write;
        child
            .stdin
            .take()
            .expect("piped competitor stdin")
            .write_all(input.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    Ok((
        process_exit_code(output.status),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

fn measure_tool(
    metric: &str,
    command: &[String],
    cwd: Option<&Path>,
    stdin: Option<&str>,
) -> Result<()> {
    let start = Instant::now();
    let (exit_code, stderr) = tool_command(command, cwd, stdin)?;
    let duration = start.elapsed().as_secs_f64() * 1_000.0;
    if exit_code != 0 {
        println!("METRIC {metric}_available=0");
        let stderr = crate::release_channel::trim_source_whitespace(&stderr);
        if !stderr.is_empty() {
            eprintln!("{} failed: {stderr}", command.join(" "));
        }
        return Ok(());
    }
    println!("METRIC {metric}_ms={}", fixed(duration, 2));
    println!("METRIC {metric}_available=1");
    Ok(())
}

fn optional_tool(
    metric: &str,
    command: &[String],
    cwd: Option<&Path>,
    stdin: Option<&str>,
) -> Result<()> {
    if command_exists(command.first().expect("optional command has a program")) {
        measure_tool(metric, command, cwd, stdin)
    } else {
        println!("METRIC {metric}_available=0");
        Ok(())
    }
}

fn synthetic_options() -> fixtures::Options {
    fixtures::Options {
        file_count: 96.0,
        lines: 180.0,
        changed_start: Some(60.0),
        changed_lines: Some(36.0),
        extension: "ts".into(),
        prefix: "src/bench".into(),
    }
}

/// Execute the optional competitor benchmark and emit the pinned metric names.
pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark competitors accepts no arguments");
    }
    let patch = fixtures::patch(&synthetic_options());
    let patch_fixture = fixtures::temporary("workdeck-competitor-patch-")?;
    let repo_fixture = fixtures::changed_repo(&synthetic_options())?;
    let result = (|| -> Result<()> {
        let patch_path = patch_fixture.path().join("large.patch");
        let before_path = patch_fixture.path().join("before.ts");
        let after_path = patch_fixture.path().join("after.ts");
        fs::write(&patch_path, &patch)?;
        let source_options = fixtures::Options {
            file_count: 1.0,
            lines: 12_000.0,
            changed_start: Some(4_000.0),
            changed_lines: Some(2_000.0),
            extension: "ts".into(),
            prefix: "src/bench".into(),
        };
        fs::write(&before_path, fixtures::source(1, false, &source_options))?;
        fs::write(&after_path, fixtures::source(1, true, &source_options))?;

        measure_tool(
            "competitor_git_diff_no_ext_diff",
            &[
                "git".into(),
                "diff".into(),
                "--no-ext-diff".into(),
                "--no-color".into(),
            ],
            Some(repo_fixture.path()),
            None,
        )?;
        // Warm Git's object lookup just as the source workload does.
        fixtures::git(repo_fixture.path(), &["status", "--short"])?;

        optional_tool(
            "competitor_delta_patch_stdin",
            &[
                "delta".into(),
                "--no-gitconfig".into(),
                "--paging=never".into(),
            ],
            None,
            Some(&patch),
        )?;

        let (program, args): (&str, Vec<String>) = if command_exists("difft") {
            (
                "difft",
                vec![
                    "--color=never".into(),
                    before_path.to_string_lossy().into_owned(),
                    after_path.to_string_lossy().into_owned(),
                ],
            )
        } else if command_exists("difftastic") {
            (
                "difftastic",
                vec![
                    "--color=never".into(),
                    before_path.to_string_lossy().into_owned(),
                    after_path.to_string_lossy().into_owned(),
                ],
            )
        } else {
            ("", Vec::new())
        };
        if program.is_empty() {
            println!("METRIC competitor_difftastic_file_pair_available=0");
        } else {
            let mut command = vec![program.into()];
            command.extend(args);
            measure_tool("competitor_difftastic_file_pair", &command, None, None)?;
        }

        optional_tool(
            "competitor_diff_so_fancy_patch_stdin",
            &["diff-so-fancy".into()],
            None,
            Some(&patch),
        )?;
        Ok(())
    })();
    // Explicit drops preserve the source's finally cleanup semantics even if a
    // command fails before all metrics are emitted.
    drop(repo_fixture);
    drop(patch_fixture);
    result
}

/// Verify both pinned source blobs and the executable native replacement.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "competitor verifier received unexpected baseline {baseline}"
    );
    let source = crate::git_stdout_bytes(repo, ["show", &format!("{BASELINE}:{SOURCE_PATH}")])?;
    let stable = crate::git_stdout_bytes(repo, ["show", &format!("{STABLE}:{SOURCE_PATH}")])?;
    for (pin, bytes) in [(BASELINE, &source), (STABLE, &stable)] {
        ensure!(
            bytes.len() == SOURCE_BYTES,
            "pinned {SOURCE_PATH} {pin} changed size"
        );
        ensure!(
            bytes.split(|byte| *byte == b'\n').count() == SOURCE_LINES + 1,
            "pinned {SOURCE_PATH} {pin} changed line count"
        );
        ensure!(
            format!("{:x}", sha2::Sha256::digest(bytes)) == SOURCE_SHA256,
            "pinned {SOURCE_PATH} {pin} changed SHA-256"
        );
    }
    ensure!(
        source == stable,
        "pinned competitor benchmark diverged between pins"
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "Optional informational comparisons",
        "createChangedRepo",
        "createSyntheticPatch",
        "createSyntheticSource",
        "createTemporaryDirectory",
        "commandExists",
        "Bun.spawnSync",
        "measureTool",
        "competitor_git_diff_no_ext_diff",
        "competitor_delta_patch_stdin",
        "competitor_difftastic_file_pair",
        "competitor_diff_so_fancy_patch_stdin",
        "METRIC",
    ] {
        ensure!(
            source.contains(marker),
            "pinned competitor benchmark is missing marker {marker:?}"
        );
    }
    for (path, marker) in [
        ("xtask/src/benchmark/competitors.rs", "pub(super) fn run("),
        (
            "xtask/src/benchmark/competitors.rs",
            "pub(crate) fn verify(",
        ),
        (
            "xtask/src/benchmark/competitors.rs",
            "competitor_git_diff_no_ext_diff",
        ),
        ("xtask/src/benchmark.rs", "Some(\"competitors\")"),
        ("xtask/src/benchmark/runner.rs", "competitors.ts"),
        ("docs/benchmarks.md", "cargo xtask benchmark competitors"),
    ] {
        let native = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read competitor native surface {path}"))?;
        ensure!(
            native.contains(marker),
            "competitor native surface {path} is missing {marker:?}"
        );
    }
    let docs = fs::read_to_string(repo.join("docs/competitors-benchmark-migration.md"))
        .context("read competitor benchmark migration documentation")?;
    for marker in [
        SOURCE_PATH,
        "3,432",
        SOURCE_SHA256,
        "Git",
        "delta",
        "difftastic",
        "diff-so-fancy",
        "optional",
        "Rust",
        "no JavaScript runtime",
    ] {
        ensure!(
            docs.contains(marker),
            "competitor migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_competitor_source_is_verified_at_both_pins() {
        let repo = super::super::super::repo_root().unwrap();
        verify(&repo, BASELINE).unwrap();
    }

    #[test]
    fn competitor_command_is_deterministic_and_rejects_options() {
        assert!(run(["--unexpected".into()].into_iter()).is_err());
        run(std::iter::empty()).unwrap();
    }

    #[test]
    fn command_exists_distinguishes_present_and_missing_programs() {
        assert!(command_exists("git"));
        assert!(!command_exists("workdeck-command-that-does-not-exist"));
    }
}
