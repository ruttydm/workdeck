#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
executable="$repo_dir/dist/Workdeck.app/Contents/MacOS/Workdeck"
report="$repo_dir/artifacts/performance/workdeck-packaged.json"

test -x "$executable"
test -s "$report"
executable_sha256=$(shasum -a 256 "$executable" | awk '{print $1}')
jq -e --arg executable_sha256 "$executable_sha256" '
    .schema == 1 and
    .driver == "codex-computer-use" and
    .executable_sha256 == $executable_sha256 and
    (.runs | length) >= 3 and
    ([.runs[] | select(
        .status != "pass" or
        .usable_launch_ms > 1200 or
        .first_inbox_ms > 300 or
        .area_switch_p95_ms > 50 or
        .first_diff_ms > 250 or
        .idle_memory_mb > 350
    )] | length) == 0
' "$report" >/dev/null

echo "Workdeck exact-package performance evidence passed."
