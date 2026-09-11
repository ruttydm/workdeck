# Website workflow migration

The native `.github/workflows/website.yml` is the checked-in workflow.

The pinned website workflow's documentation generation, Zola/links checks,
static build, and browser-smoke responsibilities are owned by the native
`cargo xtask site` pipeline. The workflow installs the pinned Zola release,
checks generated routes and static links, builds Markdown exports, and runs the
preview-isolation test (preview isolation) that exercises refresh/removal behavior in a disposable
tree.

The old Bun/Node/Playwright steps are not retained or executed. Native output
is deliberately no-application-JavaScript; visual/browser validation remains a
release-environment concern and is represented by the Rust preview and static
checks in CI.
