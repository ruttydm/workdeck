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
