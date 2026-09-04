# Native line highlighter

This compiled extension registers an `attention` highlighter and returns declarative source-line
ranges for host-owned Ratatui painting. It also registers a deliberately hung `hang` highlighter
used by the integration tests to verify the 1.5-second deadline and cooperative cancellation.

The request includes immutable old/new document snapshots. Extensions return source coordinates in
UTF-16 code units; they never receive terminal or renderer ownership.
