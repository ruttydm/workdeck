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
