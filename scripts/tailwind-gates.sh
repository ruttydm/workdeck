#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
tool="$repo_dir/target/tools/tailwindcss-4.3.3-macos-arm64"
expected_sha256=cdf646702987a743464dff4d9c60fd4480d1c1e73dd819a9a67f1078815dce9d
scratch=$(mktemp "${TMPDIR:-/tmp}/workdeck-tailwind.XXXXXX.css")
cleanup() { rm -f "$scratch"; }
trap cleanup EXIT HUP INT TERM

test -x "$tool"
test "$(shasum -a 256 "$tool" | awk '{print $1}')" = "$expected_sha256"
"$tool" --help 2>&1 | grep -Fq 'tailwindcss v4.3.3'
cd "$repo_dir"
"$tool" -i crates/workdeck-ui/tailwind.css -o "$scratch" --minify >/dev/null
cmp -s "$scratch" crates/workdeck-ui/assets/workdeck.css || {
    echo "compiled Workdeck CSS is stale; rebuild it with the pinned Tailwind binary" >&2
    exit 1
}

if find crates/workdeck-ui crates/workdeck-desktop -type f \
    \( -name '*.js' -o -name '*.ts' -o -name '*.tsx' \) -print -quit | grep -q .; then
    echo "hand-authored JavaScript or TypeScript exists in shipping source" >&2
    exit 1
fi

echo "Tailwind 4.3.3 checksum and deterministic Workdeck CSS output passed."
