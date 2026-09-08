# Optimized interaction profile

- Code: `7bc6ed7efb9fc7c1aedcb784e251ad4cc1940ad3`; release xtask binary.
- Host: macOS 26.5.2 arm64.
- Workload: `target/release/xtask benchmark interaction-diagnostic`.
- Method: native `sample` at 1 ms intervals for 2 seconds, attached during the workload.
- First attachment attempt missed an already-exited process; a new diagnostic run was sampled successfully. No prior live run was restarted.
- This is a diagnostic profile, not benchmark timing evidence. The sample perturbs execution and covers only part of the process. Counts below are collapsed top-of-stack observations, not wall-time percentages or exclusive per-stage attribution.

## Captured top-of-stack excerpt

```text
Sort by top of stack, same collapsed (when >= 5):
        sha2::sha256::compress256::hbfdce3fbb39b658b  (in xtask)        230
        _xzm_free  (in libsystem_malloc.dylib)        168
        _platform_memmove  (in libsystem_platform.dylib)        120
        syntect::parsing::parser::ParseState::parse_line::h1f6d3ae6753a24b1  (in xtask)        93
        workdeck_diff::word_diff::word_diff_ranges::h2ba78a63d161f191  (in xtask)        93
        forward_search  (in xtask)        82
        _xzm_xzone_malloc  (in libsystem_malloc.dylib)        76
        _malloc_zone_malloc  (in libsystem_malloc.dylib)        73
        workdeck_tui::emphasize_spans::h035ec1af9f293f26  (in xtask)        68
        _platform_memcmp  (in libsystem_platform.dylib)        63
        match_at  (in xtask)        57
        core::iter::traits::double_ended::DoubleEndedIterator::rfold::he47f9143768f997f  (in xtask)        55
        <deduplicated_symbol>  (in libsystem_malloc.dylib)        53
        _free  (in libsystem_malloc.dylib)        43
        _$LT$syntect..parsing..syntax_definition..MatchIter$u20$as$u20$core..iter..traits..iterator..Iterator$GT$::next::h5f316a8206633e29  (in xtask)        41
        syntect::highlighting::highlighter::Highlighter::update_single_cache_for_push::h17e27175aa2b66fe  (in xtask)        40
        _xzm_xzone_malloc_tiny  (in libsystem_malloc.dylib)        36
        workdeck_core::identity::review_content_digest::hcba71a3d5ab7a930  (in xtask)        35
        workdeck_diff::word_diff::tokenize::h77abc6a79b66f2e2  (in xtask)        33
        indexmap::map::IndexMap$LT$K$C$V$C$S$GT$::insert_full::hd28f7a2211991ff0  (in xtask)        29
        xzm_malloc_zone_size  (in libsystem_malloc.dylib)        24
        xzm_realloc  (in libsystem_malloc.dylib)        21
        mbc_enc_len  (in xtask)        19
        workdeck_tui::highlighted_diff_runtime::highlighted_content_fingerprint::h056ff1f343520fce  (in xtask)        19
```

## Code correlation and next investigation

`build_review_rows_with_chrome` iterates the review files and calls `prefetch_highlighted_diff_shared` for highlighted rows. That path constructs a full content-based highlight key before looking up the coordinator cache. The sample includes `highlighted_content_fingerprint` and `highlight_worker_cache_key` under those calls, plus syntax parsing and word-emphasis work. This supports investigating repeated identity construction and offscreen painting, but does not prove that any cache can safely omit content, source-provider, theme or generation identity. Preserve collision/stale-generation tests and complete terminal geometry when optimizing. No performance or ledger gate is passed by this profile.

