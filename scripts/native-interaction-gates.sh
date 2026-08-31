#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
app="$repo_dir/dist/Workdeck.app"
executable="$app/Contents/MacOS/Workdeck"
manifest="$repo_dir/artifacts/native-interaction-manifest.json"

test -x "$executable"
test -s "$manifest"
executable_sha256=$(shasum -a 256 "$executable" | awk '{print $1}')
bundle_id=$(plutil -extract CFBundleIdentifier raw "$app/Contents/Info.plist")

jq -e \
    --arg bundle_id "$bundle_id" \
    --arg executable_sha256 "$executable_sha256" '
    .schema == 1 and
    .driver == "codex-computer-use" and
    .bundle_id == $bundle_id and
    .executable_sha256 == $executable_sha256 and
    .repository_state_unchanged == true and
    (.interactions | length) >= 12 and
    (([.interactions[].kind] | unique) as $kinds |
        ["menu","shortcut","dialog","file-panel","resize","quit"] |
        all(. as $kind | ($kinds | index($kind)) != null)) and
    ([.interactions[] | select(
        .status != "pass" or
        .computer_use != true or
        .focus_restored != true or
        .clean_shutdown != true
    )] | length) == 0
' "$manifest" >/dev/null

echo "Exact-package Codex Computer Use interaction evidence passed."
