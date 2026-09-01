#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo fmt --all --check
cargo test --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked --release --package workdeck-cli --bin workdeck
cargo package --allow-dirty --locked --package workdeck-cli

scratch="$(mktemp -d "${TMPDIR:-/tmp}/workdeck-soak.XXXXXX")"
cleanup() {
  status=$?
  case "$scratch" in
    "${TMPDIR:-/tmp}"/workdeck-soak.*) rm -rf -- "$scratch" ;;
    *) printf 'refusing to remove unexpected soak path: %s\n' "$scratch" >&2 ;;
  esac
  exit "$status"
}
trap cleanup EXIT HUP INT TERM

CARGO_TARGET_DIR="$scratch/install-target" \
  cargo install --path crates/workdeck-cli --root "$scratch/install" --locked \
  >"$scratch/install.log" 2>&1

env -u HOME "$scratch/install/bin/workdeck" --help >/dev/null

if script --version >/dev/null 2>&1; then
  printf 'q' | script -qec "env -u HOME '$repo_root/target/release/workdeck'" /dev/null >/dev/null
  { sleep 0.2; printf '\003'; } | script -qec "env -u HOME '$repo_root/target/release/workdeck'" /dev/null >/dev/null
else
  printf 'q' | env -u HOME script -q /dev/null "$repo_root/target/release/workdeck" >/dev/null
  { sleep 0.2; printf '\003'; } | env -u HOME script -q /dev/null "$repo_root/target/release/workdeck" >/dev/null
fi

fixture_repo="$scratch/repository"
mkdir -p "$fixture_repo"
git -C "$fixture_repo" init >/dev/null
git -C "$fixture_repo" config user.email workdeck@example.test
git -C "$fixture_repo" config user.name "Workdeck Test"
mkdir -p "$fixture_repo/src" "$fixture_repo/resources/js/pages"

for i in $(seq 1 600); do
  printf 'line %s\n' "$i" >"$fixture_repo/src/file_$i.rs"
done

git -C "$fixture_repo" add . >/dev/null
git -C "$fixture_repo" commit -m initial >/dev/null

for i in $(seq 1 200); do
  printf 'line %s\nchanged\n' "$i" >"$fixture_repo/src/file_$i.rs"
done

for i in $(seq 1 100); do
  printf 'new %s\n' "$i" >"$fixture_repo/resources/js/pages/page_$i.vue"
done

env -u HOME target/release/workdeck --cwd "$fixture_repo" --status-json \
  >"$scratch/large-status.json"

python3 - "$scratch/large-status.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    data = json.load(handle)
payload = data.get("data", data)
assert len(payload["changes"]) == 300, len(payload["changes"])
PY

echo "Workdeck terminal soak passed."
