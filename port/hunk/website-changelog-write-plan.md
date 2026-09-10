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
formatting and whitespace checks pass. Dedicated plan CLI coverage remains open.
