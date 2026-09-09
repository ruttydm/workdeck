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

The `inputs` object fingerprints the root manifest, CLI manifest, lockfile and
pending fragments with SHA-256 and repository-relative paths. Duplicate paths
are collapsed and nonregular files are rejected. The CLI test compares hashes
against the actual input bytes. These fingerprints are groundwork for future
stale-plan detection, not a filesystem snapshot, lock or implemented apply
transaction; concurrent-edit consistency still needs that integration.

`cargo xtask changelog check-plan <saved-plan.json>` regenerates the current
plan and compares the complete JSON value with a saved plan. It returns
`valid: true, applied: false` only when they match. Changed fragment text or
new fragments invalidate the saved plan; neither checking nor failure changes
the saved plan or repository inputs. This detects drift between inspections,
but does not provide locking against edits during a future apply operation.

Before returning a plan, fragment parsing and input hashes are checked again.
Regression tests inject a semantic edit between parsing and fingerprinting,
and a whitespace-only edit after fingerprinting; both are rejected. This
narrows the consistency window but is not an atomic filesystem snapshot or
an apply-time lock. Cargo-metadata/read races and adversarial ABA edits remain
outside this check's guarantees.

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
