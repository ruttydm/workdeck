# Optional install-VM workflow migration

The pinned `install-vm.yml` was a manual-only Firecracker suite with an
explicit scenario input and always-uploaded structured results. The native
replacement keeps the manual-only trigger, read-only permissions, timeout,
scenario selection, and always-run oracle test while using Rust installation
transactions on the host.

Privileged Firecracker execution, Bun setup, and shell result filtering are not
retained. The native Rust oracle is the reproducible evidence source; it does
not mutate the repository or upload credentials, sockets, images, or identity
material.
