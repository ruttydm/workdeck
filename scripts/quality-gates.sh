#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
release=false
live=false
native=false

for argument in "$@"; do
    case "$argument" in
        --release) release=true ;;
        --live) live=true ;;
        --native-evidence) native=true ;;
        *) echo "usage: $0 [--release] [--live] [--native-evidence]" >&2; exit 2 ;;
    esac
done
[ "$native" = false ] || [ "$release" = true ] || {
    echo "--native-evidence requires --release" >&2
    exit 2
}

cd "$repo_dir"
if [ -d /Applications/Xcode.app/Contents/Developer ]; then
    export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi

echo "[1/18] format"
cargo fmt --all --check

echo "[2/18] locked metadata and shipping graph"
cargo metadata --locked --format-version 1 >/dev/null
test -z "$(cargo tree --workspace --all-features --edges normal,build | rg -i 'gpui|zed-industries|longbridge' || true)"

echo "[3/18] Tailwind pin, checksum, and deterministic output"
scripts/tailwind-gates.sh

echo "[4/18] desktop and all-target checks"
cargo check --workspace --all-targets --all-features --locked

echo "[5/18] wasm renderer check"
cargo check --locked --package workdeck-ui --target wasm32-unknown-unknown --no-default-features --features web

echo "[6/18] unit, integration, security, schema, and fixture tests"
cargo test --workspace --all-targets --all-features --locked

echo "[7/18] Clippy warnings denied"
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

echo "[8/18] rustdoc warnings denied"
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked

echo "[9/18] dependency, provenance, and source policy"
scripts/generate-release-metadata.sh
cargo deny check advisories bans licenses sources
scripts/license-gates.sh

echo "[10/18] shell, metadata, icon, render, and accessibility contracts"
shellcheck scripts/*.sh
plutil -lint crates/workdeck-desktop/Info.plist
jq -e '.iconPath == "favicon.png" and .defaultThreadEnvMode == "worktree"' t3.json >/dev/null
test -s favicon.png
test -s assets/Workdeck.icns
scripts/ui-contract-gates.sh --source-only

echo "[11/18] CLI smoke and JSON contracts"
cargo build --locked --package workdeck-cli --bin workdeck
cargo build --locked --package workdeck-app-cli --bin workdeck-app
"$repo_dir/target/debug/workdeck" --version >/dev/null
WORKDECK_APP_BINARY="$repo_dir/target/debug/workdeck-app" scripts/feature-matrix.sh

echo "[12/18] deterministic 74/75/109 fixture"
WORKDECK_APP_BINARY="$repo_dir/target/debug/workdeck-app" scripts/scale-fixture-gates.sh

echo "[13/18] Dioxus web fixture build"
dx build --platform web --package workdeck-ui --bin workdeck-web

echo "[14/18] Playwright interaction, Axe, and visual matrix"
(cd web-tests && npx playwright test)

echo "[15/18] live read-only GitHub probes"
if [ "$live" = true ]; then
    WORKDECK_APP_BINARY="$repo_dir/target/debug/workdeck-app" scripts/live-provider-gates.sh
else
    echo "skipped; pass --live"
fi

echo "[16/18] package and strict signature"
if [ "$release" = true ]; then
    scripts/package-macos.sh --replace
    test -x dist/Workdeck.app/Contents/MacOS/Workdeck
    test -x dist/Workdeck.app/Contents/Resources/bin/workdeck
    test -x dist/Workdeck.app/Contents/Resources/bin/workdeck-app
    codesign --verify --deep --strict --verbose=2 dist/Workdeck.app
    test "$(plutil -extract CFBundleIdentifier raw dist/Workdeck.app/Contents/Info.plist)" = app.gingermedia.workdeck
else
    echo "skipped; pass --release"
fi

echo "[17/18] exact-package Codex Computer Use evidence"
if [ "$native" = true ]; then
    scripts/ui-contract-gates.sh
    scripts/profile-packaged-launch.sh
else
    echo "skipped; pass --release --native-evidence after Codex Computer Use QA"
fi

echo "[18/18] artifact security and helper lifecycle"
cargo test --locked --package workdeck-artifacts

echo "All requested Workdeck quality gates passed."
