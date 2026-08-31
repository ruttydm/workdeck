#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
cd "$repo_dir"

manifest=crates/workdeck-ui/Cargo.toml
for forbidden in workdeck-core workdeck-db workdeck-git workdeck-github workdeck-artifacts rusqlite git2 reqwest tiny_http; do
    if grep -Eq "(^|[[:space:]])${forbidden}[[:space:]]*=" "$manifest"; then
        echo "workdeck-ui directly depends on forbidden runtime crate: $forbidden" >&2
        exit 1
    fi
done

findings=$(rg -n \
    'std::fs|std::process|Command::new|std::net|Tcp(Stream|Listener)|reqwest|ureq|rusqlite|git2::|block_on\(|recv_blocking\(|std::thread::sleep|thread::sleep|RuntimeHandle|WorkdeckService|ArtifactStore' \
    crates/workdeck-ui/src -g '*.rs' || true)
if [ -n "$findings" ]; then
    echo "direct I/O or blocking work entered Workdeck renderer code:" >&2
    printf '%s\n' "$findings" >&2
    exit 1
fi

grep -Fq 'workdeck-api' "$manifest"
grep -Fq 'pub struct WorkdeckClient' crates/workdeck-api/src/lib.rs
grep -Fq 'pub trait WorkdeckTransport' crates/workdeck-api/src/lib.rs
grep -Fq 'request_id' crates/workdeck-api/src/lib.rs
grep -Fq 'source_revision' crates/workdeck-api/src/lib.rs

echo "Workdeck renderer I/O boundary passed."
