# Public review-skill page migration

At the pinned baseline, `website/public/docs/hunk-review-skill.md` and
`skills/hunk-review/SKILL.md` are byte-identical generated artifacts. Workdeck
keeps one authoritative native renderer in
`crates/workdeck-session/src/skill_document.rs`; it emits both
`skills/workdeck-review/SKILL.md` and `site/static/docs/workdeck-review-skill.md`.

`xtask/src/skill.rs::verify_pinned_web_review_skill` reads both exact pinned
blobs through `git show`, checks their 12,927-byte identity, and verifies that
the checked-in website artifact is the current renderer output. The generated
text uses Workdeck commands and explicitly keeps agent/TUI ownership and the
native session boundary. No JavaScript or Bun runtime is retained or executed.

This is a generated-content replacement, not a second source mirror. The
original Hunk source remains covered by the existing MIT attribution and
bundled-skill oracle; this record accounts for the duplicate public artifact
with an independently checked destination and evidence path.
