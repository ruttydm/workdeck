---
name: workdeck-review
description: Inspect and annotate a live Workdeck review through its authenticated local session.
---

# Workdeck review

Use `workdeck session list --json` to discover live reviews. Select by session id or repository,
then inspect `workdeck session context --repo . --json` before navigating or adding a comment.

Read the provider-neutral model with `workdeck session review --repo . --include-patch --json`.
Navigate with `workdeck session navigate --repo . --file PATH --hunk N` or one of
`--old-line N` and `--new-line N`. Attach concise review evidence with
`workdeck session comment add --repo . --file PATH --new-line N --summary TEXT`.

Do not infer process ownership from a review session. Herder owns agents and PTYs; Workdeck owns
the review model and inline review notes.
