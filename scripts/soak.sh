#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo fmt --all --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
cargo package --allow-dirty -p workdeck-cli

tmp_install="$(mktemp -d)"
tmp_home="$(mktemp -d)"
tmp_repo=""
trap 'rm -rf "$tmp_install" "$tmp_home" "$tmp_repo" /tmp/workdeck-large-status.json /tmp/workdeck-install.log' EXIT

CARGO_TARGET_DIR="$tmp_install/target" \
  cargo install --path crates/workdeck-cli --root "$tmp_install/install" --locked \
  >/tmp/workdeck-install.log 2>&1
HOME="$tmp_home" "$tmp_install/install/bin/workdeck" --help >/dev/null

/usr/bin/time -p sh -c \
  "printf 'q' | HOME='$tmp_home' script -q /dev/null target/release/workdeck >/dev/null"

/usr/bin/time -p sh -c \
  "{ sleep 0.2; printf '\003'; } | HOME='$tmp_home' script -q /dev/null target/release/workdeck >/dev/null"

tmp_repo="$(mktemp -d)"
git -C "$tmp_repo" init >/dev/null
git -C "$tmp_repo" config user.email workdeck@example.test
git -C "$tmp_repo" config user.name "Workdeck Test"
mkdir -p "$tmp_repo/src" "$tmp_repo/resources/js/pages"

for i in $(seq 1 600); do
  printf 'line %s\n' "$i" >"$tmp_repo/src/file_$i.rs"
done

git -C "$tmp_repo" add . >/dev/null
git -C "$tmp_repo" commit -m initial >/dev/null

for i in $(seq 1 200); do
  printf 'line %s\nchanged\n' "$i" >"$tmp_repo/src/file_$i.rs"
done

for i in $(seq 1 100); do
  printf 'new %s\n' "$i" >"$tmp_repo/resources/js/pages/page_$i.vue"
done

/usr/bin/time -p env HOME="$tmp_home" target/release/workdeck --cwd "$tmp_repo" --status-json \
  >/tmp/workdeck-large-status.json

python3 - <<'PY'
import json
with open("/tmp/workdeck-large-status.json") as handle:
    data = json.load(handle)
assert len(data["changes"]) == 300, len(data["changes"])
PY

echo "soak ok"
