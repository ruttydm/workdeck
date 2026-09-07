//! MIT translation of Hunk benchmark result aggregation (Modem Labs Inc.).
//! Historical thresholds describe source reports, not the strict semantic-port release gate.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Threshold {
    pub max_regression_ratio: f64,
    pub min_absolute_regression: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Metric {
    pub name: String,
    pub unit: String,
    pub samples: Vec<f64>,
    pub median: f64,
    pub p75: f64,
    pub p95: f64,
    pub min: f64,
    pub max: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<Threshold>,
    pub comparable: bool,
    pub source: String,
}

fn percentile(samples: &[f64], percent: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|left, right| (left - right).partial_cmp(&0.0).unwrap_or(Ordering::Equal));
    let index = ((percent / 100.0 * sorted.len() as f64).ceil() - 1.0)
        .max(0.0)
        .min((sorted.len() - 1) as f64) as usize;
    sorted[index]
}

fn classify(name: &str) -> (&'static str, bool, Option<Threshold>) {
    if name.starts_with("competitor_") {
        return ("ms", false, None);
    }
    if name.ends_with("_ms") {
        return (
            "ms",
            true,
            Some(Threshold {
                max_regression_ratio: 1.15,
                min_absolute_regression: 5.0,
            }),
        );
    }
    if name.starts_with("is_")
        || name.ends_with("_ready_before_move")
        || name.ends_with("_available")
    {
        return ("boolean", false, None);
    }
    if name.contains("rss") || name.contains("heap") {
        return (
            "bytes",
            true,
            Some(Threshold {
                max_regression_ratio: 1.2,
                min_absolute_regression: 8.0 * 1024.0 * 1024.0,
            }),
        );
    }
    if name.ends_with("_bytes") {
        return ("bytes", false, None);
    }
    ("count", false, None)
}

fn aggregate(source: &str, name: &str, samples: Vec<f64>) -> Metric {
    let (unit, comparable, threshold) = classify(name);
    Metric {
        name: format!("{source}/{name}"),
        source: source.into(),
        median: percentile(&samples, 50.0),
        p75: percentile(&samples, 75.0),
        p95: percentile(&samples, 95.0),
        min: percentile(&samples, 0.0),
        max: percentile(&samples, 100.0),
        samples,
        unit: unit.into(),
        comparable,
        threshold,
    }
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().as_deref() != Some("aggregate") {
        bail!("benchmark requires aggregate SOURCE METRIC SAMPLES_JSON");
    }
    let source = args.next();
    let name = args.next();
    let samples = args.next();
    let (Some(source), Some(name), Some(samples)) = (source, name, samples) else {
        bail!("benchmark requires aggregate SOURCE METRIC SAMPLES_JSON");
    };
    if args.next().is_some() {
        bail!("unexpected benchmark aggregate argument");
    }
    let samples: Vec<f64> = serde_json::from_str(&samples)?;
    println!(
        "{}",
        serde_json::to_string(&aggregate(&source, &name, samples))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_dual_pin_aggregation_matches_every_native_field() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../port/hunk/oracles/benchmark-aggregation.json"
        ))
        .unwrap();
        for expected in oracle["metrics"].as_array().unwrap() {
            let expected: Metric = serde_json::from_value(expected.clone()).unwrap();
            let name = expected.name.strip_prefix("fixture/").unwrap();
            assert_eq!(
                aggregate("fixture", name, vec![9.0, 1.0, 5.0, 3.0]),
                expected
            );
        }
        for case in oracle["percentiles"].as_array().unwrap() {
            assert_eq!(
                percentile(&[9.0, 1.0, 5.0, 3.0], case["percent"].as_f64().unwrap()),
                case["value"].as_f64().unwrap()
            );
        }
        let empty: Metric = serde_json::from_value(oracle["empty"].clone()).unwrap();
        assert_eq!(aggregate("fixture", "render_ms", vec![]), empty);
    }

    #[test]
    fn nearest_rank_keeps_input_order_and_handles_empty_and_boundary_percentiles() {
        let samples = [9.0, 1.0, 5.0, 3.0];
        for (percent, expected) in [
            (-10.0, 1.0),
            (0.0, 1.0),
            (50.0, 3.0),
            (75.0, 5.0),
            (95.0, 9.0),
            (110.0, 9.0),
        ] {
            assert_eq!(percentile(&samples, percent), expected);
        }
        assert_eq!(percentile(&[], 50.0), 0.0);
        let result = aggregate("fixture", "render_ms", samples.to_vec());
        assert_eq!(result.samples, samples);
        assert_eq!(
            (
                result.median,
                result.p75,
                result.p95,
                result.min,
                result.max
            ),
            (3.0, 5.0, 9.0, 1.0, 9.0)
        );
    }

    #[test]
    fn classification_preserves_ordered_source_rules() {
        for (name, unit, comparable) in [
            ("competitor_heap_ms", "ms", false),
            ("is_ready_ms", "ms", true),
            ("is_ready", "boolean", false),
            ("scroll_ready_before_move", "boolean", false),
            ("git_available", "boolean", false),
            ("peak_rss", "bytes", true),
            ("heap_used", "bytes", true),
            ("patch_bytes", "bytes", false),
            ("rows", "count", false),
        ] {
            let metric = aggregate("fixture", name, vec![]);
            assert_eq!(metric.unit, unit, "{name}");
            assert_eq!(metric.comparable, comparable, "{name}");
            assert_eq!(metric.threshold.is_some(), comparable, "{name}");
            assert_eq!(metric.min, 0.0);
            assert_eq!(metric.max, 0.0);
        }
        assert_eq!(classify("render_ms").2.unwrap().max_regression_ratio, 1.15);
        assert_eq!(
            classify("rss_bytes").2.unwrap().min_absolute_regression,
            8_388_608.0
        );
    }

    #[test]
    fn aggregate_cli_rejects_missing_extra_and_malformed_samples() {
        for args in [
            vec![],
            vec!["aggregate"],
            vec!["aggregate", "source", "metric", "null"],
            vec!["aggregate", "source", "metric", "[]", "extra"],
        ] {
            assert!(run(args.into_iter().map(str::to_owned)).is_err());
        }
        run(["aggregate", "source", "render_ms", "[9,1,5,3]"]
            .into_iter()
            .map(str::to_owned))
        .unwrap();
    }
}
