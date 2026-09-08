//! Executable corpus portion of Hunk's MIT terminal-width benchmark.
//! The independent string-width reference and complete workload remain unported.

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
