# Workdeck Distribution Checklist

## Automated local release

- [ ] Complete source quality gates pass.
- [ ] Playwright interaction, Axe, and visual suites pass from current WebAssembly/CSS.
- [ ] `dist/Workdeck.app` contains Workdeck desktop, the `workdeck` TUI, the `workdeck-app` catalog helper, icon, fonts, notices, SBOM, provenance, and licenses.
- [ ] No hand-authored JavaScript/TypeScript or GPUI/Zed/Longbridge dependency ships.
- [ ] SQLite integrity and foreign-key checks pass.
- [ ] Artifact security and helper shutdown pass.
- [ ] Strict deep codesign passes.
- [ ] Codex Computer Use exact-package matrix passes and is hash-bound.
- [ ] Portfolio repository digest is unchanged across native QA.
- [ ] Zero known P0/P1 and no unexplained P2 defects.

## External public-distribution requirements

- [ ] Apple Developer ID Application certificate.
- [ ] Hardened-runtime and entitlement review.
- [ ] Notarization and stapling credentials.
- [ ] Gatekeeper test on a clean supported macOS installation.
- [ ] Final legal approval of dependency, copied-source, font, icon, and provenance notices.
- [ ] Release-channel hosting, update signing, privacy policy, and support metadata.

Ad-hoc signing makes the local artifact runnable; it is not represented as notarized public distribution.
