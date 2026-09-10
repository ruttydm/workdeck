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
