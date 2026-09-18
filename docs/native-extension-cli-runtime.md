# Native extension CLI runtime

Workdeck exposes extension-owned top-level command trees without handing a native subprocess the
terminal. The host keeps stdin, stdout, stderr, signal, delegation, and process-lifecycle ownership;
the extension receives an immutable JSON-RPC invocation and returns a validated `exit` or
`delegate` result.

## Wire flow

The host starts `workdeck/cli/invoke` with the exact argument vector and canonical working
directory. While that request is active, the extension may send:

- `workdeck/cli/output` with a request id, `stdout` or `stderr`, and byte-exact data;
- `workdeck/cli/stdin/read` with a unique read id and a requested size of 1 through 65,536 bytes.

The host answers a read with `workdeck/cli/stdin/chunk`. It reads from terminal stdin only after the
first valid read request, so creating an iterator or running a command that never reads cannot claim
stdin. The host records `stdin_read_started` and `stdin_consumed` itself and overwrites any values a
child reports. Binary chunks, including NUL and invalid UTF-8, remain byte exact.

Output is leased to one invocation. The host drains every output notification ordered before the
final response, remembers the first local writer failure, and reports it only after the protocol
response arrives. Notifications carrying a settled request id are revoked: they are discarded and
cannot reach the terminal or be decoded as a later request's response. Delegation is rejected after
stdout output, after any stdin read attempt, for empty/NUL-bearing argv, or when argv changes
extension bootstrap flags.

SIGINT and SIGTERM become one cooperative `$/cancelRequest`. A second interrupt exits with status
130. Rust's signal backend owns one process-wide callback rather than removable JavaScript
listeners, so `ExtensionCliSignalLease::retire` revokes the callback's access to extension state and
restores the same observable default-exit behavior before delegated Workdeck work begins.

The frozen upstream results and the one-to-one test translation live in
`port/hunk/oracles/extension-cli-runtime.json`. The stable pin predates this subsystem, so the oracle
records it as absent rather than inventing a stable test result.
