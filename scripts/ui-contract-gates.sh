#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$script_dir/.." && pwd)
source_only=false

case ${1:-} in
    '') ;;
    --source-only) source_only=true ;;
    *) echo "usage: $0 [--source-only]" >&2; exit 2 ;;
esac
[ "$#" -le 1 ] || { echo "usage: $0 [--source-only]" >&2; exit 2; }
cd "$repo_dir"

for document in \
    docs/WORKDECK_DIOXUS_IMPLEMENTATION_PLAN.md \
    docs/ARCHITECTURE.md \
    docs/IMPLEMENTATION.md \
    docs/DESIGN_SYSTEM.md \
    docs/NAVIGATION_MODEL.md \
    docs/KEYBOARD_ACCESSIBILITY_MATRIX.md \
    docs/SECURITY_REPORT.md \
    docs/PERFORMANCE_REPORT.md \
    docs/PACKAGED_NATIVE_QA.md \
    docs/DISTRIBUTION_CHECKLIST.md \
    docs/COMPONENT_PROVENANCE.md \
    docs/DEPENDENCY_POLICY.md
do
    test -s "$document"
done

test "$(wc -l <crates/workdeck-ui/src/lib.rs | tr -d ' ')" -le 180
largest_ui_module=$(find crates/workdeck-ui/src -name '*.rs' -exec wc -l {} + |
    awk '$2 != "total" && $1 > maximum { maximum = $1 } END { print maximum + 0 }')
test "$largest_ui_module" -le 1400

grep -Fq -- '--rail-width: 44px' crates/workdeck-ui/tailwind.css
grep -Fq -- '--titlebar-height: 52px' crates/workdeck-ui/tailwind.css
grep -Fq -- '--macos-traffic-gutter: 80px' crates/workdeck-ui/tailwind.css
grep -Fq 'grid-column: 1 / -1' crates/workdeck-ui/tailwind.css
if grep -Fq 'app-rail__traffic-space' crates/workdeck-ui/src/components/shell.rs; then
    echo "Workdeck rail must begin below the unified native titlebar" >&2
    exit 1
fi
if grep -Fq 'app-rail__mark' crates/workdeck-ui/src/components/shell.rs; then
    echo "Workdeck rail content must not occupy the native traffic-light region" >&2
    exit 1
fi
if grep -Fq -- '--toolbar-height:' crates/workdeck-ui/tailwind.css; then
    echo "Workdeck must not restore a persistent second toolbar row" >&2
    exit 1
fi
grep -Fq 'with_min_inner_size(LogicalSize::new(900.0, 600.0))' crates/workdeck-desktop/src/main.rs
grep -Fq 'with_fullsize_content_view(true)' crates/workdeck-desktop/src/main.rs
grep -Fq 'MENU_LIFETIME_GUARD' crates/workdeck-desktop/src/main.rs
grep -Fq 'Area::Artifacts' crates/workdeck-ui/src/components/shell.rs
grep -Fq 'Area::PullRequests' crates/workdeck-ui/src/components/shell.rs
if grep -Fq 'viewport-tabs' crates/workdeck-ui/tailwind.css; then
    echo "Workdeck must keep global destinations in the icon rail, not titlebar tabs" >&2
    exit 1
fi
grep -Fq 'ReviewLens::Split' crates/workdeck-ui/src/surfaces/review.rs
grep -Fq 'ReviewLens::Ast' crates/workdeck-ui/src/surfaces/review.rs
grep -Fq 'project_count > 0' crates/workdeck-ui/src/app.rs
grep -Fq 'area_uses_navigator(active())' crates/workdeck-ui/src/app.rs
if grep -Fq 'ContextToolbar' crates/workdeck-ui/src/app.rs; then
    echo "Workdeck must not restore the removed context toolbar" >&2
    exit 1
fi
grep -Fq 'minimum layout never overlays navigator and inspector' web-tests/tests/workdeck.spec.ts
grep -Fq 'empty onboarding removes portfolio chrome' web-tests/tests/workdeck.spec.ts
grep -Fq 'offline state is explicit without disabling local commits' web-tests/tests/workdeck.spec.ts

scripts/render-thread-gates.sh
scripts/accessibility-gates.sh

if rg -n 'TODO|FIXME|unimplemented!\(|todo!\(' crates/workdeck-ui/src crates/workdeck-desktop/src; then
    echo "Workdeck shipping UI contains unfinished markers" >&2
    exit 1
fi

if [ "$source_only" = false ]; then
    scripts/capture-visual-qa.sh
    scripts/native-interaction-gates.sh
fi

if [ "$source_only" = true ]; then
    echo "Workdeck source UI contracts passed; exact-package evidence was not evaluated."
else
    echo "Workdeck source and exact-package UI contracts passed."
fi
