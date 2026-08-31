#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
CDPATH='' cd -- "$repo_dir"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/workdeck-repository-state.XXXXXX")
cleanup() {
    status=$?
    case "$scratch" in
        "${TMPDIR:-/tmp}"/workdeck-repository-state.*) rm -rf -- "$scratch" ;;
        *) echo "refusing to remove unexpected repository-state path: $scratch" >&2 ;;
    esac
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

usage() { echo "usage: $0 snapshot OUTPUT.json | verify BASELINE.json OUTPUT.json" >&2; exit 2; }

snapshot() {
    output=$1
    : >"$scratch/roots"
    portfolio_roots=${WORKDECK_PORTFOLIO_ROOTS:-"$HOME/Projects:$HOME/Sites"}
    printf '%s\n' "$portfolio_roots" | tr ':' '\n' |
        while IFS= read -r portfolio; do
            [ -d "$portfolio" ] || continue
            find "$portfolio" -maxdepth 6 \( -type d -o -type f \) -name .git -print 2>/dev/null |
                while IFS= read -r marker; do
                    dirname -- "$marker"
                done >>"$scratch/roots"
        done
    LC_ALL=C sort -u "$scratch/roots" >"$scratch/unique-roots"
    : >"$scratch/records"
    while IFS= read -r root; do
        [ -n "$root" ] || continue
        # The repository under test is expected to change during implementation.
        # Guard only the sibling portfolio repositories Workdeck treats as read-only input.
        [ "$root" != "$repo_dir" ] || continue
        identity=$(printf '%s' "$root" | shasum -a 256 | awk '{print $1}')
        head=$(GIT_OPTIONAL_LOCKS=0 git -C "$root" rev-parse --verify HEAD 2>/dev/null || printf unavailable)
        status=$(GIT_OPTIONAL_LOCKS=0 git -C "$root" status --porcelain=v2 --branch --untracked-files=all 2>/dev/null || printf unavailable)
        worktrees=$(GIT_OPTIONAL_LOCKS=0 git -C "$root" worktree list --porcelain 2>/dev/null || printf unavailable)
        digest=$(printf '%s\n%s\n%s' "$head" "$status" "$worktrees" | shasum -a 256 | awk '{print $1}')
        jq -nc --arg id "$identity" --arg digest "$digest" '{id:$id,digest:$digest}' >>"$scratch/records"
    done <"$scratch/unique-roots"
    mkdir -p "$(dirname -- "$output")"
    jq -s '{schema:1,privacy:"SHA-256 identifiers and Git-state digests only",repositories:(sort_by(.id))}' "$scratch/records" >"$output"
}

case ${1:-} in
    snapshot)
        [ "$#" -eq 2 ] || usage
        snapshot "$2"
        ;;
    verify)
        [ "$#" -eq 3 ] || usage
        test -s "$2"
        snapshot "$3"
        cmp -s "$2" "$3" || {
            echo "portfolio repository state changed during Workdeck QA" >&2
            diff -u "$2" "$3" >&2 || true
            exit 1
        }
        ;;
    *) usage ;;
esac

echo "Workdeck portfolio repository state digest gate passed."
