# Workdeck Keyboard and Accessibility Matrix

| Surface | Keyboard contract | Semantic contract |
| --- | --- | --- |
| Global shell | `⌘1–5`, `⌘K`, Escape | navigation landmark, current page, named command dialog |
| Viewport tabs | Tab, Enter/Space | tablist, one selected tab |
| Navigator | Tab, Enter/Space, text search | named navigation, selected rows, honest counts |
| Workspaces | Tab, Enter/Space, Expand all | treegrid, row hierarchy, expanded state, column headers |
| Inbox | Tab, Enter/Space, scan/cancel | grouped decisions, progressbar, status text independent of color |
| Search | typing, Tab, Enter/Space | search input, grouped bounded results, useful names |
| Review | Tab, Enter/Space across lenses and marks | review-lens tablist, table/cell diff semantics, canonical tree levels |
| Git | typing, Tab, Enter/Space | references navigation, commit listbox/options, selected range state |
| Pull requests | typing, Tab, Enter/Space | master listbox/options, detail tabs, named external action |
| CI | Tab, Enter/Space | run/job lists, status text, readable progressive logs |
| Artifacts | Tab, Enter/Space, Escape close | artifact listbox, named sandbox frame, import dialog |

All icon-only controls have authored names. Focus is never indicated by color alone. Focus rings meet contrast in light and dark appearance. Reduced motion removes non-essential transition/animation duration.

The fixture web target runs Axe against every primary surface. Playwright covers pointer and keyboard navigation, focus restoration, hierarchy disclosure, review lenses, empty onboarding, offline behavior, and minimum-width pane exclusion. Native focus, WebView accessibility exposure, titlebar, menus, and file panels are verified only with Codex Computer Use against the signed package.
