# Synthetic legacy repository

All data here is invented. Copy this directory into a temporary Git repository before invoking
commands that mutate it. No fixture command/test annotation is proof of successful execution.
Do not initialize or migrate the development checkout's own PM store during tests.

Expected migration semantics:

- `WD-1` preserves a Markdown body, date-only due date, explicit timestamps, project/cycle/label
  IDs, actor attribution, paths with spaces, abbreviated commit references, and nested extra data.
- `WD-2` deliberately omits optional fields and timestamps. The legacy loader supplies Todo,
  Medium, and current timestamps; migration must distinguish omissions and must not invent
  historical times. Its absent status maps through the documented legacy default.
- `WD-3` is historically Done. Migration must preserve that state without inventing qualification
  receipts or an exact completion timestamp.
- `WD-4` has an explicit creation/update time and Todo state; it exercises the normal Ready
  mapping independently of the missing-timestamp diagnostic in `WD-2`.
- `projects.toml` includes custom reference metadata ignored by the old typed reference loader;
  raw-table migration must retain it. `cycles.toml` uses date-only boundaries.
- Application settings remain TOML and are separate from PM schema configuration. The known
  default `paths.data_dir` is eligible for explicit root mapping in the migration preview.
- Imported session notes remain imported annotations and do not create a claim or live session.
- Events preserve their identities as historical log content. They are not check evidence.

The Markdown compatibility inventory lists baseline command and JSON behavior. These files
provide conversion inputs; passing a TOML parser check alone is not migration qualification.
