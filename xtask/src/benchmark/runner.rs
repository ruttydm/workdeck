//! Incremental MIT port of Hunk's benchmark runner. Execution of the complete suite is pending.

use super::*;
use std::io::Write;
use std::process::{Command, Stdio};

fn decoded_output(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    // Response.text() uses replacement decoding and strips exactly one initial UTF-8 BOM.
    String::from_utf8_lossy(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
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

fn workload_command(name: &str) -> Result<&'static str> {
    match name {
        "bootstrap-load.ts" => Ok("bootstrap-load"),
        "changeset-parse.ts" => Ok("changeset-parse"),
        "working-tree-load.ts" => Ok("working-tree"),
        "render-layout.ts" => Ok("render-layout"),
        _ => bail!("Native benchmark workload is not yet fully ported: {name}"),
    }
}

fn execute(
    command: &mut Command,
    label: &str,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<Vec<(String, f64)>> {
    command.stdin(Stdio::null());
    if std::env::var_os("CI").is_none() {
        command.env("CI", "1");
    }
    // std::process drains stdout and stderr concurrently, including when either pipe fills.
    let output = command.output()?;
    let diagnostic = decoded_output(&output.stderr);
    let trimmed = super::super::release_channel::trim_source_whitespace(&diagnostic);
    if !trimmed.is_empty() {
        writeln!(stderr, "{trimmed}")?;
    }
    if !output.status.success() {
        let code = process_exit_code(output.status);
        bail!("{label} failed with exit code {code}\n{diagnostic}");
    }
    let text = decoded_output(&output.stdout);
    stdout.write_all(text.as_bytes())?;
    Ok(parse_metrics(&text))
}

fn collect(
    scripts: &[String],
    samples: f64,
    stdout: &mut dyn Write,
    mut execute_sample: impl FnMut(&str, &mut dyn Write) -> Result<Vec<(String, f64)>>,
) -> Result<Vec<Metric>> {
    let mut collected: BTreeMap<(String, String), Vec<f64>> = BTreeMap::new();
    for script in scripts {
        let source = script.strip_suffix(".ts").unwrap_or(script);
        writeln!(stdout, "\n## {source}")?;
        let mut sample = 1.0;
        while sample <= samples {
            writeln!(stdout, "\n# sample {sample}/{samples}")?;
            for (name, value) in execute_sample(script, stdout)? {
                collected
                    .entry((source.into(), name))
                    .or_default()
                    .push(value);
            }
            sample += 1.0;
        }
    }
    let mut results = collected
        .into_iter()
        .map(|((source, name), values)| aggregate(&source, &name, values))
        .collect::<Vec<_>>();
    // The currently executable workload emits lowercase ASCII names with fixed separators.
    // Broader locale-sensitive identifier ordering remains part of the unmapped runner work.
    results.sort_by(|a, b| a.name.cmp(&b.name));
    writeln!(stdout, "\n## Aggregated benchmark medians")?;
    for result in &results {
        let suffix = match result.unit.as_str() {
            "ms" => "ms",
            "bytes" => " bytes",
            _ => "",
        };
        let display = |value: f64| fixed(value, if value.abs() >= 100.0 { 1 } else { 2 });
        writeln!(
            stdout,
            "{}: median={}{suffix} p95={}{suffix}",
            result.name,
            display(result.median),
            display(result.p95)
        )?;
    }
    Ok(results)
}

pub(super) fn run_command(args: impl Iterator<Item = String>) -> Result<()> {
    let samples = std::env::var("WORKDECK_BENCHMARK_SAMPLES").ok();
    let huge = std::env::var("WORKDECK_BENCH_INCLUDE_HUGE").ok();
    let options = options(args, samples.as_deref(), huge.as_deref())?;
    let scripts = selected_scripts(&options);
    // Validate the entire selection before launching anything or writing a report.
    for script in &scripts {
        workload_command(script)?;
    }
    let root = super::super::repo_root()?;
    let executable = std::env::current_exe()?;
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let results = collect(&scripts, options.samples, &mut stdout, |script, stdout| {
        let mut command = Command::new(&executable);
        command
            .current_dir(&root)
            .args(["benchmark", workload_command(script)?]);
        execute(&mut command, script, stdout, &mut stderr)
    })?;
    let git_sha = Command::new("git")
        .current_dir(&root)
        .args(["rev-parse", "HEAD"])
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    let package_version = cargo_metadata::MetadataCommand::new()
        .current_dir(&root)
        .no_deps()
        .exec()
        .ok()
        .and_then(|m| {
            m.packages
                .into_iter()
                .find(|p| p.name.as_str() == "workdeck-cli")
        })
        .map(|p| p.version.to_string());
    let run = Run {
        version: 1,
        generated_at: Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        git_sha,
        package_version,
        runtime: Some(RuntimeInfo {
            bun_version: None,
            platform: match std::env::consts::OS {
                "macos" => "darwin",
                "windows" => "win32",
                value => value,
            }
            .into(),
            arch: match std::env::consts::ARCH {
                "aarch64" => "arm64",
                "x86_64" => "x64",
                value => value,
            }
            .into(),
        }),
        samples_per_benchmark: Some(options.samples),
        accepted_regressions: None,
        results,
    };
    if let Some(out) = options.out {
        let out = super::release::resolve(&std::env::current_dir()?, &out)?;
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&out, format!("{}\n", serde_json::to_string_pretty(&run)?))?;
        writeln!(stdout, "\nWrote {}", out.display())?;
    }
    Ok(())
}

const DEFAULT_SCRIPTS: [&str; 10] = [
    "bootstrap-load.ts",
    "working-tree-load.ts",
    "changeset-parse.ts",
    "render-layout.ts",
    "highlight-prefetch.ts",
    "large-stream.ts",
    "interaction-latency.ts",
    "non-ascii-stream.ts",
    "wrapped-cjk.ts",
    "terminal-width.ts",
];

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Options {
    samples: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    out: Option<String>,
    include_competitors: bool,
    include_huge: bool,
    scripts: Vec<String>,
}

fn options(
    mut args: impl Iterator<Item = String>,
    samples_env: Option<&str>,
    huge_env: Option<&str>,
) -> Result<Options> {
    let mut result = Options {
        samples: sample_number(samples_env.unwrap_or("3")),
        out: None,
        include_competitors: false,
        include_huge: huge_env == Some("1"),
        scripts: vec![],
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--include-competitors" => result.include_competitors = true,
            "--include-huge" => result.include_huge = true,
            "--samples" | "--out" | "--script" => {
                let value = args
                    .next()
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("Missing value for {arg}"))?;
                match arg.as_str() {
                    "--samples" => result.samples = sample_number(&value),
                    "--out" => result.out = Some(value),
                    _ => result.scripts.push(value),
                }
            }
            _ => bail!("Unknown benchmark runner argument: {arg}"),
        }
    }
    if !result.samples.is_finite() || result.samples < 1.0 {
        bail!("--samples must be a positive number");
    }
    Ok(result)
}

