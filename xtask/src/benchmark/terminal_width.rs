//! Native executable corpus for Hunk's MIT terminal-width benchmark.
//!
//! Workdeck's Rust cell-width implementation is the benchmark authority. The
//! pinned string-width package remains an oracle input, not a shipped runtime.

use anyhow::{Result, bail};
use std::time::{Duration, Instant};

const ITERATIONS: usize = 2_000;
const WARMUP_ITERATIONS: usize = 50;
const CJK_SCALAR: &[&str] = &[
    "export const message = 日本語のコメントです。変更内容を確認してください。",
    "export function 測定値を計算する(入力: number) { return 入力 * 2; }",
    "中文注释内容：这个变更优化了终端单元格宽度测量。",
    "请检查新增、删除和未修改的代码行是否正确对齐。",
    "한국어 주석 내용과 함수 이름을 함께 측정합니다.",
    "터미널 셀 너비를 빠르게 계산하고 정렬을 유지합니다.",
];
const EMOJI_SCALAR: &[&str] = &[
    "🚀 ✨ 🔧 💡 🎯 📦 🔍 standalone emoji scalars",
    "✅ 🚧 🐛 🎉 🧪 📊 terminal status glyphs",
];
const COMPLEX_CLUSTER: &[&str] = &[
    "🧑‍💻 👩‍🔬 👨‍👩‍👧‍👦 complex emoji clusters stay aligned",
    "e\u{0301} a\u{0308} o\u{0302} u\u{0308} combining clusters keep their reference widths",
];

fn checksum(corpus: &[&str], iterations: usize) -> usize {
    let mut checksum = 0;
    for _ in 0..iterations {
        for line in corpus {
            checksum += workdeck_tui::measure_text_width(std::hint::black_box(line));
        }
    }
    std::hint::black_box(checksum)
}

fn measure_width_calls(corpus: &[&str], iterations: usize) -> (Duration, usize) {
    let start = Instant::now();
    let checksum = checksum(corpus, iterations);
    (start.elapsed(), checksum)
}

fn measure_scenario(name: &str, corpus: &[&str]) {
    let (warmup_time, warmup_checksum) = measure_width_calls(corpus, WARMUP_ITERATIONS);
    let (elapsed, measured_checksum) = measure_width_calls(corpus, ITERATIONS);
    assert_eq!(
        measured_checksum,
        warmup_checksum * (ITERATIONS / WARMUP_ITERATIONS),
        "{name} checksum changed after warmup"
    );
    println!(
        "METRIC {name}_text_width_ms={:.2}",
        elapsed.as_secs_f64() * 1_000.0
    );
    println!(
        "METRIC {name}_warmup_width_ms={:.2}",
        warmup_time.as_secs_f64() * 1_000.0
    );
    println!(
        "METRIC {name}_width_measurements={}",
        ITERATIONS * corpus.len()
    );
    println!("METRIC {name}_width_checksum={measured_checksum}");
}

/// Run the deterministic native benchmark and print the same observable
/// `METRIC` lines as the pinned script's measured workload.
pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark terminal-width accepts no options");
    }
    measure_scenario("cjk_scalar", CJK_SCALAR);
    measure_scenario("emoji_scalar", EMOJI_SCALAR);
    measure_scenario("complex_cluster", COMPLEX_CLUSTER);
    Ok(())
}

#[test]
fn exact_source_width_corpora_match_both_pinned_checksums_after_warmup() {
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../port/hunk/oracles/benchmark-terminal-width.json"
    ))
    .unwrap();
    for (name, corpus) in [
        ("cjk_scalar", CJK_SCALAR),
        ("emoji_scalar", EMOJI_SCALAR),
        ("complex_cluster", COMPLEX_CLUSTER),
    ] {
        let expected = oracle["lineWidths"][name].as_array().unwrap();
        assert_eq!(corpus.len(), expected.len());
        for (line, width) in corpus.iter().zip(expected) {
            assert_eq!(
                workdeck_tui::measure_text_width(line),
                width.as_u64().unwrap() as usize,
                "individual width mismatch for {line:?}"
            );
        }
        let warmup = checksum(corpus, WARMUP_ITERATIONS);
        let measured = checksum(corpus, ITERATIONS);
        assert_eq!(measured, warmup * (ITERATIONS / WARMUP_ITERATIONS));
        for run in oracle["runs"].as_array().unwrap() {
            assert_eq!(run["exitCode"], 0);
            assert_eq!(
                measured,
                run["checksums"][name].as_u64().unwrap() as usize,
                "{name}"
            );
            assert_eq!(
                ITERATIONS * corpus.len(),
                run["measurements"][name].as_u64().unwrap() as usize
            );
        }
    }
}

#[test]
fn native_terminal_width_benchmark_cli_rejects_options() {
    assert!(run(["--unexpected".to_owned()].into_iter()).is_err());
    run(std::iter::empty()).unwrap();
}
