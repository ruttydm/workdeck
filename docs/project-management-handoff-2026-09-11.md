# Workdeck project-management implementation handoff

Date: 2026-09-11  
Repository: `/Users/rutger/.herdr/worktrees/workdeck/linear-project-management`

## Resumable `/goal` prompt

```text
/goal Resume and finish the standalone Workdeck project-management implementation from the current checkout, completing every applicable PM-00..PM-12 and PM-X acceptance item in the latest airdropped specification. Work only in this Workdeck checkout. Do not modify Bowerbird, sibling repositories, agents-owned paths, deployment state, tags, commits, remotes, or published artifacts. The canonical project-management authority is the repository-root `.workdeck/` directory.

Use subagents heavily for independent performance, CI/trust, platform/release, and final-review audits. Keep all findings and changes in this checkout. Preserve unrelated dirty worktree changes; never reset, clean the repository, stage everything, commit, push, merge, or publish.

First inspect the current source and these documents:
- docs/project-management.md
- docs/project-management-implementation-plan.md
- docs/project-management-validation.md
- docs/project-management-performance.md
- docs/project-management-schema.md
- docs/project-management-compatibility.md
- docs/project-management-collaboration.md
- docs/native-release-policy.md
- ci/workdeck-pm-release.json
- the latest airdropped specification at /Users/rutger/.codex/attachments/f80d3cdb-d458-4b72-b290-ed7bfd09ab7e/pasted-text-1.txt

Current implementation state:
- PM-00..PM-09 are implemented.
- PM-10..PM-12 remain active; PM-X remains open where its evidence gates are not proven.
- Formal phase progress is 10/13, about 77%.
- Ledger progress after the 2026-09-11 final gate closure is exactly 137 checked / 82 unchecked / 219 total, about 63%; the eight newly closed rows are PM-V.G1–G3 and G5–G9, each closed on a fresh exact-command final-source run. Do not inflate this count.
- Root `.workdeck/` runtime, CLI, projection, history, source routing, transactions, snapshots, claims, gates, CI evidence, release inspection, and documentation are present.
- PM-10 added mounted full-size probes and hash-based selected-key/collapsed-ID tree lookups. Structural feature-list/tree and issue-board caps pass, but independent performance acceptance thresholds, richer fixture coverage, and cold/RSS enforcement are still open.
- PM-11 hardened the standalone release profile to an exact allowlist, requires a full 40-character lowercase commit SHA or safe refs/tags baseline plus an exact lowercase SHA-256 source digest, and added focused validator tests. External validator execution, authenticated immutable baseline resolution, and external CI/release receipts are still open.
- PM-12 added shared archive inspection/checksum helpers, post-write package reinspection, and tar/zip structural provenance tests. Unix/macOS local checks pass; Windows compiler/runtime, signing, external release receipts, and independent final qualification are still open.
- The macOS watcher setup-replay bug was repaired with exact-entry metadata fingerprints (length, modified time, readonly state, and Unix ctime where available), conservative error behavior, and tree-root marker suppression. The focused watcher suite is 19/19 green. Do not claim content-hash-level protection for same-size writes with unchanged metadata.

An isolated repaired-source full-workspace test was run under:
  CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 \
  CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  CARGO_TARGET_DIR=/tmp/workdeck-pm-workspace-repair-20260911 \
  cargo test --offline --locked --workspace --all-targets --no-fail-fast \
  2>&1 | tee /tmp/workdeck-pm-workspace-repair-20260911.log

It completed with 4,943 passed, 0 failed and 2 ignored across 205 result groups (the two
ignored tests are the opt-in mounted full-size probe and the pinned CI-oracle capture); its
isolated target directory was removed and the log retained.

Required continuation:
1. Record the exact repaired-source workspace result and update the validation/implementation-plan docs. Keep historical pre-repair failures labeled as historical.
2. Run final source checks with isolated target directories and offline/locked flags: cargo fmt check, git diff --check, targeted PM/VCS/xtask clippy with `-D warnings`, `cargo xtask architecture check`, `cargo xtask skill check`, and the standalone PM release-check. Run full workspace clippy only when storage and time permit; do not overlap Cargo processes.
3. Re-run the local-link checker over all project-management docs and inspect `git status --short` for scope. Preserve every unrelated modification.
4. Have independent subagents review PM-10 performance realism, PM-11 CI trust/validator boundaries, and PM-12 platform/release evidence. Then perform a final requirement-by-requirement audit against the latest specification.
5. Close a ledger item only when its required implementation and evidence are both proven. Keep PM-10, PM-11, PM-12, and PM-X items open when their documented external/platform/performance evidence is absent.
6. Report the final phase percentage and exact ledger numerator/denominator, tests and logs, storage cleanup, changed files, and remaining blockers. Do not create tags, commits, releases, or external receipts.

Known successful evidence to retain:
- repaired-source `cargo xtask pm release-check --profile standalone` passed; final log `/tmp/workdeck-pm-release-final-repair-20260911.log`.
- feature benchmark: roughly 11.1s cold, 8.66s incremental, warm-filter p95 below 1ms, peak RSS roughly 1.11GB.
- issue benchmark: roughly 7.96s cold, 6.21s incremental, warm-filter p95 below 1ms, peak RSS roughly 338MB.
- focused watcher suite: 19 passed, 0 failed, `/tmp/workdeck-pm-watcher-fingerprint-20260911.log`.
- focused projection audit and PM/VCS clippy logs are `/tmp/workdeck-pm-projection-audit-20260911.log`, `/tmp/workdeck-pm-pm-clippy-20260911.log`, and `/tmp/workdeck-pm-vcs-clippy-20260911.log`.
- package structural/provenance smoke logs are `/tmp/workdeck-pm-package-audit-provenance-20260911.json` and `/tmp/workdeck-pm-package-audit-provenance-windows-20260911.json`; they are structural only, not signed/runtime proof.
```

