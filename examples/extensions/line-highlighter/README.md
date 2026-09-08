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

The native diagnostic fixture method `example/stderr-flood` writes a 4 MiB diagnostic line,
1,100 short diagnostic lines, and a completion marker to stderr before replying on
stdout. Its integration test verifies bounded host retention, explicit truncation/drop
accounting, complete pipe drainage, and a successful subsequent protocol request.

`example/late-response-burst` settles a serialized request and then emits 128 late
replies with that request's ID. The integration test immediately runs a routed
highlighter and another serialized request; revoked output must not block either.

`example/stop-reading` acknowledges its request and then stops consuming stdin.
Unix integration tests send a large frame to saturate that pipe and require a
write deadline or cancellation to return promptly, release routed ownership,
reject later requests on the failed stream, and permit retirement. Windows uses
the same fixture but does not yet have a cancellable native pipe implementation;
these pipe-saturation tests are not claimed as Windows evidence.