fn selected_scripts(options: &Options) -> Vec<String> {
    let mut scripts = if options.scripts.is_empty() {
        DEFAULT_SCRIPTS.iter().map(|s| (*s).to_owned()).collect()
    } else {
        options.scripts.clone()
    };
    if options.include_huge {
        scripts.push("huge-stream.ts".into());
    }
    if options.include_competitors {
        scripts.push("competitors.ts".into());
    }
    scripts
}

pub(super) fn plan_command(args: impl Iterator<Item = String>) -> Result<()> {
    let samples = std::env::var("WORKDECK_BENCHMARK_SAMPLES").ok();
    let huge = std::env::var("WORKDECK_BENCH_INCLUDE_HUGE").ok();
    let options = options(args, samples.as_deref(), huge.as_deref())?;
    let scripts = selected_scripts(&options);
    let execution_available = scripts
        .iter()
        .all(|script| workload_command(script).is_ok());
    println!(
        "{}",
        serde_json::json!({"options":options,"scripts":scripts,"executionAvailable":execution_available})
    );
    Ok(())
}

fn parse_metrics(output: &str) -> Vec<(String, f64)> {
    // ECMAScript whitespace differs from Rust/Unicode \s (notably U+0085 and U+FEFF).
    let pattern = regex::Regex::new(
        r"^METRIC[\x09-\x0d\x20\x{00a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}]+([A-Za-z0-9_.:-]+)=(-?[0-9]+(?:\.[0-9]+)?)$",
    ).expect("literal metric regex");
    let mut metrics: Vec<(String, f64)> = vec![];
    let mut positions = BTreeMap::new();
    for line in output.split('\n') {
        let line = super::super::release_channel::trim_source_whitespace(line);
        let Some(captures) = pattern.captures(line) else {
            continue;
        };
        let name = captures[1].to_owned();
        let value: f64 = captures[2].parse().expect("regex validated decimal number");
        if let Some(&position) = positions.get(&name) {
            metrics[position] = (name, value);
        } else {
            positions.insert(name.clone(), metrics.len());
            metrics.push((name, value));
        }
    }
    metrics
}

