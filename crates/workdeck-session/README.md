# workdeck-session

Runtime-neutral session broker daemon, authenticated caller client, producer connection helper, and
Workdeck review-session behavior.

This is Workdeck's main native broker crate. It combines the low-level broker primitives and the
runtime-neutral orchestration that were previously split across Hunk workspaces. The crate is an
internal foundation layer (`publish = false`); application code normally reaches it through the
single shipped `workdeck` executable. Native extensions use `workdeck-extension-api` and the
subprocess host instead.

The security and compatibility contract includes signed producer/caller hello, short-lived caller
sessions, replay admission, strict message parsing, resource budgets, and default-deny raw HTTP
authorization. Credential discovery and process supervision belong to Workdeck's native
composition layer. There is no bearer-token fallback and no JavaScript runtime.

## When to use this crate

The crate provides reusable behavior to:

- track live review sessions;
- register and update immutable session snapshots;
- route a typed command to one selected live session;
- serve broker liveness plus optional authenticated list/get/dispatch controls;
- manage producer-side websocket state, heartbeats, command replies, and reconnects;
- project generic broker state into Workdeck reviews, comments, selections, and resources.

## Ownership boundaries

The former package split is represented by Rust modules behind one public crate root:

- **broker primitives** — wire models, strict parsers, selectors, canonical JSON, authentication,
  replay windows, limits, budgets, and bounded body handling;
- **runtime-neutral broker** — `SessionBroker`, `SessionBrokerDaemon`,
  `SessionBrokerConnection`, and the signed caller client;
- **Workdeck session behavior** — review projections, resources, commands, error catalog, and event
  protocol;
- **native transport composition** — loopback listeners, platform transport, discovery, and process
  lifecycle composed by `workdeck-cli`.

The crate does not own application launch policy, Herder's agent/PTTY process ownership, terminal
rendering, or extension subprocess supervision.

## Public surface

`src/lib.rs` is the single export facade. It re-exports the complete native equivalents of the
former core, types, broker, daemon, connection, crypto, authentication, caller-authentication, and
protocol-parser surfaces. Consumers do not import internal module paths.

Important entry points include:

- `SessionBroker` — raw registry, selection, ownership, stale pruning, and command FIFO;
- `SessionBrokerDaemon` — health, authenticated HTTP controls, websocket producer authority,
  reconnect reconciliation, and idle shutdown;
- `SessionBrokerConnection` — session-side registration, snapshots, heartbeat, bridge queue, and
  reconnect behavior;
- `SessionBrokerAuthenticator` — Ed25519 challenge/proof, caller request verification, response
  signing, expiry, revocation, and replay admission;
- `SessionBrokerCallerClient` — signed finite HTTP controls with challenge negotiation and response
  verification;
- `SessionBrokerProtocolParsers` — one strict app parser registry shared by all boundaries.

## Create a broker

Applications supply parsers for their immutable registration/snapshot payloads and for each command
revision. The parser registry is the only place that turns untrusted JSON into app-owned types.

```rust,ignore
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use workdeck_session::{
    SessionBroker, SessionBrokerAppParserRegistry, SessionBrokerCommandParsers,
    SessionBrokerLimitOptions, SessionBrokerOptions, create_session_broker_protocol_parsers,
    parse_session_registration_envelope, parse_session_snapshot_envelope,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionInfo { title: String }

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionState { selected_index: u64 }

let protocol_parsers = Arc::new(create_session_broker_protocol_parsers(
    SessionBrokerAppParserRegistry {
        broker_revision: None,
        app_revision: 1,
        features: Vec::new(),
        parse_registration: Arc::new(|value| {
            parse_session_registration_envelope(value, |info| {
                serde_json::from_value(info.clone()).ok()
            })
        }),
        parse_snapshot: Arc::new(|value| {
            parse_session_snapshot_envelope(value, |state| {
                serde_json::from_value(state.clone()).ok()
            })
        }),
        commands: vec![SessionBrokerCommandParsers {
            command: "select".into(),
            version: 1,
            parse_input: Arc::new(|value| value.as_u64().map(Value::from)),
            parse_result: Arc::new(|value| value.as_bool().map(Value::from)),
        }],
    },
) ?);

let broker = Arc::new(SessionBroker::new(SessionBrokerOptions {
    protocol_parsers,
    limit_options: SessionBrokerLimitOptions::default(),
    describe_session: None,
})?);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Create a daemon engine

The daemon wraps an existing broker without choosing a socket or HTTP server implementation.

```rust,ignore
use workdeck_session::{SessionBrokerCapabilities, SessionBrokerDaemon,
    SessionBrokerDaemonOptions};

let mut options = SessionBrokerDaemonOptions::new(broker);
options.capabilities = Some(SessionBrokerCapabilities {
    version: 1,
    name: Some("example-broker".into()),
    features: None,
    extra: Default::default(),
});
let daemon = SessionBrokerDaemon::new(options)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The engine can then:

