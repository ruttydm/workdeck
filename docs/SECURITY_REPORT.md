# Workdeck Security Report

## Trust model

Repositories, Git output, GitHub payloads, ZIP entries, HTML, Markdown, source, logs, refs, paths, and stored presentation data are untrusted. Workdeck-owned mutable state is confined to its central application directory or an explicit `WORKDECK_DATA_DIR`.

## Repository boundary

Workdeck performs read-only discovery, status, history, refs, stashes, diffs, and analysis. It exposes no mutation controls. Tests that stage or commit use isolated temporary repositories. Input repositories receive no Workdeck files and no optional Git locks during state-verification probes.

## Provider boundary

GitHub authentication stays in the official `gh` CLI. Workdeck stores no token. Provider commands use fixed executables, validated arguments, bounded output, sanitised errors, timeouts, cancellation, and no mutation endpoints. External navigation is restricted to validated HTTPS GitHub links.

## Artifact boundary

ZIP import rejects absolute paths, traversal, symlinks, duplicate and normalized-name collisions, file/directory collisions, excessive file count, excessive expanded bytes, suspicious ratios, and unsupported records. Imported bytes are copied into Workdeck-owned storage.

Preview servers bind only `127.0.0.1`, use an unguessable session path, exact-origin URLs, strict response headers and CSP, bounded requests, and no application-runtime bridge. Frames are sandboxed without same-origin or top-navigation privileges. `file:` and external network navigation are denied. Drop/cancel/close joins the helper.

## Renderer boundary

`workdeck-ui` depends only on serializable `workdeck-api` data. It has no filesystem, SQLite, Git, provider, process, or network dependency. Markdown is parsed into safe RSX nodes; untrusted HTML is not injected. No `eval` or `use_eval` occurs in product source.

## Supply chain

Cargo is locked; git dependencies are pinned. Cargo-deny evaluates advisories, licenses, bans, and sources. Release metadata includes CycloneDX SBOM, dependency notices, provenance, and complete collected license texts. Waku GPL is behavior-only inspiration and is excluded from source/assets/dependencies.

## Remaining external release work

The local bundle is ad-hoc signed. Public distribution requires Developer ID Application credentials, hardened-runtime/entitlement review, notarization, stapling, Gatekeeper validation on a clean Mac, and final legal review of the generated notices.
