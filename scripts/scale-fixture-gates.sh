#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
binary=${WORKDECK_APP_BINARY:-"$repo_dir/target/debug/workdeck-app"}
scratch=$(mktemp -d "${TMPDIR:-/tmp}/workdeck-scale.XXXXXX")
cleanup() {
    status=$?
    case "$scratch" in
        "${TMPDIR:-/tmp}"/workdeck-scale.*) rm -rf -- "$scratch" ;;
        *) echo "refusing to remove unexpected fixture path: $scratch" >&2 ;;
    esac
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

test -x "$binary"
export WORKDECK_DATA_DIR="$scratch/catalog"
fixture=$("$binary" --json fixture polished)
printf '%s' "$fixture" | jq -e '
    .ok == true and
    .data.portfolio.project_count == 74 and
    .data.portfolio.repository_count == 75 and
    .data.portfolio.worktree_count == 109 and
    .data.portfolio.unavailable_count == 4 and
    (.data.inbox.items | length) >= 3 and
    (.data.reviews | length) >= 2
' >/dev/null

start=$(python3 -c 'import time; print(time.monotonic_ns())')
for query in sampleapp workdeck dashboard; do
    "$binary" --json fixture polished >/dev/null
    "$binary" --json search "$query" >/dev/null
done
end=$(python3 -c 'import time; print(time.monotonic_ns())')
elapsed_ms=$(((end - start) / 1000000))
test "$elapsed_ms" -lt 1200

echo "Workdeck deterministic 74/75/109 fixture passed in ${elapsed_ms}ms."
