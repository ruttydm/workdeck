# Native extension type boundary

Pinned Hunk kept the public authoring declarations and host-owned registry types together in
`src/extensions/types.ts`. Workdeck keeps the same ownership split without importing JavaScript
runtime architecture into Rust:

- `workdeck-extension-api` owns versioned, serializable author-facing declarations, invocation
  snapshots, registrations, notifications, VCS values, and declarative host actions;
- `workdeck-extension-host` owns discovery candidates and origins, loaded metadata, atomic
  registration validation, runtime authority, shared notification and log hubs, load state,
  failures, and bounded retirement;
- `workdeck-tui` owns the committed pane/command/mode/file-view/highlighter registries and dispatches
  lifecycle and custom events only after the review context exists.

Native manifests replace inferred source-file identity for execution. The legacy
`derive_extension_id` helper remains for migration and inventory: `foo.ts` and `foo/index.ts` map to
`foo`, including Node path's observable bare `index.ts` result. A loaded process retains its
canonical manifest path and exact `bundled`, `flag`, `config`, `global`, or `repo` provenance.

One load result retains the complete candidate/config snapshot, successful processes, attributed
issues, exact cwd, pending trust root, notification hub, ordered stderr log hub, and authority
control. Disabled or candidate-free startup constructs that state without touching disk. It begins
in `loading`; successful execution transitions to `ready`, retirement first revokes it to `closing`,
and teardown seals it `closed`.

`port/hunk/oracles/extension-types.json` records both pinned source blobs, every empty-registry and
lifecycle-event key, path-derived identities, default notification behavior, and empty-result shape.
The baseline is authoritative where the older stable tag lacks later CLI, matcher, and lifecycle
additions; stable-only fixes are applied separately and do not regress those baseline capabilities.
