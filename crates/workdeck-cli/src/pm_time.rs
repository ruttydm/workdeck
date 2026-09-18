//! Time declarations use the shared append-only engine and captured attribution.
use super::pm_cli;
use clap::{Args, Subcommand};
use serde_json::Value;
use workdeck_pm::{
    Repository, RequestId, Result, TimeEntryAmendment, TimeEntryInput, TimeReportQuery, Timestamp,
};

#[derive(Debug, Default, Args)]
pub(super) struct TimeOptions {
    #[command(flatten)]
    mutation: pm_cli::IssueOptions,
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub(super) struct Work {
    #[arg(
        long,
        value_name = "INTEGER",
        help = "Exact nonnegative duration in seconds; no heuristic deduplication"
    )]
    seconds: String,
    #[arg(
        long,
        value_name = "RFC3339",
        help = "Timestamp of the work being declared"
    )]
    worked_at: String,
    #[arg(
        long,
        help = "Explicit identity of the person or agent whose work is recorded"
    )]
    user: String,
    #[arg(long, help = "Explicit identity making this declaration or correction")]
    actor: String,
}

impl Work {
    fn input(&self) -> Result<TimeEntryInput> {
        Ok(TimeEntryInput {
            user: self.user.clone(),
            actor: self.actor.clone(),
            seconds: self.seconds.parse().map_err(|_| {
                pm_cli::invalid("seconds must be a nonnegative integer within u64 range")
            })?,
            worked_at: timestamp(&self.worked_at)?,
        })
    }
}

#[derive(Debug, Subcommand)]
pub(super) enum TimeCommand {
    #[command(about = "Append a time entry and capture the issue's current cycle")]
    Log {
        issue: String,
        #[command(flatten)]
        work: Work,
    },
    #[command(about = "Append a correction of an active entry; retain its original file and cycle")]
    Amend {
        issue: String,
        #[arg(value_name = "TIME_ID")]
        entry: String,
        #[command(flatten)]
        work: Work,
        #[arg(
            long,
            value_name = "SHA256",
            help = "Exact content identity of the time entry being superseded"
        )]
        expected_entry_content: String,
        #[arg(long, help = "Explicit reason for the correction")]
        reason: String,
    },
    #[command(about = "Read all time records for an issue, including superseded history")]
    List { issue: String },
    #[command(about = "Sum active amendment chains once, using historical cycle attribution")]
    Report {
        #[arg(long)]
        issue: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        cycle: Option<String>,
        #[arg(long, value_name = "RFC3339", help = "Inclusive work timestamp")]
        from: Option<String>,
        #[arg(long, value_name = "RFC3339", help = "Exclusive work timestamp")]
        to: Option<String>,
    },
}

pub(super) fn run(
    repository: &Repository,
    source: &Value,
    options: &TimeOptions,
    command: &TimeCommand,
) -> Result<()> {
    match command {
        TimeCommand::List { issue } => {
            pm_cli::read_options(&options.mutation)?;
            pm_cli::emit(
                options.json,
                "time_entries",
                source,
                &repository.time_entries(issue)?,
                None,
            )
        }
        TimeCommand::Report {
            issue,
            user,
            cycle,
            from,
            to,
        } => {
            pm_cli::read_options(&options.mutation)?;
            let query = TimeReportQuery {
                issue: issue.clone(),
                user: user.clone(),
                cycle: cycle.clone(),
                from: from.as_deref().map(timestamp).transpose()?,
                to: to.as_deref().map(timestamp).transpose()?,
            };
            pm_cli::emit(
                options.json,
                "time_report",
                source,
                &repository.time_report(&query)?,
                None,
            )
        }
        TimeCommand::Log { issue, work } | TimeCommand::Amend { issue, work, .. } => {
            let request = options
                .mutation
                .request_id
                .as_deref()
                .map(str::parse)
                .transpose()?
                .unwrap_or_else(RequestId::new);
            let expected = pm_cli::expected(&options.mutation)?;
            let input = work.input()?;
            let receipt = match command {
                TimeCommand::Log { .. } => {
                    repository.log_time(issue, expected.as_ref(), &input, &request)?
                }
                TimeCommand::Amend {
                    entry,
                    expected_entry_content,
                    reason,
                    ..
                } => repository.amend_time(
                    issue,
                    expected.as_ref(),
                    &TimeEntryAmendment {
                        entry: input,
                        supersedes: entry.parse()?,
                        expected: expected_entry_content.parse()?,
                        reason: reason.clone(),
                    },
                    &request,
                )?,
                _ => unreachable!(),
            };
            pm_cli::emit_mutation(
                repository,
                &options.mutation,
                options.json,
                "time_entry",
                source,
                &receipt,
            )
        }
    }
}

fn timestamp(value: &str) -> Result<Timestamp> {
    chrono::DateTime::parse_from_rfc3339(value).map(|date|date.with_timezone(&chrono::Utc))
        .map_err(|_|pm_cli::invalid("work timestamps and report boundaries must be RFC3339 timestamps with an explicit offset"))
}
