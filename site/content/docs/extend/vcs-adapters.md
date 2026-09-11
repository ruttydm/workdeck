+++
title = "VCS adapters"
description = "Contribute a version-control backend with detection, watch support, exact file sources, and rich failures."
template = "docs.html"
+++

`workdeck-extension-api` lets a native extension contribute an additional VCS
backend. The bundled Git, Jujutsu, and Sapling providers use the same
provider-neutral registration boundary.

```rust
use workdeck_extension_api::{VcsAdapter, VcsOperation, VcsOperationKind};

api.register_vcs_adapter(VcsAdapter {
    id: "hg".into(),
    name: "Mercurial".into(),
    detect: |cwd| {
        cwd.join(".hg").is_dir().then(|| VcsDetection {
            id: "hg".into(),
            repo_root: cwd.to_path_buf(),
        })
    },
    operations: [(VcsOperationKind::WorkingTreeDiff, VcsOperation::load(run_hg_diff))]
        .into_iter()
        .collect(),
});
```

The IDs Workdeck ships with—`git`, `jj`, and `sl`—are reserved. An adapter that
reuses one is skipped with a startup notice. `operations` is optional and may
implement any of `working-tree-diff`, `revision-show`, and `stash-show`; an
operation left out (or an omitted map) produces a clear “not supported” error
for that command instead of a crash.

A load result is patch text plus labels. Everything else is optional, and each
optional field adds one capability:

| Field | What it adds |
| --- | --- |
| `untracked_paths` | repo-root-relative unknown files synthesized into added-file diffs |
| `read_file_source` | exact whole-file contents for context expansion and highlighting |
| `extra_files` | files reviewed outside the patch, including skipped placeholders |

`untracked_paths` is the shorthand: list the repo-root-relative paths your VCS
reports as unknown and Workdeck synthesizes added-file diffs, skipping binaries
and files too large to render. Honor `input.options.exclude_untracked` so
`--exclude-untracked` retains its meaning. The other fields are covered below.

## Detection order

Detection prefers the **nearest** checkout. A Git repository nested inside a jj
workspace is reviewed as Git, whatever the priorities say; a Mercurial checkout
inside Git is likewise reviewed as Mercurial. `detection_priority` only decides
which backend wins when several recognize the same directory (the colocated
case where one working copy carries two sets of markers).

| Adapter | Priority |
| --- | --- |
| bundled `jj` | 200 |
| bundled `sl` | 100 |
| bundled `git` | 0 (`WORKDECK_VCS_DETECTION_BASELINE_PRIORITY`) |
| your adapter, by default | -100 (`WORKDECK_DEFAULT_VCS_DETECTION_PRIORITY`) |

Higher priorities are consulted first; equal priorities fall back to
registration order. jj and Sapling sit above Git because a colocated jj
repository—or a Sapling repository created with `sl init --git`—also carries Git
metadata, and the Git view is the wrong one.

The default puts a user adapter below Git, so installing an extension never
silently changes how an existing repository is reviewed. Set
`detection_priority` explicitly to outrank a bundled backend; that choice is
local to the user's machine.

```rust
use workdeck_extension_api::WORKDECK_VCS_DETECTION_BASELINE_PRIORITY;

api.register_vcs_adapter(VcsAdapter {
    id: "hg".into(),
    name: "Mercurial".into(),
    detection_priority: WORKDECK_VCS_DETECTION_BASELINE_PRIORITY + 10,
    detect: detect_mercurial,
    ..Default::default()
});
```

Detection uses the same rule for every adapter, whichever tier registered it:
the nearest checkout wins, priority breaks ties between adapters recognizing the
same root, and equal priorities use registration order. Config resolves an
explicit VCS before extensions load, so detection runs again after the complete
adapter list is available; the second answer is the one the session uses.

An explicit `vcs = "<id>"` in Workdeck config is never overridden by another
adapter, however near its checkout. A repository-local adapter can bootstrap a
provider Workdeck has never seen because `.workdeck` establishes the project
root; global, config-path, and `--extension` adapters participate in the staged
root/config pass before review loading.

## Watch support

`--watch` works through native adapters. Each operation may add:

- `watch_signature(input, ctx)`: a cheap fingerprint of the reviewed state;
  Workdeck polls it and reloads when it changes;
- `watch_plan(input, ctx)`: filesystem targets that cover that state, allowing
  event-driven reloads instead of timer polling.

