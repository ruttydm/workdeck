# Project management compatibility and migration inventory

This is the PM-00 compatibility inventory for the
[standalone implementation plan](project-management-implementation-plan.md).
The baseline was inspected at commit `c9a36ec6d6545570380e193a0a23d36cb591356f` on
2026-09-08. Source references below identify baseline symbols as well as files because line
numbers will move during implementation. **Baseline** describes existing code; **target** describes
required migration behavior, not an assertion that the new system is already implemented.

## 1. Ownership and cutover rules

The old `WorkdeckStore` is in the CLI crate, not `workdeck-store`. It stores TOML issues,
reference tables, imported agent-session records, and events. `workdeck-store` separately owns
machine-local application state. Moving PM data must not merge those responsibilities.

The target is one `workdeck-pm` application API used by CLI and TUI, with `.workdeck/` as the
repository root. Before verified migration, the new store is explicitly selected. Existing and
new stores together without a verified cutover manifest must produce an ambiguity diagnostic.
After verified cutover, all active writers use the new source. Legacy files remain available
until explicit cleanup; normal startup does not delete or migrate them.

Compatibility preserves identity, meaning, useful entrypoints, configuration, source links,
and automation contracts. It does not preserve silent overwrite, unsafe replacement, duplicate
execution, missing validation, or a route around acceptance policy.

### Implemented cutover behavior and current qualification

