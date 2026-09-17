---
"workdeck-cli": patch
---

Support large canonical planning sources with streamed input hashing up to 256 MiB
per file, retaining the 1 GiB aggregate bound. Reuse one retirement-history index
per planning list so large portfolios remain responsive as evidence accumulates.

Restore Linux and stable-Rust Windows builds by using portable Unix timestamps
and Windows handle-based file identity. File replacement still invalidates the
untracked-file watch signature even when size and modification time are unchanged.
