# Interaction profile after bounded highlight prefetch

- Code: `4cca95b458ac6c57c8d74e582b48fdba35b7cca5`, optimized xtask.
- Host: macOS arm64; workload `benchmark interaction-diagnostic`.
- Native sample of process 65990, requested one second at one-millisecond intervals.
- The preceding process completed before a live handle was retained; absence was verified before starting this sampled process.
- Local raw sample: `/tmp/workdeck-interaction-65990.sample.txt`.
- The sample includes renderer construction and interaction, is partial and perturbed, and is not benchmark timing evidence.

Selected collapsed top-of-stack observations were allocation/free (54 `_xzm_free`,
27 `_platform_memmove`, 24 `_malloc_zone_malloc`), JSON map construction (23
`IndexMap::insert_full`), content identity (21 `review_content_digest`), and gap geometry
(15 `review_gap_geometry_for_file`). Counts are not wall-time percentages.

Call stacks show `commit_extension_runtime_bridge` calling
`build_extension_review_selection_from_snapshot`, which projected every file before selecting
one by mounted ID. The selected result can instead project only the first matching file,
preserving the existing duplicate-ID lookup and selection semantics. The complete bridge
snapshot remains current; no subscriber-based shortcut or stale-cache assumption is introduced.
The effect on interaction timings must be measured separately. No ledger gate is passed here.
