#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
metadata_file=$(mktemp)
sbom_file=$(mktemp)
notices_file=$(mktemp)
cleanup() { rm -f "$metadata_file" "$sbom_file" "$notices_file"; }
trap cleanup EXIT HUP INT TERM

cd "$repo_dir"
cargo metadata --locked --format-version 1 >"$metadata_file"
jq --sort-keys '
  . as $root
  | def cargo_ref($package): "pkg:cargo/\($package.name)@\($package.version)?source=\(($package.source // "workspace") | @uri)";
    def package_for($id): first($root.packages[] | select(.id == $id));
  {
    bomFormat: "CycloneDX", specVersion: "1.5", version: 1,
    metadata: {
      component: {type: "application", "bom-ref": "pkg:cargo/workdeck@0.1.0", name: "Workdeck", version: "0.1.0", licenses: [{license: {id: "MIT"}}]},
      properties: [{name: "workdeck:cargo_locked", value: "true"}, {name: "workdeck:generated_without_timestamp", value: "true"}]
    },
    components: ([.packages[] | {type: "library", "bom-ref": cargo_ref(.), name: .name, version: .version, licenses: [{expression: .license}], properties: [{name: "cargo:source", value: (.source // "workspace-path")}]}] + [
      {type: "library", "bom-ref": "pkg:github/zeronsh/comet@2ebe6ed06f8e7e1ed911c0adf31ec91ad2e94274", name: "Comet design reference", version: "2ebe6ed06f8e7e1ed911c0adf31ec91ad2e94274", licenses: [{license: {id: "MIT"}}]},
      {type: "library", "bom-ref": "pkg:github/pingdotgg/t3code@348367dcc6e1ac8b11baf94a31c47f33d46313cd", name: "T3 Code interaction reference", version: "348367dcc6e1ac8b11baf94a31c47f33d46313cd", licenses: [{license: {id: "MIT"}}]},
      {type: "library", "bom-ref": "pkg:github/stablyai/orca@5e900b10b31f12db885e4448c3e1f6300e066efb", name: "Orca ADE interaction reference", version: "5e900b10b31f12db885e4448c3e1f6300e066efb", licenses: [{license: {id: "MIT"}}]},
      {type: "library", "bom-ref": "pkg:github/DioxusLabs/dioxus-components@bf007c15d0cf4d04d3181cc46cf12325aa773955", name: "Dioxus Components primitive reference", version: "bf007c15d0cf4d04d3181cc46cf12325aa773955", licenses: [{license: {id: "MIT"}}]}
    ]) | sort_by(.name, .version, .["bom-ref"]),
    dependencies: [.resolve.nodes[] | {ref: cargo_ref(package_for(.id)), dependsOn: ([.deps[].pkg | cargo_ref(package_for(.))] | sort | unique)}] | sort_by(.ref)
  }
' "$metadata_file" >"$sbom_file"

{
    printf '%s\n\n' '# Third-Party Notices'
    printf '%s\n\n' 'Generated deterministically from the exact locked Workdeck Cargo graph.'
    printf '%s\n\n' 'Workdeck is MIT licensed. Dependency terms remain with their owners; complete resolved license texts ship in Workdeck.app.'
    printf '%s\n\n' '| Package | Version | SPDX expression | Source |'
    printf '%s\n' '| --- | --- | --- | --- |'
    jq -r '.packages | sort_by(.name, .version, (.source // "workspace-path")) | .[] | "| `\(.name)` | `\(.version)` | `\(.license)` | `\(.source // "workspace-path")` |"' "$metadata_file"
    printf '\n%s\n' '## Reviewed source and behavior references'
    printf '%s\n' '- Dioxus Components bf007c15d0cf4d04d3181cc46cf12325aa773955: pure-Rust primitive semantics adapted under the MIT option; upstream focus-trap JavaScript is not copied or shipped.'
    printf '%s\n' '- Comet/Zeron 2ebe6ed06f8e7e1ed911c0adf31ec91ad2e94274: permissive design and interaction reference; Geist fonts ship under SIL OFL 1.1.'
    printf '%s\n' '- T3 Code 348367dcc6e1ac8b11baf94a31c47f33d46313cd and Orca ADE 5e900b10b31f12db885e4448c3e1f6300e066efb: permissive interaction references with independent Workdeck styling and Rust implementation.'
    printf '%s\n' '- Waku is GPL behavior-only inspiration. No Waku code, CSS, assets, icons, or constants are copied or distributed.'
} >"$notices_file"

mkdir -p "$repo_dir/sbom"
mv "$sbom_file" "$repo_dir/sbom/Workdeck.cdx.json"
mv "$notices_file" "$repo_dir/THIRD_PARTY_NOTICES.md"
echo "generated sbom/Workdeck.cdx.json and THIRD_PARTY_NOTICES.md"
