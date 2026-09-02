# 5-pager-tour

A tall single-file Rust diff made to show line scrolling, paging, and hunk jumps.

## Run

```bash
workdeck difftool examples/5-pager-tour/before.rs examples/5-pager-tour/after.rs --pager
```

## What to look for

- enough changed content to exceed a normal terminal viewport
- `↑` and `↓` for line-by-line movement
- `PageUp`, `PageDown`, `Home`, and `End` for larger jumps
- multiple hunks so `[` and `]` are worth trying too

The complete 36-line source pair is compiled and tested by `cargo test -p workdeck-examples`.
