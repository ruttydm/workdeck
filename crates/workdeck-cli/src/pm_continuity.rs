//! Authored questions and handoffs retain their inspected inputs on every write.
use super::{pm_cli, pm_context::OutputOptions};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use workdeck_pm::{
    ContentHash, QuestionMutation, QuestionQuery, Repository, RequestId, Result, SourceToken,
};

#[derive(Debug, Default, Args)]
pub(super) struct ContinuityOptions {
    #[command(flatten)]
    pub native: pm_cli::NativeOptions,
    #[command(flatten)]
    pub output: OutputOptions,
    #[arg(
        long,
        global = true,
        help = "Maximum list records (default 20, at most 100)"
    )]
    pub limit: Option<usize>,
    #[arg(
        long,
        global = true,
        help = "JSON next_cursor from the same query and source snapshot"
    )]
    pub cursor: Option<String>,
}

#[derive(Debug, Subcommand)]
pub(super) enum QuestionCommand {
    List {
        #[arg(long)]
        query_file: Option<PathBuf>,
    },
    Show {
        id: String,
    },
    #[command(about = "Inspect current or stale question applicability and permitted actions")]
    Applicability {
        id: String,
    },
    #[command(about = "Create from source-bound CreateQuestion JSON; '-' reads stdin")]
    Create {
        input: PathBuf,
    },
    Answer {
        id: String,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        body_file: PathBuf,
        #[arg(long)]
        decisions_file: Option<PathBuf>,
    },
    Supersede {
        id: String,
        replacement: String,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        expected_replacement_revision: String,
        #[arg(long)]
        expected_replacement_content: String,
    },
}
impl QuestionCommand {
    pub fn is_mutation(&self) -> bool {
        matches!(
            self,
            Self::Create { .. } | Self::Answer { .. } | Self::Supersede { .. }
        )
    }
}

#[derive(Debug, Subcommand)]
pub(super) enum HandoffCommand {
    List {
        #[arg(long)]
        issue: String,
    },
    Show {
        id: String,
        #[arg(long)]
        issue: String,
    },
    #[command(
        about = "Append immutable CreateHandoff JSON with its inspected context anchor; '-' reads stdin"
    )]
    Create { input: PathBuf },
}
impl HandoffCommand {
    pub fn is_mutation(&self) -> bool {
        matches!(self, Self::Create { .. })
    }
}

pub(super) fn question(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &ContinuityOptions,
    command: &QuestionCommand,
) -> Result<()> {
    options.output.validate()?;
    if !matches!(command, QuestionCommand::List { .. }) {
        options.reject_page()?;
    }
    if !command.is_mutation() {
        pm_cli::read_options(&options.native.mutation)?;
        return match command {
            QuestionCommand::List { query_file } => {
                let query: QuestionQuery = query_file
                    .as_ref()
                    .map(|path| pm_cli::typed_input(cwd, path))
                    .transpose()?
                    .unwrap_or_default();
                let records = repository.questions(&query)?;
                options.page("questions", source, &query, &records)
            }
            QuestionCommand::Show { id } => options.output.emit(
                options.native.json,
                "question",
                source,
                &repository.question(&id.parse()?)?,
            ),
            QuestionCommand::Applicability { id } => options.output.emit(
                options.native.json,
                "question_applicability",
                source,
                &repository.question_applicability(&id.parse()?)?,
            ),
            _ => unreachable!(),
        };
    }
    let expected = pm_cli::expected(&options.native.mutation)?;
    let request = request(options)?;
    let receipt = match command {
        QuestionCommand::Create { input } => {
            if expected.is_some() {
                return Err(pm_cli::invalid(
                    "question creation uses reviewed subject sources inside its input",
                ));
            }
            repository.create_question(&pm_cli::typed_input(cwd, input)?, &request)?
        }
        QuestionCommand::Answer {
            id,
            actor,
            body_file,
            decisions_file,
        } => {
            if body_file == Path::new("-") && decisions_file.as_deref() == Some(Path::new("-")) {
                return Err(pm_cli::invalid(
                    "body and decision references cannot both read stdin",
                ));
            }
            let expected = require_expected(expected)?;
            let body = pm_cli::read_input(cwd, body_file)?;
            let decision_refs = decisions_file
                .as_ref()
                .map(|path| pm_cli::typed_input(cwd, path))
                .transpose()?
                .unwrap_or_default();
            repository.mutate_question(
                &id.parse()?,
                &expected,
                &QuestionMutation::Answer {
                    actor: actor.clone(),
                    body,
                    decision_refs,
                },
                &request,
            )?
        }
        QuestionCommand::Supersede {
            id,
            replacement,
            actor,
            reason,
            expected_replacement_revision,
            expected_replacement_content,
        } => {
            let expected = require_expected(expected)?;
            let replacement_source = SourceToken {
                revision: workdeck_pm::Revision::new(
                    expected_replacement_revision.parse().map_err(|_| {
                        pm_cli::invalid("expected-replacement-revision must be a positive integer")
                    })?,
                )?,
                content: expected_replacement_content.parse()?,
            };
            repository.mutate_question(
                &id.parse()?,
                &expected,
                &QuestionMutation::Supersede {
                    actor: actor.clone(),
                    reason: reason.clone(),
                    replacement: replacement.parse()?,
                    replacement_source,
                },
                &request,
            )?
        }
        _ => unreachable!(),
    };
    options.output.mutation(
        repository,
        &options.native,
        "question_mutation",
        source,
        &receipt,
    )
}

