# Workdeck PM command reference

Generated from the executable Clap definitions. Run `workdeck protocol render commands` to regenerate.

Document and cross-record policy is validated by the shared PM engine in addition to parser constraints.

## `index`

Explicitly refresh disposable planning indexes or inspect cached source-bound queries

```text
Usage: workdeck index [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--reference` | false |  | Full proposal ref; required only with --source proposal |
| `--source` | false | working-tree, accepted, proposal, coordination | Planning source slot; cached reads never fetch or refresh it |

## `index refresh`

Explicitly build or refresh a disposable local index; does not mutate planning files

```text
Usage: workdeck index refresh [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--rebuild` | false |  | Rebuild the selected source's disposable index from native files |
| `--reference` | false |  | Full proposal ref; required only with --source proposal |
| `--source` | false | working-tree, accepted, proposal, coordination | Planning source slot; cached reads never fetch or refresh it |

## `index query`

Read a bounded cached query; never creates, repairs or refreshes the index

```text
Usage: workdeck index query [OPTIONS] --input <INPUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--expected-query` | false |  | Exact serialized query handle from a prior response; required for later windows |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--input` | true |  | ProjectionQuery JSON file, or - for bounded stdin; see schema projection-query |
| `--json` | false |  |  |
| `--limit` | false |  | Rows per page, 1–500; see schema projection-limits |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--offset` | false |  |  |
| `--reference` | false |  | Full proposal ref; required only with --source proposal |
| `--source` | false | working-tree, accepted, proposal, coordination | Planning source slot; cached reads never fetch or refresh it |

## `index board`

Read bounded grouped issue columns from one cached query generation

```text
Usage: workdeck index board [OPTIONS] --input <INPUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--columns` | false |  | Visible columns, 1–8; columns times rows must not exceed 500 |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--expected-query` | false |  | Exact serialized query handle from a prior response; required for later windows |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--first-group` | false |  |  |
| `--help` | false |  | Print help |
| `--input` | true |  | ProjectionQuery JSON file, or - for bounded stdin; see schema projection-query |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--reference` | false |  | Full proposal ref; required only with --source proposal |
| `--rows` | false |  |  |
| `--selected` | false |  | Selected ordinal within the captured query |
| `--source` | false | working-tree, accepted, proposal, coordination | Planning source slot; cached reads never fetch or refresh it |

## `index show`

Open the bounded inert excerpt identified by an exact cached row token

```text
Usage: workdeck index show [OPTIONS] --input <INPUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--input` | true |  | ProjectionRowToken JSON file, or - for bounded stdin |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--reference` | false |  | Full proposal ref; required only with --source proposal |
| `--source` | false | working-tree, accepted, proposal, coordination | Planning source slot; cached reads never fetch or refresh it |

## `index help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck index help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--reference` | false |  | Full proposal ref; required only with --source proposal |
| `--source` | false | working-tree, accepted, proposal, coordination | Planning source slot; cached reads never fetch or refresh it |

## `index help refresh`

Explicitly build or refresh a disposable local index; does not mutate planning files

```text
Usage: workdeck index help refresh
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--reference` | false |  | Full proposal ref; required only with --source proposal |
| `--source` | false | working-tree, accepted, proposal, coordination | Planning source slot; cached reads never fetch or refresh it |

## `index help query`

Read a bounded cached query; never creates, repairs or refreshes the index

```text
Usage: workdeck index help query
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--reference` | false |  | Full proposal ref; required only with --source proposal |
| `--source` | false | working-tree, accepted, proposal, coordination | Planning source slot; cached reads never fetch or refresh it |

## `index help board`

Read bounded grouped issue columns from one cached query generation

```text
Usage: workdeck index help board
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--reference` | false |  | Full proposal ref; required only with --source proposal |
| `--source` | false | working-tree, accepted, proposal, coordination | Planning source slot; cached reads never fetch or refresh it |

## `index help show`

Open the bounded inert excerpt identified by an exact cached row token

```text
Usage: workdeck index help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--reference` | false |  | Full proposal ref; required only with --source proposal |
| `--source` | false | working-tree, accepted, proposal, coordination | Planning source slot; cached reads never fetch or refresh it |

## `index help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck index help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--reference` | false |  | Full proposal ref; required only with --source proposal |
| `--source` | false | working-tree, accepted, proposal, coordination | Planning source slot; cached reads never fetch or refresh it |

## `repository`

Inspect and explicitly register local repository checkout mappings

```text
Usage: workdeck repository [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `repository list`

List explicit checkout mappings and the registry revision

```text
Usage: workdeck repository list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `repository my-work`

List work across explicit mappings with assignment, review, overdue, blocker and claim facets

```text
Usage: workdeck repository my-work [OPTIONS] --assignee <ASSIGNEE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--repository` | false |  | Registered alias; repeat to select a subset |
| `--as-of` | false |  | RFC3339 evaluation instant; required for overdue/claimed and retained across pages |
| `--assignee` | true |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--facet` | false | assigned, review-requested, overdue, blocked, claimed |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--timeout-ms` | false |  |  |

## `repository inspect`

Inspect a checkout and prepare an exact registration request without writing

```text
Usage: workdeck repository inspect [OPTIONS] <ALIAS> <CHECKOUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `alias` | true |  |  |
| `checkout` | true |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--reference` | false |  | Full proposal ref; required only with --source proposal |
| `--source` | false | working-tree, accepted, proposal, coordination |  |

## `repository register`

Register the exact reviewed RegistryRequest JSON locally

```text
Usage: workdeck repository register [OPTIONS] --input <INPUT> --request-id <REQUEST_ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--input` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--request-id` | true |  |  |

## `repository remove`

Remove a mapping without modifying or requiring its target

```text
Usage: workdeck repository remove [OPTIONS] --expected-revision <EXPECTED_REVISION> --expected-content <EXPECTED_CONTENT> --request-id <REQUEST_ID> <ALIAS>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `alias` | true |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--expected-content` | true |  |  |
| `--expected-revision` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--request-id` | true |  |  |

## `repository show`

Resolve one exact mapping and verify its current checkout identity

```text
Usage: workdeck repository show [OPTIONS] <ALIAS>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `alias` | true |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `repository help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck repository help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `repository help list`

List explicit checkout mappings and the registry revision

```text
Usage: workdeck repository help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `repository help my-work`

List work across explicit mappings with assignment, review, overdue, blocker and claim facets

```text
Usage: workdeck repository help my-work
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `repository help inspect`

Inspect a checkout and prepare an exact registration request without writing

```text
Usage: workdeck repository help inspect
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `repository help register`

Register the exact reviewed RegistryRequest JSON locally

```text
Usage: workdeck repository help register
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `repository help remove`

Remove a mapping without modifying or requiring its target

```text
Usage: workdeck repository help remove
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `repository help show`

Resolve one exact mapping and verify its current checkout identity

```text
Usage: workdeck repository help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `repository help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck repository help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source`

Inspect planning source identities and explicitly fetch or sync

```text
Usage: workdeck source [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source proposal`

Review and publish planning proposals separately from accepted state

```text
Usage: workdeck source proposal [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source proposal preview`



```text
Usage: workdeck source proposal preview [OPTIONS] --ref <REFERENCE> --title <TITLE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--ref` | true |  | Full ref inside the configured proposal namespace |
| `--title` | true |  |  |

## `source proposal publish`



