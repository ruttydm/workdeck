---
name: workdeck-extensions
description: Build trusted native Workdeck extensions using JSON-RPC 2.0 over NDJSON stdio.
---

# Workdeck native extensions

Create a compiled executable beside `workdeck-extension.toml`. Set `api_version = 1`, a safe
relative `executable`, and only the capabilities the extension needs. The first request is
`workdeck/handshake`; stdout is reserved for one JSON-RPC response per line and stderr is logs.

Validate with `workdeck extension validate PATH`. Repository extensions under
`.agents/workdeck/extensions` require an explicit `workdeck extension trust --repo . --allow --yes`.
Extensions are trusted native processes with timeouts and crash isolation, not security sandboxes.
