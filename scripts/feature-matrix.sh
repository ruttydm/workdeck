#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
binary=${WORKDECK_APP_BINARY:-"$repo_dir/target/debug/workdeck-app"}
scratch=$(mktemp -d "${TMPDIR:-/tmp}/workdeck-feature-matrix.XXXXXX")
cleanup() {
    status=$?
    case "$scratch" in
        "${TMPDIR:-/tmp}"/workdeck-feature-matrix.*) rm -rf -- "$scratch" ;;
        *) echo "refusing to remove unexpected fixture path: $scratch" >&2 ;;
    esac
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

test -x "$binary"
export WORKDECK_DATA_DIR="$scratch/catalog"
repository="$scratch/repository"
artifact_source="$scratch/artifact"
mkdir -p "$repository" "$artifact_source"

git -C "$repository" init -q -b main
git -C "$repository" config user.name "Workdeck Matrix"
git -C "$repository" config user.email "workdeck@example.invalid"
printf '%s\n' '# Fixture' >"$repository/README.md"
printf '%s\n' 'fn main() {}' >"$repository/main.rs"
git -C "$repository" add -- README.md main.rs
git -C "$repository" commit -q -m 'initial fixture'
printf '%s\n' 'fn main() { println!("Workdeck"); }' >"$repository/main.rs"
git -C "$repository" add -- main.rs
git -C "$repository" commit -q -m 'reviewable change'
printf '%s\n' '// working tree' >>"$repository/main.rs"

before=$(GIT_OPTIONAL_LOCKS=0 git -C "$repository" status --porcelain=v2 --branch)

"$binary" --json catalog scan "$repository" |
    jq -e '.ok == true and .kind == "catalog_scan" and .data.repositories == 1' >/dev/null
"$binary" --json catalog scan "$repository" >/dev/null
"$binary" --json catalog refresh >/dev/null
catalog_json=$("$binary" --json catalog list)
printf '%s' "$catalog_json" | jq -e '
    .ok == true and
    (.data.projects | length) == 1 and
    (.data.repositories | length) == 1 and
    (.data.worktrees | length) == 1
' >/dev/null
project_id=$(printf '%s' "$catalog_json" | jq -er '.data.projects[0].id')
"$binary" --json catalog show "$project_id" | jq -e '.ok == true' >/dev/null

"$binary" --json git --path "$repository" status | jq -e '.ok == true and .kind == "git_status"' >/dev/null
"$binary" --json git --path "$repository" changes | jq -e '.ok == true and (.data | length) >= 1' >/dev/null
"$binary" --json git --path "$repository" commits --limit 2 | jq -e '.ok == true and (.data | length) == 2' >/dev/null
"$binary" --json git --path "$repository" graph --limit 2 | jq -e '.ok == true and (.data.rows | length) == 2' >/dev/null

"$binary" --json updates list | jq -e '.ok == true and .kind == "updates"' >/dev/null
"$binary" --json search fixture | jq -e '.ok == true and .kind == "search"' >/dev/null
"$binary" --json fixture polished | jq -e '.data.portfolio.project_count == 74' >/dev/null
"$binary" --json fixture empty | jq -e '.data.portfolio.project_count == 0' >/dev/null
"$binary" --json fixture offline | jq -e '.data.provider_state == "offline"' >/dev/null

printf '%s\n' '<!doctype html><title>Workdeck fixture</title><main>safe</main>' >"$artifact_source/index.html"
(cd "$artifact_source" && zip -q "$scratch/artifact.zip" index.html)
artifact_json=$("$binary" --json artifact import "$scratch/artifact.zip" 'Fixture report')
artifact_id=$(printf '%s' "$artifact_json" | jq -er '.data.id')
"$binary" --json artifact list | jq -e '(.data | length) == 1' >/dev/null
"$binary" --json artifact inspect "$artifact_id" | jq -e '.ok == true and .data.file_count == 1' >/dev/null
"$binary" --json doctor | jq -e '.ok == true and .data.catalog.integrity.integrity == "ok"' >/dev/null

after=$(GIT_OPTIONAL_LOCKS=0 git -C "$repository" status --porcelain=v2 --branch)
test "$before" = "$after"
test "$(sqlite3 "$WORKDECK_DATA_DIR/workdeck.sqlite3" 'PRAGMA integrity_check;')" = ok
test -z "$(sqlite3 "$WORKDECK_DATA_DIR/workdeck.sqlite3" 'PRAGMA foreign_key_check;')"

echo "Workdeck desktop-catalog CLI and read-only repository matrix passed."
