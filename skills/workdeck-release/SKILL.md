---
name: workdeck-release
description: Prepare, benchmark, validate, tag, publish, verify, document, or recover a Workdeck release across GitHub artifacts, Cargo, Homebrew, Nix, direct installers, the Zola site, and terminal media.
---

# Workdeck release workflow

This is a maintainer-only, source-checkout workflow. A release ships exactly one product
executable, `workdeck`, in five native archives. Each archive also carries license notices, the
dependency inventory, and a CycloneDX SBOM; adjacent checksums and GitHub provenance attestations
bind the published artifacts to the tagged source.

Strict semantic-port completion is an indivisible release prerequisite. Passing ordinary Rust
tests is not enough while the ledger, stable fixes, or upstream-delta queue is incomplete.

## Safety

- Ask when the version, release branch, previous tag, target channel, or source remote is ambiguous.
- Get explicit confirmation before pushing a tag, triggering publication, editing a public release,
  changing a distribution pointer, or uploading media.
- Never move or reuse a published version or tag.
- Never waive the benchmark thresholds without an approved, recorded reason and reproducible raw
  results.
- Never retry a partial publication until every archive, checksum, attestation, release field, and
  downstream channel has been inventoried.
- Do not publish from a dirty checkout, an unreviewed commit, or a commit different from the one
  whose native and installer evidence passed.

## 1. Confirm the release

Record all of these before changing metadata:

- semantic version and annotated tag (`vX.Y.Z`);
- release branch and exact reviewed commit;
- actual previous tag on that branch;
- stable, prerelease, or older-series backport channel;
- expected GitHub Latest behavior and distribution channels;
- benchmark baseline and the same-host runner identity;
- five target triples: macOS arm64/x64, Linux arm64/x64, and Windows x64.

A newer stable release may advance GitHub Latest and stable Homebrew, Nix, Cargo-source, curl,
PowerShell, and direct-download surfaces. A prerelease must remain marked prerelease and must not
advance stable pointers. An older-series backport stays on its maintenance branch and must not move
either the latest stable or prerelease series.

For a backport, derive the release content only from the actual previous-tag-to-tip comparison on
that maintenance branch. Never pull unrelated newer-series notes into it.

## 2. Prepare

Start from a clean, current, reviewed branch:

```sh
git status --short --branch
git fetch origin --tags --prune
git tag --sort=-version:refname | head -10
git log -1 --format='%H %s'
```

Inspect every version-bearing Cargo manifest and lockfile. Keep workspace crates on the intended
coherent version and regenerate `Cargo.lock` through Cargo. Release-note and site outputs must come
from their checked-in Rust/Zola generators; if a required generator does not exist or its output is
not reproducible, the release is blocked rather than hand-edited around the gap.

Refresh the upstream semantic source and demand strict completion:

```sh
cargo xtask port fetch
cargo xtask port audit
cargo xtask port status
```

The audit must report exactly 1,257 baseline files, complete non-overlapping byte coverage, no
unmapped records, valid destinations/evidence, all five stable-only commits, and zero upstream
delta commits. `--allow-incomplete` is never valid in a release workflow.

Run the complete local gate from the same reviewed commit:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo deny check
cargo audit
cargo xtask verify
cargo xtask architecture check
cargo xtask nix check
cargo xtask skill check
cargo xtask site check
```

`cargo xtask verify` is an aggregator, not permission to omit the explicit release gates. Verify
the command log identifies the exact source commit and toolchain. A skipped platform, installer,
website, dependency, or semantic-port check is not a pass.

Generate and compare the release benchmark on the same otherwise-idle host. Store machine identity,
toolchain, source commits, raw samples, medians, and peak memory for launch, reload, navigation, and
rendering. Compare the candidate against both the pinned semantic source runtime and the previous
Workdeck release. The candidate fails if any latency is more than 10% slower or peak memory is
higher. Commit the reproducible benchmark result with the release metadata; never replace a failed
sample set with a favorable subset.

Before tagging, require native CI and installed-artifact smoke evidence for all five target triples.
Each job must build `workdeck-cli --bin workdeck`, package that exact binary, inspect the archive,
verify its checksum, install it through the target channel under test, and run at least
`workdeck --version` and `workdeck --help`. Linux virtualization evidence supplements rather than
replaces the native macOS and Windows jobs. Keep the result bundle tied to the reviewed commit and
reject stale output directories.

Build and inspect a representative local archive:

```sh
cargo build --locked --release --package workdeck-cli --bin workdeck
cargo xtask licenses --output target/release-licenses.json
cargo xtask release package \
  --target "$(rustc -vV | sed -n 's/^host: //p')" \
  --binary target/release/workdeck \
  --output dist