pub(super) fn parse_command(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark parse-metrics reads stdin and accepts no arguments");
    }
    use std::io::Read;
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    println!("{}", serde_json::to_string(&parse_metrics(&input))?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_text_matches_pinned_runtime_bom_and_replacement_decoding() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-process-output.json"
        ))
        .unwrap();
        for case in oracle["decoding"].as_array().unwrap() {
            let bytes: Vec<u8> = serde_json::from_value(case["bytes"].clone()).unwrap();
            assert_eq!(decoded_output(&bytes), case["text"].as_str().unwrap());
        }
    }

    #[cfg(unix)]
    #[test]
    fn signal_termination_uses_the_pinned_runtime_exit_code() {
        let mut stdout = vec![];
        let mut stderr = vec![];
        let mut command = Command::new("sh");
        command.args(["-c", "kill -TERM $$"]);
        let error = execute(&mut command, "signal-probe", &mut stdout, &mut stderr).unwrap_err();
        assert_eq!(
            error.to_string(),
            "signal-probe failed with exit code 143\n"
        );
        assert!(stdout.is_empty() && stderr.is_empty());
    }

    #[test]
    fn native_runner_child_probe() {
        let Ok(mode) = std::env::var("WORKDECK_BENCHMARK_TEST_PROBE") else {
            return;
        };
        if mode == "fail" {
            println!("METRIC hidden_ms=1");
            eprintln!("  deliberate child failure  ");
            std::process::exit(7);
        }
        assert_eq!(mode, "pipes");
        assert!(std::env::var_os("CI").is_some());
        use std::io::Read;
        let mut input = vec![];
        std::io::stdin().read_to_end(&mut input).unwrap();
        assert!(input.is_empty());
        println!("{}", "x".repeat(128 * 1024));
        eprintln!("{}", "y".repeat(128 * 1024));
        println!("METRIC load_ms=1.25");
    }

    #[test]
    fn native_process_drains_both_pipes_and_preserves_failure_without_forwarding_stdout() {
        let command = |mode: &str| {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "benchmark::runner::tests::native_runner_child_probe",
                    "--nocapture",
                ])
                .env("WORKDECK_BENCHMARK_TEST_PROBE", mode);
            command
        };
        let (mut stdout, mut stderr) = (vec![], vec![]);
        assert_eq!(
            execute(&mut command("pipes"), "probe", &mut stdout, &mut stderr).unwrap(),
            [("load_ms".into(), 1.25)]
        );
        assert!(stdout.len() > 128 * 1024 && stderr.len() > 128 * 1024);
        let (mut stdout, mut stderr) = (vec![], vec![]);
        let error = execute(&mut command("fail"), "probe", &mut stdout, &mut stderr).unwrap_err();
        assert_eq!(
            error.to_string(),
            "probe failed with exit code 7\n  deliberate child failure  \n"
        );
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "deliberate child failure\n"
        );
    }

    #[test]
    fn collector_preserves_fractional_sampling_repeated_workloads_and_missing_metrics() {
        let mut calls = 0;
        let mut output = vec![];
        let results = collect(
            &["render-layout.ts".into(), "render-layout.ts".into()],
            2.5,
            &mut output,
            |script, _| {
                assert_eq!(script, "render-layout.ts");
                calls += 1;
                Ok(if calls == 2 {
                    vec![]
                } else {
                    vec![("render_ms".into(), f64::from(calls))]
                })
            },
        )
        .unwrap();
        assert_eq!(calls, 4);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].samples, [1.0, 3.0, 4.0]);
        assert_eq!(results[0].median, 3.0);
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.matches("# sample 1/2.5").count(), 2);
        assert!(output.ends_with("render-layout/render_ms: median=3.00ms p95=4.00ms\n"));
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("must-not-exist/report.json");
        let error = run_command(
            [
                "--samples".into(),
                "1".into(),
                "--out".into(),
                out.to_string_lossy().into_owned(),
            ]
            .into_iter(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("highlight-prefetch.ts"));
        assert!(!out.parent().unwrap().exists());
        let mut calls = 0;
        let result = collect(&["render-layout.ts".into()], 3.0, &mut vec![], |_, _| {
            calls += 1;
            bail!("child failed")
        });
        assert_eq!(result.unwrap_err().to_string(), "child failed");
        assert_eq!(calls, 1);
    }

    #[test]
    fn runner_options_match_both_pins_without_executing_source_scripts() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-runner-options.json"
        ))
        .unwrap();
        for case in oracle["cases"].as_array().unwrap() {
            let args: Vec<String> = serde_json::from_value(case["args"].clone()).unwrap();
            let actual = options(
                args.into_iter(),
                case["env"]["HUNK_BENCHMARK_SAMPLES"].as_str(),
                case["env"]["HUNK_BENCH_INCLUDE_HUGE"].as_str(),
            );
            if let Some(error) = case["error"].as_str() {
                assert_eq!(actual.unwrap_err().to_string(), error);
            } else {
                let expected: Options = serde_json::from_value(case["result"].clone()).unwrap();
                assert_eq!(actual.unwrap(), expected);
            }
        }
        let defaults = options(std::iter::empty(), None, None).unwrap();
        assert_eq!(selected_scripts(&defaults), DEFAULT_SCRIPTS);
        let custom = options(
            [
                "--script",
                "huge-stream.ts",
                "--include-huge",
                "--include-competitors",
            ]
            .map(str::to_owned)
            .into_iter(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            selected_scripts(&custom),
            ["huge-stream.ts", "huge-stream.ts", "competitors.ts"]
        );
        assert_eq!(custom.scripts, ["huge-stream.ts"]);
    }

    #[test]
    fn dual_pin_metric_lines_preserve_duplicates_whitespace_and_numeric_grammar() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-metric-parser.json"
        ))
        .unwrap();
        for case in oracle["cases"].as_array().unwrap() {
            let actual = parse_metrics(case["input"].as_str().unwrap());
            let expected: Vec<(String, f64)> =
                serde_json::from_value(case["metrics"].clone()).unwrap();
            assert_eq!(actual, expected, "{case}");
        }
        assert!(parse_metrics("METRIC zero=-0")[0].1.is_sign_negative());
        assert!(parse_command(["extra".into()].into_iter()).is_err());
    }
}
