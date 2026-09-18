# Release-page install selection

The native `install_target_tracks_published_stable_not_newest_heading` regression
exercises a series containing an unpublished stable heading, a dated beta and an
older dated stable release. It verifies that only the dated stable release is
offered as a pinned Cargo install, for absent, current and superseding latest
release settings. Update guidance appears only when the selected release is the
current release. Every heading retains its stable anchor. With no publication
dates, no install or update command is offered.

This supplements the pinned Hunk `series page` tests in
`scripts/generate-changelog.test.ts` (baseline interval 8520–13066). It does not
complete that interval: the current-release installer guidance still differs.
The baseline uses its default curl installer for current releases and pinned npm
for superseded releases. Workdeck requires native installation paths and no npm;
the current renderer uses pinned Cargo instructions in both cases. The native
installer remains preflight-only, so adding a claimed working curl installation
URL would not establish parity. Implement and verify that workflow before
completing the current-release guidance and its executable tests.

The targeted native regression passes. No ledger mapping is changed.
