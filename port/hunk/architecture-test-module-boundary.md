# Production session-boundary scanning

The workspace/all-target run after the precompiled syntax fix passed the
review-triage, VCS and TUI suites but failed the final tooling architecture test.
The reported violation was `workdeck_session` in `source_controller.rs`; those
references are inside its explicit `#[cfg(test)]` module. The former boundary
check used raw substring matching on the entire source file.

The session-import check now parses Rust and visits production identifiers.
Explicit `#[cfg(test)]` modules are excluded, including nested modules. Mixed
conditions such as `cfg(any(test, feature = "runtime"))` remain checked, as do
`cfg(not(test))` modules. Macro token bodies are checked conservatively because
expansion is unavailable to this source scanner. Parse failures are errors.
Comments and ordinary string contents are not imports.

The regression covers those cases, direct imports, nested runtime calls, runtime
code following a test module, and malformed Rust. All six architecture tests
passed in 4.11 seconds, including the actual workspace graph and module checks.
The allowed adapter list and empty violation baseline are unchanged. No file
exception or source-ledger mapping was added.
The full tooling suite then passed: 245 tests passed, one existing oracle-capture
test was explicitly ignored, and no tests failed (35.82 seconds).

The complete workspace gate still requires a new terminal success; passing
these focused checks does not retroactively turn the failed full run into a pass.
