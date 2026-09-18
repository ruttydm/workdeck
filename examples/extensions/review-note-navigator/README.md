# Review note navigator extension

This trusted native Rust extension lists every note currently saved in Workdeck's shared review state, then navigates to the selected visible note's authoritative source anchor. A snapshot recovers notes that existed before the extension started and complements live selection and note events.

Stage and run it from this checkout:

```bash
cargo xtask extension stage-example review-note-navigator
cargo run -p workdeck-cli -- --extension ./target/workdeck-extension-examples/review-note-navigator diff
```

Save one or more review notes, then choose **Extensions → Navigate saved review note…** or press `F8`. Each choice includes its reconciliation status, file, preferred line, side, and summary.

- Active notes reveal their current source line.
- Stale notes reveal the last authoritative anchor Workdeck retained.
- Orphaned notes remain in the inventory but produce a warning because they have no current review location.
- A note whose file is hidden by the current review filter remains in the inventory, while guarded navigation refuses the hidden target with a warning.
- Drafts and static sidecar annotations that never entered shared review state are absent.

The command captures the complete note inventory before opening its host-rendered selector. After selection, it reads the authoritative snapshot again and resolves the note by stable id, so a concurrent edit cannot navigate with an obsolete anchor. A reload cancels the dialog and retires the command's review controls.
