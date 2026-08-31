#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
binary=${WORKDECK_APP_BINARY:-"$repo_dir/target/debug/workdeck-app"}
scratch=$(mktemp -d "${TMPDIR:-/tmp}/workdeck-provider.XXXXXX")
cleanup() {
    status=$?
    case "$scratch" in
        "${TMPDIR:-/tmp}"/workdeck-provider.*) rm -rf -- "$scratch" ;;
        *) echo "refusing to remove unexpected provider path: $scratch" >&2 ;;
    esac
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

test -x "$binary"
command -v gh >/dev/null
gh auth status >/dev/null
export WORKDECK_DATA_DIR="$scratch/catalog"

repository=zed-industries/zed
pr=$(gh pr list --repo "$repository" --state open --limit 1 --json number --jq '.[0].number')
test -n "$pr"
"$binary" --json github prs "$repository" | jq -e '.ok == true' >/dev/null
"$binary" --json github pr "$repository" "$pr" | jq -e '.ok == true and .data.number > 0' >/dev/null

run=$(gh run list --repo ruttydm/workdeck --limit 1 --json databaseId --jq '.[0].databaseId')
test -n "$run"
"$binary" --json github runs ruttydm/workdeck | jq -e '.ok == true' >/dev/null
"$binary" --json github run ruttydm/workdeck "$run" | jq -e '.ok == true' >/dev/null
"$binary" --json github jobs ruttydm/workdeck "$run" | jq -e '.ok == true' >/dev/null
"$binary" --json github artifacts ruttydm/workdeck "$run" | jq -e '.ok == true' >/dev/null

echo "Workdeck live read-only GitHub provider probes passed."
