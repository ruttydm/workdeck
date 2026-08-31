#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
app="$repo_dir/dist/Workdeck.app"
executable="$app/Contents/MacOS/Workdeck"
manifest="$repo_dir/artifacts/native-visual-manifest.json"

test -x "$executable"
test -s "$manifest"
bundle_id=$(plutil -extract CFBundleIdentifier raw "$app/Contents/Info.plist")
executable_sha256=$(shasum -a 256 "$executable" | awk '{print $1}')

jq -e \
    --arg bundle_id "$bundle_id" \
    --arg executable_sha256 "$executable_sha256" '
    .schema == 1 and
    .driver == "codex-computer-use" and
    .bundle_id == $bundle_id and
    .executable_sha256 == $executable_sha256 and
    .repository_state_unchanged == true and
    (.captures | length) >= 16 and
    (([.captures[].appearance] | unique) as $appearance |
        ($appearance | index("light")) != null and
        ($appearance | index("dark")) != null) and
    (([.captures[].layout] | unique) as $layout |
        ($layout | index("minimum")) != null and
        ($layout | index("wide")) != null) and
    (([.captures[].surface] | unique) as $surfaces |
        ["inbox","workspaces","git","search","pull-requests","ci","artifacts","review"] |
        all(. as $surface | ($surfaces | index($surface)) != null)) and
    ([.captures[] | select(.status != "pass" or .computer_use != true)] | length) == 0
' "$manifest" >/dev/null

jq -r '.captures[] | [.file, .sha256] | @tsv' "$manifest" |
    while IFS="$(printf '\t')" read -r relative expected; do
        capture="$repo_dir/artifacts/native-visual/$relative"
        test -s "$capture"
        test "$(shasum -a 256 "$capture" | awk '{print $1}')" = "$expected"
    done

echo "Exact-package Codex Computer Use visual evidence passed."
