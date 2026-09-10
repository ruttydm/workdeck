# Bundled installer assets

Release packaging now includes the four current Workdeck skill sources under
`skills/<name>/SKILL.md` inside the platform archive wrapper:

- `workdeck-review`
- `workdeck-extensions`
- `workdeck-release`
- `workdeck-launch-video`

`xtask::release_entries` reads their exact repository bytes, requires every file
to exist and assigns mode 0644. Both tar.gz and ZIP writers receive these entries
alongside the executable, license inventory, SBOM and third-party notices. The
release pipeline still attaches provenance separately before writing an archive.

The `release_entries_retain_every_complete_syntax_notice` test checks skill entry
names, bytes and modes, then writes both archive formats and reads every entry
back to compare exact contents and entry counts. This is local packaging evidence,
not a signed release or platform installer smoke test.

Remaining integration: the native authenticated installer currently commits only
the executable. It must install the accompanying asset tree with appropriate
authentication and recovery, and `workdeck skill path` still materializes embedded
content under user configuration instead of resolving an installed archive tree.
Source-compatible metadata, asset-update rollback and full installer orchestration
remain open. This change does not map the baseline installer interval complete.
