# Native extension runtime boundary

Hunk dynamically imported TypeScript extensions inside its Bun process. Its
`hostRuntimeModules` loader transpiled JSX and redirected React, OpenTUI, and public extension API
imports to the host's own module instances. Shared module identity was necessary for React hooks
and extension-rendered OpenTUI components.

Workdeck preserves the capability while replacing that runtime-specific mechanism. A native
extension is one compiled executable selected by `workdeck-extension.toml`. The host resolves the
manifest and its working directory to canonical identities, selects exactly the relative executable
named by the manifest (with only the platform executable suffix as a fallback), and starts it as a
subprocess. JavaScript and TypeScript files, `package.json`, `node_modules`, and source-language
imports are not consulted during discovery or process startup.

The subprocess links the `workdeck-extension-api` Rust crate at compile time and negotiates API v1
at runtime. Provider-neutral values cross newline-delimited JSON-RPC as owned Serde data, so there
is no host module identity to split:

- file views return bounded declarative rows and `ViewNode` trees; Ratatui alone renders them;
- extension errors retain the public `WorkdeckExtensionUserError` wire shape;
- helper Rust modules are compiled into the selected executable;
- canonical manifest and directory paths collapse filesystem aliases to one runtime boundary;
- files outside discovered and trusted manifest roots are never loaded or rewritten.

Loading is a two-stage host operation. `prepare_extension_load` validates every manifest, settles
compatible IDs first-wins in discovery order, snapshots per-extension configuration, and returns a
provisional load with phase `loading` before any executable starts. An API-incompatible or invalid
candidate does not claim its ID; a compatible candidate does claim it even if process startup or
the handshake later fails, so a lower-precedence duplicate cannot take over. Incremental suffix
passes reuse the same claims, notification hub, loaded processes, and terminal load control while
recording the full candidate order supplied by the caller.

`execute_extension_load` starts accepted processes sequentially and contains manifest, spawn,
protocol, and handshake failures as source-attributed issues. Each successful handshake publishes
all registrations atomically; partial registration is impossible. The host revalidates that the
manifest is byte-for-byte equivalent to its provisional value before starting the child, so a path
swap cannot acquire another ID or receive another extension's configuration. A load control retired while a
handshake is pending stays terminal, rejects the late process, and cannot be reopened by a resumed
pass. Completed result retirement revokes every runtime first and gives the full process set one
shared 250 ms shutdown deadline.

Repository trust is resolved by discovery before preparation. Explicit and user-configured native
paths are direct user intent; repository directory and repository-config candidates are omitted
until the repository has a current trust grant. The pending repository root is carried through the
load result so the Ratatui prompt can grant trust and perform a fresh pass against the same
notification hub.

The resolver lives in `workdeck-extension-host::resolve_native_extension_entrypoint`. Process
startup uses that resolver directly, so tests of the policy exercise the production path. The
`jsx-file-view` and `vim-navigation` examples additionally execute copied release-shaped binaries
from temporary directories, proving that declarative rendering, host API values, and compiled
helper modules do not depend on the Workdeck application bundle or any adjacent JavaScript
runtime.

The frozen oracle in `port/hunk/oracles/extension-host-runtime-modules.json` records all eight
baseline tests. It also records the pinned `v0.20.1` result: that older source removed the alias
coverage and fails one of its remaining seven tests on macOS because Bun canonicalizes `/var` to
`/private/var`. Workdeck keeps the later baseline's canonical-path fix.

`port/hunk/oracles/extension-host.json` separately records the 22 host-loading tests at both pins.
The stable test blob uses Hunk's older extension-only file-language shape; the baseline and native
API retain filename and glob matchers. Native dotted manifest IDs preserve Workdeck's established
SDK namespace, while leading separators, colons, invalid characters, product IDs, and bundled VCS
IDs remain refused before process startup.
