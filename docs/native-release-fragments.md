# Native release-note fragments

User-visible changes should carry a fragment rather than hand-editing the
generated release history. Author a fragment with Rust tooling:

```console
cargo xtask changelog add fix-unicode patch "Fix Unicode selection rendering."
cargo xtask changelog add review-command minor "Add a review command."
cargo xtask changelog add protocol-change major "Change the extension protocol."
cargo xtask changelog add maintenance-cleanup empty
```

Use `patch` for fixes or small behavior changes, `minor` for new user-facing
features, and `major` for breaking changes. Maintenance-only fragments are
empty and carry no release-note text. IDs use lowercase letters, digits and
hyphens; each identifies one `changes/<id>.md` file. Quote multi-word text as
one argument. Existing fragments are never overwritten, and invalid requests
do not create the directory. Writes use a same-directory temporary file and
atomic no-clobber publication.

Inspect pending fragments without creating state:

```console
cargo xtask changelog status
```

The JSON result lists fragments in ID order and reports the highest requested
bump (`major`, then `minor`, then `patch`, or null for maintenance-only/no
fragments). Malformed frontmatter, unsupported products/bumps and missing
user-visible text fail validation. Reading does not create `changes/` when it
is absent and does not consume fragments or change versions.

`cargo xtask changelog plan` combines pending fragments with the actual
`workdeck-cli` workspace version, read through locked, offline Cargo metadata.
It reports `current`, `next`, the requested bump, fragments and `applied: false`.
Stable patch/minor/major increments reset lower version components as needed;
maintenance-only plans retain the current version. Overflow is rejected.
Prerelease/build-metadata versions are explicitly rejected until the separate
prerelease workflow is integrated. No version, lockfile or fragment is changed.

For a version bump, `edits` contains the proposed full CLI manifest and
lockfile text. TOML editing preserves manifest comments and unrelated package
entries. The current CLI version must be explicit and match metadata; exactly
one local CLI lockfile entry must exist. Inherited CLI versions and
version-qualified CLI dependency references are rejected until coordinated
editing is implemented. These are proposed contents only, not writes; a
maintenance-only plan has no version edits.

Version-bumping plans also propose `CHANGELOG.md`: new notes are inserted
after an existing top-level heading, or prepended to headingless history.
Existing history is retained verbatim; an absent file gets a Changelog heading.
The history is fingerprinted alongside other inputs, with `absent` explicitly
recording a missing file, so creating history invalidates a previously saved
plan. Planning neither creates nor rewrites this file.

Apply a reviewed saved plan explicitly with:

```console
cargo xtask changelog apply-plan release-plan.json /outside/repository/new-backup-directory
```

The backup directory must not exist and its parent must be outside the
repository. Application takes an advisory Git-local release lock, verifies the
complete saved plan, backs up all existing targets under `originals/`, and
records originally absent paths plus the plan in `recovery.json`. A second
plan check precedes mutation. Version and history files use same-directory
replacement; consumed fragments remain recoverable in the backup directory.
Handled write failures trigger reverse-order rollback; backups remain after
success and failure. Retrying an already-applied plan fails stale validation.

Each target is checked against its original bytes immediately before mutation,
and the final targets are checked against the plan. Rollback checks for the
bytes written by this operation before restoring or removing a file. Detected
conflicts are preserved and reported with the retained backup location.
Regression tests inject edits to both a pending fragment and an already-written
changelog, and verify that neither concurrent edit is overwritten. These checks
do not close filesystem races between checking and replacement.

This is not a crash-atomic multi-file transaction. After process interruption,
restore files from `originals/` and remove only paths listed as originally
absent after reviewing intervening edits. The advisory lock coordinates this
command, not editors or other tools; do not edit release inputs during apply.
Automatic crash recovery and isolation against unrelated concurrent writers
remain incomplete. No commit, tag, push or publication is performed.

The integration test applies the proposed manifest/lockfile pair only inside
its disposable fixture, runs locked/offline Cargo checking, and confirms that
Cargo reports the proposed version without rewriting either file. Production
plan commands still perform no apply operation. This validates the simple
local-package case, not every workspace dependency/version arrangement.

The `inputs` object fingerprints the root manifest, CLI manifest, lockfile and
pending fragments with SHA-256 and repository-relative paths. Duplicate paths
are collapsed and nonregular files are rejected. The CLI test compares hashes
against the actual input bytes. These fingerprints detect stale plans but
are not an atomic filesystem snapshot or protection against concurrent editors.

`cargo xtask changelog check-plan <saved-plan.json>` regenerates the current
plan and compares the complete JSON value with a saved plan. It returns
`valid: true, applied: false` only when they match. Changed fragment text or
new fragments invalidate the saved plan; neither checking nor failure changes
the saved plan or repository inputs. This detects drift between inspections,
but checking alone does not lock inputs against subsequent edits.

Before returning a plan, fragment parsing and input hashes are checked again.
Regression tests inject a semantic edit between parsing and fingerprinting,
and a whitespace-only edit after fingerprinting; both are rejected. This
narrows the consistency window but is not an atomic filesystem snapshot or
an apply-time lock. The CLI manifest is first discovered, then authoritative
version metadata is read between the initial and final fingerprints; a changed
manifest location also fails planning. Tests independently change the root
manifest, CLI manifest and lockfile and require rejection. Adversarial ABA
edits and modifications after the final check remain outside these guarantees.

The plan's `notes` field previews Markdown grouped into Major, Minor and Patch
Changes, with fragment-ID ordering within each group. Multiline text stays
inside its list item; maintenance-only fragments produce no release-note
entry, and an entirely maintenance-only plan has empty notes. GitHub PR/commit
annotations and exact upstream changelog formatting remain unported.

Bump precedence is tested across every patch/minor/major combination in both
orders. Unix tests also verify that directory and fragment symlinks are not
followed and that an existing symlink target cannot be overwritten. These
checks are not a claim of adversarial filesystem race isolation or native
Windows symlink validation.

The CLI integration test runs the actual tooling executable inside a temporary
Git repository: empty status, fragment creation, status JSON, overwrite and
extra-argument rejection, unchanged fragment bytes, and command help are
checked without creating fragments in the project checkout.

A second CLI integration case creates an isolated Cargo workspace at version
`1.2.3`, authors minor and patch fragments, and verifies a `1.3.0` proposal
with exact grouped notes. It compares manifest, lockfile and fragment bytes
before and after planning to prove that the tested path is read-only.

Hand-authored fragments use the same ID rules as the authoring command. NUL
bytes and duplicate frontmatter keys are rejected rather than silently
reinterpreted. CRLF fragments are accepted and normalized for parsing without
rewriting the source file. Regression tests preserve rejected files unchanged.

The frontmatter targets the sole `workdeck` product. This is not an npm
workspace or a claim that individual internal Rust crates are separately
published. Fragment creation does not commit, push, bump versions, consume
fragments, publish artifacts or update Homebrew.

Release preparation, fragment consumption, prerelease policy and automatic
version/changelog updates remain unfinished. Do not treat this authoring
command as a complete release pipeline. Existing pinned Hunk fragment history
remains separate and can be checked with
`cargo xtask changelog upstream-history --check`.

This workflow is a partial MIT-licensed adaptation of Hunk's
`.changeset/README.md`; source attribution is retained in THIRD_PARTY_NOTICES.
