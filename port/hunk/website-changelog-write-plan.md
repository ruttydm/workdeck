# Read-only artifact write plan

`cargo xtask changelog artifacts-plan <markdown-file> <recorded-dates.json> [notes.json]`
returns a schema-1 JSON plan without writing output. It reuses artifact generation
and the collapsed-output guard. Only changed, missing or orphaned outputs appear.

`edits` maps each destination to replacement text or null for an orphan removal.
`originals` records exact original byte arrays, including malformed UTF-8, or null
for an absent file. Unchanged files are excluded. Existing changed targets must
be regular non-symlink files. The plan is deterministic on unchanged inputs.

This is preparation for an apply transaction, not an implemented writer. Apply
must regenerate and compare the plan, validate destination parents, acquire a
lock, back up originals/permissions, atomically replace files, and support rollback
before any write command is enabled. Source Hunk writes are not mapped complete.
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
Workdeck state. Applying the plan remains unimplemented.

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
Strict xtask Clippy, formatting and whitespace checks pass. Apply remains open.

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
