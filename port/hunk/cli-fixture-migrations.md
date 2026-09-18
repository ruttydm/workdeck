# Hunk compiled CLI fixture migrations

The pinned compiled fixtures are test inputs, not runtime dependencies. They
are read from the source Git object during `cargo xtask port audit` and are
never copied into the Workdeck tree or executed by a JavaScript engine.

| Pinned fixture | Native disposition | Verification |
| --- | --- | --- |
| `test/cli/fixtures/compiled-highlight-worker-control.ts` | translated test for the native background highlighter | `xtask/src/port_oracles.rs::compiled_cli_fixtures_have_native_worker_or_boundary_replacements` and `workdeck-tui::highlighted_diff_runtime` tests |
| `test/cli/fixtures/compiled-opentui-positive-control.ts` | negative runtime-boundary control; OpenTUI is rejected rather than retained | `xtask/src/port_oracles.rs` and `docs/native-extension-runtime-boundary.md` |

The worker control's unsupported-version request and bounded settlement are
represented by the native `HighlightCache` worker and its explicit offload
tests. The OpenTUI control exists to prove that a compiled source fixture from
the old runtime is not accidentally accepted as a Workdeck executable.

Source fixtures are from Hunk `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`, MIT,
copyright Modem Labs Inc.; see `THIRD_PARTY_NOTICES`.
