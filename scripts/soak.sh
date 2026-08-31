#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/workdeck-soak.XXXXXX")
cleanup() {
    status=$?
    case "$scratch" in
        "${TMPDIR:-/tmp}"/workdeck-soak.*) rm -rf -- "$scratch" ;;
        *) echo "refusing to remove unexpected soak path: $scratch" >&2 ;;
    esac
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

cd "$repo_dir"
cargo build --release --locked --package workdeck-app-cli --bin workdeck-app
export WORKDECK_DATA_DIR="$scratch/catalog"
for iteration in 1 2 3 4 5 6 7 8 9 10; do
    target/release/workdeck-app --json fixture polished >/dev/null
    target/release/workdeck-app --json doctor >/dev/null
    printf '%s\n' "$iteration" >/dev/null
done
test "$(sqlite3 "$WORKDECK_DATA_DIR/workdeck.sqlite3" 'PRAGMA integrity_check;')" = ok
echo "Workdeck CLI restart and catalog soak passed."