```

Every platform archive must contain only the release root with:

- `workdeck` or `workdeck.exe` as the sole executable;
- `LICENSE` and `THIRD_PARTY_NOTICES`;
- `licenses.json` and `sbom.cdx.json`;
- every retained grammar, theme, algorithm, asset, and external-tool notice required by the
  packaged product.

The checksum file, signature, and provenance statement stay adjacent to the immutable archive.
Verify that no second product executable or undeclared runtime tree entered the package.

Read the actual previous-tag comparison and draft release notes from user-visible behavior, not
commit-title transcription. Ensure compatibility changes, migrations, update behavior, extension
API changes, contributors, and benchmark results are present. Run Zola link, accessibility, and
visual checks after generating the release page. A stable release page must identify all supported
install/update routes; prerelease and backport pages must preserve stable defaults.

Commit generated metadata, changelog/site output, benchmark evidence, and any version changes under
normal review policy. Push that reviewed tip and wait for required branch CI only after receiving
the authority to push commits. Tagging must use that exact tip.

## 3. Tag and publish

Immediately before tagging, fail unless the reviewed branch is clean and equals its remote:

```sh
set -euo pipefail
branch=$(git branch --show-current)
test -n "$branch"
git fetch origin "$branch" --tags
test "$(git rev-parse HEAD)" = "$(git rev-parse "origin/$branch")"
test -z "$(git status --porcelain --untracked-files=all)"
git log -1 --format='%H %s'
```

Present the version, tag, commit, previous tag, channel, strict audit summary, benchmark comparison,
native/install matrix, archive manifest, site validation, and final highlights. Only after explicit
confirmation:

```sh
version=X.Y.Z
tag="v$version"
git tag -a "$tag" -m "$tag"
git push origin "$tag"
```

The tag starts `.github/workflows/release.yml`. Select only the workflow run for the dereferenced
tag commit and propagate its failure:

```sh
release_sha=$(git rev-parse "$tag^{}")
run_id=$(
  gh run list \
    --workflow release.yml \
    --event push \
    --commit "$release_sha" \
    --limit 1 \
    --json databaseId \
    --jq '.[0].databaseId // empty'
)
test -n "$run_id"
gh run view "$run_id" --json url,status,conclusion
gh run watch "$run_id" --exit-status
```

If GitHub has not registered the run, repeat the lookup later. Never select a nearby branch or tag
run. Success requires semantic-port preflight, all five builds, packaging, checksums, signatures,
SBOMs, provenance attestations, artifact uploads, and creation of the correctly classified GitHub
release.

## 4. Verify publication

Inspect the release and its expected artifact set:

```sh
gh release view "$tag" --json tagName,name,isDraft,isPrerelease,url,assets,body
```

Expect one archive and one checksum for each target:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-unknown-linux-gnu`
- `x86_64-unknown-linux-gnu`
- `x86_64-pc-windows-msvc`

Download the release into a new temporary directory, never a reusable evidence directory. Verify
every checksum, detached signature, and provenance attestation against the repository and tag;
then inspect every archive entry and run the host-compatible binary from the extracted archive.
Confirm the executable reports the intended version and that all required notices match the tagged
source byte-for-byte.

Stop if the version, channel, tag commit, asset count, file name, checksum, signature, attestation,
SBOM, provenance subject, archive contents, or smoke output disagrees. A green workflow does not
override contradictory downloaded evidence.

After publication is verified, regenerate the Zola release/changelog surfaces so the tag date,
exact anchor, feed item, social card, stable install instruction, and release ribbon reflect the
published state. Generator-owned dates must remain stable on subsequent runs. Commit only the
expected generated site/media metadata and run `cargo xtask site check` plus link, accessibility,
and visual checks again. Backports and prereleases receive their own pages without advancing stable
defaults.

