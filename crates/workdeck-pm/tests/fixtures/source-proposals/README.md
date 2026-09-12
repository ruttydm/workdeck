# Reserved source-selection fixture

This synthetic schema-1 configuration declares the shared-source mode planned in
PM-09. The initial file engine must reject its `sources` field as unsupported; it
must never silently treat this declaration as working shared coordination.

The names are local fixture refs, not real remotes. Once PM-09 implements this
contract, replace the unsupported assertion with actual source/proposal tests
against temporary Git repositories and local bare remotes.
