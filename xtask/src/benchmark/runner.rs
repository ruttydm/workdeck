//! Incremental MIT port of Hunk's benchmark runner. Execution of the complete suite is pending.

use super::*;

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
    println!(
        "{}",
        serde_json::json!({"options":options,"scripts":selected_scripts(&options),"executionAvailable":false})
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
