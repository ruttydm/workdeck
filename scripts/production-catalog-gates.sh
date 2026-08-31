#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
binary=${WORKDECK_APP_BINARY:-"$repo_dir/dist/Workdeck.app/Contents/Resources/bin/workdeck-app"}
test -x "$binary"

doctor=$("$binary" --json doctor)
database=$(printf '%s' "$doctor" | jq -er '.data.database')
test -f "$database"
test "$(sqlite3 "$database" 'PRAGMA integrity_check;')" = ok
test -z "$(sqlite3 "$database" 'PRAGMA foreign_key_check;')"
test "$(sqlite3 "$database" 'SELECT MAX(version) FROM schema_migrations;')" = 1
printf '%s' "$doctor" | jq -e '
    .ok == true and
    .data.repository_policy == "read_only" and
    .data.catalog.integrity.integrity == "ok" and
    .data.catalog.projects >= 0 and
    .data.catalog.repositories >= 0 and
    .data.catalog.worktrees >= 0
' >/dev/null

printf '%s\n' "$doctor" >"$repo_dir/artifacts/fresh-live-discovery.json"
echo "Workdeck fresh catalog integrity and current discovered counts passed."
