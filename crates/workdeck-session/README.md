# workdeck-session

Low-level shared primitives and app-owned behavior for Workdeck's local review-session broker.

This crate is an internal foundation layer. Application code normally reaches it through
`workdeck-cli`; native extensions use `workdeck-extension-api` and the subprocess host instead.

## Core broker boundary

The runtime-neutral core includes:

- shared session envelope types;
- registration and snapshot wire parsers;
- the bounded in-memory `SessionBrokerState`;
- selectors for session ID, session path, and repository root;
- canonical JSON, authentication, validation, budgets, and limits;
- generic terminal metadata capture.

These modules are public Rust exports from `src/lib.rs`. They correspond to the former
`@hunk/session-broker-core` package without requiring Bun, Node, or a JavaScript runtime.

## Higher-level boundary

The same crate owns Workdeck's broker protocol, review projections and resource cache, app-specific
commands, daemon HTTP contract, and live review-session models. Listener setup and process lifecycle
remain composition concerns of the Workdeck CLI and daemon.

The intended split is:

- **core primitives** — wire models, parsers, authentication, state, selectors, and budgets;
- **Workdeck session behavior** — review resources, projections, command semantics, and protocols;
- **native transport composition** — loopback listeners and process lifecycle in the shipped binary.

If code only needs extension-facing models, depend on `workdeck-extension-api` instead of this
internal session layer.

## License

MIT. Translated Hunk portions retain their attribution in the repository notices and semantic-port
ledger.
