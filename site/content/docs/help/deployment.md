+++
title = "Deployment integration"
description = "Build and deploy the Workdeck landing page and documentation as one native static artifact."
template = "docs.html"
+++

The `site/` Zola project owns the complete Workdeck website:

- `/` is the marketing landing page.
- `/docs/` and `/docs/*` are the native documentation pages.
- `/llms.txt`, `/llms-small.txt`, and `/llms-full.txt` are generated Markdown indexes.
- `/docs/workdeck-review-skill.md` publishes the generated agent skill.

One static build keeps navigation, metadata, generated references, and deployment atomic. Do not
operate a second documentation origin or copy the docs into another repository.

## Build the immutable artifact

From the Workdeck repository root:

```bash
cargo xtask docs check
cargo xtask site check
cargo xtask site build
```

`site build` runs the Rust asset SBOM, skill checks, Zola build, Markdown exports, and the native
link/metadata gate. The resulting `site/public/` directory is the deployable artifact. Zola is the
only external site builder; no Node, Bun, application JavaScript, or WASM runtime is required.

## Deploy with a static host

Configure one static-site project with:

- **Repository:** `ruttydm/workdeck`
- **Root directory:** repository root
- **Build command:** `cargo xtask site build`
- **Output directory:** `site/public`
- **Production branch:** `main`
- **Domain:** `workdeck.dev` and `www.workdeck.dev`

Preview deployments should use the same build command and must pass `cargo xtask site check`
before traffic is switched. Keep the release binary and website artifacts in their separate,
signed archives.

## Verify before switching traffic

```bash
curl --fail --location https://workdeck.dev/
curl --fail --location https://workdeck.dev/docs/
curl --fail https://workdeck.dev/sitemap.xml
curl --fail https://workdeck.dev/llms.txt
curl --fail https://workdeck.dev/docs/workdeck-review-skill.md
curl --fail https://workdeck.dev/og.svg
```

In a browser, confirm the landing page links to the docs, the install command copies, documentation
routes and anchors resolve, and the GitHub edit link opens the matching Workdeck source file.

## Roll back

Promote the last known-good static artifact or reassign the domain to the previous static host.
The site has no runtime data migration, so rollback is an artifact or deployment promotion rather
than an application recovery.

Adapted from Hunk's MIT deployment guide, Copyright Modem Labs Inc. Workdeck's Rust/Zola pipeline
and domain replace the original Astro/Vercel runtime.
