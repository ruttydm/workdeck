//! Explicit cache publication and source-qualified, read-only cached queries.
use super::{pm_claims::Options, pm_cli, pm_registry::Role};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use workdeck_pm::{projection::*, *};

#[derive(Debug, Args)]
pub(super) struct Source {
    #[arg(
        long,
        global = true,
        value_enum,
        default_value = "working-tree",
        help = "Planning source slot; cached reads never fetch or refresh it"
    )]
    source: Role,
    #[arg(
        long,
        global = true,
        help = "Full proposal ref; required only with --source proposal"
    )]
    reference: Option<String>,
}
impl Source {
    fn selector(&self) -> Result<SourceSelector> {
        match (self.source, &self.reference) {
            (Role::WorkingTree, None) => Ok(SourceSelector::WorkingTree),
            (Role::Accepted, None) => Ok(SourceSelector::Accepted),
            (Role::Coordination, None) => Ok(SourceSelector::Coordination),
            (Role::Proposal, Some(reference)) => Ok(SourceSelector::Proposal {
                reference: reference.parse()?,
            }),
            _ => Err(invalid(
                "--reference is required exactly when --source proposal is selected",
            )),
        }
    }
}

#[derive(Debug, Args)]
pub(super) struct QueryInput {
    #[arg(
        long,
        help = "ProjectionQuery JSON file, or - for bounded stdin; see schema projection-query"
    )]
    input: PathBuf,
    #[arg(
        long,
        help = "Exact serialized query handle from a prior response; required for later windows"
    )]
    expected_query: Option<String>,
}

#[derive(Debug, Subcommand)]
pub(super) enum IndexCommand {
    #[command(
        about = "Explicitly build or refresh a disposable local index; does not mutate planning files"
    )]
    Refresh {
        #[arg(
            long,
            help = "Rebuild the selected source's disposable index from native files"
        )]
        rebuild: bool,
    },
    #[command(about = "Read a bounded cached query; never creates, repairs or refreshes the index")]
    Query {
        #[command(flatten)]
        query: QueryInput,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(
            long,
            default_value_t = 50,
            help = "Rows per page, 1–500; see schema projection-limits"
        )]
        limit: usize,
    },
    #[command(about = "Read bounded grouped issue columns from one cached query generation")]
    Board {
        #[command(flatten)]
        query: QueryInput,
        #[arg(long, default_value_t = 0)]
        first_group: usize,
        #[arg(
            long,
            default_value_t = 3,
            help = "Visible columns, 1–8; columns times rows must not exceed 500"
        )]
        columns: usize,
        #[arg(long, default_value_t = 5)]
        rows: usize,
        #[arg(long, help = "Selected ordinal within the captured query")]
        selected: Option<usize>,
    },
    #[command(about = "Open the bounded inert excerpt identified by an exact cached row token")]
    Show {
        #[arg(long, help = "ProjectionRowToken JSON file, or - for bounded stdin")]
        input: PathBuf,
    },
}
impl IndexCommand {
    pub(super) fn is_mutation(&self) -> bool {
        matches!(self, Self::Refresh { .. })
    }
}

pub(super) fn run(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &Options,
    selection: &Source,
    command: &IndexCommand,
) -> Result<Option<u8>> {
    options.output.validate()?;
    let selector = selection.selector()?;
    let selected_source = json!(&selector);
    let root = repository
        .root()
        .parent()
        .ok_or_else(|| invalid("Planning source has no worktree"))?;
    let limits = ProjectionLimits::default();
    if let IndexCommand::Refresh { rebuild } = command {
        let mut store = ProjectionStore::open(root, selector, limits)?;
        let (outcome, view) =
            match store.refresh(&ProjectionRefreshRequest { rebuild: *rebuild })? {
                ProjectionRefresh::Published(view) => ("published", view.id().clone()),
                ProjectionRefresh::Unchanged(view) => ("unchanged", view),
                ProjectionRefresh::Superseded(view) => ("superseded", view),
            };
        let mut source = source.clone();
        source["selector"] = selected_source;
        source["projection"] = json!(view);
        source["freshness"] = json!(store.status().state);
        options.output.emit(
            options.json,
            "index_refresh",
            &source,
            &json!({"outcome":outcome,"status":store.status()}),
        )?;
        return Ok(None);
    }
    // Input errors must not create or repair an index. Opening cached storage
    // also retains no-follow checkout identities and cannot acquire write mode.
    let mut store = ProjectionStore::open_cached(root, selector, limits)?;
    let view = store.load()?.ok_or_else(|| {
        PmError::new(ErrorCode::NotFound, "No usable cached index is available")
            .hint("Run workdeck index refresh for the selected source, then repeat the query.")
    })?;
    let mut source = source.clone();
    source["selector"] = selected_source;
    source["projection"] = json!(view.id());
    source["freshness"] = json!("cached");
    let (kind, result) = match command {
        IndexCommand::Query {
            query,
            offset,
            limit,
        } => {
            let handle = query_handle(cwd, &view, query, *offset > 0)?;
            (
                "index_query",
                json!({"status":store.status(),"page":view.page(&handle,*offset,*limit)?,"groups":view.groups(&handle)?}),
            )
        }
        IndexCommand::Board {
            query,
            first_group,
            columns,
            rows,
            selected,
        } => {
            let request: ProjectionQuery = pm_cli::typed_input(cwd, &query.input)?;
            if !matches!(
                request,
                ProjectionQuery::Issues {
                    group_by: Some(_),
                    ..
                }
            ) {
                return Err(invalid(
                    "Board queries require kind issues and an explicit group_by",
                ));
            }
            let handle = checked_handle(
                &view,
                query,
                &request,
                *first_group > 0 || selected.is_some(),
            )?;
            let window = ProjectionBoardRequest {
                first_group: *first_group,
                columns: *columns,
                rows: *rows,
                selected: *selected,
            };
            (
                "index_board",
                json!({"status":store.status(),"handle":handle,"groups":view.groups(&handle)?,"columns":view.board(&handle,&window)?}),
            )
        }
        IndexCommand::Show { input } => {
            let token: ProjectionRowToken = pm_cli::typed_input(cwd, input)?;
            (
                "index_document",
                json!({"status":store.status(),"detail":view.detail(&token)?}),
            )
        }
        IndexCommand::Refresh { .. } => unreachable!("refresh handled before read-only open"),
    };
    options.output.emit(options.json, kind, &source, &result)?;
    Ok(None)
}

fn query_handle(
    cwd: &Path,
    view: &ProjectionReadView,
    input: &QueryInput,
    later: bool,
) -> Result<ProjectionQueryHandle> {
    let query: ProjectionQuery = pm_cli::typed_input(cwd, &input.input)?;
    checked_handle(view, input, &query, later)
}
fn checked_handle(
    view: &ProjectionReadView,
    input: &QueryInput,
    query: &ProjectionQuery,
    later: bool,
) -> Result<ProjectionQueryHandle> {
    if later && input.expected_query.is_none() {
        return Err(invalid(
            "Later windows require --expected-query from the inspected response",
        ));
    }
    let handle = view.query(query)?;
    if let Some(expected) = &input.expected_query {
        if expected.len() > 64 * 1024 {
            return Err(invalid("Expected query handle exceeds its byte bound"));
        }
        let expected: ProjectionQueryHandle = serde_json::from_str(expected)
            .map_err(|error| invalid(&format!("Invalid expected query handle: {error}")))?;
        if expected != handle {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Cached source or query changed; restart pagination from the first window",
            ));
        }
    }
    Ok(handle)
}
fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