pub(super) fn handoff(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &ContinuityOptions,
    command: &HandoffCommand,
) -> Result<()> {
    options.output.validate()?;
    if !matches!(command, HandoffCommand::List { .. }) {
        options.reject_page()?;
    }
    if !command.is_mutation() {
        pm_cli::read_options(&options.native.mutation)?;
        return match command {
            HandoffCommand::List { issue } => options.page(
                "handoffs",
                source,
                &json!({"issue":issue}),
                &repository.handoffs(&issue.parse()?)?,
            ),
            HandoffCommand::Show { id, issue } => options.output.emit(
                options.native.json,
                "handoff",
                source,
                &repository.handoff(&issue.parse()?, &id.parse()?)?,
            ),
            _ => unreachable!(),
        };
    }
    if pm_cli::expected(&options.native.mutation)?.is_some() {
        return Err(pm_cli::invalid(
            "handoff creation uses its captured context anchor; handoffs are immutable",
        ));
    }
    let HandoffCommand::Create { input } = command else {
        unreachable!()
    };
    let receipt =
        repository.create_handoff(&pm_cli::typed_input(cwd, input)?, &request(options)?)?;
    options.output.mutation(
        repository,
        &options.native,
        "handoff_create",
        source,
        &receipt,
    )
}

fn require_expected(expected: Option<SourceToken>) -> Result<SourceToken> {
    expected.ok_or_else(|| pm_cli::invalid("question mutation requires both expected-revision and expected-content from the inspected question"))
}
fn request(options: &ContinuityOptions) -> Result<RequestId> {
    options
        .native
        .mutation
        .request_id
        .as_deref()
        .map(str::parse)
        .transpose()
        .map(|value| value.unwrap_or_else(RequestId::new))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageCursor {
    fingerprint: ContentHash,
    offset: usize,
}

impl ContinuityOptions {
    fn reject_page(&self) -> Result<()> {
        if self.limit.is_some() || self.cursor.is_some() {
            return Err(pm_cli::invalid(
                "limit and cursor are only available for list operations",
            ));
        }
        Ok(())
    }
    fn page(
        &self,
        kind: &str,
        source: &Value,
        query: &impl Serialize,
        records: &impl Serialize,
    ) -> Result<()> {
        let limit = self.limit.unwrap_or(20);
        if !(1..=100).contains(&limit) {
            return Err(pm_cli::invalid("limit must be between 1 and 100"));
        }
        let records =
            serde_json::to_value(records).map_err(|error| pm_cli::invalid(error.to_string()))?;
        let records = records
            .as_array()
            .ok_or_else(|| pm_cli::invalid("list result must be an array"))?;
        let encoded = serde_json::to_vec(
            &json!({"kind":kind,"source":source,"query":query,"records":records,"limit":limit}),
        )
        .map_err(|error| pm_cli::invalid(error.to_string()))?;
        let fingerprint = ContentHash::of(&encoded);
        let cursor: Option<PageCursor> = self
            .cursor
            .as_deref()
            .map(|value| {
                if value.len() > 4096 {
                    return Err(pm_cli::invalid("cursor exceeds 4096 bytes"));
                }
                serde_json::from_str(value)
                    .map_err(|error| pm_cli::invalid(format!("invalid cursor JSON: {error}")))
            })
            .transpose()?;
        let offset = if let Some(cursor) = cursor {
            if cursor.fingerprint != fingerprint {
                return Err(workdeck_pm::PmError::new(
                    workdeck_pm::ErrorCode::StaleSource,
                    "list source or query changed; restart pagination",
                ));
            }
            if cursor.offset > records.len() {
                return Err(pm_cli::invalid("cursor offset exceeds list membership"));
            }
            cursor.offset
        } else {
            0
        };
        let end = offset.saturating_add(limit).min(records.len());
        let next_cursor = (end < records.len()).then(|| PageCursor {
            fingerprint: fingerprint.clone(),
            offset: end,
        });
        self.output.emit(self.native.json,kind,source,&json!({"records":&records[offset..end],"total":records.len(),"fingerprint":fingerprint,"next_cursor":next_cursor}))
    }
}