```text
Usage: workdeck source proposal publish [OPTIONS] --plan <PLAN> --expected-plan <EXPECTED_PLAN> --request-id <REQUEST_ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--expected-plan` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--plan` | true |  | Saved proposal fingerprint or exact ProposalPlan JSON file |
| `--request-id` | true |  |  |

## `source proposal status`



```text
Usage: workdeck source proposal status [OPTIONS] --request-id <REQUEST_ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--request-id` | true |  |  |

## `source proposal resume`



```text
Usage: workdeck source proposal resume [OPTIONS] --request-id <REQUEST_ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--request-id` | true |  | Resume only this request's retained original proposal |

## `source proposal help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck source proposal help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source proposal help preview`



```text
Usage: workdeck source proposal help preview
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source proposal help publish`



```text
Usage: workdeck source proposal help publish
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source proposal help status`



```text
Usage: workdeck source proposal help status
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source proposal help resume`



```text
Usage: workdeck source proposal help resume
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source proposal help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck source proposal help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source status`



```text
Usage: workdeck source status [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source fetch`



```text
Usage: workdeck source fetch [OPTIONS] --expected-config <EXPECTED_CONFIG> --expected-binding <EXPECTED_BINDING> --request-id <REQUEST_ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--expected-binding` | true |  | Exact remote binding fingerprint from source status |
| `--expected-config` | true |  | Exact configuration fingerprint from source status |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--request-id` | true |  |  |

## `source sync`



```text
Usage: workdeck source sync [OPTIONS] --expected-config <EXPECTED_CONFIG> --expected-binding <EXPECTED_BINDING> --request-id <REQUEST_ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--expected-binding` | true |  | Exact remote binding fingerprint from source status |
| `--expected-config` | true |  | Exact configuration fingerprint from source status |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--request-id` | true |  |  |

## `source help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck source help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source help proposal`

Review and publish planning proposals separately from accepted state

```text
Usage: workdeck source help proposal [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source help proposal preview`



```text
Usage: workdeck source help proposal preview
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source help proposal publish`



```text
Usage: workdeck source help proposal publish
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source help proposal status`



```text
Usage: workdeck source help proposal status
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source help proposal resume`



```text
Usage: workdeck source help proposal resume
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source help status`



```text
Usage: workdeck source help status
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source help fetch`



```text
Usage: workdeck source help fetch
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source help sync`



```text
Usage: workdeck source help sync
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `source help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck source help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim`

Acquire and maintain cooperative work claims with exact ownership tokens

```text
Usage: workdeck claim [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim contract`



```text
Usage: workdeck claim contract [OPTIONS] <ISSUE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `issue` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim list`



```text
Usage: workdeck claim list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim status`



```text
Usage: workdeck claim status [OPTIONS] <ISSUE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `issue` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim explain`



```text
Usage: workdeck claim explain [OPTIONS] <ISSUE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `issue` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim validate`



```text
Usage: workdeck claim validate [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim complete`

Complete an issue using its current claim; release remains a separate operation

```text
Usage: workdeck claim complete [OPTIONS] --token <TOKEN> --generation <GENERATION> --expected-content <EXPECTED_CONTENT> --actor <ACTOR> --request-id <REQUEST_ID> --contract <CONTRACT> --expected-contract <EXPECTED_CONTRACT> --expected-issue-revision <EXPECTED_ISSUE_REVISION> --expected-issue-content <EXPECTED_ISSUE_CONTENT> <ISSUE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--contract` | true |  | Saved contract fingerprint or exact ClaimWorkContract JSON file |
| `--expected-binding` | false |  | Required for shared actions: inspected binding from source status |
| `--expected-content` | true |  |  |
| `--expected-contract` | true |  |  |
| `--expected-issue-content` | true |  |  |
| `--expected-issue-revision` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--generation` | true |  |  |
| `--help` | false |  | Print help |
| `issue` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--release-reason` | false |  |  |
| `--release-request-id` | false |  |  |
| `--request-id` | true |  |  |
| `--token` | true |  |  |
| `--verification-file` | false |  | Authenticated CompleteVerifiedIssue JSON; keeps claim ownership and CI proof in one receipt |

## `claim acquire`



```text
Usage: workdeck claim acquire [OPTIONS] --contract <CONTRACT> --expected-contract <EXPECTED_CONTRACT> --actor <ACTOR> --request-id <REQUEST_ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--contract` | true |  | Saved contract fingerprint or exact ClaimWorkContract JSON file |
| `--expected-binding` | false |  | Required for shared actions: inspected binding from source status |
| `--expected-contract` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--request-id` | true |  |  |
| `--ttl-seconds` | false |  |  |

## `claim recover`



```text
Usage: workdeck claim recover [OPTIONS] --contract <CONTRACT> --expected-contract <EXPECTED_CONTRACT> --actor <ACTOR> --request-id <REQUEST_ID> --token <TOKEN> --generation <GENERATION> --expected-content <EXPECTED_CONTENT> --reason <REASON>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--contract` | true |  | Saved contract fingerprint or exact ClaimWorkContract JSON file |
| `--expected-binding` | false |  | Required for shared actions: inspected binding from source status |
| `--expected-content` | true |  |  |
| `--expected-contract` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--generation` | true |  |  |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--reason` | true |  |  |
| `--request-id` | true |  |  |
| `--token` | true |  |  |
| `--ttl-seconds` | false |  |  |

## `claim renew`



```text
Usage: workdeck claim renew [OPTIONS] --token <TOKEN> --generation <GENERATION> --expected-content <EXPECTED_CONTENT> --actor <ACTOR> --request-id <REQUEST_ID> <ISSUE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--expected-binding` | false |  | Required for shared actions: inspected binding from source status |
| `--expected-content` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--generation` | true |  |  |
| `--help` | false |  | Print help |
| `issue` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--request-id` | true |  |  |
| `--token` | true |  |  |
| `--ttl-seconds` | false |  |  |

## `claim revalidate`



```text
Usage: workdeck claim revalidate [OPTIONS] --token <TOKEN> --generation <GENERATION> --expected-content <EXPECTED_CONTENT> --actor <ACTOR> --request-id <REQUEST_ID> --contract <CONTRACT> --expected-contract <EXPECTED_CONTRACT> <ISSUE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--contract` | true |  | Saved contract fingerprint or exact ClaimWorkContract JSON file |
| `--expected-binding` | false |  | Required for shared actions: inspected binding from source status |
| `--expected-content` | true |  |  |
| `--expected-contract` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--generation` | true |  |  |
| `--help` | false |  | Print help |
| `issue` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--request-id` | true |  |  |
| `--token` | true |  |  |
| `--ttl-seconds` | false |  |  |

## `claim release`



```text
Usage: workdeck claim release [OPTIONS] --token <TOKEN> --generation <GENERATION> --expected-content <EXPECTED_CONTENT> --actor <ACTOR> --request-id <REQUEST_ID> --reason <REASON> <ISSUE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--expected-binding` | false |  | Required for shared actions: inspected binding from source status |
| `--expected-content` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--generation` | true |  |  |
| `--help` | false |  | Print help |
| `issue` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--reason` | true |  |  |
| `--request-id` | true |  |  |
| `--token` | true |  |  |

## `claim cancel`



```text
Usage: workdeck claim cancel [OPTIONS] --token <TOKEN> --generation <GENERATION> --expected-content <EXPECTED_CONTENT> --actor <ACTOR> --request-id <REQUEST_ID> --reason <REASON> <ISSUE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--expected-binding` | false |  | Required for shared actions: inspected binding from source status |
| `--expected-content` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--generation` | true |  |  |
| `--help` | false |  | Print help |
| `issue` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--reason` | true |  |  |
| `--request-id` | true |  |  |
| `--token` | true |  |  |

## `claim supersede`



```text
Usage: workdeck claim supersede [OPTIONS] --token <TOKEN> --generation <GENERATION> --expected-content <EXPECTED_CONTENT> --actor <ACTOR> --request-id <REQUEST_ID> --reason <REASON> <ISSUE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--expected-binding` | false |  | Required for shared actions: inspected binding from source status |
| `--expected-content` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--generation` | true |  |  |
| `--help` | false |  | Print help |
| `issue` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--reason` | true |  |  |
| `--request-id` | true |  |  |
| `--token` | true |  |  |

## `claim help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck claim help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim help contract`



```text
Usage: workdeck claim help contract
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim help list`



```text
Usage: workdeck claim help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim help status`



```text
Usage: workdeck claim help status
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim help explain`



```text
Usage: workdeck claim help explain
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim help validate`



```text
Usage: workdeck claim help validate
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim help complete`

Complete an issue using its current claim; release remains a separate operation

```text
Usage: workdeck claim help complete
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim help acquire`



```text
Usage: workdeck claim help acquire
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim help recover`



```text
Usage: workdeck claim help recover
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim help renew`



```text
Usage: workdeck claim help renew
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim help revalidate`



```text
Usage: workdeck claim help revalidate
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim help release`



```text
Usage: workdeck claim help release
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim help cancel`



```text
Usage: workdeck claim help cancel
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim help supersede`



```text
Usage: workdeck claim help supersede
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `claim help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck claim help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `hooks`

Preview and explicitly manage the local planning validation hook

```text
Usage: workdeck hooks [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |

## `hooks preview`



```text
Usage: workdeck hooks preview [OPTIONS] [HOOK]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `hook` | false | pre-commit |  |
| `--json` | false |  |  |
| `--mode` | false | install, update, remove |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |

## `hooks install`



```text
Usage: workdeck hooks install [OPTIONS] --expected-plan <EXPECTED_PLAN> --request-id <REQUEST_ID> [HOOK]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-plan` | true |  | Fingerprint from hooks preview; changed target bytes, modes, and configuration are rejected |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `hook` | false | pre-commit |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |
| `--request-id` | true |  | Exact request identity retained for retry and recovery |

## `hooks update`



```text
Usage: workdeck hooks update [OPTIONS] --expected-plan <EXPECTED_PLAN> --request-id <REQUEST_ID> [HOOK]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-plan` | true |  | Fingerprint from hooks preview; changed target bytes, modes, and configuration are rejected |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `hook` | false | pre-commit |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |
| `--request-id` | true |  | Exact request identity retained for retry and recovery |

## `hooks remove`



```text
Usage: workdeck hooks remove [OPTIONS] --expected-plan <EXPECTED_PLAN> --request-id <REQUEST_ID> [HOOK]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-plan` | true |  | Fingerprint from hooks preview; changed target bytes, modes, and configuration are rejected |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `hook` | false | pre-commit |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |
| `--request-id` | true |  | Exact request identity retained for retry and recovery |

## `hooks status`



```text
Usage: workdeck hooks status [OPTIONS] [HOOK]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `hook` | false | pre-commit |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |

## `hooks recover`



```text
Usage: workdeck hooks recover [OPTIONS] --request-id <REQUEST_ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |
| `--request-id` | true |  |  |

## `hooks help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck hooks help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |

## `hooks help preview`



```text
Usage: workdeck hooks help preview
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |

## `hooks help install`



```text
Usage: workdeck hooks help install
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |

## `hooks help update`



```text
Usage: workdeck hooks help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |

## `hooks help remove`



```text
Usage: workdeck hooks help remove
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |

## `hooks help status`



```text
Usage: workdeck hooks help status
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |

## `hooks help recover`



```text
Usage: workdeck hooks help recover
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |

## `hooks help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck hooks help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt; mutations always require a reviewed plan and request ID |

## `command`

Discover repository recipes and explicitly run reviewed local plans

```text
Usage: workdeck command [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `command list`



```text
Usage: workdeck command list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `command show`



```text
Usage: workdeck command show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `command validate`



```text
Usage: workdeck command validate [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `command plan`

Capture a named command's inputs without executing its recipe

```text
Usage: workdeck command plan [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--arguments-file` | false |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `command run`

Explicitly execute the exact reviewed local command plan

```text
Usage: workdeck command run [OPTIONS] --plan <PLAN> --actor <ACTOR> --expected-plan <EXPECTED_PLAN>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  | Actor attribution for the explicitly requested local run |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-plan` | true |  | Exact fingerprint of the reviewed plan |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--plan` | true |  | Saved plan fingerprint or a CheckPlan JSON file |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `command help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck command help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `command help list`



```text
Usage: workdeck command help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `command help show`



```text
Usage: workdeck command help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `command help validate`



```text
Usage: workdeck command help validate
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `command help plan`

Capture a named command's inputs without executing its recipe

```text
Usage: workdeck command help plan
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `command help run`

Explicitly execute the exact reviewed local command plan

```text
Usage: workdeck command help run
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `command help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck command help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check`

Plan local verification and inspect source-bound results

```text
Usage: workdeck check [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check list`



```text
Usage: workdeck check list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check show`



```text
Usage: workdeck check show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check validate`



```text
Usage: workdeck check validate [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check profile`



```text
Usage: workdeck check profile [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check profile list`



```text
Usage: workdeck check profile list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check profile show`



```text
Usage: workdeck check profile show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check profile validate`



```text
Usage: workdeck check profile validate [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check profile help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck check profile help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check profile help list`



```text
Usage: workdeck check profile help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check profile help show`



```text
Usage: workdeck check profile help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check profile help validate`



```text
Usage: workdeck check profile help validate
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check profile help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck check profile help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check plan`

Select required checks and capture exact local inputs without execution

```text
Usage: workdeck check plan [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--arguments-file` | false |  |  |
| `--changed-path` | false |  | Advisory changed path; incomplete impact information never removes required checks |
| `--check` | false |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--issue` | false |  |  |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--profile` | false |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check run`

Execute an exact reviewed plan as bounded foreground local verification

```text
Usage: workdeck check run [OPTIONS] --plan <PLAN> --actor <ACTOR> --expected-plan <EXPECTED_PLAN>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  | Actor attribution for the explicitly requested local run |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-plan` | true |  | Exact fingerprint of the reviewed plan |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--plan` | true |  | Saved plan fingerprint or a CheckPlan JSON file |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check status`



```text
Usage: workdeck check status [OPTIONS] <RUN>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `run` | true |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check export`

Export a portable terminal report with local-feedback provenance and freshness

```text
Usage: workdeck check export [OPTIONS] <RUN>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `run` | true |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check recover`

Reconcile a retained run without starting another process

```text
Usage: workdeck check recover [OPTIONS] <RUN>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `run` | true |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check results`



```text
Usage: workdeck check results [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--issue` | false |  |  |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--status` | false |  |  |

## `check explain`



```text
Usage: workdeck check explain [OPTIONS] <RUN>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--check` | false |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `run` | true |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck check help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help list`



```text
Usage: workdeck check help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help show`



```text
Usage: workdeck check help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help validate`



```text
Usage: workdeck check help validate
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help profile`



```text
Usage: workdeck check help profile [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help profile list`



```text
Usage: workdeck check help profile list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help profile show`



```text
Usage: workdeck check help profile show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help profile validate`



```text
Usage: workdeck check help profile validate
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help plan`

Select required checks and capture exact local inputs without execution

```text
Usage: workdeck check help plan
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help run`

Execute an exact reviewed plan as bounded foreground local verification

```text
Usage: workdeck check help run
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help status`



```text
Usage: workdeck check help status
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help export`

Export a portable terminal report with local-feedback provenance and freshness

```text
Usage: workdeck check help export
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help recover`

Reconcile a retained run without starting another process

```text
Usage: workdeck check help recover
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help results`



```text
Usage: workdeck check help results
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help explain`



```text
Usage: workdeck check help explain
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `check help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck check help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature`

Author native capabilities and inspect their work coverage

```text
Usage: workdeck feature [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature list`



```text
Usage: workdeck feature list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature delete`

Permanently retire a feature while retaining its source and reserving its identity

```text
Usage: workdeck feature delete [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--dry-run` | false |  | Preview native retirement and incoming references without writing |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-preview` | false |  | Require the fingerprint from a reviewed deletion preview |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--yes` | false |  |  |

## `feature relate`

Add a single symmetric related-feature link without changing declared maturity

```text
Usage: workdeck feature relate [OPTIONS] <ID> <OTHER>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-other-content` | false |  |  |
| `--expected-other-revision` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `other` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature unrelate`



```text
Usage: workdeck feature unrelate [OPTIONS] <ID> <OTHER>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-other-content` | false |  |  |
| `--expected-other-revision` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `other` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature show`



```text
Usage: workdeck feature show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature create`

Create a capability declaration with an immutable feature identity

```text
Usage: workdeck feature create [OPTIONS] [NAME]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body-file` | false |  |  |
| `--directory` | false |  | Optional grouping directory beneath .workdeck/features |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--from-json` | false |  | Read CreateFeature JSON, or '-' for stdin; explicit flags override it |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `name` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature update`



```text
Usage: workdeck feature update [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body-file` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields-file` | false |  | Read a metadata patch JSON object, or '-' for stdin |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--name` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature parent`

Change logical hierarchy while preserving feature identity and placement

```text
Usage: workdeck feature parent [OPTIONS] <ID> [PARENT]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--clear` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `parent` | false |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature move`

Move the feature file beneath a grouping directory, retaining its immutable ID

```text
Usage: workdeck feature move [OPTIONS] <ID> <DIRECTORY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `directory` | true |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature archive`



```text
Usage: workdeck feature archive [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--restore` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature custom`



```text
Usage: workdeck feature custom [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--set` | false |  | Repeat to set individual custom keys; strings require JSON quotes |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--unset` | false |  | Repeat to remove individual custom keys without changing others |

## `feature coverage`

Inspect declared feature coverage and unresolved references; issue completion does not promote maturity

```text
Usage: workdeck feature coverage [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--archive` | false | active, all, archived | Archive scope; omitted preserves the CLI default of all records (native PM) |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--milestone` | false |  | Exact milestone identity (native PM) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--query` | false |  | Literal text in issue identity, title, body, labels or assignee (native PM) |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--sort` | false |  | Repeatable sort: created_at, updated_at, priority, title or id; defaults to created_at:asc (native PM) |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--target-match` | false | all, any | Match all supplied targets (default) or any supplied target |
| `--target` | false |  | Repeatable target identity; includes direct, project and milestone membership (native PM) |

## `feature assess`

Assess a feature maturity transition without writing or treating declarations as evidence

```text
Usage: workdeck feature assess [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--to` | false |  |  |

## `feature promote`

Promote one feature maturity stage after explicit attributed acceptance

```text
Usage: workdeck feature promote [OPTIONS] --to <TO> --actor <ACTOR> --reason <REASON> <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--reason` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--to` | true | specified, implemented |  |

## `feature help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck feature help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help list`



```text
Usage: workdeck feature help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help delete`

Permanently retire a feature while retaining its source and reserving its identity

```text
Usage: workdeck feature help delete
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help relate`

Add a single symmetric related-feature link without changing declared maturity

```text
Usage: workdeck feature help relate
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help unrelate`



```text
Usage: workdeck feature help unrelate
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help show`



```text
Usage: workdeck feature help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help create`

Create a capability declaration with an immutable feature identity

```text
Usage: workdeck feature help create
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help update`



```text
Usage: workdeck feature help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help parent`

Change logical hierarchy while preserving feature identity and placement

```text
Usage: workdeck feature help parent
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help move`

Move the feature file beneath a grouping directory, retaining its immutable ID

```text
Usage: workdeck feature help move
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help archive`



```text
Usage: workdeck feature help archive
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help custom`



```text
Usage: workdeck feature help custom
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help coverage`

Inspect declared feature coverage and unresolved references; issue completion does not promote maturity

```text
Usage: workdeck feature help coverage
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help assess`

Assess a feature maturity transition without writing or treating declarations as evidence

```text
Usage: workdeck feature help assess
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help promote`

Promote one feature maturity stage after explicit attributed acceptance

```text
Usage: workdeck feature help promote
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `feature help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck feature help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate`

Author gates, resolve criteria and assess exact subjects

```text
Usage: workdeck gate [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate list`



```text
Usage: workdeck gate list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate verify-red-green`

Verify every committed gate requirement against original retained red/green proof and independent authority

```text
Usage: workdeck gate verify-red-green [OPTIONS] <INPUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `input` | true |  | RedGreenGateRequest JSON file, or '-' for stdin; requires independent authority and exact source/evidence pins |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate verify-green`

Verify every committed gate requirement against signed passed checks and independent producer authority

```text
Usage: workdeck gate verify-green [OPTIONS] <INPUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `input` | true |  | VerifiedGateRequest JSON file, or '-' for stdin; requires independent authority and exact source/evidence pins |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate delete`

Permanently retire a gate while retaining source, history and its reserved identity

```text
Usage: workdeck gate delete [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--dry-run` | false |  | Preview native retirement and incoming references without writing |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-preview` | false |  | Require the fingerprint from a reviewed deletion preview |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--yes` | false |  |  |

## `gate show`



```text
Usage: workdeck gate show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate criterion`

Resolve a stable acceptance criterion and its exact semantic definition hash

```text
Usage: workdeck gate criterion [OPTIONS] <OWNER_KIND> <OWNER> <CRITERION>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `criterion` | true |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `owner` | true |  |  |
| `owner_kind` | true | issue, feature, milestone, project |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate create`

Create a nonempty AND gate from CreateGate JSON; '-' reads stdin

```text
Usage: workdeck gate create [OPTIONS] <INPUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `input` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate update`

Apply a metadata patch JSON object to the exact inspected gate revision

```text
Usage: workdeck gate update [OPTIONS] <ID> <INPUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `input` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate archive`



```text
Usage: workdeck gate archive [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--restore` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate custom`



```text
Usage: workdeck gate custom [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--set` | false |  | Repeat to set individual custom keys; strings require JSON quotes |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--unset` | false |  | Repeat to remove individual custom keys without changing others |

## `gate assess`

Assess an explicitly declared exact subject; declaration alone never proves verification

```text
Usage: workdeck gate assess [OPTIONS] --subject-file <SUBJECT_FILE> --as-of <RFC3339> <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--as-of` | true |  | Explicit assessment time used for evidence freshness |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--subject-file` | true |  | ExactSubject JSON file, or '-' for stdin |

## `gate help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck gate help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate help list`



```text
Usage: workdeck gate help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate help verify-red-green`

Verify every committed gate requirement against original retained red/green proof and independent authority

```text
Usage: workdeck gate help verify-red-green
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate help verify-green`

Verify every committed gate requirement against signed passed checks and independent producer authority

```text
Usage: workdeck gate help verify-green
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate help delete`

Permanently retire a gate while retaining source, history and its reserved identity

```text
Usage: workdeck gate help delete
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate help show`



```text
Usage: workdeck gate help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate help criterion`

Resolve a stable acceptance criterion and its exact semantic definition hash

```text
Usage: workdeck gate help criterion
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate help create`

Create a nonempty AND gate from CreateGate JSON; '-' reads stdin

```text
Usage: workdeck gate help create
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate help update`

Apply a metadata patch JSON object to the exact inspected gate revision

```text
Usage: workdeck gate help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate help archive`



```text
Usage: workdeck gate help archive
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate help custom`



```text
Usage: workdeck gate help custom
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate help assess`

Assess an explicitly declared exact subject; declaration alone never proves verification

```text
Usage: workdeck gate help assess
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `gate help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck gate help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `evidence`

Declare immutable evidence references with explicit provenance

```text
Usage: workdeck evidence [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `evidence list`

List active declared evidence; query JSON can include superseded history

```text
Usage: workdeck evidence list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--query-file` | false |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `evidence show`



```text
Usage: workdeck evidence show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `evidence verify-red-green`

Reverify a pinned criterion-to-attestation link with independent current authority; does not complete work

```text
Usage: workdeck evidence verify-red-green [OPTIONS] --expected-evidence-content <EXPECTED_EVIDENCE_CONTENT> --authority-file <AUTHORITY_FILE> <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--authority-file` | true |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-evidence-content` | true |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `evidence declare`

Append DeclareEvidence JSON with exact source and provenance; '-' reads stdin

```text
Usage: workdeck evidence declare [OPTIONS] <INPUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `input` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `evidence supersede`

Append a correction of the exact inspected record, retaining immutable history

```text
Usage: workdeck evidence supersede [OPTIONS] --expected-evidence-content <EXPECTED_EVIDENCE_CONTENT> --reason <REASON> <ID> <INPUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-evidence-content` | true |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `input` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--reason` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `evidence help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck evidence help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `evidence help list`

List active declared evidence; query JSON can include superseded history

```text
Usage: workdeck evidence help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `evidence help show`



```text
Usage: workdeck evidence help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `evidence help verify-red-green`

Reverify a pinned criterion-to-attestation link with independent current authority; does not complete work

```text
Usage: workdeck evidence help verify-red-green
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `evidence help declare`

Append DeclareEvidence JSON with exact source and provenance; '-' reads stdin

```text
Usage: workdeck evidence help declare
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `evidence help supersede`

Append a correction of the exact inspected record, retaining immutable history

```text
Usage: workdeck evidence help supersede
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `evidence help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck evidence help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `user`

Manage repository user, agent and service identities

```text
Usage: workdeck user [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `user list`

List declared identities and the aggregate source token

```text
Usage: workdeck user list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `user show`

Show one declared identity

```text
Usage: workdeck user show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `user create`

Declare a stable user, agent or service identity

```text
Usage: workdeck user create [OPTIONS] <ID> [NAME]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--from-json` | false |  | Read a complete UserDefinition JSON, or '-' for stdin |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--kind` | false | human, agent, service |  |
| `name` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `user update`

Patch an identity while retaining unmentioned metadata

```text
Usage: workdeck user update [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--from-json` | false |  | Explicitly replace the complete UserDefinition; cannot be combined with patch flags |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--kind` | false | human, agent, service |  |
| `--name` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--set` | false |  | Repeat to set individual custom keys; strings require JSON quotes |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--unset` | false |  | Repeat to remove individual custom keys without changing others |

## `user archive`

Archive an identity, retaining historical references

```text
Usage: workdeck user archive [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--restore` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `user mode`

Choose open identities or require registered active identities

```text
Usage: workdeck user mode [OPTIONS] <MODE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `mode` | true | open, registered |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `user help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck user help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `user help list`

List declared identities and the aggregate source token

```text
Usage: workdeck user help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `user help show`

Show one declared identity

```text
Usage: workdeck user help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `user help create`

Declare a stable user, agent or service identity

```text
Usage: workdeck user help create
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `user help update`

Patch an identity while retaining unmentioned metadata

```text
Usage: workdeck user help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `user help archive`

Archive an identity, retaining historical references

```text
Usage: workdeck user help archive
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `user help mode`

Choose open identities or require registered active identities

```text
Usage: workdeck user help mode
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `user help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck user help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization`

Manage custom-field policy and inspect estimates and compliance

```text
Usage: workdeck organization [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization schema`

Inspect and change repository custom-field and estimate policy

```text
Usage: workdeck organization schema [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization schema show`

Inspect custom-field declarations and estimate units

```text
Usage: workdeck organization schema show [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization schema preview`

Preview a SchemaChange JSON against current records without writing

```text
Usage: workdeck organization schema preview [OPTIONS] <INPUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `input` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization schema apply`

Apply the exact reviewed SchemaChange when its sources still match

```text
Usage: workdeck organization schema apply [OPTIONS] --expected-preview <SHA256> <INPUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-preview` | true |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `input` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization schema help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck organization schema help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization schema help show`

Inspect custom-field declarations and estimate units

```text
Usage: workdeck organization schema help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization schema help preview`

Preview a SchemaChange JSON against current records without writing

```text
Usage: workdeck organization schema help preview
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization schema help apply`

Apply the exact reviewed SchemaChange when its sources still match

```text
Usage: workdeck organization schema help apply
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization schema help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck organization schema help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization compliance`

Inspect current policy violations; --check returns nonzero when noncompliant

```text
Usage: workdeck organization compliance [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--check` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization estimate-report`

Report exact estimate totals per unit, without combining incompatible units

```text
Usage: workdeck organization estimate-report [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--archive` | false | active, all, archived | Archive scope; omitted preserves the CLI default of all records (native PM) |
| `--assignee` | false |  |  |
| `--cycle` | false |  |  |
| `--due-at` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--label` | false |  |  |
| `--milestone` | false |  | Exact milestone identity (native PM) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--priority` | false |  |  |
| `--project` | false |  |  |
| `--query` | false |  | Literal text in issue identity, title, body, labels or assignee (native PM) |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--sort` | false |  | Repeatable sort: created_at, updated_at, priority, title or id; defaults to created_at:asc (native PM) |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--status` | false |  |  |
| `--target-match` | false | all, any | Match all supplied targets (default) or any supplied target |
| `--target` | false |  | Repeatable target identity; includes direct, project and milestone membership (native PM) |

## `organization help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck organization help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization help schema`

Inspect and change repository custom-field and estimate policy

```text
Usage: workdeck organization help schema [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization help schema show`

Inspect custom-field declarations and estimate units

```text
Usage: workdeck organization help schema show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization help schema preview`

Preview a SchemaChange JSON against current records without writing

```text
Usage: workdeck organization help schema preview
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization help schema apply`

Apply the exact reviewed SchemaChange when its sources still match

```text
Usage: workdeck organization help schema apply
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization help compliance`

Inspect current policy violations; --check returns nonzero when noncompliant

```text
Usage: workdeck organization help compliance
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization help estimate-report`

Report exact estimate totals per unit, without combining incompatible units

```text
Usage: workdeck organization help estimate-report
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `organization help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck organization help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `view`

Author and evaluate versioned issue views

```text
Usage: workdeck view [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  |  |

## `view list`



```text
Usage: workdeck view list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  |  |

## `view show`



```text
Usage: workdeck view show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  |  |

## `view run`

Evaluate the saved query and issue records from one source snapshot

```text
Usage: workdeck view run [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  |  |

## `view create`



```text
Usage: workdeck view create [OPTIONS] --name <NAME> <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--archived` | false |  | Archive this definition without changing its issue predicate |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--name` | true |  |  |
| `--no-extensions` | false |  |  |
| `--query` | false |  | Exact IssueQuery JSON; {} selects active issues |
| `--query-file` | false |  | Read IssueQuery JSON from a file or '-' for stdin |
| `--request-id` | false |  |  |
| `--stage` | false |  |  |

## `view update`



```text
Usage: workdeck view update [OPTIONS] --name <NAME> --expected-content <EXPECTED_CONTENT> <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--archived` | false |  | Archive this definition without changing its issue predicate |
| `--expected-content` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--name` | true |  |  |
| `--no-extensions` | false |  |  |
| `--query` | false |  | Exact IssueQuery JSON; {} selects active issues |
| `--query-file` | false |  | Read IssueQuery JSON from a file or '-' for stdin |
| `--request-id` | false |  |  |
| `--stage` | false |  |  |

## `view help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck view help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  |  |

## `view help list`



```text
Usage: workdeck view help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  |  |

## `view help show`



```text
Usage: workdeck view help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  |  |

## `view help run`

Evaluate the saved query and issue records from one source snapshot

```text
Usage: workdeck view help run
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  |  |

## `view help create`



```text
Usage: workdeck view help create
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  |  |

## `view help update`



```text
Usage: workdeck view help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  |  |

## `view help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck view help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  |  |

## `time`

Record and report issue time with explicit amendment history

```text
Usage: workdeck time [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `time log`

Append a time entry and capture the issue's current cycle

```text
Usage: workdeck time log [OPTIONS] --seconds <INTEGER> --worked-at <RFC3339> --user <USER> --actor <ACTOR> <ISSUE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  | Explicit identity making this declaration or correction |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `issue` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--seconds` | true |  | Exact nonnegative duration in seconds; no heuristic deduplication |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--user` | true |  | Explicit identity of the person or agent whose work is recorded |
| `--worked-at` | true |  | Timestamp of the work being declared |

## `time amend`

Append a correction of an active entry; retain its original file and cycle

```text
Usage: workdeck time amend [OPTIONS] --seconds <INTEGER> --worked-at <RFC3339> --user <USER> --actor <ACTOR> --expected-entry-content <SHA256> --reason <REASON> <ISSUE> <TIME_ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  | Explicit identity making this declaration or correction |
| `entry` | true |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-entry-content` | true |  | Exact content identity of the time entry being superseded |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `issue` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--reason` | true |  | Explicit reason for the correction |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--seconds` | true |  | Exact nonnegative duration in seconds; no heuristic deduplication |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--user` | true |  | Explicit identity of the person or agent whose work is recorded |
| `--worked-at` | true |  | Timestamp of the work being declared |

## `time list`

Read all time records for an issue, including superseded history

```text
Usage: workdeck time list [OPTIONS] <ISSUE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `issue` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `time report`

Sum active amendment chains once, using historical cycle attribution

```text
Usage: workdeck time report [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--cycle` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--from` | false |  | Inclusive work timestamp |
| `--help` | false |  | Print help |
| `--issue` | false |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--to` | false |  | Exclusive work timestamp |
| `--user` | false |  |  |

## `time help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck time help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `time help log`

Append a time entry and capture the issue's current cycle

```text
Usage: workdeck time help log
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `time help amend`

Append a correction of an active entry; retain its original file and cycle

```text
Usage: workdeck time help amend
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `time help list`

Read all time records for an issue, including superseded history

```text
Usage: workdeck time help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `time help report`

Sum active amendment chains once, using historical cycle attribution

```text
Usage: workdeck time help report
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `time help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck time help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `wiki`

Author plain Markdown in the repository wiki

```text
Usage: workdeck wiki [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  | Stage this exact operation and its receipt |

## `wiki list`

List plain Markdown wiki documents and exact content identities

```text
Usage: workdeck wiki list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  | Stage this exact operation and its receipt |

## `wiki show`

Read inert Markdown without opening links or executing content

```text
Usage: workdeck wiki show [OPTIONS] <PATH>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `path` | true |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  | Stage this exact operation and its receipt |

## `wiki create`

Create a new wiki document without replacing existing content

```text
Usage: workdeck wiki create [OPTIONS] <PATH>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body` | false |  |  |
| `--body-file` | false |  | Read exact Markdown from a file or '-' for stdin |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `path` | true |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  | Stage this exact operation and its receipt |

## `wiki update`

Replace a wiki document only at its expected content hash

```text
Usage: workdeck wiki update [OPTIONS] --expected-content <SHA256> <PATH>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body` | false |  |  |
| `--body-file` | false |  | Read exact Markdown from a file or '-' for stdin |
| `--expected-content` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `path` | true |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  | Stage this exact operation and its receipt |

## `wiki help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck wiki help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  | Stage this exact operation and its receipt |

## `wiki help list`

List plain Markdown wiki documents and exact content identities

```text
Usage: workdeck wiki help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  | Stage this exact operation and its receipt |

## `wiki help show`

Read inert Markdown without opening links or executing content

```text
Usage: workdeck wiki help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  | Stage this exact operation and its receipt |

## `wiki help create`

Create a new wiki document without replacing existing content

```text
Usage: workdeck wiki help create
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  | Stage this exact operation and its receipt |

## `wiki help update`

Replace a wiki document only at its expected content hash

```text
Usage: workdeck wiki help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  | Stage this exact operation and its receipt |

## `wiki help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck wiki help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--request-id` | false |  |  |
| `--stage` | false |  | Stage this exact operation and its receipt |

## `protocol`

Inspect the generated project-management protocol

```text
Usage: workdeck protocol [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `protocol render`

Render an installed PM protocol document without reading or modifying repository instructions

```text
Usage: workdeck protocol render [OPTIONS] <DOCUMENT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `document` | true | skill, commands, schemas |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `protocol preview`

Preview a thin AGENTS.md pointer without modifying repository instructions or recovering interrupted writes

```text
Usage: workdeck protocol preview [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--mode` | false | install, update |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `protocol install`

Explicitly install the thin AGENTS.md pointer in an initialized native repository

```text
Usage: workdeck protocol install [OPTIONS] --expected-repository <EXPECTED_REPOSITORY> --request-id <REQUEST_ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expect-absent` | false |  | Require AGENTS.md to be absent |
| `--expected-content` | false |  | Exact AGENTS.md SHA-256 from protocol preview |
| `--expected-repository` | true |  | Repository identity returned by protocol preview |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--request-id` | true |  | Stable request identity; reuse the original value and precondition to resume or replay |

## `protocol update`

Explicitly update an existing managed AGENTS.md pointer, preserving surrounding instructions

```text
Usage: workdeck protocol update [OPTIONS] --expected-repository <EXPECTED_REPOSITORY> --request-id <REQUEST_ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expect-absent` | false |  | Require AGENTS.md to be absent |
| `--expected-content` | false |  | Exact AGENTS.md SHA-256 from protocol preview |
| `--expected-repository` | true |  | Repository identity returned by protocol preview |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |
| `--request-id` | true |  | Stable request identity; reuse the original value and precondition to resume or replay |

## `protocol help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck protocol help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `protocol help render`

Render an installed PM protocol document without reading or modifying repository instructions

```text
Usage: workdeck protocol help render
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `protocol help preview`

Preview a thin AGENTS.md pointer without modifying repository instructions or recovering interrupted writes

```text
Usage: workdeck protocol help preview
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `protocol help install`

Explicitly install the thin AGENTS.md pointer in an initialized native repository

```text
Usage: workdeck protocol help install
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `protocol help update`

Explicitly update an existing managed AGENTS.md pointer, preserving surrounding instructions

```text
Usage: workdeck protocol help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `protocol help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck protocol help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `question`

Record source-bound questions and explicit answers or supersession

```text
Usage: workdeck question [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `question list`



```text
Usage: workdeck question list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--query-file` | false |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `question show`



```text
Usage: workdeck question show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `question applicability`

Inspect current or stale question applicability and permitted actions

```text
Usage: workdeck question applicability [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `question create`

Create from source-bound CreateQuestion JSON; '-' reads stdin

```text
Usage: workdeck question create [OPTIONS] <INPUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `input` | true |  |  |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `question answer`



```text
Usage: workdeck question answer [OPTIONS] --actor <ACTOR> --body-file <BODY_FILE> <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--body-file` | true |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--decisions-file` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `question supersede`



```text
Usage: workdeck question supersede [OPTIONS] --actor <ACTOR> --reason <REASON> --expected-replacement-revision <EXPECTED_REPLACEMENT_REVISION> --expected-replacement-content <EXPECTED_REPLACEMENT_CONTENT> <ID> <REPLACEMENT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-replacement-content` | true |  |  |
| `--expected-replacement-revision` | true |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--reason` | true |  |  |
| `replacement` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `question help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck question help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `question help list`



```text
Usage: workdeck question help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `question help show`



```text
Usage: workdeck question help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `question help applicability`

Inspect current or stale question applicability and permitted actions

```text
Usage: workdeck question help applicability
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `question help create`

Create from source-bound CreateQuestion JSON; '-' reads stdin

```text
Usage: workdeck question help create
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `question help answer`



```text
Usage: workdeck question help answer
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `question help supersede`



```text
Usage: workdeck question help supersede
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `question help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck question help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `handoff`

Create and inspect immutable source-bound task continuity declarations

```text
Usage: workdeck handoff [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `handoff list`



```text
Usage: workdeck handoff list [OPTIONS] --issue <ISSUE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--issue` | true |  |  |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `handoff show`



```text
Usage: workdeck handoff show [OPTIONS] --issue <ISSUE> <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--issue` | true |  |  |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `handoff create`

Append immutable CreateHandoff JSON with its inspected context anchor; '-' reads stdin

```text
Usage: workdeck handoff create [OPTIONS] <INPUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `input` | true |  |  |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `handoff help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck handoff help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `handoff help list`



```text
Usage: workdeck handoff help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `handoff help show`



```text
Usage: workdeck handoff help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `handoff help create`

Append immutable CreateHandoff JSON with its inspected context anchor; '-' reads stdin

```text
Usage: workdeck handoff help create
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `handoff help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck handoff help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from the same query and source snapshot |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--json` | false |  |  |
| `--limit` | false |  | Maximum list records (default 20, at most 100) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `context`

Obtain bounded source-bound task context without conversation history

```text
Usage: workdeck context [OPTIONS] --issue <ISSUE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--as-of` | false |  | Explicit RFC3339 time for evidence freshness; omitted means unknown |
| `--budget` | false |  | Maximum UTF-8 stdout bytes, including compact JSON envelope and newline |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-context` | false |  | Reject a changed captured context fingerprint |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--issue` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `next`

Explain suggested actions and their current source preconditions

```text
Usage: workdeck next [OPTIONS] --issue <ISSUE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-context` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--issue` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `capabilities`

Discover supported commands, schemas, and planning source availability

```text
Usage: workdeck capabilities [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  |  |

## `schema`

Inspect generated project-management data schemas

```text
Usage: workdeck schema [OPTIONS] [NAME]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `name` | false |  |  |
| `--no-extensions` | false |  |  |

## `operation`

Inspect and recover interrupted planning operations

```text
Usage: workdeck operation [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--source` | false |  | Explicit planning source directory |

## `operation pending`

Inspect pending durable writes without applying them

```text
Usage: workdeck operation pending [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--source` | false |  | Explicit planning source directory |

## `operation recover`

Finish interrupted writes when all recorded preconditions still hold

```text
Usage: workdeck operation recover [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--dry-run` | false |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--source` | false |  | Explicit planning source directory |

## `operation help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck operation help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--source` | false |  | Explicit planning source directory |

## `operation help pending`

Inspect pending durable writes without applying them

```text
Usage: workdeck operation help pending
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--source` | false |  | Explicit planning source directory |

## `operation help recover`

Finish interrupted writes when all recorded preconditions still hold

```text
Usage: workdeck operation help recover
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--source` | false |  | Explicit planning source directory |

## `operation help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck operation help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--source` | false |  | Explicit planning source directory |

## `init`

Initialize repository project management in .workdeck

```text
Usage: workdeck init [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--prefix` | false |  |  |

## `migrate legacy`

Preview, apply, or resume migration of prototype planning files

```text
Usage: workdeck migrate legacy [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--apply` | false |  |  |
| `--destination` | false |  | Native root (default: <repository>/.workdeck) |
| `--dry-run` | false |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--plan` | false |  | Apply this exact saved preview; '-' reads stdin |
| `--plan-out` | false |  | Save the exact preview to a new file for review and application |
| `--prefix` | false |  | New record prefix for preview (default: WD) |
| `--request-id` | false |  | Stable idempotency key; required for apply and resume |
| `--resume` | false |  | Resume the persisted migration without generating a new plan |
| `--source` | false |  | Prototype root (default: <repository>/.agents/workdeck) |

## `search`

Search files, changes, issues, and agent data

```text
Usage: workdeck search [OPTIONS] <QUERY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print search results as JSON |
| `--no-extensions` | false |  |  |
| `query` | true |  |  |
| `--target` | false |  | Limit targets: files,changes,issues,agents |

## `events`

Inspect Workdeck event log

```text
Usage: workdeck events [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--no-extensions` | false |  |  |

## `events list`

List event log records

```text
Usage: workdeck events list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print events as JSON |
| `--no-extensions` | false |  |  |

## `events help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck events help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |

## `events help list`

List event log records

```text
Usage: workdeck events help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |

## `events help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck events help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |

## `import`

Import a Workdeck snapshot

```text
Usage: workdeck import [OPTIONS] [PATH]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--dry-run` | false |  | Validate without writing |
| `--expected-plan` | false |  | Require the exact reviewed native import-plan fingerprint |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--imported-at` | false |  | RFC 3339 legacy import timestamp from the reviewed preview |
| `--json` | false |  | Print import result as JSON |
| `--merge` | false |  | Merge imported data |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the native noninteractive import contract |
| `path` | false |  |  |
| `--replace` | false |  | Native: replace eligible matching records, retain unrelated records; legacy: replace legacy data |
| `--request-id` | false |  | Stable idempotency key for native import |
| `--restore` | false |  | Restore missing native authority, including original receipts and repository identity |
| `--resume` | false |  | Resume a pending native restoration from its retained input |
| `--stage` | false |  | Stage only paths changed by this native import |

## `ci`

Validate immutable planning revisions for CI

```text
Usage: workdeck ci [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci red-green`

Verify signed assertion failure/pass evidence under an accepted red baseline; does not complete work

```text
Usage: workdeck ci red-green [OPTIONS] --red-report-file <RED_REPORT_FILE> --green-report-file <GREEN_REPORT_FILE> --red-artifact-file <RED_ARTIFACT_FILE> --green-artifact-file <GREEN_ARTIFACT_FILE> --policy-file <POLICY_FILE> --expected-policy <EXPECTED_POLICY> --expected-base-commit <EXPECTED_BASE_COMMIT> --expected-base-contract <EXPECTED_BASE_CONTRACT> --revision <REVISION> --check <CHECK>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--accepted-commit` | false |  |  |
| `--accepted-contract` | false |  |  |
| `--baseline-review-file` | false |  | Original signed review admitting the red baseline from prior acceptance |
| `--check` | true |  |  |
| `--expected-base-commit` | true |  |  |
| `--expected-base-contract` | true |  |  |
| `--expected-policy` | true |  |  |
| `--expected-review-policy` | false |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--green-artifact-file` | true |  |  |
| `--green-report-file` | true |  |  |
| `--help` | false |  | Print help |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |
| `--policy-file` | true |  |  |
| `--red-artifact-file` | true |  |  |
| `--red-report-file` | true |  |  |
| `--review-policy-file` | false |  |  |
| `--revision` | true |  |  |

## `ci review-coverage`

Assess retained review coverage for an exact revision and optional subject

```text
Usage: workdeck ci review-coverage [OPTIONS] --revision <REVISION>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-base-commit` | false |  |  |
| `--expected-base-contract` | false |  |  |
| `--expected-policy` | false |  |  |
| `--expected-subject` | false |  | Exact selected document hash, to detect dirty or mismatched selection |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |
| `--policy-file` | false |  |  |
| `--revision` | true |  |  |
| `--subject` | false |  | issue:ID, feature:ID, gate:ID or planning-kind:ID |
| `--working-tree` | false |  | Compare live planning contracts and declared evaluator files with the selected commit |

## `ci import-review`

Retain an authenticated contract review with durable replay

```text
Usage: workdeck ci import-review [OPTIONS] --policy-file <POLICY_FILE> --expected-policy <EXPECTED_POLICY> --expected-base-commit <EXPECTED_BASE_COMMIT> --expected-base-contract <EXPECTED_BASE_CONTRACT> --expected-commit <EXPECTED_COMMIT> --review-file <REVIEW_FILE> --actor <ACTOR> --request-id <REQUEST_ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--expected-base-commit` | true |  |  |
| `--expected-base-contract` | true |  |  |
| `--expected-commit` | true |  |  |
| `--expected-policy` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |
| `--policy-file` | true |  |  |
| `--request-id` | true |  |  |
| `--review-file` | true |  |  |

## `ci reviews`

List historical contract-review summaries without renewing trust

```text
Usage: workdeck ci reviews [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci review`

Read the original retained contract-review proof

```text
Usage: workdeck ci review [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci reauthenticate-review`

Revalidate retained review proof against independently pinned current authority

```text
Usage: workdeck ci reauthenticate-review [OPTIONS] --policy-file <POLICY_FILE> --expected-policy <EXPECTED_POLICY> --expected-base-commit <EXPECTED_BASE_COMMIT> --expected-base-contract <EXPECTED_BASE_CONTRACT> --expected-commit <EXPECTED_COMMIT> <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-base-commit` | true |  |  |
| `--expected-base-contract` | true |  |  |
| `--expected-commit` | true |  |  |
| `--expected-policy` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |
| `--policy-file` | true |  |  |

## `ci validate-reviewed`

Validate an exact candidate with authenticated contract review; does not qualify checks or completion

```text
Usage: workdeck ci validate-reviewed [OPTIONS] --base <BASE> --head <HEAD> --expected-base-commit <EXPECTED_BASE_COMMIT> --expected-base-contract <EXPECTED_BASE_CONTRACT> --policy-file <POLICY_FILE> --expected-policy <EXPECTED_POLICY> --review-file <REVIEW_FILE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--base` | true |  |  |
| `--expected-base-commit` | true |  |  |
| `--expected-base-contract` | true |  |  |
| `--expected-policy` | true |  | Reviewer policy fingerprint obtained independently of candidate files |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--head` | true |  |  |
| `--help` | false |  | Print help |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |
| `--policy-file` | true |  |  |
| `--review-file` | true |  | Original DSSE envelope signed by every required reviewer |

## `ci review-policy`

Inspect a required reviewer policy without establishing trust

```text
Usage: workdeck ci review-policy [OPTIONS] --policy-file <POLICY_FILE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |
| `--policy-file` | true |  |  |

## `ci import-report`

Import a signed report with durable replay; does not qualify completion

```text
Usage: workdeck ci import-report [OPTIONS] --report-file <REPORT_FILE> --policy-file <POLICY_FILE> --expected-policy <EXPECTED_POLICY> --expected-commit <EXPECTED_COMMIT> --actor <ACTOR> --request-id <REQUEST_ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--expected-commit` | true |  |  |
| `--expected-policy` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |
| `--policy-file` | true |  |  |
| `--red-green-file` | false |  | RetainedRedGreenProof JSON; original red report, artifacts and optional signed baseline review |
| `--report-file` | true |  |  |
| `--request-id` | true |  |  |

## `ci reports`

List retained historical report imports without renewing trust

```text
Usage: workdeck ci reports [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci report`

Read a retained historical report import

```text
Usage: workdeck ci report [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci reauthenticate`

Reauthenticate retained signed bytes under an independently pinned current policy

```text
Usage: workdeck ci reauthenticate [OPTIONS] --policy-file <POLICY_FILE> --expected-policy <EXPECTED_POLICY> --expected-commit <EXPECTED_COMMIT> <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-commit` | true |  |  |
| `--expected-policy` | true |  |  |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |
| `--policy-file` | true |  |  |

## `ci reauthenticate-red-green`

Reverify retained red/green proof with independent current baseline and producer/reviewer authority

```text
Usage: workdeck ci reauthenticate-red-green [OPTIONS] --authority-file <AUTHORITY_FILE> <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--authority-file` | true |  | RetainedRedGreenAuthority JSON obtained independently of the retained import |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci verify-imported-check`

Qualify a pinned imported check against current HEAD, inputs and independent authority

```text
Usage: workdeck ci verify-imported-check [OPTIONS] <INPUT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `input` | true |  | VerifyImportedCheck JSON with current authority and exact attestation selection |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci policy`

Inspect a producer policy and its fingerprint without establishing trust

```text
Usage: workdeck ci policy [OPTIONS] --policy-file <POLICY_FILE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |
| `--policy-file` | true |  |  |

## `ci authenticate`

Authenticate a DSSE report against an explicit producer policy and commit; does not qualify completion

```text
Usage: workdeck ci authenticate [OPTIONS] --report-file <REPORT_FILE> --policy-file <POLICY_FILE> --expected-policy <EXPECTED_POLICY> --expected-commit <EXPECTED_COMMIT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-commit` | true |  | Exact expected commit ID; revision expressions are not accepted |
| `--expected-policy` | true |  | Policy fingerprint supplied independently of the candidate and signed report |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |
| `--policy-file` | true |  |  |
| `--report-file` | true |  |  |

## `ci validate`

Validate committed planning and compare required check and subject acceptance contracts; does not execute checks or qualify completion

```text
Usage: workdeck ci validate [OPTIONS] --base <BASE> --head <HEAD>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--base` | true |  | Baseline exact commit ID, full ref, local branch name, or HEAD; selection does not establish trust |
| `--expected-base-commit` | false |  | Accepted baseline commit supplied independently of the candidate |
| `--expected-base-contract` | false |  | Accepted baseline contract fingerprint supplied independently of the candidate |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--head` | true |  | Candidate exact commit ID, full ref, local branch name, or HEAD |
| `--help` | false |  | Print help |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci plan`

Prepare revision-bound check feedback in the supplied checkout; does not execute or grant CI trust

```text
Usage: workdeck ci plan [OPTIONS] --revision <REVISION> --profile <PROFILE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--issue` | false |  |  |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |
| `--profile` | true |  |  |
| `--revision` | true |  |  |

## `ci check`

Execute an exact prepared revision-bound check plan with durable replay; results remain local feedback

```text
Usage: workdeck ci check [OPTIONS] --plan-file <PLAN_FILE> --expected-plan <EXPECTED_PLAN> --actor <ACTOR> --request-id <REQUEST_ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--expected-plan` | true |  | Exact binding.fingerprint of the reviewed revision-bound plan |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |
| `--plan-file` | true |  | CiPreparedCheck JSON or the complete successful ci plan JSON output |
| `--request-id` | true |  | Stable request identity; retain for replay and recovery |

## `ci help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck ci help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help red-green`

Verify signed assertion failure/pass evidence under an accepted red baseline; does not complete work

```text
Usage: workdeck ci help red-green
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help review-coverage`

Assess retained review coverage for an exact revision and optional subject

```text
Usage: workdeck ci help review-coverage
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help import-review`

Retain an authenticated contract review with durable replay

```text
Usage: workdeck ci help import-review
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help reviews`

List historical contract-review summaries without renewing trust

```text
Usage: workdeck ci help reviews
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help review`

Read the original retained contract-review proof

```text
Usage: workdeck ci help review
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help reauthenticate-review`

Revalidate retained review proof against independently pinned current authority

```text
Usage: workdeck ci help reauthenticate-review
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help validate-reviewed`

Validate an exact candidate with authenticated contract review; does not qualify checks or completion

```text
Usage: workdeck ci help validate-reviewed
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help review-policy`

Inspect a required reviewer policy without establishing trust

```text
Usage: workdeck ci help review-policy
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help import-report`

Import a signed report with durable replay; does not qualify completion

```text
Usage: workdeck ci help import-report
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help reports`

List retained historical report imports without renewing trust

```text
Usage: workdeck ci help reports
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help report`

Read a retained historical report import

```text
Usage: workdeck ci help report
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help reauthenticate`

Reauthenticate retained signed bytes under an independently pinned current policy

```text
Usage: workdeck ci help reauthenticate
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help reauthenticate-red-green`

Reverify retained red/green proof with independent current baseline and producer/reviewer authority

```text
Usage: workdeck ci help reauthenticate-red-green
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help verify-imported-check`

Qualify a pinned imported check against current HEAD, inputs and independent authority

```text
Usage: workdeck ci help verify-imported-check
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help policy`

Inspect a producer policy and its fingerprint without establishing trust

```text
Usage: workdeck ci help policy
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help authenticate`

Authenticate a DSSE report against an explicit producer policy and commit; does not qualify completion

```text
Usage: workdeck ci help authenticate
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help validate`

Validate committed planning and compare required check and subject acceptance contracts; does not execute checks or qualify completion

```text
Usage: workdeck ci help validate
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help plan`

Prepare revision-bound check feedback in the supplied checkout; does not execute or grant CI trust

```text
Usage: workdeck ci help plan
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help check`

Execute an exact prepared revision-bound check plan with durable replay; results remain local feedback

```text
Usage: workdeck ci help check
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `ci help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck ci help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  | Print source-bound CI output as JSON |
| `--no-extensions` | false |  |  |

## `doctor`

Validate repo, config, and local Workdeck data

```text
Usage: workdeck doctor [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-index` | false |  | Reject an index different from this reviewed content hash |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--index` | false |  | Explicit candidate index; otherwise honor the hook's GIT_INDEX_FILE |
| `--json` | false |  | Print doctor results as JSON |
| `--no-extensions` | false |  |  |
| `--staged` | false |  | Validate the actual Git candidate index and its planning relation closure |

## `export`

Export local Workdeck data as JSON or JSONL

```text
Usage: workdeck export [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Emit JSON; default unless --jsonl is used |
| `--jsonl` | false |  | Emit one JSON object per line |
| `--no-extensions` | false |  |  |

## `issue`

Manage local Workdeck issues

```text
Usage: workdeck issue [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue next`

Select ready work with source-bound eligibility and exclusion explanations

```text
Usage: workdeck issue next [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--assignee` | false |  |  |
| `--compact` | false |  | Emit compact JSON, also for human output |
| `--cursor` | false |  | JSON next_cursor from a prior page; changed query/source is rejected |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--fields` | false |  | Select result fields by dotted path; envelope identity is retained |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--label` | false |  |  |
| `--limit` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--priority` | false |  |  |
| `--project` | false |  |  |
| `--query` | false |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--status` | false |  |  |

## `issue parent`

Set or clear an issue's parent without weakening completed-parent requirements

```text
Usage: workdeck issue parent [OPTIONS] <KEY> [PARENT]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--clear` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `parent` | false |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue prerequisite`

Add, resolve, replace or explicitly waive a hard prerequisite

```text
Usage: workdeck issue prerequisite [OPTIONS] <KEY> <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue prerequisite add`



```text
Usage: workdeck issue prerequisite <KEY> add [OPTIONS] <PREREQUISITE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `prerequisite` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue prerequisite remove`

Resolve a requirement explicitly, retaining the reason in operation history

```text
Usage: workdeck issue prerequisite <KEY> remove [OPTIONS] --reason <REASON> <PREREQUISITE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `prerequisite` | true |  |  |
| `--reason` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue prerequisite replace`



```text
Usage: workdeck issue prerequisite <KEY> replace [OPTIONS] --reason <REASON> <PREREQUISITE> <REPLACEMENT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `prerequisite` | true |  |  |
| `--reason` | true |  |  |
| `replacement` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue prerequisite waive`

Record a source-bound waiver when repository acceptance policy permits it

```text
Usage: workdeck issue prerequisite <KEY> waive [OPTIONS] --actor <ACTOR> --reason <REASON> <PREREQUISITE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `prerequisite` | true |  |  |
| `--reason` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue prerequisite revoke-waiver`



```text
Usage: workdeck issue prerequisite <KEY> revoke-waiver [OPTIONS] --reason <REASON> <PREREQUISITE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `prerequisite` | true |  |  |
| `--reason` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue prerequisite help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck issue prerequisite <KEY> help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue prerequisite help add`



```text
Usage: workdeck issue prerequisite help add
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue prerequisite help remove`

Resolve a requirement explicitly, retaining the reason in operation history

```text
Usage: workdeck issue prerequisite help remove
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue prerequisite help replace`



```text
Usage: workdeck issue prerequisite help replace
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue prerequisite help waive`

Record a source-bound waiver when repository acceptance policy permits it

```text
Usage: workdeck issue prerequisite help waive
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue prerequisite help revoke-waiver`



```text
Usage: workdeck issue prerequisite help revoke-waiver
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue prerequisite help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck issue prerequisite help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue relate`

Add one canonical symmetric related link; this does not block readiness

```text
Usage: workdeck issue relate [OPTIONS] <KEY> <OTHER>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `other` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue unrelate`



```text
Usage: workdeck issue unrelate [OPTIONS] <KEY> <OTHER>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-graph` | false |  |  |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `other` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue relations`

Inspect parents, children, prerequisites, dependents and related issues from one snapshot

```text
Usage: workdeck issue relations [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue ready`

Explain hard-prerequisite readiness, including unresolved and canceled requirements

```text
Usage: workdeck issue ready [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue dependency-path`

Find a directed hard-prerequisite path; this is not a calendar forecast

```text
Usage: workdeck issue dependency-path [OPTIONS] <FROM> <TO>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `from` | true |  |  |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `to` | true |  |  |

## `issue custom`

Patch individual custom fields without replacing unmentioned values

```text
Usage: workdeck issue custom [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--set` | false |  | Repeat to set individual custom keys; strings require JSON quotes |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--unset` | false |  | Repeat to remove individual custom keys without changing others |

## `issue estimate`

Set an exact estimate in an explicit unit, or clear the estimate

```text
Usage: workdeck issue estimate [OPTIONS] <KEY> [VALUE]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--clear` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--unit` | false |  |  |
| `value` | false |  |  |

## `issue list`

List local issues

```text
Usage: workdeck issue list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--archive` | false | active, all, archived | Archive scope; omitted preserves the CLI default of all records (native PM) |
| `--assignee` | false |  |  |
| `--cycle` | false |  |  |
| `--due-at` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print issues as JSON |
| `--label` | false |  |  |
| `--milestone` | false |  | Exact milestone identity (native PM) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--priority` | false |  |  |
| `--project` | false |  |  |
| `--query` | false |  | Literal text in issue identity, title, body, labels or assignee (native PM) |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--sort` | false |  | Repeatable sort: created_at, updated_at, priority, title or id; defaults to created_at:asc (native PM) |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--status` | false |  |  |
| `--target-match` | false | all, any | Match all supplied targets (default) or any supplied target |
| `--target` | false |  | Repeatable target identity; includes direct, project and milestone membership (native PM) |

## `issue templates`

List repository issue templates

```text
Usage: workdeck issue templates [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue create`

Create a local issue

```text
Usage: workdeck issue create [OPTIONS] [TITLE]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--assignee` | false |  |  |
| `--body-file` | false |  | Read Markdown body from a file, or '-' for stdin |
| `--clear` | false |  | Repeat to clear project, cycle, milestone, targets, features or gates; overrides JSON or template values |
| `--cycle` | false |  |  |
| `--description` | false |  |  |
| `--due-at` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--feature` | false |  | Repeat to replace the issue's native feature associations |
| `--from-json` | false |  | Read issue fields from JSON file, or '-' for stdin |
| `--gate` | false |  | Repeat to replace the issue's native completion gates |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the created issue as JSON |
| `--label` | false |  |  |
| `--commit` | false |  |  |
| `--file` | false |  |  |
| `--milestone` | false |  | Milestone identity; must belong to the issue's project |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--priority` | false |  |  |
| `--project` | false |  |  |
| `--reporter` | false |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--reviewer` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--status` | false |  |  |
| `--target` | false |  | Repeat to replace the issue's direct target associations |
| `--template` | false |  | Apply a repository issue template |
| `title` | false |  |  |

## `issue update`

Update a local issue

```text
Usage: workdeck issue update [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--assignee` | false |  |  |
| `--body-file` | false |  | Read Markdown body from a file, or '-' for stdin |
| `--clear` | false |  | Repeat to clear project, cycle, milestone, targets, features or gates; overrides JSON or template values |
| `--cycle` | false |  |  |
| `--description` | false |  |  |
| `--due-at` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--feature` | false |  | Repeat to replace the issue's native feature associations |
| `--from-json` | false |  | Read update fields from JSON, or minus for stdin |
| `--gate` | false |  | Repeat to replace the issue's native completion gates |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the updated issue as JSON |
| `key` | true |  |  |
| `--label` | false |  |  |
| `--commit` | false |  |  |
| `--milestone` | false |  | Milestone identity; must belong to the issue's project |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--priority` | false |  |  |
| `--project` | false |  |  |
| `--reporter` | false |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--reviewer` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--status` | false |  |  |
| `--target` | false |  | Repeat to replace the issue's direct target associations |
| `--title` | false |  |  |

## `issue edit`

Edit issue Markdown in an external editor or from a draft file

```text
Usage: workdeck issue edit [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--from-file` | false |  | Apply a complete Markdown draft without launching an editor |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue link`

Link a file path to an issue

```text
Usage: workdeck issue link [OPTIONS] <KEY> <PATH>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the updated issue as JSON |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `path` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue link-file`

Link a file path to an issue

```text
Usage: workdeck issue link-file [OPTIONS] <KEY> <PATH>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the updated issue as JSON |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `path` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue unlink-file`

Remove a linked file path from an issue

```text
Usage: workdeck issue unlink-file [OPTIONS] <KEY> <PATH>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the updated issue as JSON |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `path` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue link-commit`

Link a commit SHA to an issue

```text
Usage: workdeck issue link-commit [OPTIONS] <KEY> <SHA>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the updated issue as JSON |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `sha` | true |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue unlink-commit`

Remove a linked commit SHA from an issue

```text
Usage: workdeck issue unlink-commit [OPTIONS] <KEY> <SHA>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the updated issue as JSON |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `sha` | true |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue link-document`

Link an inert document path, URL, or stable reference

```text
Usage: workdeck issue link-document [OPTIONS] <KEY> <REFERENCE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `reference` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue unlink-document`

Remove an issue document reference

```text
Usage: workdeck issue unlink-document [OPTIONS] <KEY> <REFERENCE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `reference` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue cancel`

Cancel an issue using a configured canceled workflow state

```text
Usage: workdeck issue cancel [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue done`

Complete an issue using its completion policy

```text
Usage: workdeck issue done [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--dry-run` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--manual-actor` | false |  |  |
| `--manual-reason` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--verification-file` | false |  | CompleteRedGreenIssue JSON with independent authority and original proof pins; supports --dry-run |

## `issue comment`

Add an immutable issue comment

```text
Usage: workdeck issue comment [OPTIONS] --author <AUTHOR> <KEY> [BODY]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--author` | true |  |  |
| `body` | false |  |  |
| `--body-file` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue attach`

Attach an inert file to an issue

```text
Usage: workdeck issue attach [OPTIONS] --author <AUTHOR> <KEY> <PATH>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--author` | true |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--media-type` | false |  |  |
| `--name` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `path` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue attachments`

List issue attachment metadata

```text
Usage: workdeck issue attachments [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue comments`

List issue comments

```text
Usage: workdeck issue comments [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue archive`

Archive an issue or restore it

```text
Usage: workdeck issue archive [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--restore` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue close`

Close an issue

```text
Usage: workdeck issue close [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the updated issue as JSON |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue reopen`

Reopen an issue as Todo

```text
Usage: workdeck issue reopen [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the updated issue as JSON |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue move`

Move an issue to a status

```text
Usage: workdeck issue move [OPTIONS] --status <STATUS> <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the updated issue as JSON |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--status` | true |  |  |

## `issue assign`

Assign an issue

```text
Usage: workdeck issue assign [OPTIONS] <KEY> <ASSIGNEE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `assignee` | true |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the updated issue as JSON |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue unassign`

Unassign an issue

```text
Usage: workdeck issue unassign [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the updated issue as JSON |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue label`

Manage issue labels

```text
Usage: workdeck issue label [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue label add`

Add a label to an issue

```text
Usage: workdeck issue label add [OPTIONS] <KEY> <LABEL>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the updated issue as JSON |
| `key` | true |  |  |
| `label` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue label remove`

Remove a label from an issue

```text
Usage: workdeck issue label remove [OPTIONS] <KEY> <LABEL>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the updated issue as JSON |
| `key` | true |  |  |
| `label` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue label help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck issue label help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue label help add`

Add a label to an issue

```text
Usage: workdeck issue label help add
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue label help remove`

Remove a label from an issue

```text
Usage: workdeck issue label help remove
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue label help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck issue label help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue delete`

Delete an issue

```text
Usage: workdeck issue delete [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--dry-run` | false |  | Preview native retirement and incoming references without writing |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-preview` | false |  | Require the fingerprint from a reviewed deletion preview |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print deletion result as JSON |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--yes` | false |  | Confirm deletion |

## `issue show`

Show one local issue

```text
Usage: workdeck issue show [OPTIONS] <KEY>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print the issue as JSON |
| `key` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck issue help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help next`

Select ready work with source-bound eligibility and exclusion explanations

```text
Usage: workdeck issue help next
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help parent`

Set or clear an issue's parent without weakening completed-parent requirements

```text
Usage: workdeck issue help parent
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help prerequisite`

Add, resolve, replace or explicitly waive a hard prerequisite

```text
Usage: workdeck issue help prerequisite [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help prerequisite add`



```text
Usage: workdeck issue help prerequisite add
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help prerequisite remove`

Resolve a requirement explicitly, retaining the reason in operation history

```text
Usage: workdeck issue help prerequisite remove
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help prerequisite replace`



```text
Usage: workdeck issue help prerequisite replace
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help prerequisite waive`

Record a source-bound waiver when repository acceptance policy permits it

```text
Usage: workdeck issue help prerequisite waive
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help prerequisite revoke-waiver`



```text
Usage: workdeck issue help prerequisite revoke-waiver
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help relate`

Add one canonical symmetric related link; this does not block readiness

```text
Usage: workdeck issue help relate
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help unrelate`



```text
Usage: workdeck issue help unrelate
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help relations`

Inspect parents, children, prerequisites, dependents and related issues from one snapshot

```text
Usage: workdeck issue help relations
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help ready`

Explain hard-prerequisite readiness, including unresolved and canceled requirements

```text
Usage: workdeck issue help ready
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help dependency-path`

Find a directed hard-prerequisite path; this is not a calendar forecast

```text
Usage: workdeck issue help dependency-path
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help custom`

Patch individual custom fields without replacing unmentioned values

```text
Usage: workdeck issue help custom
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help estimate`

Set an exact estimate in an explicit unit, or clear the estimate

```text
Usage: workdeck issue help estimate
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help list`

List local issues

```text
Usage: workdeck issue help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help templates`

List repository issue templates

```text
Usage: workdeck issue help templates
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help create`

Create a local issue

```text
Usage: workdeck issue help create
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help update`

Update a local issue

```text
Usage: workdeck issue help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help edit`

Edit issue Markdown in an external editor or from a draft file

```text
Usage: workdeck issue help edit
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help link`

Link a file path to an issue

```text
Usage: workdeck issue help link
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help link-file`

Link a file path to an issue

```text
Usage: workdeck issue help link-file
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help unlink-file`

Remove a linked file path from an issue

```text
Usage: workdeck issue help unlink-file
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help link-commit`

Link a commit SHA to an issue

```text
Usage: workdeck issue help link-commit
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help unlink-commit`

Remove a linked commit SHA from an issue

```text
Usage: workdeck issue help unlink-commit
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help link-document`

Link an inert document path, URL, or stable reference

```text
Usage: workdeck issue help link-document
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help unlink-document`

Remove an issue document reference

```text
Usage: workdeck issue help unlink-document
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help cancel`

Cancel an issue using a configured canceled workflow state

```text
Usage: workdeck issue help cancel
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help done`

Complete an issue using its completion policy

```text
Usage: workdeck issue help done
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help comment`

Add an immutable issue comment

```text
Usage: workdeck issue help comment
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help attach`

Attach an inert file to an issue

```text
Usage: workdeck issue help attach
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help attachments`

List issue attachment metadata

```text
Usage: workdeck issue help attachments
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help comments`

List issue comments

```text
Usage: workdeck issue help comments
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help archive`

Archive an issue or restore it

```text
Usage: workdeck issue help archive
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help close`

Close an issue

```text
Usage: workdeck issue help close
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help reopen`

Reopen an issue as Todo

```text
Usage: workdeck issue help reopen
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help move`

Move an issue to a status

```text
Usage: workdeck issue help move
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help assign`

Assign an issue

```text
Usage: workdeck issue help assign
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help unassign`

Unassign an issue

```text
Usage: workdeck issue help unassign
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help label`

Manage issue labels

```text
Usage: workdeck issue help label [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help label add`

Add a label to an issue

```text
Usage: workdeck issue help label add
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help label remove`

Remove a label from an issue

```text
Usage: workdeck issue help label remove
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help delete`

Delete an issue

```text
Usage: workdeck issue help delete
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help show`

Show one local issue

```text
Usage: workdeck issue help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `issue help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck issue help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `agent`

Manage local agent sessions

```text
Usage: workdeck agent [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent list`

List local agent sessions

```text
Usage: workdeck agent list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print sessions as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent record`

Record a local agent session

```text
Usage: workdeck agent record [OPTIONS] <TITLE>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--agent` | false |  |  |
| `--command` | false |  |  |
| `--cwd` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--goal` | false |  |  |
| `--note` | false |  |  |
| `--help` | false |  | Print help |
| `--id` | false |  |  |
| `--json` | false |  | Print the recorded session as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--plan` | false |  |  |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |
| `--status` | false |  |  |
| `--summary` | false |  |  |
| `--test` | false |  |  |
| `title` | true |  |  |
| `--file` | false |  |  |

## `agent show`

Show one agent session

```text
Usage: workdeck agent show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print the session as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent update`

Update a local agent session

```text
Usage: workdeck agent update [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--agent` | false |  |  |
| `--cwd` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--goal` | false |  |  |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print the updated session as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |
| `--status` | false |  |  |
| `--summary` | false |  |  |
| `--title` | false |  |  |

## `agent finish`

Mark an agent session done

```text
Usage: workdeck agent finish [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print the updated session as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |
| `--summary` | false |  |  |

## `agent append-plan`

Append a plan item to an agent session

```text
Usage: workdeck agent append-plan [OPTIONS] <ID> <TEXT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print the updated session as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |
| `text` | true |  |  |

## `agent add-file`

Append a touched file to an agent session

```text
Usage: workdeck agent add-file [OPTIONS] <ID> <PATH>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--change-type` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print the updated session as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `path` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent add-command`

Append a command to an agent session

```text
Usage: workdeck agent add-command [OPTIONS] <ID> <TEXT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print the updated session as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |
| `text` | true |  |  |

## `agent add-test`

Append a test command to an agent session

```text
Usage: workdeck agent add-test [OPTIONS] <ID> <TEXT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print the updated session as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |
| `text` | true |  |  |

## `agent add-note`

Append a handoff note to an agent session

```text
Usage: workdeck agent add-note [OPTIONS] <ID> <TEXT>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print the updated session as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |
| `text` | true |  |  |

## `agent delete`

Delete an agent session

```text
Usage: workdeck agent delete [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print deletion result as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |
| `--yes` | false |  | Confirm deletion |

## `agent import`

Import agent sessions from JSON or JSONL

```text
Usage: workdeck agent import [OPTIONS] <PATH>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print imported sessions as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `path` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck agent help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent help list`

List local agent sessions

```text
Usage: workdeck agent help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent help record`

Record a local agent session

```text
Usage: workdeck agent help record
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent help show`

Show one agent session

```text
Usage: workdeck agent help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent help update`

Update a local agent session

```text
Usage: workdeck agent help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent help finish`

Mark an agent session done

```text
Usage: workdeck agent help finish
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent help append-plan`

Append a plan item to an agent session

```text
Usage: workdeck agent help append-plan
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent help add-file`

Append a touched file to an agent session

```text
Usage: workdeck agent help add-file
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent help add-command`

Append a command to an agent session

```text
Usage: workdeck agent help add-command
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent help add-test`

Append a test command to an agent session

```text
Usage: workdeck agent help add-test
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent help add-note`

Append a handoff note to an agent session

```text
Usage: workdeck agent help add-note
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent help delete`

Delete an agent session

```text
Usage: workdeck agent help delete
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent help import`

Import agent sessions from JSON or JSONL

```text
Usage: workdeck agent help import
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `agent help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck agent help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 of the recorded session TOML |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Require the noninteractive native annotation interface |
| `--request-id` | false |  | Stable idempotency key for a native annotation mutation |
| `--stage` | false |  | Stage only paths changed by this native mutation |

## `initiative`

Manage native initiatives spanning projects

```text
Usage: workdeck initiative [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `initiative custom`

Patch individual custom fields without replacing unmentioned values

```text
Usage: workdeck initiative custom [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--set` | false |  | Repeat to set individual custom keys; strings require JSON quotes |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--unset` | false |  | Repeat to remove individual custom keys without changing others |

## `initiative list`

List native planning records

```text
Usage: workdeck initiative list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--status` | false |  |  |

## `initiative show`

Show a native planning record

```text
Usage: workdeck initiative show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--archive` | false | active, all, archived | Member issue archive scope; defaults to active |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--members` | false |  | Show a snapshot of the record and its issue/planning membership (native PM) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `initiative create`

Create a planning record with a stable identity

```text
Usage: workdeck initiative create [OPTIONS] <NAME>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body-file` | false |  | Read Markdown body from a file, or '-' for stdin |
| `--clear` | false |  | Explicitly clear an optional field; cannot be combined with setting the same field |
| `--color` | false |  |  |
| `--custom` | false |  | Explicitly replace the complete custom object; use the custom subcommand for per-key updates |
| `--description` | false |  |  |
| `--ends-at` | false |  |  |
| `--exit-criterion` | false |  | Repeat to replace declared project exit criteria; declarations are not verification evidence |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--goal` | false |  |  |
| `--help` | false |  | Print help |
| `--id` | false |  |  |
| `--initiative` | false |  |  |
| `--json` | false |  |  |
| `--lead` | false |  |  |
| `name` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--outcome` | false |  | Repeat to replace declared initiative, milestone or target outcomes |
| `--project` | false |  | Owning project for a milestone |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--scope` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--starts-at` | false |  |  |
| `--status` | false |  |  |
| `--target` | false |  | Repeat to replace project or milestone target membership |

## `initiative update`

Update a planning record without changing its identity

```text
Usage: workdeck initiative update [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body-file` | false |  | Read Markdown body from a file, or '-' for stdin |
| `--clear` | false |  | Explicitly clear an optional field; cannot be combined with setting the same field |
| `--color` | false |  |  |
| `--custom` | false |  | Explicitly replace the complete custom object; use the custom subcommand for per-key updates |
| `--description` | false |  |  |
| `--ends-at` | false |  |  |
| `--exit-criterion` | false |  | Repeat to replace declared project exit criteria; declarations are not verification evidence |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--goal` | false |  |  |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--initiative` | false |  |  |
| `--json` | false |  |  |
| `--lead` | false |  |  |
| `--name` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--outcome` | false |  | Repeat to replace declared initiative, milestone or target outcomes |
| `--project` | false |  | Owning project for a milestone |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--scope` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--starts-at` | false |  |  |
| `--status` | false |  |  |
| `--target` | false |  | Repeat to replace project or milestone target membership |

## `initiative archive`

Archive a record while retaining its references and history

```text
Usage: workdeck initiative archive [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--restore` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `initiative assess`

Assess project or milestone exit policy without writing

```text
Usage: workdeck initiative assess [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `initiative complete`

Complete a project or milestone after attributed manual acceptance

```text
Usage: workdeck initiative complete [OPTIONS] --actor <ACTOR> --reason <REASON> <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--reason` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `initiative help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck initiative help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `initiative help custom`

Patch individual custom fields without replacing unmentioned values

```text
Usage: workdeck initiative help custom
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `initiative help list`

List native planning records

```text
Usage: workdeck initiative help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `initiative help show`

Show a native planning record

```text
Usage: workdeck initiative help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `initiative help create`

Create a planning record with a stable identity

```text
Usage: workdeck initiative help create
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `initiative help update`

Update a planning record without changing its identity

```text
Usage: workdeck initiative help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `initiative help archive`

Archive a record while retaining its references and history

```text
Usage: workdeck initiative help archive
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `initiative help assess`

Assess project or milestone exit policy without writing

```text
Usage: workdeck initiative help assess
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `initiative help complete`

Complete a project or milestone after attributed manual acceptance

```text
Usage: workdeck initiative help complete
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `initiative help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck initiative help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone`

Manage native project milestones

```text
Usage: workdeck milestone [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone custom`

Patch individual custom fields without replacing unmentioned values

```text
Usage: workdeck milestone custom [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--set` | false |  | Repeat to set individual custom keys; strings require JSON quotes |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--unset` | false |  | Repeat to remove individual custom keys without changing others |

## `milestone list`

List native planning records

```text
Usage: workdeck milestone list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--status` | false |  |  |

## `milestone show`

Show a native planning record

```text
Usage: workdeck milestone show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--archive` | false | active, all, archived | Member issue archive scope; defaults to active |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--members` | false |  | Show a snapshot of the record and its issue/planning membership (native PM) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone create`

Create a planning record with a stable identity

```text
Usage: workdeck milestone create [OPTIONS] <NAME>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body-file` | false |  | Read Markdown body from a file, or '-' for stdin |
| `--clear` | false |  | Explicitly clear an optional field; cannot be combined with setting the same field |
| `--color` | false |  |  |
| `--custom` | false |  | Explicitly replace the complete custom object; use the custom subcommand for per-key updates |
| `--description` | false |  |  |
| `--ends-at` | false |  |  |
| `--exit-criterion` | false |  | Repeat to replace declared project exit criteria; declarations are not verification evidence |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--goal` | false |  |  |
| `--help` | false |  | Print help |
| `--id` | false |  |  |
| `--initiative` | false |  |  |
| `--json` | false |  |  |
| `--lead` | false |  |  |
| `name` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--outcome` | false |  | Repeat to replace declared initiative, milestone or target outcomes |
| `--project` | false |  | Owning project for a milestone |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--scope` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--starts-at` | false |  |  |
| `--status` | false |  |  |
| `--target` | false |  | Repeat to replace project or milestone target membership |

## `milestone update`

Update a planning record without changing its identity

```text
Usage: workdeck milestone update [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body-file` | false |  | Read Markdown body from a file, or '-' for stdin |
| `--clear` | false |  | Explicitly clear an optional field; cannot be combined with setting the same field |
| `--color` | false |  |  |
| `--custom` | false |  | Explicitly replace the complete custom object; use the custom subcommand for per-key updates |
| `--description` | false |  |  |
| `--ends-at` | false |  |  |
| `--exit-criterion` | false |  | Repeat to replace declared project exit criteria; declarations are not verification evidence |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--goal` | false |  |  |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--initiative` | false |  |  |
| `--json` | false |  |  |
| `--lead` | false |  |  |
| `--name` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--outcome` | false |  | Repeat to replace declared initiative, milestone or target outcomes |
| `--project` | false |  | Owning project for a milestone |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--scope` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--starts-at` | false |  |  |
| `--status` | false |  |  |
| `--target` | false |  | Repeat to replace project or milestone target membership |

## `milestone archive`

Archive a record while retaining its references and history

```text
Usage: workdeck milestone archive [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--restore` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone assess`

Assess project or milestone exit policy without writing

```text
Usage: workdeck milestone assess [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone complete`

Complete a project or milestone after attributed manual acceptance

```text
Usage: workdeck milestone complete [OPTIONS] --actor <ACTOR> --reason <REASON> <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--reason` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck milestone help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone help custom`

Patch individual custom fields without replacing unmentioned values

```text
Usage: workdeck milestone help custom
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone help list`

List native planning records

```text
Usage: workdeck milestone help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone help show`

Show a native planning record

```text
Usage: workdeck milestone help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone help create`

Create a planning record with a stable identity

```text
Usage: workdeck milestone help create
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone help update`

Update a planning record without changing its identity

```text
Usage: workdeck milestone help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone help archive`

Archive a record while retaining its references and history

```text
Usage: workdeck milestone help archive
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone help assess`

Assess project or milestone exit policy without writing

```text
Usage: workdeck milestone help assess
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone help complete`

Complete a project or milestone after attributed manual acceptance

```text
Usage: workdeck milestone help complete
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `milestone help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck milestone help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target`

Manage native delivery targets

```text
Usage: workdeck target [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target custom`

Patch individual custom fields without replacing unmentioned values

```text
Usage: workdeck target custom [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--set` | false |  | Repeat to set individual custom keys; strings require JSON quotes |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--unset` | false |  | Repeat to remove individual custom keys without changing others |

## `target list`

List native planning records

```text
Usage: workdeck target list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--status` | false |  |  |

## `target show`

Show a native planning record

```text
Usage: workdeck target show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--archive` | false | active, all, archived | Member issue archive scope; defaults to active |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--members` | false |  | Show a snapshot of the record and its issue/planning membership (native PM) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target create`

Create a planning record with a stable identity

```text
Usage: workdeck target create [OPTIONS] <NAME>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body-file` | false |  | Read Markdown body from a file, or '-' for stdin |
| `--clear` | false |  | Explicitly clear an optional field; cannot be combined with setting the same field |
| `--color` | false |  |  |
| `--custom` | false |  | Explicitly replace the complete custom object; use the custom subcommand for per-key updates |
| `--description` | false |  |  |
| `--ends-at` | false |  |  |
| `--exit-criterion` | false |  | Repeat to replace declared project exit criteria; declarations are not verification evidence |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--goal` | false |  |  |
| `--help` | false |  | Print help |
| `--id` | false |  |  |
| `--initiative` | false |  |  |
| `--json` | false |  |  |
| `--lead` | false |  |  |
| `name` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--outcome` | false |  | Repeat to replace declared initiative, milestone or target outcomes |
| `--project` | false |  | Owning project for a milestone |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--scope` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--starts-at` | false |  |  |
| `--status` | false |  |  |
| `--target` | false |  | Repeat to replace project or milestone target membership |

## `target update`

Update a planning record without changing its identity

```text
Usage: workdeck target update [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body-file` | false |  | Read Markdown body from a file, or '-' for stdin |
| `--clear` | false |  | Explicitly clear an optional field; cannot be combined with setting the same field |
| `--color` | false |  |  |
| `--custom` | false |  | Explicitly replace the complete custom object; use the custom subcommand for per-key updates |
| `--description` | false |  |  |
| `--ends-at` | false |  |  |
| `--exit-criterion` | false |  | Repeat to replace declared project exit criteria; declarations are not verification evidence |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--goal` | false |  |  |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--initiative` | false |  |  |
| `--json` | false |  |  |
| `--lead` | false |  |  |
| `--name` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--outcome` | false |  | Repeat to replace declared initiative, milestone or target outcomes |
| `--project` | false |  | Owning project for a milestone |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--scope` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--starts-at` | false |  |  |
| `--status` | false |  |  |
| `--target` | false |  | Repeat to replace project or milestone target membership |

## `target archive`

Archive a record while retaining its references and history

```text
Usage: workdeck target archive [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--restore` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target assess`

Assess project or milestone exit policy without writing

```text
Usage: workdeck target assess [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target complete`

Complete a project or milestone after attributed manual acceptance

```text
Usage: workdeck target complete [OPTIONS] --actor <ACTOR> --reason <REASON> <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--reason` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck target help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target help custom`

Patch individual custom fields without replacing unmentioned values

```text
Usage: workdeck target help custom
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target help list`

List native planning records

```text
Usage: workdeck target help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target help show`

Show a native planning record

```text
Usage: workdeck target help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target help create`

Create a planning record with a stable identity

```text
Usage: workdeck target help create
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target help update`

Update a planning record without changing its identity

```text
Usage: workdeck target help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target help archive`

Archive a record while retaining its references and history

```text
Usage: workdeck target help archive
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target help assess`

Assess project or milestone exit policy without writing

```text
Usage: workdeck target help assess
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target help complete`

Complete a project or milestone after attributed manual acceptance

```text
Usage: workdeck target help complete
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `target help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck target help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project`

Manage local Workdeck projects

```text
Usage: workdeck project [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project custom`

Patch individual custom fields without replacing unmentioned values

```text
Usage: workdeck project custom [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--set` | false |  | Repeat to set individual custom keys; strings require JSON quotes |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--unset` | false |  | Repeat to remove individual custom keys without changing others |

## `project create`

Create a new native planning record

```text
Usage: workdeck project create [OPTIONS] <NAME>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body-file` | false |  | Read Markdown body from a file, or '-' for stdin |
| `--clear` | false |  | Explicitly clear an optional field; cannot be combined with setting the same field |
| `--color` | false |  |  |
| `--custom` | false |  | Explicitly replace the complete custom object; use the custom subcommand for per-key updates |
| `--description` | false |  |  |
| `--ends-at` | false |  |  |
| `--exit-criterion` | false |  | Repeat to replace declared project exit criteria; declarations are not verification evidence |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--goal` | false |  |  |
| `--help` | false |  | Print help |
| `--id` | false |  |  |
| `--initiative` | false |  |  |
| `--json` | false |  |  |
| `--lead` | false |  |  |
| `name` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--outcome` | false |  | Repeat to replace declared initiative, milestone or target outcomes |
| `--project` | false |  | Owning project for a milestone |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--scope` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--starts-at` | false |  |  |
| `--status` | false |  |  |
| `--target` | false |  | Repeat to replace project or milestone target membership |

## `project update`

Update a native planning record

```text
Usage: workdeck project update [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body-file` | false |  | Read Markdown body from a file, or '-' for stdin |
| `--clear` | false |  | Explicitly clear an optional field; cannot be combined with setting the same field |
| `--color` | false |  |  |
| `--custom` | false |  | Explicitly replace the complete custom object; use the custom subcommand for per-key updates |
| `--description` | false |  |  |
| `--ends-at` | false |  |  |
| `--exit-criterion` | false |  | Repeat to replace declared project exit criteria; declarations are not verification evidence |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--goal` | false |  |  |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--initiative` | false |  |  |
| `--json` | false |  |  |
| `--lead` | false |  |  |
| `--name` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--outcome` | false |  | Repeat to replace declared initiative, milestone or target outcomes |
| `--project` | false |  | Owning project for a milestone |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--scope` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--starts-at` | false |  |  |
| `--status` | false |  |  |
| `--target` | false |  | Repeat to replace project or milestone target membership |

## `project archive`

Archive or restore a native planning record, retaining references

```text
Usage: workdeck project archive [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--restore` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project assess`

Assess project exit policy without writing

```text
Usage: workdeck project assess [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project complete`

Complete a project after attributed manual acceptance

```text
Usage: workdeck project complete [OPTIONS] --actor <ACTOR> --reason <REASON> <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--actor` | true |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--reason` | true |  |  |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project list`

List local projects

```text
Usage: workdeck project list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print projects as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--status` | false |  |  |

## `project save`

Create or update a local project

```text
Usage: workdeck project save [OPTIONS] <NAME>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--description` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--id` | false |  |  |
| `--json` | false |  | Print the saved project as JSON |
| `name` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--status` | false |  |  |

## `project show`

Show one project

```text
Usage: workdeck project show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--archive` | false | active, all, archived | Member issue archive scope; defaults to active |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print the project as JSON |
| `--members` | false |  | Show a snapshot of the record and its issue/planning membership (native PM) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project delete`

Delete a project

```text
Usage: workdeck project delete [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--dry-run` | false |  | Preview native retirement and incoming references without writing |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-preview` | false |  | Require the fingerprint from a reviewed deletion preview |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--force` | false |  | Preview association removal with --dry-run; apply requires --expected-preview |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print the deleted project as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--yes` | false |  | Confirm deletion |

## `project help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck project help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project help custom`

Patch individual custom fields without replacing unmentioned values

```text
Usage: workdeck project help custom
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project help create`

Create a new native planning record

```text
Usage: workdeck project help create
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project help update`

Update a native planning record

```text
Usage: workdeck project help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project help archive`

Archive or restore a native planning record, retaining references

```text
Usage: workdeck project help archive
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project help assess`

Assess project exit policy without writing

```text
Usage: workdeck project help assess
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project help complete`

Complete a project after attributed manual acceptance

```text
Usage: workdeck project help complete
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project help list`

List local projects

```text
Usage: workdeck project help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project help save`

Create or update a local project

```text
Usage: workdeck project help save
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project help show`

Show one project

```text
Usage: workdeck project help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project help delete`

Delete a project

```text
Usage: workdeck project help delete
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `project help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck project help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `cycle`

Manage local Workdeck cycles

```text
Usage: workdeck cycle [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `cycle carryover`

Preview unfinished work for another cycle; apply only an exact reviewed fingerprint

```text
Usage: workdeck cycle carryover [OPTIONS] <FROM> <TO>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-preview` | false |  | Apply this exact reviewed preview; omission previews without writing |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `from` | true |  |  |
| `--help` | false |  | Print help |
| `--issue` | false |  | Select an eligible source member; repeat for an explicit batch of at most 100 |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `to` | true |  |  |

## `cycle custom`

Patch individual custom fields without replacing unmentioned values

```text
Usage: workdeck cycle custom [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--set` | false |  | Repeat to set individual custom keys; strings require JSON quotes |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--unset` | false |  | Repeat to remove individual custom keys without changing others |

## `cycle create`

Create a new native planning record

```text
Usage: workdeck cycle create [OPTIONS] <NAME>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body-file` | false |  | Read Markdown body from a file, or '-' for stdin |
| `--clear` | false |  | Explicitly clear an optional field; cannot be combined with setting the same field |
| `--color` | false |  |  |
| `--custom` | false |  | Explicitly replace the complete custom object; use the custom subcommand for per-key updates |
| `--description` | false |  |  |
| `--ends-at` | false |  |  |
| `--exit-criterion` | false |  | Repeat to replace declared project exit criteria; declarations are not verification evidence |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--goal` | false |  |  |
| `--help` | false |  | Print help |
| `--id` | false |  |  |
| `--initiative` | false |  |  |
| `--json` | false |  |  |
| `--lead` | false |  |  |
| `name` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--outcome` | false |  | Repeat to replace declared initiative, milestone or target outcomes |
| `--project` | false |  | Owning project for a milestone |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--scope` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--starts-at` | false |  |  |
| `--status` | false |  |  |
| `--target` | false |  | Repeat to replace project or milestone target membership |

## `cycle update`

Update a native planning record

```text
Usage: workdeck cycle update [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body-file` | false |  | Read Markdown body from a file, or '-' for stdin |
| `--clear` | false |  | Explicitly clear an optional field; cannot be combined with setting the same field |
| `--color` | false |  |  |
| `--custom` | false |  | Explicitly replace the complete custom object; use the custom subcommand for per-key updates |
| `--description` | false |  |  |
| `--ends-at` | false |  |  |
| `--exit-criterion` | false |  | Repeat to replace declared project exit criteria; declarations are not verification evidence |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--goal` | false |  |  |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--initiative` | false |  |  |
| `--json` | false |  |  |
| `--lead` | false |  |  |
| `--name` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--outcome` | false |  | Repeat to replace declared initiative, milestone or target outcomes |
| `--project` | false |  | Owning project for a milestone |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--scope` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--starts-at` | false |  |  |
| `--status` | false |  |  |
| `--target` | false |  | Repeat to replace project or milestone target membership |

## `cycle archive`

Archive or restore a native planning record, retaining references

```text
Usage: workdeck cycle archive [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--restore` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `cycle list`

List local cycles

```text
Usage: workdeck cycle list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print cycles as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--status` | false |  |  |

## `cycle save`

Create or update a local cycle

```text
Usage: workdeck cycle save [OPTIONS] <NAME>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--ends-at` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--id` | false |  |  |
| `--json` | false |  | Print the saved cycle as JSON |
| `name` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--starts-at` | false |  |  |
| `--status` | false |  |  |

## `cycle show`

Show one cycle

```text
Usage: workdeck cycle show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--archive` | false | active, all, archived | Member issue archive scope; defaults to active |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print the cycle as JSON |
| `--members` | false |  | Show a snapshot of the record and its issue/planning membership (native PM) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `cycle delete`

Delete a cycle

```text
Usage: workdeck cycle delete [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--dry-run` | false |  | Preview native retirement and incoming references without writing |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-preview` | false |  | Require the fingerprint from a reviewed deletion preview |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--force` | false |  | Preview association removal with --dry-run; apply requires --expected-preview |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print the deleted cycle as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--yes` | false |  | Confirm deletion |

## `cycle help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck cycle help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `cycle help carryover`

Preview unfinished work for another cycle; apply only an exact reviewed fingerprint

```text
Usage: workdeck cycle help carryover
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `cycle help custom`

Patch individual custom fields without replacing unmentioned values

```text
Usage: workdeck cycle help custom
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `cycle help create`

Create a new native planning record

```text
Usage: workdeck cycle help create
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `cycle help update`

Update a native planning record

```text
Usage: workdeck cycle help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `cycle help archive`

Archive or restore a native planning record, retaining references

```text
Usage: workdeck cycle help archive
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `cycle help list`

List local cycles

```text
Usage: workdeck cycle help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `cycle help save`

Create or update a local cycle

```text
Usage: workdeck cycle help save
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `cycle help show`

Show one cycle

```text
Usage: workdeck cycle help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `cycle help delete`

Delete a cycle

```text
Usage: workdeck cycle help delete
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `cycle help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck cycle help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label`

Manage local Workdeck labels

```text
Usage: workdeck label [OPTIONS] <COMMAND>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label custom`

Patch individual custom fields without replacing unmentioned values

```text
Usage: workdeck label custom [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--set` | false |  | Repeat to set individual custom keys; strings require JSON quotes |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--unset` | false |  | Repeat to remove individual custom keys without changing others |

## `label create`

Create a new native planning record

```text
Usage: workdeck label create [OPTIONS] <NAME>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body-file` | false |  | Read Markdown body from a file, or '-' for stdin |
| `--clear` | false |  | Explicitly clear an optional field; cannot be combined with setting the same field |
| `--color` | false |  |  |
| `--custom` | false |  | Explicitly replace the complete custom object; use the custom subcommand for per-key updates |
| `--description` | false |  |  |
| `--ends-at` | false |  |  |
| `--exit-criterion` | false |  | Repeat to replace declared project exit criteria; declarations are not verification evidence |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--goal` | false |  |  |
| `--help` | false |  | Print help |
| `--id` | false |  |  |
| `--initiative` | false |  |  |
| `--json` | false |  |  |
| `--lead` | false |  |  |
| `name` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--outcome` | false |  | Repeat to replace declared initiative, milestone or target outcomes |
| `--project` | false |  | Owning project for a milestone |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--scope` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--starts-at` | false |  |  |
| `--status` | false |  |  |
| `--target` | false |  | Repeat to replace project or milestone target membership |

## `label update`

Update a native planning record

```text
Usage: workdeck label update [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--body-file` | false |  | Read Markdown body from a file, or '-' for stdin |
| `--clear` | false |  | Explicitly clear an optional field; cannot be combined with setting the same field |
| `--color` | false |  |  |
| `--custom` | false |  | Explicitly replace the complete custom object; use the custom subcommand for per-key updates |
| `--description` | false |  |  |
| `--ends-at` | false |  |  |
| `--exit-criterion` | false |  | Repeat to replace declared project exit criteria; declarations are not verification evidence |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--goal` | false |  |  |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--initiative` | false |  |  |
| `--json` | false |  |  |
| `--lead` | false |  |  |
| `--name` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--outcome` | false |  | Repeat to replace declared initiative, milestone or target outcomes |
| `--project` | false |  | Owning project for a milestone |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--scope` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--starts-at` | false |  |  |
| `--status` | false |  |  |
| `--target` | false |  | Repeat to replace project or milestone target membership |

## `label archive`

Archive or restore a native planning record, retaining references

```text
Usage: workdeck label archive [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--restore` | false |  |  |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label list`

List local labels

```text
Usage: workdeck label list [OPTIONS]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--color` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--json` | false |  | Print labels as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label save`

Create or update a local label

```text
Usage: workdeck label save [OPTIONS] <NAME>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--color` | false |  |  |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `--id` | false |  |  |
| `--json` | false |  | Print the saved label as JSON |
| `name` | true |  |  |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label show`

Show one label

```text
Usage: workdeck label show [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--archive` | false | active, all, archived | Member issue archive scope; defaults to active |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print the label as JSON |
| `--members` | false |  | Show a snapshot of the record and its issue/planning membership (native PM) |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label delete`

Delete a label

```text
Usage: workdeck label delete [OPTIONS] <ID>
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--dry-run` | false |  | Preview native retirement and incoming references without writing |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-preview` | false |  | Require the fingerprint from a reviewed deletion preview |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--force` | false |  | Preview label removal with --dry-run; apply requires --expected-preview |
| `--help` | false |  | Print help |
| `id` | true |  |  |
| `--json` | false |  | Print the deleted label as JSON |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
| `--yes` | false |  | Confirm deletion |

## `label help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck label help [COMMAND]
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label help custom`

Patch individual custom fields without replacing unmentioned values

```text
Usage: workdeck label help custom
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label help create`

Create a new native planning record

```text
Usage: workdeck label help create
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label help update`

Update a native planning record

```text
Usage: workdeck label help update
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label help archive`

Archive or restore a native planning record, retaining references

```text
Usage: workdeck label help archive
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label help list`

List local labels

```text
Usage: workdeck label help list
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label help save`

Create or update a local label

```text
Usage: workdeck label help save
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label help show`

Show one label

```text
Usage: workdeck label help show
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label help delete`

Delete a label

```text
Usage: workdeck label help delete
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |

## `label help help`

Print this message or the help of the given subcommand(s)

```text
Usage: workdeck label help help
```

| Argument | Required | Values | Description |
| --- | --- | --- | --- |
| `--expected-content` | false |  | Expected SHA-256 content hash; requires expected-revision |
| `--expected-revision` | false |  | Expected positive record revision; requires expected-content |
| `--experimental` | false |  | Enable experimental review features (currently STML) |
| `--extension` | false |  |  |
| `--extensions` | false |  |  |
| `--fast` | false |  | Use experimental fast syntax highlighting |
| `--no-extensions` | false |  |  |
| `--no-input` | false |  | Never prompt for missing input |
| `--request-id` | false |  | Stable idempotency key for a mutation |
| `--stage` | false |  | Stage only this operation's exact changed files and durable receipt |