- answer public liveness checks without exposing broker identity, paths, counts, or process facts;
- process websocket register/snapshot/heartbeat/result messages;
- prune stale sessions and reconcile retained producer authority;
- request shutdown after a truly idle interval;
- expose authenticated finite controls when explicitly configured.

The raw API is fail-closed. Setting `expose_http_api` does not expose a route unless the options also
contain an explicit valid `app_id`, the singleton `app_revision`, a caller authenticator, and an app
authorizer. `SessionBrokerAuthenticator` performs no filesystem, environment, coordinator, or
Workdeck credential discovery: the composition root injects grants, public verifiers, daemon signing
identity, revocation policy, and its default-deny authorization callback.

## Native transport adapter

The listener converts native HTTP requests into `SessionBrokerHttpRequest` and maps
`SessionBrokerHttpResponse` back to its platform server. Its websocket peer implements
`SessionBrokerDaemonPeer`, forwarding text frames, close codes/reasons, and the authenticated marker.
The daemon's cell-free protocol engine remains testable without opening a port.

Workdeck's shipped listener is native and loopback-only by default. Platform transport and process
lifecycle are composed in the CLI; they are not hidden inside this crate.

## Session-side connection helper

`SessionBrokerConnection` keeps an application window or live process registered with the broker.
The caller supplies a socket factory, registration, snapshot, parser registry, and command bridge.
The helper owns:

- the initial registration and later snapshot replacements;
- authenticated producer hello before registration;
- periodic heartbeats for only the current live generation;
- command-result replies and strict result parsing;
- a bounded FIFO for commands received before the bridge is ready;
- serialized bridge execution and source-socket identity fencing;
- reconnect preparation, close directives, and stop-generation fencing.

Queued or running work never migrates a late result onto a replacement socket. Resource reservations
remain charged until their actual work settles, even across disconnect and reconnect.

## Raw broker API

The daemon always recognizes its configured health path (default `GET /health`). The authenticated
capability/control API is disabled by default. When explicitly and completely enabled, the defaults
are:

- `GET /broker/capabilities`;
- `POST /broker`.

Control body shapes are the Rust wire equivalents of:

```json
{ "action": "list" }
```

```json
{ "action": "get", "selector": { "sessionId": "..." } }
```

```json
{
  "action": "dispatch",
  "selector": { "sessionId": "..." },
  "command": "...",
  "commandVersion": 1,
  "input": {}
}
```

An omitted command version deliberately defaults to revision 1. Authentication covers the exact
bounded transport bytes before strict UTF-8 and JSON decoding. Authenticated responses use
`{ "body": ..., "authentication": ... }`; the signature binds daemon generation, broker revision,
the target application contract when applicable, request ID, HTTP status, and canonical body digest.

`handle_bounded_control` shares the daemon's concurrent-control and in-flight-body budgets with
application-owned finite routes. A route may lower its body ceiling and supply its own oversized-body
response. Reservations are released on every success and failure path.

## Producer authentication and reconnects

When a producer endpoint and hello authenticator are configured, no registration-shaped value
reaches broker state before challenge/proof completion. Registration requires `register` scope;
reclaiming an existing or retained identity requires `reconnect` scope and the exact immutable
producer binding. Authority is rechecked before every producer message and every outbound command.

Disconnect retains only bounded, still-active reconnect ownership. Stale-session reconciliation
retires invalid transports before publishing replacement ownership. A displaced socket cannot use a
queued message to reclaim the session.

## Workdeck-specific layering

Generic broker lifecycle remains separate from what review data means. Workdeck layers these modules
on top:

- `workdeck_wire` and `workdeck_broker_state` for app registration, snapshots, resources, and
  lifecycle events;
- `session_bridge` and `review_commands` for semantic navigation, reload, comments, and highlights;
- `broker_projections` for lists, selected context, review export, notes, and comments;
- `review_event_protocol`, `review_mirror`, and `review_resource_cache` for bounded publication and
  transport-independent review resources;
- `daemon_protocol` and `agent_surface` for the Workdeck CLI/session command contract.

Herder remains responsible for agents, PTYs, and process ownership. The session daemon owns only
live review-session discovery, authentication, protocol, and lifecycle.

## State and side effects

Importing the crate has no side effects. Constructing a broker retains only bounded in-memory state;
constructing a daemon starts its native lifecycle clock. Repository files are not created by broker
construction, health, listing, or initial viewing. Discovery files and credential stores are managed
by their explicit native composition APIs with private permissions and atomic replacement.

## License

MIT. Translated Hunk portions retain Hunk's MIT attribution in the repository notices and semantic
port ledger. Third-party algorithms, grammars, themes, and assets are covered by
`THIRD_PARTY_NOTICES` and the generated dependency inventory.
