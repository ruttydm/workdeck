# Native release metadata HTTP

The default updater release fetcher now uses Rust `ureq` with Rustls rather than
curl or PowerShell. It preserves supplied headers, uses a Workdeck User-Agent,
enforces the supplied global timeout and ten-redirect limit, returns the actual
HTTP status, and caps response bodies at 1 MiB. Invalid UTF-8 is rejected.
Cancellation is checked before and after the bounded request; an in-flight
request still relies on its timeout to finish rather than immediate interruption.

The 64 existing updater tests pass. A new real loopback HTTP test verifies request
headers, successful and non-success status/body handling, invalid UTF-8 rejection
and the body limit. This is local HTTP evidence, not public HTTPS connectivity or
cross-platform certification. Release asset downloads, direct-update shell
replacement, package-manager execution and authenticated native installation
integration remain separate work. No source ledger mapping is advanced here.

## Native archive downloads

`install::download_release` downloads a version-pinned archive and adjacent
`.sha256` file from the fixed Workdeck GitHub release repository. It shares the
updater's platform-target mapping, requires HTTPS (including redirects), limits
redirects to ten, and applies a 120-second timeout to each request. Archive bytes
are capped at 2 GiB and checksum bytes at 1 MiB without relying on Content-Length.
Files are newly created within an owned temporary directory; dropping the returned
`DownloadedRelease` or encountering a fetch failure removes that directory.
The returned paths are inputs to staging/authentication, not installation proof.

Two tests cover exact URLs/names/limits, cleanup on success and a second-download
failure, and a bounded endless reader. Download callbacks are injected in these
tests, so live GitHub HTTPS transfer remains unverified. No binary is installed
by downloading, and public updater orchestration remains unfinished.
