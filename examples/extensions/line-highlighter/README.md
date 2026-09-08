# Native line highlighter

This compiled extension registers an `attention` highlighter and returns declarative source-line
ranges for host-owned Ratatui painting. It also registers a deliberately hung `hang` highlighter
used by the integration tests to verify the 1.5-second deadline and cooperative cancellation.

Eager requests include immutable old/new document snapshots. With `documentReader: true`, the
example requests only the new side through `ExtensionDocumentCallbacks`. Its main input loop
continues dispatching other requests and cancellations while each source read is pending; it does
not nest a blocking read loop. It requests the side twice to exercise the host's shared-read
deduplication. Callback IDs are unique across parents and late cancelled replies are discarded.

Extensions return source coordinates in UTF-16 code units; they never receive terminal or renderer
ownership. The callback router does not supply framing or transport deadlines.
