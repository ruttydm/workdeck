# Recursive watcher fixture setup race

The full workspace/all-target test run at `dfdb8686` exited 101 in
`watch_observer::tests::recursive_target_ignores_excluded_metadata_churn`.
The VCS suite reported 241 passes and one failure; the complete workspace gate
did not pass. The 98 terminal pager tests and 1,255 TUI tests passed in that run.

An isolated rerun passed, but repetition with temporary test-only event logging
reproduced the failure immediately. The accepted event was `Create(Folder)` for
the temporary watched root itself, followed by extended metadata for that root.
It was not an event beneath the excluded `.git` directory. The diagnostic
logging was removed after capture; production filtering is unchanged.

The regression now drains setup events until a 250 ms quiet interval before
writing `.git/index`, bounded by a three-second deadline. It fails on a
disconnected observer or setup that never settles. After the excluded write,
the original no-event assertion remains unchanged; no post-write event is
discarded. All 18 observer tests passed in 0.56 seconds after this correction.
Thirty consecutive isolated repetitions then passed using the compiled native
test executable. Formatting and diff checks passed.

This corrects native fixture timing, not source parity. No ledger interval is
newly mapped, and a fresh complete workspace run is still required.
