//! Incremental MIT port of Hunk's benchmark runner. Execution of the complete suite is pending.

use super::*;

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
