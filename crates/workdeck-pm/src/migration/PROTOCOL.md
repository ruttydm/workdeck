# Legacy migration protocol

`preview(source, destination, &PreviewOptions)` is read-only. It records explicit
configuration and import time, inventories every legacy file and directory,
records source and destination hashes, and produces conversion drafts. A complete
preview accounts for every input without blockers. Unknown timestamps, unsupported
input dispositions, unsafe paths, conflicts, and size limits remain blockers.

`apply(&preview, &request_id)` applies that exact intent. `resume(destination,
&request_id)` uses the persisted intent; it never creates a replacement preview or
fabricates historical times. Both return `MigrationReceipt`. A different request
or preview cannot take over the recorded migration.

1. Acquire the stable `.tmp/migration.lock` coordination inode and the ordinary
   writer inode. Both use bounded OS locks; process exit releases the locks. Lock
   files are never unlinked to break contention.
2. Before the admission barrier, verify the full preview fingerprint, source
   inventory, target preconditions, and encoded journal capacity. Write and sync
   the plan metadata and byte-exact draft payloads under `.tmp/migrations/<ID>/`.
   A failure here leaves no configuration or authoritative migration record.
3. Atomically publish and sync `migration.yml` with `completion: null`. Ordinary
   repository discovery, reads, writes, initialization, recovery, and staging now
   reject the partial source. Only this recorded migration can obtain the private
   engine admission token.
4. Publish immutable metadata at `migrations/<ID>/plan.json`, then bootstrap
   `config.yml` if absent. Configuration is create-only; orphaned native records
   cannot receive a new repository identity. Existing compatible configuration,
   legacy data, and unrelated destination files are preserved.
5. Apply stable batches through the normal redo-journal engine. Batches target
   8 MiB or 128 files; an individual draft may be at most 32 MiB. The preflight
   includes serialized path/dependency overhead and base64 payload sizes under
   the 64 MiB journal bound. Source input limits remain 64 MiB per file, 128 MiB
   total, 10,000 entries, and 32 directory levels. Plan and manifest metadata each
   have a 32 MiB limit. Preview reports these known size blockers.
6. Verify legacy source identity/bytes/membership and every target before each
   batch and final publication. All target preconditions also enter transaction
   read dependencies, including previously applied targets. Recheck external
   source content before journal and receipt publication. A conflict preserves
   external edits and keeps migration pending.
7. Publish `migrations/<ID>/manifest.yml` and the main `migration.apply` receipt.
   The manifest records the reviewed fingerprint, repository/request/migration
   identities, original and migrated content hashes, and exact batch IDs.
8. Publish completion in `migration.yml` through a separate transaction. Admission
   requires the immutable plan and manifest hashes, repository identity, main
   receipt, and durable cutover receipt that names the completed marker hash.
   An interrupted final replacement therefore remains unavailable until recovery.

Recovery accepts only journals whose repository, request, operation, input hash,
result, and exact changed paths/hashes match the recorded migration. A file may
match its reviewed before-state or an already applied after-state. A third value
blocks recovery without overwriting it. Restore or reconcile the conflict before
resuming the same request. Keep local journals and staged payloads until recovery
finishes; deleting `.tmp` during a pending operation is not supported.

After completion, replay validates the immutable migration records and current
repository identity. It does not require the legacy root to still exist or
require migrated issue/configuration bytes to remain unchanged forever. Normal
native changes remain valid. No automatic legacy or staging cleanup occurs.

The aggregate receipt includes bootstrap changes, batch IDs, the main receipt,
manifest identity, and cutover operation/hash. The main transaction receipt alone
covers manifest publication. Staging an entire migration must include aggregate
changed paths, `migrations/<ID>/plan.json`, manifest, `migration.yml`, and the
batch/main/cutover receipts. This protocol does not automatically stage or commit.

Durability currently requires Unix directory syncing and a filesystem honoring
locks, sync, and same-filesystem rename. Other platforms fail explicitly where
qualified durability is unavailable. This protects cooperating processes and
detects observed external edits; it does not claim to defeat a malicious process
swapping filesystem paths or bytes in a remaining check/rename interval. Legacy
commands, session scripts, extension declarations, and handoff content are never
executed by preview, apply, or recovery.
