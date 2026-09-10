# Reply anchor inheritance

Replies now inherit the parent's complete `CommentAnchor` and resolution rather
than replacing a range anchor with a newly calculated single-line anchor. Native
composer targeting also accepts retained stale notes, while excluding orphaned
notes. This follows the pinned semantic intent's non-orphaned reply eligibility
and parent anchor/resolution inheritance.

`source_controller::tests::reply_inherits_parent_range_anchor_and_resolution`
creates a parent with a new-side range of lines 5–7 and stale resolution, opens
and saves a reply, and asserts exact anchor equality, parent identity and
inherited resolution. Its first run exposed the active-only targeting filter:
the reply composer was not opened for a stale parent.

This is runtime integration evidence, not a new source-test mapping. Retained
notes whose former hunk disappears and sidecar-only parents still require their
own end-to-end checks. No ledger disposition changes.
All 1,241 TUI library tests passed in 8.81 seconds after the changes; formatting
and diff checks passed.
