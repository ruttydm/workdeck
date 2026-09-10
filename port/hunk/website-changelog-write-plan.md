# Changelog artifact planning and application

## Recoverable application

`cargo xtask changelog artifacts-apply <saved-plan.json> <new-external-backup-directory> <markdown-file> <recorded-dates.json> [notes.json]`
acquires the shared Git-local release lock, regenerates the current artifact plan
and requires it to equal the saved plan before applying. Only the already
validated generated output paths can change. Originals are checked again before
each mutation; parent symlinks and nonregular targets are rejected. Creation
uses no-clobber publication, replacement uses same-directory temporary files,
and orphan deletion is limited to validated numeric release pages.

The backup parent must exist outside the repository, and the backup directory
must be new. Before output writes, `recovery.json` records the complete plan,
including original bytes or absence; `permissions.json` records original
readonly flags and Unix modes where available. Existing file permissions are
preserved. A handled failure rolls back completed writes in reverse order,
preserving and reporting any intervening content changes. Backups remain on
success or failure. Newly created parent directories may remain after rollback.

Application is not crash-atomic across files, does not isolate unrelated editors,
and does not close check-to-replacement filesystem races. After interruption,
review current bytes against the saved plan before manually restoring originals
and permissions. Windows reparse-point behavior is not validated. No social-card
image generation, Zola build, site publication or full source mapping is claimed.

Tests cover creation/replacement/removal, injected failure after each write and
rollback, actual CLI generation/application, retained recovery bytes, and stale
repeat rejection before a second backup is created.
The two application tests and all 23 changelog CLI tests pass; strict xtask
all-target Clippy passes. The concurrent-editor regression verifies that rollback
reports the conflict and retains both the edited target and original recovery bytes.
Additional adversarial-path coverage brings the application suite to four passing
tests: a symlinked site parent cannot redirect writes outside the repository,
stale original bytes cause rejection before backup creation, and a backup inside
the repository is refused without modifying targets. These tests run on macOS;
they do not establish Windows reparse-point behavior or race-free path traversal.

## Read-only planning and historical checkpoints

`cargo xtask changelog artifacts-plan <markdown-file> <recorded-dates.json> [notes.json]`
returns a schema-1 JSON plan without writing output. It reuses artifact generation
and the collapsed-output guard. Only changed, missing or orphaned outputs appear.

`edits` maps each destination to replacement text or null for an orphan removal.
`originals` records exact original byte arrays, including malformed UTF-8, or null
for an absent file. Unchanged files are excluded. Existing changed targets must
be regular non-symlink files. The plan is deterministic on unchanged inputs.

Planning itself does not write outputs; explicit application is described above.
Source Hunk writes are not mapped complete.
No image validity or missing-image gate is implied by generating this plan.

The native test checks exact binary original bytes, orphan removals, missing-file
edits, exclusion of unchanged files, repeated-plan identity and no output writes.

The plan test and all 20 existing changelog CLI tests pass. Strict xtask Clippy,
formatting and whitespace checks pass.

## CLI plan coverage

The real CLI test starts with no generated files and verifies six creations with
null originals and no site directory created. A repeated invocation returns
identical bytes. After the test installs those files, the next plan has no edits.
Changing latest.json to invalid UTF-8 produces one replacement with the exact
original byte array; the command leaves those bytes untouched and creates no
Workdeck state. Application was not implemented at that historical checkpoint.

All 21 changelog CLI tests pass. Strict xtask Clippy, formatting and whitespace
checks pass. No new source-ledger mapping is claimed.

Plans now use a typed schema that rejects unknown fields, unsupported versions,
mismatched edit/original key sets and unchanged edits. Destinations are limited to
the five fixed generated outputs and numeric minor-series Markdown paths. Only
an existing numeric series page can be proposed for removal. Unexpected Markdown
names cause planning to fail rather than proposing their deletion; this is a
deliberate preservation boundary for the future writer, not source-equivalence
evidence for arbitrary orphan filenames. The checker still reports such orphans.

Typed-plan validation, byte-preserving plan construction and plan CLI tests pass.
Strict xtask Clippy, formatting and whitespace checks passed at that checkpoint.

## Saved-plan checking

`cargo xtask changelog artifacts-plan-check <saved-plan.json> <markdown-file> <recorded-dates.json> [notes.json]`
validates the typed saved plan and compares it with a newly generated plan using
the current inputs, tag dates and destination bytes. Success is silent. Edited
replacement content, changed output-affecting dates, changed destination bytes,
and unknown plan fields fail without modifying the plan or generated files.
This checks the resulting plan, not an input-file hash: input changes that leave
the resulting plan identical are permitted. It does not reserve files against
later changes, validate destination parents for writing, or authorize an apply.

The CLI regression checks these rejection paths and preservation of destination
and saved-plan bytes, with no `.agents` state creation. The pending implementation
initially lacked the `anyhow::Context` import; compilation exposed this and the
import is now explicit. No additional Hunk source interval is claimed complete.
