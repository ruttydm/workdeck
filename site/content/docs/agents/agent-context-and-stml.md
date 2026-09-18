+++
title = "Agent context and experimental STML notes"
description = "Load sidecar rationale and opt into terminal-native note markup."
template = "docs.html"
+++

Live session comments are the recommended workflow. A sidecar is useful when the annotations already exist before Workdeck starts or need to travel with a patch.

## Load a JSON sidecar

```bash
workdeck diff --agent-context notes.json
workdeck patch change.patch --agent-context notes.json
```

The sidecar can set narrative file order and attach hunk-level annotations. Keep it concise: one changeset summary, short file summaries, and only rationale that improves the review. The visible UI prioritizes workdeck notes rather than generic explainer cards.

A compact example lives at `examples/3-agent-review-demo/agent-context.json` in the repository.

## Opt into STML

STML is experimental rich markup for terminal note bodies. It is off by default:

```bash
workdeck --experimental diff --agent-context notes.json
```

The launch flag is the authority for that session; a reload cannot turn the capability on later. Plain `summary` text remains required as the fallback.

Before sending markup, an agent should inspect support and width:

```bash
workdeck session context --repo . --json
workdeck markup guide
workdeck markup render - --width <reported-noteMarkupWidth>
```

Only send `--markup` when `experimentalFeatures` includes `stml`. Keep colors symbolic and markup compact so notes retain a clear spatial relationship to their code.

Use [live session control](/docs/agents/live-session-control/) to inspect the
selected review before adding [comments](/docs/agents/comments-and-annotations/).
Treat sidecar files as review input, not instructions granting an agent extra
permissions. Read-only inspection does not require writing repository state.

Adapted from Hunk's pinned MIT guide, Copyright Modem Labs Inc. Native STML
capture has been exercised, but complete markup, width and fallback parity
remains a release gate; this source interval stays unmapped.