```rust
watch_plan: |input, ctx| WatchPlan {
    coverage: WatchCoverage::Hybrid,
    targets: vec![WatchTarget::DirectoryTree {
        directory: ctx.cwd.clone(),
        ignored_roots: vec![ctx.cwd.join(".hg")],
        sources: vec![WatchSource::Worktree],
    }],
}
```

`WatchCoverage::Hybrid` promises that the targets cover the reviewed state.
Leaving `watch_plan` out is equivalent to `PollOnly` and still works; it costs
one adapter subprocess per tick.

## Exact file sources

A patch carries changed lines and a little context. If a VCS can produce a
file's whole contents on each side, expose `read_file_source`; Workdeck can then
expand context past the hunk, highlight against the real file, and calculate
word differences accurately.

```rust
async fn load(input: VcsLoadInput, ctx: VcsContext) -> Result<VcsLoadResult> {
    // Pin revisions while loading; the reader closes over the immutable pair.
    let (old_rev, new_rev) = resolve_hg_revisions(&input, &ctx.cwd).await?;
    Ok(VcsLoadResult {
        repo_root: ctx.cwd.clone(),
        source_label: ctx.cwd.display().to_string(),
        title: "Mercurial working copy".into(),
        patch_text: run_hg_diff(&ctx.cwd).await?,
        read_file_source: Some(Arc::new(move |request| {
            let old_rev = old_rev.clone();
            let new_rev = new_rev.clone();
            Box::pin(async move {
                match request.side {
                    SourceSide::Old if request.change_type.is_new() => Ok(None),
                    SourceSide::Old => hg_cat(&old_rev, request.previous_path.as_deref().unwrap_or(&request.path)),
                    SourceSide::New if request.change_type.is_deleted() => Ok(None),
                    SourceSide::New => hg_cat(&new_rev, &request.path),
                }
            })
        })),
        ..Default::default()
    })
}
```

Return `None` for a side with no content (the old side of an added file or a
path the revision never contained), rather than throwing. Return
`SourceRead::TooLarge { max_bytes }` when a read exceeds the adapter's resource
limit; Workdeck shows expansion as unavailable without treating it as an
extension failure. The host calls the reader at most once per file and side and
caches the result, and never calls it for a binary diff. Omitting the reader is
valid: Workdeck falls back to content carried in the patch, with less context.

## Files outside the patch

`extra_files` lists files that `patch_text` does not contain, in display order.
Each entry is either a one-file patch or a skipped placeholder; the adapter
describes files and Workdeck builds the diff model.

A **patch** entry is useful when the VCS produces better text than reading the
working copy—for example, its own binary detection or path quoting:

```rust
extra_files: vec![ExtraFile::Patch {
    path: "notes.md".into(),
    patch_text: hg_diff_one_file("notes.md").await?,
    is_untracked: true,
}],
```

A **skipped** entry lists a file without rendering it. Reporting a reason for a
multi-hundred-megabyte generated file is more useful than producing a diff no
one can read:

```rust
extra_files: vec![ExtraFile::Skipped {
    path: "dist/bundle.js".into(),
    reason: SkipReason::TooLarge,
    change_type: ChangeType::Change,
    stats: FileStats { additions: 100_001, deletions: 0 },
    stats_truncated: true,
}],
```

`read_file_source` also covers patch entries. Skipped entries have no content
reader. `untracked_paths` remains the shorthand for ordinary unknown files;
use `extra_files` only when the VCS has a better representation.

## Moved lines

`input.options.color_moved` is true when the user asks for move detection.
Emit ANSI-colored diff text painting moved additions cyan and moved deletions
magenta (the convention produced by `git diff --color-moved`) and those lines
render as moved. This is post-processing over the returned patch, not a Git
special case; a backend without move classes may ignore the option.

## Failures the user can fix

Return `VcsUserError` when the problem is how Workdeck was invoked rather than a
backend bug: no repository, an unresolvable revision, or a missing binary.
Workdeck prints the message without a stack trace and lists suggestions beneath
it. Other failures are reported as unexpected, source-attributed extension
errors.

```rust
return Err(VcsUserError::new(
    "`workdeck stash show` is not supported by Mercurial.",
    ["Use `workdeck show <rev>` to review a commit instead."],
));
```

The host detects this error structurally so extensions built with another copy
of the SDK behave the same way. Bundled Git, Jujutsu, and Sapling adapters use
the same user-error boundary. All requests remain subject to JSON-RPC frame
limits, deadlines, cancellation, and crash isolation.

Adapted from Hunk's MIT-licensed VCS-adapter guide, Copyright Modem Labs Inc.
The native page retains the documented provider semantics while replacing the
TypeScript callback runtime with Workdeck's Rust extension SDK.
