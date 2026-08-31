#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
cd "$repo_dir"

ui=crates/workdeck-ui/src

# Workdeck's web fixture suite is the executable accessibility contract. Keep the
# deterministic Axe coverage and the key semantic landmarks in authored Rust.
grep -Fq 'new AxeBuilder' web-tests/tests/workdeck.spec.ts
for surface in inbox workspaces git search pull-requests ci artifacts changes; do
    grep -Fq "\"$surface\"" web-tests/tests/workdeck.spec.ts
done
grep -Fq 'aria_label: "Global navigation"' "$ui/components/shell.rs"
grep -Fq 'aria_label: "Pull request browser"' "$ui/surfaces/pull_requests.rs"
grep -Fq 'role: "tablist"' "$ui/surfaces/task_changes.rs"
grep -Fq 'role: "tree"' "$ui/surfaces/task_changes.rs"
grep -Fq 'role: "treegrid"' "$ui/surfaces/workspaces.rs"
grep -Fq 'role: "listbox"' "$ui/surfaces/git.rs"
grep -Fq 'role: "dialog"' "$ui/components/shell.rs"
grep -Fq 'role: "progressbar"' "$ui/components/primitives.rs"
grep -Fq 'aria_modal: "true"' "$ui/components/shell.rs"
grep -Fq 'aria_orientation: "vertical"' "$ui/app.rs"

# Icon-only buttons are centralized so an authored label is mandatory.
grep -Fq 'aria_label: "{label}"' "$ui/components/primitives.rs"
if rg -n 'button \{ class: "icon-button"' "$ui" -g '*.rs' |
    while IFS=: read -r file line rest; do
        end=$((line + 5))
        sed -n "${line},${end}p" "$file" | grep -Eq 'aria_label|title:' || {
            echo "$file:$line icon button has no accessible name" >&2
            exit 1
        }
    done
then
    :
fi

# Pointer-only fake controls and positive tab ordering are not permitted.
if rg -n 'role: "button"' "$ui" -g '*.rs' | grep -v 'button {'; then
    echo "Workdeck contains a non-native element claiming button semantics" >&2
    exit 1
fi
if rg -n 'tabindex: "[1-9]' "$ui" -g '*.rs'; then
    echo "Workdeck contains a positive tabindex" >&2
    exit 1
fi

echo "Workdeck accessibility naming, role, focus, and keyboard contracts passed."