Ordinary commands treat a selected legacy planning source as read-only compatibility.
Issue/reference/session mutations and applying top-level imports return `legacy_store`
with explicit migration guidance before creating a record, event, or legacy scaffold.
List/show, search, events, export and immutable import preview remain available.
Native entrypoints keep their established spelling; native records and receipts use
the documented versioned envelope. The established lifecycle tests are being moved
to native fixtures deliberately, and duplicate prototype writer helpers are being
removed. See the [cutover validation checkpoint](project-management-validation.md#cutover-work-in-progress--2026-09-09)
for the current combined-qualification status.

Application preferences are separate from PM records. An explicit `config init` or
validated `config set` copies the complete selected legacy TOML layer into
`.workdeck/config.toml`, retaining old bytes, comments, unknown settings and original
permissions. Invalid candidates reject before creating a native root; locked publication
rechecks the selected source and destination. Review preference saves against a legacy
repository layer require that explicit move and cannot silently save to the global layer.
Global and native saves retain their normal routing. App-config-only local lock and
temporary files are ignored independently of PM initialization.

Canonical-root creation changes automatic extension discovery. A remaining legacy
extension directory produces an actionable notice; explicit repository-configured
paths preserve access under the existing trust rules. No extension binary is copied,
run, or newly trusted by preference migration.

## 2. Existing issue commands and target behavior

Definitions: [`IssueCommand` and `IssueLabelCommand`](../crates/workdeck-cli/src/main.rs).
Handlers: `handle_issue_command`, `issue_create_input`, `issue_update`, `filter_issues` in the
same file. Storage: [`WorkdeckStore` and `IssueUpdate`](../crates/workdeck-cli/src/store/mod.rs).
All commands below accept `--json`.

| Baseline command | Baseline behavior | Required target treatment |
| --- | --- | --- |
| `issue list` | Exact filters `--status`, `--priority`, `--project`, `--cycle`, `--label`, `--assignee`, `--due-at`; numeric issue-key order | Preserve filters and deterministic ordering; document new cursor/order fields and short-ID resolution |
| `issue create [TITLE]` | Optional `--from-json PATH` (`-` reads stdin); field flags override input JSON; absent status/priority become `todo`/`medium`; implicitly initializes legacy store | Preserve entrypoint, flags, and input precedence; validate the entire request before any writes; new IDs use ULIDs; default status change to `ready` must be documented |
| `issue update KEY` | Optional title/description/status/priority/project/cycle/assignee/due-at; supplied labels replace all labels; supplied commits append/deduplicate | Preserve update semantics for these flags; add explicit clearing and expected-source support; do not change append into replacement accidentally |
| `issue link KEY PATH` | Alias behavior for linking a file; action string is `link-file` | Keep as an alias to the shared file-link operation |
| `issue link-file KEY PATH` | Adds an exact path string once and sorts file list | Preserve strings and idempotent set membership; distinguish unresolved references from verified files |
| `issue unlink-file KEY PATH` | Removes matching file strings | Shared unlink operation; no direct CLI file writes |
| `issue link-commit KEY SHA` | Adds a string once; abbreviations are accepted without Git verification | Preserve original reference; resolve qualified source separately, without asserting an abbreviated SHA is verified |
| `issue unlink-commit KEY SHA` | Removes matching commit strings | Shared unlink operation |
| `issue close KEY` | Assigns `done` without acceptance checks | Alias to `issue done`; enforce the same completion contract |
| `issue reopen KEY` | Assigns `todo` | Reopen into configured unstarted state, default `ready`; retain old CLI spelling and document intentional status mapping |
| `issue move KEY --status STATUS` | Assigns any fixed enum status directly | Shared validated transition; transitions into completion enforce completion policy |
| `issue assign KEY ASSIGNEE` | Stores any string | Preserve imported attribution; target identity validation must not silently drop unknown historical actors |
| `issue unassign KEY` | Stores the empty string | Explicit absent assignment in target; compatibility serialization may retain empty-string form |
| `issue label add KEY LABEL` | Adds exact label once; no reference lookup | Shared validated operation; migrate existing dangling labels visibly |
| `issue label remove KEY LABEL` | Removes label and touches issue | Shared operation; replay must not duplicate history |
| `issue show KEY` | Exact-key match; human output is only key and title; JSON includes full issue | Keep exact existing identities; richer human detail is permitted; version changed JSON representation |
| `issue delete KEY --yes` | Deletes issue file; logs event; deleted high IDs can later be reallocated | Keep confirmed deletion entrypoint only with explicit reference resolution/tombstone semantics; prefer archive; never reuse identity |

Create flags are `--description`, `--status`, `--priority`, `--project`, `--cycle`, `--assignee`,
`--due-at`, comma-delimited `--label`, comma-delimited `--commit`, and repeated `--file`.
Update has the same field flags except `--file` and `--from-json`; files use link commands.
An empty label vector in the baseline update means "no update", not "clear all labels".

Baseline normalized CLI status aliases ignore non-ASCII-alphanumeric separators and case:
`in-progress`/`inprogress`/`progress`, `in-review`/`inreview`/`review`, and `done`/`closed`.
Priority aliases are `none`/`no`, `medium`/`med`, and `urgent`/`critical`, plus `low` and `high`.
TOML deserialization itself uses exact kebab-case enum spellings. Preserve useful CLI aliases
without claiming arbitrary legacy file spellings were supported.

The current create implementation writes the initial issue before parsing all requested fields;
an invalid status can therefore leave an issue behind. This is a defect to remove, not a
compatibility promise. The existing test `issue_command_rejects_invalid_status` checks failure
text but does not establish absence of a partial write.

## 3. Record mapping

Baseline types: `Issue`, `Project`, `Cycle`, `Label`, `ReferenceData`, `AgentSession`,
`AgentTouchedFile`, and `StoreEvent` in
[`store/mod.rs`](../crates/workdeck-cli/src/store/mod.rs).

| Legacy issue field | Target semantic mapping |
| --- | --- |
| `key` | Immutable issue identity; preserve `WD-N` byte-for-byte; qualify with repository identity when crossing sources |
| `title` | Issue title |
| `description` | Markdown body; preserve content and intentional line breaks |
| `status` | Explicit workflow mapping below |
| `priority` | Preserve `none`, `low`, `medium`, `high`, `urgent` meanings |
| `project`, `cycle` | Stable references to migrated reference IDs; empty string becomes absent |
| `assignee` | Historical identity reference/attribution; empty string becomes absent |
| `created_at`, `updated_at` | Preserve original strings and precision; report invalid timestamps instead of inventing a time |
| `due_at` | Preserve date-only versus timestamp meaning; empty string becomes absent |
| `labels` | Preserve stable label IDs and unresolved membership explicitly |
| `linked_files` | File source references, preserving original repository-relative text and paths containing spaces |
| `linked_commits` | Commit source references, preserving full or abbreviated original text |
| Flattened `extra` | Preserve arbitrary TOML metadata under an explicit legacy/custom namespace, with type-preserving conversion and diagnostics for unsupported YAML representations |

| Legacy stored status | Target default state | Category |
| --- | --- | --- |
| `inbox` | `inbox` | Triage |
| `backlog` | `backlog` | Backlog |
| `todo` | `ready` | Unstarted |
| `in-progress` | `in_progress` | Started |
| `in-review` | `in_review` | Review |
| `done` | `done` | Completed |

The mapping is explicit; migration records the original `todo` token where needed for legacy
round trips. Migrated `done` records are historical completion declarations. Migration must not
fabricate a completion timestamp, reviewer, check receipt, or qualified feature maturity.
The baseline has no `verification` or `canceled` issue state; target states are additions.

`Issue` defaults missing timestamps using the current clock during deserialization. Migration
must parse the raw table to distinguish a missing historical timestamp from an actual value;
it must not accidentally certify the migration time as the original creation time. Similarly,
only issue and agent-session structs flatten unknown fields. Reference-table structs do not;
raw-table migration is required to retain manually added project/cycle/label metadata.

Reference IDs are explicit or generated from an ASCII lowercase name slug. Explicit IDs permit
ASCII letters, digits, `-`, and `_`. Existing IDs are stable references even if the name changes.

| Legacy reference record | Fields to retain | Target addition constraints |
| --- | --- | --- |
| Project (`[[projects]]`) | `id`, `name`, `description`, `status`, `created_at`, `updated_at` | Preserve free-form status; do not invent lead, acceptance criteria, initiative, milestone, or delivery qualification |
| Cycle (`[[cycles]]`) | `id`, `name`, `starts_at`, `ends_at`, `status` | Preserve date strings; do not reinterpret cycle as release |
| Label (`[[labels]]`) | `id`, `name`, `color` | Preserve named/hex/custom color strings; validate new values without losing historical ones |

## 4. Reference commands

Definitions: `ProjectCommand`, `CycleCommand`, `LabelCommand`; handlers:
`handle_project_command`, `handle_cycle_command`, `handle_label_command` in
[`main.rs`](../crates/workdeck-cli/src/main.rs). All accept `--json`.

| Family | Baseline commands and flags | Required target treatment |
| --- | --- | --- |
| Project | `list [--status]`, `show ID`, `save NAME [--id --description --status]`, `delete ID --yes [--force]` | Preserve `save` as an explicit upsert adapter alongside new create/update/archive operations |
| Cycle | `list [--status]`, `show ID`, `save NAME [--id --starts-at --ends-at --status]`, `delete ID --yes [--force]` | Same; preserve date-only inputs and memberships |
| Label | `list [--color]`, `show ID`, `save NAME [--id --color]`, `delete ID --yes [--force]` | Same; preserve label IDs and membership |

Baseline `save` updates only supplied optional fields and sorts the full reference array by ID.
Delete rejects referenced records unless `--force`; force deletes the reference and then clears
the corresponding project/cycle/label value across issues in separate writes. The target must
validate a concrete resolution plan and make this a recoverable change set. It must not emulate
partial success. No hidden reassignment or unresolved reference deletion is acceptable.

## 5. JSON, JSONL, and failure contracts

Baseline emitters: `json_success`, `print_json_error`, `classify_exit_code`,
`print_jsonl_record`, `print_event_jsonl_record`, and `print_issue_action` in
[`main.rs`](../crates/workdeck-cli/src/main.rs).

```json
{"ok":true,"kind":"issue","action":"create","data":{"key":"WD-1","title":"Example"}}
```

The example omits other issue fields for brevity. `action` is absent for read operations.
Lists put arrays directly in `data`; there is no baseline `items` wrapper or schema-version field.
Kinds include `issue`, `issue_list`, `project`, `project_list`, `cycle`, `cycle_list`, `label`,
`label_list`, `agent_session`, `agent_session_list`, `event_list`, `export`, `import`, and
`search_results`. Mutations carry an action such as `create`, `update`, `link-file`, `close`,
`reopen`, `save`, or `delete`. Issue deletion returns `{ "deleted": true, "key": "WD-N" }`.

The target should retain `ok`, `kind`, and `data` envelope semantics for established entrypoints,
while explicitly versioning record changes, pagination, source metadata, and receipts. Keep
legacy `key` and link fields in a compatibility projection or declare a versioned migration;
do not silently change arrays into page objects for existing consumers. New machine APIs need
typed errors rather than substring classification.

Baseline JSON failures go to stdout as `{ "ok": false, "error": { "code": "…", "message": "…" } }`.
Exit codes are 1 general, 2 validation, 3 not found, 4 conflict, 5 config/store. The current
implementation classifies message substrings; the same semantic codes should be retained where
applicable, with structured paths, remediation, and conflict/retry metadata added. Clap parser
errors occur separately before handlers; noninteractive output tests cover them too.
PM-07's new `context`, `next`, `question`, `handoff`, `protocol` and `issue next`
commands render parser validation failures through the bounded native error envelope
when machine output is requested. No source is read until arguments validate.
Help/version and older entrypoint parser presentation retain their existing behavior.

JSONL export records use `{ "kind": "issue", "payload": { … } }`, not the JSON success envelope.
The first record is `repo` with `{ "root": PATH }`; kinds also include `project`, `cycle`,
`label`, `agent_session`, and `event`. Event payload contains `kind`, `payload`, and `created_at`.

Search targets use `{ "kind": "issue", "key": "WD-N" }`, or `project`/`cycle`/`label` with `id`.
The baseline `--target issues` group includes all four. See
[`search_target_payload` and `search_target_group`](../crates/workdeck-cli/src/payload.rs).
Add source qualification without dropping established identity fields or changing search groups
without documentation. The current in-memory search rebuild reads the whole store; it is not
the future persistent PM projection.

## 6. Export and import hazards that migration must handle

Baseline `handle_export` and `handle_import_command` live in
[`main.rs`](../crates/workdeck-cli/src/main.rs).

- `export` without flags emits a bare JSON object with `repo_root`, `issues`, `projects`,
  `cycles`, `labels`, `agent_sessions`, and `events`.
- `export --json` wraps that object in the success envelope; `--jsonl` emits typed records.
- `import PATH [--merge | --replace] [--dry-run] [--json]` currently reads only a bare JSON
  object. It does not unwrap `export --json`, accept JSONL, or accept stdin. A wrapped export
  can be interpreted as zero records because absent arrays silently default to empty.
- `--merge` is ignored as a separate behavior: regular import overwrites matching issue/session
  files and upserts reference records. Project upsert does not preserve exported timestamps.
- `--replace` recursively deletes the entire active data directory, including config, extensions,
  or unrelated data colocated there. This behavior must not survive into the new import engine.
- Events are exported but ignored by the importer. Imports append a new `import_completed` event.
- Dry run deserializes but does not exercise all write-time validation or destination collisions.

Target import must recognize supported bare/enveloped/versioned formats explicitly, reject
unknown shapes, preview collisions and affected records, preserve timestamps and metadata, and
apply recoverable scoped changes. Repeated operation IDs return original receipts. JSONL
support, if advertised, must validate record types and reject malformed/unknown records rather
than silently skipping them. Historical events remain history, not verified execution evidence.

## 7. Every active legacy-root consumer

This table covers production readers/writers discovered by searching `.agents`,
`WorkdeckStore`, `data_dir`, issue/session path helpers, and repository boundary discovery in
all product/xtask Rust sources. Test fixtures and documentation are listed separately below.

| Consumer and source symbol | Baseline path/behavior | Required target treatment |
| --- | --- | --- |
| [`config.rs`](../crates/workdeck-cli/src/config.rs): `default_data_dir`, `resolve_repo_data_dir`, `Config::load`, `load_for_review`, `load_for_extension_cli` | Default data root `.agents/workdeck`; repo app config always discovered under that default path | Single explicit cutover resolver; new app config `.workdeck/config.toml`; PM schema config `.workdeck/config.yml` |
| Same: `Config::data_dir`, `PathsConfig` | `[paths].data_dir` can be repo-relative or absolute and relocates the store, but not where repo config is initially discovered | Preserve explicit override intent; inventory custom roots and avoid rewriting external locations automatically; distinguish app data from PM planning source |
| [`main.rs`](../crates/workdeck-cli/src/main.rs): repository command composition, `--init` | Constructs `WorkdeckStore` from configured root; init writes legacy scaffold | All PM handlers share selected PM source; retain `--init` as alias to new init |
| Same: `handle_config_command` | Hardcoded `.agents/workdeck/config.toml` for `path`, editing, and settings writes; `init` uses store root instead | Cut over all config actions together and eliminate disagreement between resolved path and write destination |
| Same: `handle_doctor`, `handle_export`, `handle_import_command`, `handle_events_command`, `handle_search_command` | Load legacy issues, reference data, sessions and events through store | Shared source resolver with explicit legacy compatibility; schema-aware doctor; export/import mappings above |
| Same: `handle_issue_command`, reference handlers, agent handlers | Legacy mutation paths include direct issue/session deletion and read-modify-save outside the store | Route PM mutations through one API; retain imported sessions separately |
| [`store/mod.rs`](../crates/workdeck-cli/src/store/mod.rs): `init` and all path helpers | Creates `issues/`, `agents/`, `index/`, `config.toml`, `projects.toml`, `cycles.toml`, `labels.toml`; appends `events.jsonl` | Explicit per-kind conversion; no whole-directory rename masquerading as migration |
| Baseline `crates/workdeck-cli/src/app.rs` (removed): `App::new`, refresh and PM actions | Built old store and reloaded full arrays; status/priority/label/assignment actions directly mutated `Issue` then `save_issue` | Replaced by [native workbench operations](../crates/workdeck-tui/src/workbench/mod.rs); removed the closed old UI island |
| Same: `open_selected_issue_in_editor`, `open_selected_agent_in_editor` | Opens legacy TOML files in external editor | Issue editing uses safe draft/validate/apply with expected content; imported-session editor retains separate format |
| [`search/mod.rs`](../crates/workdeck-cli/src/search/mod.rs): `SearchIndex::rebuild`; baseline `crates/workdeck-cli/src/views/mod.rs` (removed): issue rendering | Consumed legacy `Issue` and `ReferenceData`; fixed status groups | [Native provider](../crates/workdeck-cli/src/repository_panels/mod.rs) preserves configured workflows and source-qualified selection; the old renderer is removed |
| [`workdeck-migration/src/lib.rs`](../crates/workdeck-migration/src/lib.rs): `plan_with_roots` | Hunk repo-config import destination `.agents/workdeck/config.toml` | New target `.workdeck/config.toml`, respecting the same cutover conflict rules; do not recreate obsolete root |
| [`workdeck-vcs/src/catalog.rs`](../crates/workdeck-vcs/src/catalog.rs): `find_project_root_candidate_with_catalog` | Recognizes nearest ancestor directory `.agents/workdeck` as a boundary alongside VCS adapters | Recognize `.workdeck`; legacy boundary only as explicit compatibility; nested directories and non-directory markers remain correctly handled |
| [`workdeck-extension-host/src/extension_discovery.rs`](../crates/workdeck-extension-host/src/extension_discovery.rs): `discover_manifests_with_config` | Hardcoded `.agents/workdeck/extensions` plus configured repo paths; repository trust gates execution | Move canonical default to `.workdeck/extensions`; preserve manifests/binaries and explicit paths; retain repository trust and no-execution-on-migration guarantees |
| [`workdeck-tui/src/lib.rs`](../crates/workdeck-tui/src/lib.rs): repository extension trust prompt | Displays `.agents/workdeck/extensions` | Prompt must describe actually resolved canonical/legacy source; do not leave stale path text |

Repository app configuration layers defaults, then global TOML, then repository TOML. Review
preferences additionally apply command and pager sections within each layer. Native extension
config uses a deliberate shallow property merge for opaque extension values. Global keybindings
are read from the user config. Preserve these semantics; PM YAML must not become a replacement
parser for them. `view_preferences_config_path` chooses the existing repo config or falls back
to user config, and must follow the selected root after cutover.

Global config, state, and extensions stay under the existing user configuration directory
(`XDG_CONFIG_HOME`, otherwise home configuration resolution). See
[`workdeck-core/src/paths.rs`](../crates/workdeck-core/src/paths.rs):
`resolve_global_config_path`, `resolve_app_state_path`, `resolve_global_extensions_dir`.
The global `workdeck/state.json` and review-session protocol are not repository PM records.
The forgiving app-state loader in
[`workdeck-store/src/app_state_file.rs`](../crates/workdeck-store/src/app_state_file.rs)
must not be reused to interpret malformed authoritative issue files as empty state.

## 8. Imported sessions, handoffs, and review links

`agent` is the legacy metadata command family, separate from live `session` control.
`AgentCommand` currently exposes `list`, `record`, `show`, `update`, `finish`, `append-plan`,
`add-file`, `add-command`, `add-test`, `add-note`, `delete --yes`, and `import PATH`.
Agent import accepts a JSON object/array or line-delimited JSON through
`read_agent_sessions`; it does not start a process.

Preserve `id`, `title`, `agent`, `cwd`, `status`, `started_at`, `ended_at`, `goal`, `summary`,
`plan`, `commands_run`, `tests_run`, `handoff_notes`, `touched_files` (`path`, `change_type`),
and flattened extra fields. Command/test strings are historical annotations, not check receipts.
`cwd` is historical machine-local context, not stable repository identity. Migration must not
reinterpret `active` or `finished` metadata as a live process observation or issue claim.

Recommended compatibility location is `.workdeck/imported-sessions/<ID>.toml`, with explicit
provenance in the migration manifest. Preserve raw historical session metadata independently
from new structured PM handoffs; any generated handoff references its imported source and
labels unverified statements as such. Preserve existing free-form handoff files inventoried in
the old directory without guessing their schema or turning them into executable instructions.

The old PM links are issue-to-file and issue-to-commit strings. Old TUI navigation jumps from
an issue to its selected linked file, and from a file to the first issue with a matching string.
There is no existing typed issue foreign key in the review/session/core/store source schemas
found by searching `issue_key`, `issueKey`, `issue_id`, `issueId`, and `linked_issue`.
The new issue-to-review association is therefore an additive contract, not an existing relation
to invent during migration. Keep current review source identity, note IDs, hunk/line anchors,
and source-content guards intact.

## 9. Terminal behavior and keys

Baseline dispatch: [`main.rs`](../crates/workdeck-cli/src/main.rs), default startup's
`loaded.changeset.is_empty()` branch. Dirty repositories open `workdeck-tui`; clean repositories
open `workdeck_cli::tui::run(App)`. PM-04 intentionally removes this split for normal startup,
while specialized `diff`, `show`, `patch`, `pager`, `difftool`, and stash review remain supported.

Old bindings are defined in [`config.rs`](../crates/workdeck-cli/src/config.rs), emitted by
`WorkdeckStore::init`, and dispatched by
the baseline `crates/workdeck-cli/src/tui/mod.rs` (now removed). Native key ownership
and actions live in the [workbench shell](../crates/workdeck-tui/src/workbench/shell.rs):

| Default key / setting | Existing meaning | Target contract |
| --- | --- | --- |
| `i` / `issues` | Issues tab | Persistent issue navigation |
| `n` / `new_issue` | Creates "Follow up PATH" from selection, or "New issue" | Contextual create flow preserving selected source |
| Enter or `e` / `edit_issue` | Opens issue TOML in editor; terminal restored around editor | Validated issue editing; preserve terminal lifecycle and draft |
| `s` / `status` | Cycles fixed enum, including Done back to Inbox | Configured workflow action with acceptance checks; no raw enum bypass |
| `p` / `priority` | Cycles priority | Shared priority operation; resolve conflicts with review key map contextually |
| `l` / `labels` | Cycles membership through configured labels | Shared label operations |
| `A` / `assign` | Toggles current `$USER` assignment | Explicit identity-aware shared assignment |
| Space / `jump` | Issue/file navigation | Preserve return issue, file/hunk/source, preview, and draft context |
| `L` / `link_file` | Link selected file to selected issue | Source-qualified file link |
| `/`, `r`, `t`, `?`, `q` | Search, refresh, preview toggle, help, quit | Preserve user remaps and terminal behavior |

The baseline tab view groups by the fixed `IssueStatus::ALL` sequence, independently of numeric
storage order. Selection recovery uses exact keys; replace it with stable qualified identities,
not list indices. Current narrow-layout tests ensure issue content is shown instead of a linked
file preview; retain that behavior while introducing the unified workbench.

## 10. Generated skills, tests, and secondary references

The current generated skill is `skills/workdeck-review/SKILL.md`, rendered from the typed
session command surface in `workdeck-session`. [`xtask/src/skill.rs`](../xtask/src/skill.rs)
checks generated output and pinned mappings for extension/release skills.
`workdeck skill path [NAME]` is composed by `handle_skill_command` in `main.rs`, which embeds
review, extensions, release, and launch-video assets and materializes them under the user config
directory. There are no separate `show` or `install` subcommands and no existing generated PM skill.
PM-07 should add a distinct catalog-driven PM skill without changing the live-review protocol
or hand-editing generated review text.

The extension skill now documents `.workdeck/extensions/`, read-only legacy discovery before
the native root exists, and explicit configured paths under the existing trust rules. Its updated
destination digest and adaptation evidence are recorded in
[`port/hunk/oracles/bundled-skills.json`](../port/hunk/oracles/bundled-skills.json), while retaining
the pinned upstream source identities and complete source-section coverage. The existing
`cargo xtask skill check` passes; the checker has not been disabled or weakened.

| Existing validation / reference | Required migration work |
| --- | --- |
| [`crates/workdeck-cli/tests/cli.rs`](../crates/workdeck-cli/tests/cli.rs) | Init/path expectations; issue lifecycle/aliases/filtering; reference CRUD; agent metadata; JSON/error envelopes; config/doctor; export/import; extension discovery |
| Store tests in `store/mod.rs` | Legacy fixtures remain readable; new store tests cover safe writes rather than preserving sequential-ID allocation |
| App/view tests in `app.rs`, `views/mod.rs` | New shared PM operations and equivalent narrow layouts, selection, linked-file jumps |
| [`terminal_pager.rs`](../crates/workdeck-cli/tests/terminal_pager.rs), `terminal_lifecycle.rs`, `terminal_pager/extensions.rs`, `terminal_pager/layout.rs` | Clean/dirty workbench interactions and existing pager/session/extension trust behavior; no read-side initialization |
| [`examples/tests/startup_lifecycle.rs`](../examples/tests/startup_lifecycle.rs) | Repository extension fixture root follows new discovery; explicit legacy cases remain labeled |
| `xtask` benchmark fixtures, working-tree/bootstrap/release checks, release-status/release-notes tests | Existing assertions of no `.agents` creation must additionally reject unexpected `.workdeck` initialization; changing only the old path assertion would weaken coverage |
| [`README.md`](../README.md), [`crates/workdeck-cli/README.md`](../crates/workdeck-cli/README.md), [`docs/themes.md`](themes.md), [`docs/MIGRATION.md`](MIGRATION.md) | Update active user guidance and app-config paths |
| `docs/workdeck-plan.md`, `docs/files-ux-and-cli-plan.md`, PM reference spec | Historical path descriptions need an explicit historical/superseded note; do not rewrite archived specification as if it originally described the new root |
| `examples/extensions/file-view-gallery/mixed-review/fixtures/{before,after}/README.md` | Frozen diff-content fixtures contain legacy paths intentionally; retain as historical content unless deliberately regenerating the fixture and its evidence |

## 11. Synthetic migration fixture and acceptance checklist

[`legacy-repo`](../crates/workdeck-pm/tests/fixtures/legacy-repo) contains synthetic legacy TOML
and JSONL only; it has no real user data or executable extension. Copy it into a temporary Git
repository for mutation tests. It includes a minimal issue, a rich issue with Markdown, custom
metadata and links, historical completed work, reference tables, app configuration, an imported
session and events. The fixture README defines expected semantic preservation and intentionally
unverified fields. These fixtures are inputs, not proof of migration implementation.

- [ ] Every command above reaches the shared PM application operation after cutover.
- [ ] Bare/enveloped export input cannot silently import zero records due to a shape mismatch.
- [ ] Legacy IDs, dates, body, extra fields, references, and attribution survive verified conversion.
- [ ] Missing timestamps and old `done` states do not acquire invented evidence.
- [ ] Partial destination conflicts preserve both sources and prevent cutover.
- [ ] Migration reruns resume or return the prior receipt without duplicate records/history.
- [ ] Custom data roots and repository config discovery receive explicit mappings.
- [ ] Issue editing, imports, forced reference resolution, and UI actions cannot bypass validation.
- [ ] Extension discovery, trust text, Hunk migration, VCS boundaries, and config writes agree on the new root.
- [ ] Imported session metadata, structured PM handoffs, and live review sessions remain distinct.
- [ ] Read-only commands leave both legacy and new authoritative roots untouched.
- [ ] Tests and active docs cover the new root; historical fixtures remain explicitly historical.

Migration and cutover evidence belongs in the canonical implementation plan as it is obtained.
