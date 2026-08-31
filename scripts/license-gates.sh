#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
metadata_file=$(mktemp)
violations_file=$(mktemp)
cleanup() { rm -f "$metadata_file" "$violations_file"; }
trap cleanup EXIT HUP INT TERM

cd "$repo_dir"
cargo metadata --locked --format-version 1 >"$metadata_file"
jq -r '.packages[] | select(.license == null or .license == "") | "\(.name) \(.version): missing license"' "$metadata_file" >"$violations_file"
test ! -s "$violations_file" || { cat "$violations_file" >&2; exit 1; }
jq -r '.packages[] | select(.license | test("(^|[^A-Z])A?GPL"; "i")) | select(.license | test("MIT|Apache-2\\.0|BSD|ISC|0BSD|Zlib|Unlicense") | not) | "\(.name) \(.version): unapproved copyleft \(.license)"' "$metadata_file" >"$violations_file"
test ! -s "$violations_file" || { cat "$violations_file" >&2; exit 1; }
jq -r '.packages[] | select(.license | test("MPL-2\\.0")) | "\(.name) \(.version)"' "$metadata_file" | sort -u >"$violations_file"
diff -u docs/MPL_ALLOWLIST.txt "$violations_file"

if jq -e '.packages[] | select((.manifest_path | contains("/inspiration/")) or ((.source // "") | contains("/inspiration/")))' "$metadata_file" >/dev/null; then
    echo "inspiration checkout entered Cargo metadata" >&2
    exit 1
fi
if cargo tree --workspace --all-features --edges normal,build | grep -Eqi '(^| )gpui|zed-industries|longbridge'; then
    echo "GPUI, Zed, or Longbridge dependency remains in the Workdeck shipping graph" >&2
    exit 1
fi
grep -Fq 'name = "dioxus"' Cargo.lock
grep -Fq 'version = "0.7.1"' Cargo.lock
grep -Fq 'name = "dioxus-desktop"' Cargo.lock
grep -Fq 'version = "0.7.10"' Cargo.lock
grep -Fq 'name = "dioxus-free-icons"' Cargo.lock
grep -Fq 'version = "0.10.0"' Cargo.lock
grep -Fq 'bf007c15d0cf4d04d3181cc46cf12325aa773955' Cargo.toml
test "$(cat third_party/dioxus-components/LICENSE-MIT)" = "$(cat inspiration/dioxus-components/LICENSE-MIT)"

for required in docs/COMPONENT_PROVENANCE.md docs/DEPENDENCY_POLICY.md THIRD_PARTY_NOTICES.md sbom/Workdeck.cdx.json third_party/comet/LICENSE third_party/t3code/LICENSE third_party/orca/LICENSE; do
    test -s "$required"
done
grep -Fq 'bf007c15d0cf4d04d3181cc46cf12325aa773955' docs/COMPONENT_PROVENANCE.md
grep -Fq 'Waku' docs/COMPONENT_PROVENANCE.md
grep -Fq 'behavior-only' docs/COMPONENT_PROVENANCE.md
jq -e '.bomFormat == "CycloneDX" and .metadata.component.name == "Workdeck" and (.components | length > 0)' sbom/Workdeck.cdx.json >/dev/null
if grep -Eq '/Users/|/inspiration/' sbom/Workdeck.cdx.json THIRD_PARTY_NOTICES.md; then
    echo "release metadata contains a local path" >&2
    exit 1
fi
if find crates/workdeck-ui crates/workdeck-desktop -type f \( -name '*.js' -o -name '*.ts' -o -name '*.tsx' \) -print -quit | grep -q .; then
    echo "hand-authored JavaScript or TypeScript exists in shipping source" >&2
    exit 1
fi
if [ -d dist/Workdeck.app ] && find dist/Workdeck.app -type f \( -name '*.js' -o -name '*.ts' -o -name '*.tsx' \) -print -quit | grep -q .; then
    echo "hand-authored JavaScript or TypeScript entered Workdeck.app" >&2
    exit 1
fi
echo "Workdeck commercial dependency, provenance, and source-policy gates passed"
