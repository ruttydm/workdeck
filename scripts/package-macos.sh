#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
destination="$repo_dir/dist/Workdeck.app"
replace=false
profile=release
profile_flag=--release

for argument in "$@"; do
    case "$argument" in
        --replace) replace=true ;;
        --debug) profile=debug; profile_flag= ;;
        *) echo "usage: $0 [--replace] [--debug]" >&2; exit 2 ;;
    esac
done

test "$(uname -s)" = Darwin || { echo "macOS packaging must run on macOS" >&2; exit 1; }
if [ -e "$destination" ] && [ "$replace" != true ]; then
    echo "$destination already exists; pass --replace to package atomically" >&2
    exit 1
fi

cd "$repo_dir"
if [ -d /Applications/Xcode.app/Contents/Developer ]; then
    export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi
target/tools/tailwindcss-4.3.3-macos-arm64 -i crates/workdeck-ui/tailwind.css -o crates/workdeck-ui/assets/workdeck.css --minify
scripts/generate-release-metadata.sh
CARGO_TARGET_DIR="$repo_dir/target/workdeck-desktop" cargo build --locked $profile_flag --package workdeck-desktop --bin workdeck-desktop
CARGO_TARGET_DIR="$repo_dir/target/workdeck-app-cli" cargo build --locked $profile_flag --package workdeck-app-cli --bin workdeck-app
CARGO_TARGET_DIR="$repo_dir/target/workdeck-cli" cargo build --locked $profile_flag --package workdeck-cli --bin workdeck

staging=$(mktemp -d)
cleanup() {
    if [ ! -e "$destination" ] && [ -e "$staging/previous.app" ]; then
        mv "$staging/previous.app" "$destination"
    fi
    rm -rf "$staging"
}
trap cleanup EXIT HUP INT TERM

app="$staging/Workdeck.app"
resources="$app/Contents/Resources"
mkdir -p "$app/Contents/MacOS" "$resources/bin" "$resources/assets/fonts" "$repo_dir/dist"
cp "$repo_dir/target/workdeck-desktop/$profile/workdeck-desktop" "$app/Contents/MacOS/Workdeck"
cp "$repo_dir/target/workdeck-app-cli/$profile/workdeck-app" "$resources/bin/workdeck-app"
cp "$repo_dir/target/workdeck-cli/$profile/workdeck" "$resources/bin/workdeck"
cp "$repo_dir/crates/workdeck-desktop/Info.plist" "$app/Contents/Info.plist"
cp "$repo_dir/assets/Workdeck.icns" "$resources/Workdeck.icns"
cp "$repo_dir/crates/workdeck-ui/assets/workdeck.css" "$resources/assets/workdeck.css"
cp "$repo_dir/crates/workdeck-ui/assets/fonts/"*.ttf "$resources/assets/fonts/"
cp "$repo_dir/crates/workdeck-ui/assets/fonts/Geist.ttf" "$resources/assets/Geist.ttf"
cp "$repo_dir/crates/workdeck-ui/assets/fonts/GeistMono.ttf" "$resources/assets/GeistMono.ttf"
cp "$repo_dir/LICENSE" "$resources/Workdeck-LICENSE.txt"
cp "$repo_dir/THIRD_PARTY_NOTICES.md" "$resources/THIRD_PARTY_NOTICES.md"
cp "$repo_dir/sbom/Workdeck.cdx.json" "$resources/Workdeck.cdx.json"
cp "$repo_dir/docs/COMPONENT_PROVENANCE.md" "$resources/COMPONENT_PROVENANCE.md"
cp "$repo_dir/docs/DEPENDENCY_POLICY.md" "$resources/DEPENDENCY_POLICY.md"
scripts/collect-license-texts.sh "$resources/Licenses"
cp "$repo_dir/third_party/comet/LICENSE" "$resources/Licenses/Comet-LICENSE.txt"
cp "$repo_dir/third_party/t3code/LICENSE" "$resources/Licenses/T3-Code-LICENSE.txt"
cp "$repo_dir/third_party/orca/LICENSE" "$resources/Licenses/Orca-ADE-LICENSE.txt"
cp "$repo_dir/third_party/dioxus-components/LICENSE-MIT" "$resources/Licenses/Dioxus-Components-LICENSE-MIT.txt"
cp "$repo_dir/crates/workdeck-ui/assets/fonts/Geist-OFL.txt" "$resources/Licenses/Geist-OFL.txt"
chmod 0755 "$app/Contents/MacOS/Workdeck" "$resources/bin/workdeck-app" "$resources/bin/workdeck"

if ! strings "$app/Contents/MacOS/Workdeck" | grep -q 'grid-template-columns:var(--rail-width)'; then
    echo "compiled Workdeck design system is not embedded in the desktop renderer" >&2
    exit 1
fi
test -s "$resources/assets/fonts/Geist.ttf"
test -s "$resources/assets/fonts/GeistMono.ttf"
test -s "$resources/assets/Geist.ttf"
test -s "$resources/assets/GeistMono.ttf"

if find "$app" -type f \( -name '*.js' -o -name '*.ts' -o -name '*.tsx' \) -print -quit | grep -q .; then
    echo "hand-authored JavaScript or TypeScript entered Workdeck.app" >&2
    exit 1
fi
codesign --force --deep --sign - "$app"
if [ -e "$destination" ]; then
    mv "$destination" "$staging/previous.app"
fi
mv "$app" "$destination"
codesign --verify --deep --strict --verbose=2 "$destination"
echo "packaged $destination"