## Handoff state and boundaries

The implementation is intentionally local and standalone. No Bowerbird or sibling-repository work has been performed. The worktree is mixed and dirty; preserve unrelated changes and inspect before editing. The current code has been formatted and `git diff --check` has passed after the watcher repair.

The most important unresolved gates are:

1. **PM-10 / PM-X performance:** mounted probes demonstrate bounded geometry and useful timings, but there is no independent acceptance threshold for tree/board latency, cold-start/RSS enforcement, or a broader fixture matrix. The remaining hot paths include full authority-file listing/stat capture, broad projection validation, tree parent-forest rebuilding, per-group board paging, and a serialized reader worker. HashMap/HashSet lookup optimization is landed but does not close those gates.
2. **PM-11 trust:** the exact standalone allowlist and source-digest checks are implemented and tested, but the repository has no authenticated immutable baseline tag/receipt and no external validator or CI execution receipt. The CI profile remains candidate-controlled until those external proofs exist.
3. **PM-12 platform/release:** local Unix/macOS archive structure/checksum/provenance checks pass. Windows compilation/runtime, signing, publication/install receipts, and independent release qualification are not proven. Do not treat the synthetic Windows-named archive as a Windows runtime result.

The watcher repair is deliberately conservative. It suppresses unchanged setup replay events for exact watched entries and accepts metadata errors as potentially changed. Its fingerprint is metadata-based; it does not prove that a same-size write with preserved metadata was observed.

## Safe continuation commands

```sh
# From the repository root; first inspect the current process/log.
tail -n 80 /tmp/workdeck-pm-workspace-repair-20260911.log
ps -axo pid,etime,command | rg 'workdeck-pm-workspace-repair-20260911|cargo test.*workspace'

# Only after the workspace process has terminated:
for target_dir in /tmp/workdeck-pm-workspace-repair-20260911; do
  if [ -e "$target_dir" ]; then /usr/bin/find "$target_dir" -depth -delete; fi
done
/bin/df -h /tmp | /usr/bin/tail -1

# Use a new isolated target for each later Cargo invocation.
```

Do not use `cargo clean` against the mixed checkout's default target directory, do not use a shell variable named `path` in zsh, and do not delete the retained evidence logs.
