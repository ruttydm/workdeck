# Pending source completion after alpha-file reload

The pinned regression at `src/ui/hooks/useTerminalReview.test.tsx` bytes
54,269–56,202 (lines 1624–1678) is translated by
`pending_source_cannot_repopulate_reloaded_alpha_review`. Both original source
runs pass: one test and sixteen assertions per pin.

The native fixture retains the twelve alpha assignments, alpha8 changing from
8 to 800 in the original diff and to 900 in its replacement, three context lines,
TypeScript language, runtime ID `alpha`, empty patch field, and no annotations.
Full diff metadata is non-fetchable by itself; two separately installed
unversioned readers supply exactly `first\n` and `second\n`. Channels replace
the deferred source promise while preserving controlled completion ordering.

The old request starts in loading state. Reload clears expansion and status;
completion of that retired request cannot restore either or change selection.
Opening the new gap loads only `second\n`, and each reader must record exactly
one new-side invocation. The native test additionally checks that the retired
pending cursor reveal is cleared. That assertion initially failed, revealing
leftover cursor bookkeeping even though stale source text was correctly ignored.
The reload commit now clears a pending reveal when its source identity retires.

Only this complete 1,933-byte test body is mapped. Alpha helper definitions,
neighboring tests, and the complete hook implementation remain unmapped.

Validation: 1,186 TUI library tests pass, zero failures/ignored/filtered, in
8.34 seconds; formatting and diff checks pass. Strict audit reports 1,257 files,
1,405 records, 442 translated-test records, 273 unmapped intervals, and 11 cached
upstream commits, then fails on incomplete coverage. The unmapped interval count
increases because two unfinished neighbors surround the new mapped test. Full
workspace verification and strict Clippy were not rerun for this change.
