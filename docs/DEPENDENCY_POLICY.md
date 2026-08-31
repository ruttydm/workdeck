# Workdeck Dependency Policy

Workdeck is intended for commercial distribution. Every shipping dependency must have a known license, a lockfile identity, and a documented reason to exist.

## Rules

- Workspace crates are unpublished (`publish = false`) and MIT licensed.
- Registry and Git dependencies are locked. The only approved Git organization is DioxusLabs for the pinned Dioxus Components contract.
- Permissive alternatives in dual-license expressions are selected.
- GPL-only and AGPL-only code is prohibited. Waku remains behavior-only inspiration.
- MPL-2.0 packages are accepted only when enumerated in `docs/MPL_ALLOWLIST.txt`; Workdeck modifies no upstream MPL source file without recording and shipping that file's source.
- GPUI, Zed, and Longbridge dependencies are prohibited after cutover.
- No hand-authored JavaScript or TypeScript may ship. Dioxus-generated bootstrap/runtime output is allowed; Playwright and Axe remain development-only.
- No dependency may perform repository mutation. Repository access is centralized behind `workdeck-git` read-only command and libgit2 policies.
- Tree-sitter grammar crates are parser/query inputs confined to `workdeck-analysis`; they never enter the renderer boundary or execute repository code. New grammars require a maintained highlight query, permissive license, polyglot fixture, WASM check, and `cargo-deny` review.
- `cargo-deny`, the generated CycloneDX SBOM, copied-source provenance, and bundled license texts are release gates.

## Pinned product stack

- Dioxus `0.7.1`
- Dioxus Desktop `0.7.10`
  - Its `devtools` compile feature remains enabled as a release-build workaround for the upstream unconditional menu-handler calls to Wry's devtools methods. Workdeck exposes no developer-tools menu item; remove the feature when the pinned upstream line fixes that build defect.
- Dioxus CLI `0.7.9`
- Dioxus Components `bf007c15d0cf4d04d3181cc46cf12325aa773955`
- dioxus-free-icons `0.10.0`, Lucide feature only
- rfd `0.17.2`
- Tailwind standalone `4.3.3`, checked by version and SHA before release

Any change to these pins requires an architecture, security, license, web-target, and packaged-native regression review.
