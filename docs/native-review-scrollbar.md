# Native review scrollbar

Workdeck's review canvas owns a one-cell, auto-hiding vertical scrollbar. It is painted directly
into Ratatui's cell buffer after the review stream, so its track and thumb remain the topmost review
geometry without introducing a second renderer.

The controller preserves Hunk's behavior:

- the scrollbar is initially hidden and appears only after an overflowing viewport actually moves
  or its track/thumb is used;
- every activity starts a complete two-second hide window, identified by a monotonically increasing
  generation so a canceled callback cannot hide a newer reveal;
- expiry during a drag leaves the thumb visible, and release starts a fresh hide window;
- the thumb height is `max(2, floor(viewport * viewport / content))`, and its top is the floored
  fraction of the available track range;
- left-button track clicks move exactly one viewport; thumb drags use Hunk's track-to-content ratio,
  clamp to the available content, and round at the native scrollbox boundary;
- the border color paints the track, `accentMuted` paints the idle thumb, and `accent` paints the
  captured drag thumb.

The first measured scroll position establishes a baseline instead of pretending that a retained or
selection-aligned position was user input. Later keyboard, wheel, navigation, and programmatic
viewport changes are observed through the same scroll owner. Mouse capture keeps an active thumb
drag alive outside the track, while overlay hit testing prevents review rows underneath the visible
scrollbar from receiving its pointer sequence.

Rust uses the application's existing 100 ms terminal tick for expiry rather than starting a timer
thread. This gives the same visible deadline semantics, makes cleanup immediate when the review app
is dropped, and lets tests deliver stale generations deterministically. Viewing and interacting with
the scrollbar does not read or write repository state.