## 5. Add the release video and final notes

Only after artifact publication verifies, create a detached worktree at the immutable tag:

```sh
git worktree add --detach "../workdeck-release-video-$version" "$tag"
```

In that worktree, follow `skills/workdeck-launch-video/SKILL.md`. It owns real-PTY capture,
WebDriver composition, encoding, frame inspection, duration checks, and keeping generated media out
of Git. Use four to six user-visible headlines derived from the released notes. Preserve storyboard
source edits only with separate approval.

Host the approved video where the Zola site can serve it without committing the encoded master,
then record its URL and metadata through the canonical release generator. Present both the local
video and final public-release body for explicit confirmation before upload or edit.

Draft final notes from the released diff and changelog:

- Open with one short paragraph stating the product theme and user impact.
- Group a small number of meaningful changes under descriptive headings and link defining pull
  requests. Explain what users can now do.
- Include exact upgrade/install instructions for the channels actually published. Never advertise
  an unavailable Cargo source, Homebrew formula, Nix input, curl script, PowerShell script, or
  direct archive.
- Add a clearly labelled compatibility section for runtime requirements, CLI interpretation,
  configuration or state migration, extension API versions, and upgrade risks.
- Add a Community contributors section naming every external contributor, linking each relevant
  pull request, and describing the contribution.
- Preserve the complete merged-pull-request inventory in a collapsed details block. Derive it from
  the actual previous-tag comparison; maintenance work belongs there even when it is not a
  headline.
- Link the exact previous/new tag comparison and the release's Workdeck site page.

Use this shape:

````md
## <release name or product theme>

<one short release summary>

```sh
workdeck update <version>
# Add only verified first-install commands for this release.
```

https://github.com/user-attachments/assets/<video-id>

### <user-facing theme>

<what changed, why it matters, and links to the defining pull requests>

### Compatibility notes

- <upgrade requirement or changed behavior>

### Community contributors

- [@contributor](https://github.com/contributor) <concise contribution>. [#123](PR URL)

<details>
<summary>All merged pull requests</summary>

- <concise pull request title> by @author in [#123](PR URL)

</details>

**Release notes**: <published Workdeck release page>
**Full changelog**: https://github.com/ruttydm/workdeck/compare/<previous-tag>...<new-tag>
````

After confirmation, attach the H.264 MP4 in GitHub's release editor, replace the placeholder with
the generated user-attachment URL, and apply the reviewed body:

```sh
gh release edit "$tag" --notes-file /tmp/workdeck-release-notes.md
```

Open the public release in a browser and verify inline playback, final prose, links, channel labels,
and the unchanged immutable archives and attestations.

## 6. Distribution channels

Only a stable release that advances GitHub Latest may advance stable distribution pointers.

- Verify Cargo-source installation from the exact tag and `Cargo.lock`; publish a registry crate
  only if that channel is configured and separately authorized.
- Let the Homebrew automation update the stable formula when available. Create a manual formula
  change only when its maintainers require it or automation stalls, and test `brew install` plus
  `workdeck update` behavior.
- Update Nix inputs/packaging without rewriting consumer locks. Run `cargo xtask nix check` and an
  installed `nix run ... -- --version` smoke from fresh registry data.
- Verify the curl and PowerShell installers fetch the new target archive, enforce its checksum and
  signature, install only `workdeck`, preserve user configuration, and report the installation
  source used by `workdeck update`.
- Verify direct GitHub installs on each supported target and confirm update replacement is atomic.

Do not claim a channel merely because its source file exists. Confirm public availability and a
fresh install. Prereleases and older-series backports do not advance stable Homebrew, Nix, installer,
or update defaults.

## Failure invariants

- Before tag push: fix the issue, regenerate, rerun all affected and downstream gates, present the
  new evidence, and request confirmation again.
- After any public artifact exists: keep the version and tag immutable; inventory every asset and
  channel before choosing a recovery action.
- A missing target archive, checksum, signature, SBOM, provenance statement, or incorrect release
  classification makes the publication incomplete. Repair only from the same tagged source.
- Keep a verified software release intact when video, site follow-up, or editorial notes fail;
  retry only the separately approved media/site/edit step.
- Never delete a valid public release to hide partial state. Record the incident and reconcile every
  affected distribution surface.
