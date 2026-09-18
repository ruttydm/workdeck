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

Lifecycle and extension-bus subscriptions are separate declarations. The lifecycle namespace is
the exact closed set published by pinned Hunk; custom subscriptions retain Hunk's open nonblank
string contract, including spaces and Unicode names.

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

## Public authoring contract

Hunk's separate `src/extension-api/types.ts` surface is pinned by
`port/hunk/oracles/extension-api-contract.json`. The oracle records the exact blobs, SHA-256
digests, byte and line counts for both source anchors, all 135 baseline exports, the ten additions
relative to the stable tag, and 17 contiguous, non-overlapping source intervals covering bytes
`0..87576`. `workdeck-extension-api` tests compare that inventory with an independent hard-coded
export set, require every evidence path to exist, and verify the `index.ts` barrel adds exactly the
four key helpers for 139 exports total.

Changeset transforms cross the subprocess boundary as `ExtensionChangeset`, never Workdeck's
private `Changeset`. Every public file carries an owned JSON `metadata` object containing only the
renderer-critical state Hunk documents as opaque; exact-source snapshots are excluded and remain
available only through the explicit workspace/document capabilities. A response may change public
facts, filter files, or reorder them. The host accepts its metadata only when it is exactly equal to
an input file's opaque object, reattaches the corresponding parsed hunks, ignores returned hunk
summaries as derived data, preserves the provider source, and rejects empty/duplicate IDs or
invented/mutated metadata while keeping the previous changeset.
