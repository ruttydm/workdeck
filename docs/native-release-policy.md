# Native release-channel and version policy

These Rust tools validate release inputs and emit metadata. They do not fetch a latest version,
create a Git tag, commit changes, upload assets, or publish packages or releases.

```console
cargo xtask release channel --event push --ref v0.19.0 --current-latest 0.18.2
cargo xtask release channel --event workflow_dispatch --ref main --requested-tag beta
cargo xtask release check-version v0.1.0
```

The channel command prints one JSON object with `channel` and `makeLatest`. A push of a newer
stable version selects `latest` and permits latest-release promotion. An older stable version
selects `backport-MAJOR.MINOR` without promotion. Republishing the current latest version fails.
An alpha, beta, or release-candidate reference selects `beta` without promotion. A manual dispatch
requires an explicit nonblank channel; only the exact trimmed value `latest` permits promotion.
The caller supplies the current latest version from its verified release source; these commands
do not query an npm registry or assume authority to publish.

Stable parsing retains the pinned source's optional leading `v`, three numeric components,
leading-zero acceptance, and numeric comparison behavior. Prerelease matching retains its source
pattern rather than imposing an additional semantic-version validator. Argument parsing accepts
pairs, retains unknown pairs, and uses the last duplicate value, matching the source script.
Missing pairs and unsupported events fail. Native metadata calls the field `channel` instead of
the source's npm-specific field. Workdeck has no npm publication workflow.

The version command reads the sole executable package, `workdeck-cli`, through Cargo metadata
and requires the exact `v<package-version>` tag, including a prerelease suffix when present. It
prints a confirmation on success and fails on a missing, unprefixed, or mismatched tag. The sample
version above must be replaced with the actual package version when preparing a later release.

Evidence is in [the dual-pin release-channel oracle](../port/hunk/oracles/release-channel.json)
and the Rust tests in `xtask/src/release_channel.rs`: all seven source tests are translated, with
18 frozen differential cases plus argument and Cargo-version checks. Both pinned source suites
passed seven tests and ten assertions. Attribution is retained in
[THIRD_PARTY_NOTICES](../THIRD_PARTY_NOTICES).

These helpers do not complete native release preparation, installation tests, artifact signing,
cross-platform CI, or the remaining release gates. See [the semantic-port ledger](../port/hunk/README.md).
