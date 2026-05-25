use anyhow::{Context, Result, bail};
use chrono::Utc;
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use workdeck_cli::app::App;
use workdeck_cli::config::Config;
use workdeck_cli::git;
use workdeck_cli::store::{
    AgentSession, AgentTouchedFile, Cycle, Issue, IssueStatus, IssueUpdate, Label, Priority,
    Project, ReferenceData, StoreEvent, WorkdeckStore,
};

#[derive(Debug, Parser)]
#[command(name = "workdeck")]
#[command(about = "Terminal-native sidecar for agentic coding")]
#[command(version)]
struct Args {
    #[arg(long, value_name = "PATH", default_value = ".")]
    cwd: PathBuf,

    #[arg(long, help = "Initialize .agents/workdeck without opening the TUI")]
    init: bool,

    #[arg(long, help = "Print a JSON status snapshot without opening the TUI")]
    status_json: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Print a Git status snapshot")]
    Status {
        #[arg(long, help = "Print status as JSON")]
        json: bool,
    },
    #[command(about = "Inspect repo files without opening the TUI")]
    Files {
        #[command(subcommand)]
        command: FilesCommand,
    },
    #[command(about = "Inspect Git changes without opening the TUI")]
    Changes {
        #[command(subcommand)]
        command: ChangesCommand,
    },
    #[command(about = "Search files, changes, issues, and agent data")]
    Search {
        query: String,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Limit targets: files,changes,issues,agents"
        )]
        target: Vec<String>,
        #[arg(long, help = "Print search results as JSON")]
        json: bool,
    },
    #[command(about = "Manage Workdeck config")]
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    #[command(about = "Inspect Workdeck event log")]
    Events {
        #[command(subcommand)]
        command: EventsCommand,
    },
    #[command(about = "Import Workdeck export JSON")]
    Import {
        path: PathBuf,
        #[arg(long, conflicts_with = "replace", help = "Merge imported data")]
        merge: bool,
        #[arg(
            long,
            conflicts_with = "merge",
            help = "Replace local Workdeck data before import"
        )]
        replace: bool,
        #[arg(long, help = "Validate without writing")]
        dry_run: bool,
        #[arg(long, help = "Print import result as JSON")]
        json: bool,
    },
    #[command(about = "Validate repo, config, and local Workdeck data")]
    Doctor {
        #[arg(long, help = "Print doctor results as JSON")]
        json: bool,
    },
    #[command(about = "Export local Workdeck data as JSON or JSONL")]
    Export {
        #[arg(long, help = "Emit JSON; default unless --jsonl is used")]
        json: bool,
        #[arg(long, help = "Emit one JSON object per line")]
        jsonl: bool,
    },
    #[command(about = "Manage local Workdeck issues")]
    Issue {
        #[command(subcommand)]
        command: IssueCommand,
    },
    #[command(about = "Manage local agent sessions")]
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    #[command(about = "Manage local Workdeck projects")]
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    #[command(about = "Manage local Workdeck cycles")]
    Cycle {
        #[command(subcommand)]
        command: CycleCommand,
    },
    #[command(about = "Manage local Workdeck labels")]
    Label {
        #[command(subcommand)]
        command: LabelCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    #[command(about = "Print repo config path")]
    Path {
        #[arg(long, help = "Print config path as JSON")]
        json: bool,
    },
    #[command(about = "Print merged config")]
    Show {
        #[arg(long, help = "Print config as JSON")]
        json: bool,
    },
    #[command(about = "Initialize repo config")]
    Init {
        #[arg(long, help = "Print init result as JSON")]
        json: bool,
    },
    #[command(about = "Validate merged config")]
    Validate {
        #[arg(long, help = "Print validation result as JSON")]
        json: bool,
    },
    #[command(about = "Get one repo config key")]
    Get {
        key: String,
        #[arg(long, help = "Print value as JSON")]
        json: bool,
    },
    #[command(about = "Set one repo config key")]
    Set {
        key: String,
        value: String,
        #[arg(long, help = "Print set result as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum EventsCommand {
    #[command(about = "List event log records")]
    List {
        #[arg(long, help = "Print events as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum FilesCommand {
    #[command(about = "List direct repo file browser entries")]
    List {
        path: Option<PathBuf>,
        #[arg(long, help = "Print file entries as JSON")]
        json: bool,
    },
    #[command(about = "Show a file preview")]
    Show {
        path: PathBuf,
        #[arg(long, help = "Print file preview as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ChangesCommand {
    #[command(about = "List changed files")]
    List {
        #[arg(
            long,
            default_value = "directory",
            help = "Group by directory or status"
        )]
        group: String,
        #[arg(long, help = "Print changes as JSON")]
        json: bool,
    },
    #[command(about = "Show staged and unstaged diff preview for a path")]
    Diff {
        path: PathBuf,
        #[arg(long, help = "Print diff preview as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum IssueCommand {
    #[command(about = "List local issues")]
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        cycle: Option<String>,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        due_at: Option<String>,
        #[arg(long, help = "Print issues as JSON")]
        json: bool,
    },
    #[command(about = "Create a local issue")]
    Create {
        title: Option<String>,
        #[arg(
            long,
            value_name = "PATH",
            help = "Read issue fields from JSON file, or '-' for stdin"
        )]
        from_json: Option<PathBuf>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        cycle: Option<String>,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        due_at: Option<String>,
        #[arg(long, value_delimiter = ',')]
        label: Vec<String>,
        #[arg(long = "commit", value_delimiter = ',')]
        linked_commit: Vec<String>,
        #[arg(long = "file")]
        linked_file: Vec<String>,
        #[arg(long, help = "Print the created issue as JSON")]
        json: bool,
    },
    #[command(about = "Update a local issue")]
    Update {
        key: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        cycle: Option<String>,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        due_at: Option<String>,
        #[arg(long, value_delimiter = ',')]
        label: Vec<String>,
        #[arg(long = "commit", value_delimiter = ',')]
        linked_commit: Vec<String>,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Link a file path to an issue")]
    Link {
        key: String,
        path: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(name = "link-file", about = "Link a file path to an issue")]
    LinkFile {
        key: String,
        path: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(
        name = "unlink-file",
        about = "Remove a linked file path from an issue"
    )]
    UnlinkFile {
        key: String,
        path: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(name = "link-commit", about = "Link a commit SHA to an issue")]
    LinkCommit {
        key: String,
        sha: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(
        name = "unlink-commit",
        about = "Remove a linked commit SHA from an issue"
    )]
    UnlinkCommit {
        key: String,
        sha: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Close an issue")]
    Close {
        key: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Reopen an issue as Todo")]
    Reopen {
        key: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Move an issue to a status")]
    Move {
        key: String,
        #[arg(long)]
        status: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Assign an issue")]
    Assign {
        key: String,
        assignee: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Unassign an issue")]
    Unassign {
        key: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Manage issue labels")]
    Label {
        #[command(subcommand)]
        command: IssueLabelCommand,
    },
    #[command(about = "Delete an issue")]
    Delete {
        key: String,
        #[arg(long, help = "Confirm deletion")]
        yes: bool,
        #[arg(long, help = "Print deletion result as JSON")]
        json: bool,
    },
    #[command(about = "Show one local issue")]
    Show {
        key: String,
        #[arg(long, help = "Print the issue as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum IssueLabelCommand {
    #[command(about = "Add a label to an issue")]
    Add {
        key: String,
        label: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Remove a label from an issue")]
    Remove {
        key: String,
        label: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
enum AgentCommand {
    #[command(about = "List local agent sessions")]
    List {
        #[arg(long, help = "Print sessions as JSON")]
        json: bool,
    },
    #[command(about = "Record a local agent session")]
    Record {
        title: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        goal: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long = "plan")]
        plan_item: Vec<String>,
        #[arg(long = "file")]
        touched_file: Vec<String>,
        #[arg(long = "command")]
        command_run: Vec<String>,
        #[arg(long = "test")]
        test_run: Vec<String>,
        #[arg(long = "note")]
        handoff_note: Vec<String>,
        #[arg(long, help = "Print the recorded session as JSON")]
        json: bool,
    },
    #[command(about = "Show one agent session")]
    Show {
        id: String,
        #[arg(long, help = "Print the session as JSON")]
        json: bool,
    },
    #[command(about = "Update a local agent session")]
    Update {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        goal: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long, help = "Print the updated session as JSON")]
        json: bool,
    },
    #[command(about = "Mark an agent session done")]
    Finish {
        id: String,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long, help = "Print the updated session as JSON")]
        json: bool,
    },
    #[command(name = "append-plan", about = "Append a plan item to an agent session")]
    AppendPlan {
        id: String,
        text: String,
        #[arg(long, help = "Print the updated session as JSON")]
        json: bool,
    },
    #[command(name = "add-file", about = "Append a touched file to an agent session")]
    AddFile {
        id: String,
        path: String,
        #[arg(long, default_value = "modified")]
        change_type: String,
        #[arg(long, help = "Print the updated session as JSON")]
        json: bool,
    },
    #[command(name = "add-command", about = "Append a command to an agent session")]
    AddCommand {
        id: String,
        text: String,
        #[arg(long, help = "Print the updated session as JSON")]
        json: bool,
    },
    #[command(name = "add-test", about = "Append a test command to an agent session")]
    AddTest {
        id: String,
        text: String,
        #[arg(long, help = "Print the updated session as JSON")]
        json: bool,
    },
    #[command(name = "add-note", about = "Append a handoff note to an agent session")]
    AddNote {
        id: String,
        text: String,
        #[arg(long, help = "Print the updated session as JSON")]
        json: bool,
    },
    #[command(about = "Delete an agent session")]
    Delete {
        id: String,
        #[arg(long, help = "Confirm deletion")]
        yes: bool,
        #[arg(long, help = "Print deletion result as JSON")]
        json: bool,
    },
    #[command(about = "Import agent sessions from JSON or JSONL")]
    Import {
        path: PathBuf,
        #[arg(long, help = "Print imported sessions as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    #[command(about = "List local projects")]
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long, help = "Print projects as JSON")]
        json: bool,
    },
    #[command(about = "Create or update a local project")]
    Save {
        name: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long, help = "Print the saved project as JSON")]
        json: bool,
    },
    #[command(about = "Show one project")]
    Show {
        id: String,
        #[arg(long, help = "Print the project as JSON")]
        json: bool,
    },
    #[command(about = "Delete a project")]
    Delete {
        id: String,
        #[arg(long, help = "Confirm deletion")]
        yes: bool,
        #[arg(long, help = "Clear issue references")]
        force: bool,
        #[arg(long, help = "Print the deleted project as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum CycleCommand {
    #[command(about = "List local cycles")]
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long, help = "Print cycles as JSON")]
        json: bool,
    },
    #[command(about = "Create or update a local cycle")]
    Save {
        name: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        starts_at: Option<String>,
        #[arg(long)]
        ends_at: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long, help = "Print the saved cycle as JSON")]
        json: bool,
    },
    #[command(about = "Show one cycle")]
    Show {
        id: String,
        #[arg(long, help = "Print the cycle as JSON")]
        json: bool,
    },
    #[command(about = "Delete a cycle")]
    Delete {
        id: String,
        #[arg(long, help = "Confirm deletion")]
        yes: bool,
        #[arg(long, help = "Clear issue references")]
        force: bool,
        #[arg(long, help = "Print the deleted cycle as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum LabelCommand {
    #[command(about = "List local labels")]
    List {
        #[arg(long)]
        color: Option<String>,
        #[arg(long, help = "Print labels as JSON")]
        json: bool,
    },
    #[command(about = "Create or update a local label")]
    Save {
        name: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        color: Option<String>,
        #[arg(long, help = "Print the saved label as JSON")]
        json: bool,
    },
    #[command(about = "Show one label")]
    Show {
        id: String,
        #[arg(long, help = "Print the label as JSON")]
        json: bool,
    },
    #[command(about = "Delete a label")]
    Delete {
        id: String,
        #[arg(long, help = "Confirm deletion")]
        yes: bool,
        #[arg(long, help = "Remove label from issues")]
        force: bool,
        #[arg(long, help = "Print the deleted label as JSON")]
        json: bool,
    },
}

fn main() -> ExitCode {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(error) => {
            let code = error.exit_code();
            let _ = error.print();
            return ExitCode::from(code.try_into().unwrap_or(2));
        }
    };
    let wants_json = args.wants_json();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if is_json_error_already_printed(&error) {
                // The command printed a structured JSON error with command-specific details.
            } else if wants_json {
                let _ = print_json_error(&error);
            } else {
                eprintln!("Error: {error:#}");
            }
            ExitCode::from(classify_exit_code(&error))
        }
    }
}

fn run(args: Args) -> Result<()> {
    let repo_root = git::discover_repo_root(&args.cwd)?;

    if let Some(Command::Doctor { json }) = &args.command {
        handle_doctor(&repo_root, *json)?;
        return Ok(());
    }

    let config = Config::load(&repo_root)?;
    let store = WorkdeckStore::new(config.data_dir(&repo_root));

    if args.init {
        store.init()?;
        println!("initialized {}", store.root().display());
        return Ok(());
    }

    if args.status_json {
        print_status(&repo_root, true)?;
        return Ok(());
    }

    if let Some(command) = args.command {
        match command {
            Command::Status { json } => print_status(&repo_root, json)?,
            Command::Files { command } => handle_files_command(&repo_root, command)?,
            Command::Changes { command } => handle_changes_command(&repo_root, command)?,
            Command::Search {
                query,
                target,
                json,
            } => handle_search_command(&repo_root, &store, query, target, json)?,
            Command::Config { command } => handle_config_command(&repo_root, &store, command)?,
            Command::Events { command } => handle_events_command(&store, command)?,
            Command::Import {
                path,
                merge,
                replace,
                dry_run,
                json,
            } => handle_import_command(&store, path, merge, replace, dry_run, json)?,
            Command::Doctor { .. } => unreachable!("doctor is handled before config load"),
            Command::Export { json, jsonl } => handle_export(&repo_root, &store, json, jsonl)?,
            Command::Issue { command } => handle_issue_command(&store, command)?,
            Command::Agent { command } => handle_agent_command(&store, command)?,
            Command::Project { command } => handle_project_command(&store, command)?,
            Command::Cycle { command } => handle_cycle_command(&store, command)?,
            Command::Label { command } => handle_label_command(&store, command)?,
        }
        return Ok(());
    }

    let app = App::new(&args.cwd)?;
    workdeck_cli::tui::run(app)
}

impl Args {
    fn wants_json(&self) -> bool {
        self.status_json || self.command.as_ref().is_some_and(Command::wants_json)
    }
}

impl Command {
    fn wants_json(&self) -> bool {
        match self {
            Command::Status { json } => *json,
            Command::Files { command } => command.wants_json(),
            Command::Changes { command } => command.wants_json(),
            Command::Search { json, .. } => *json,
            Command::Config { command } => command.wants_json(),
            Command::Events { command } => command.wants_json(),
            Command::Import { json, .. } => *json,
            Command::Doctor { json } => *json,
            Command::Export { json, jsonl } => *json && !*jsonl,
            Command::Issue { command } => command.wants_json(),
            Command::Agent { command } => command.wants_json(),
            Command::Project { command } => command.wants_json(),
            Command::Cycle { command } => command.wants_json(),
            Command::Label { command } => command.wants_json(),
        }
    }
}

impl FilesCommand {
    fn wants_json(&self) -> bool {
        match self {
            FilesCommand::List { json, .. } | FilesCommand::Show { json, .. } => *json,
        }
    }
}

impl ChangesCommand {
    fn wants_json(&self) -> bool {
        match self {
            ChangesCommand::List { json, .. } | ChangesCommand::Diff { json, .. } => *json,
        }
    }
}

impl ConfigCommand {
    fn wants_json(&self) -> bool {
        match self {
            ConfigCommand::Path { json }
            | ConfigCommand::Show { json }
            | ConfigCommand::Init { json }
            | ConfigCommand::Validate { json }
            | ConfigCommand::Get { json, .. }
            | ConfigCommand::Set { json, .. } => *json,
        }
    }
}

impl EventsCommand {
    fn wants_json(&self) -> bool {
        match self {
            EventsCommand::List { json } => *json,
        }
    }
}

impl IssueCommand {
    fn wants_json(&self) -> bool {
        match self {
            IssueCommand::List { json, .. }
            | IssueCommand::Create { json, .. }
            | IssueCommand::Update { json, .. }
            | IssueCommand::Link { json, .. }
            | IssueCommand::LinkFile { json, .. }
            | IssueCommand::UnlinkFile { json, .. }
            | IssueCommand::LinkCommit { json, .. }
            | IssueCommand::UnlinkCommit { json, .. }
            | IssueCommand::Close { json, .. }
            | IssueCommand::Reopen { json, .. }
            | IssueCommand::Move { json, .. }
            | IssueCommand::Assign { json, .. }
            | IssueCommand::Unassign { json, .. }
            | IssueCommand::Delete { json, .. }
            | IssueCommand::Show { json, .. } => *json,
            IssueCommand::Label { command } => command.wants_json(),
        }
    }
}

impl IssueLabelCommand {
    fn wants_json(&self) -> bool {
        match self {
            IssueLabelCommand::Add { json, .. } | IssueLabelCommand::Remove { json, .. } => *json,
        }
    }
}

impl AgentCommand {
    fn wants_json(&self) -> bool {
        match self {
            AgentCommand::List { json }
            | AgentCommand::Record { json, .. }
            | AgentCommand::Show { json, .. }
            | AgentCommand::Update { json, .. }
            | AgentCommand::Finish { json, .. }
            | AgentCommand::AppendPlan { json, .. }
            | AgentCommand::AddFile { json, .. }
            | AgentCommand::AddCommand { json, .. }
            | AgentCommand::AddTest { json, .. }
            | AgentCommand::AddNote { json, .. }
            | AgentCommand::Delete { json, .. }
            | AgentCommand::Import { json, .. } => *json,
        }
    }
}

impl ProjectCommand {
    fn wants_json(&self) -> bool {
        match self {
            ProjectCommand::List { json, .. }
            | ProjectCommand::Save { json, .. }
            | ProjectCommand::Show { json, .. }
            | ProjectCommand::Delete { json, .. } => *json,
        }
    }
}

impl CycleCommand {
    fn wants_json(&self) -> bool {
        match self {
            CycleCommand::List { json, .. }
            | CycleCommand::Save { json, .. }
            | CycleCommand::Show { json, .. }
            | CycleCommand::Delete { json, .. } => *json,
        }
    }
}

impl LabelCommand {
    fn wants_json(&self) -> bool {
        match self {
            LabelCommand::List { json, .. }
            | LabelCommand::Save { json, .. }
            | LabelCommand::Show { json, .. }
            | LabelCommand::Delete { json, .. } => *json,
        }
    }
}

fn classify_exit_code(error: &anyhow::Error) -> u8 {
    let message = format!("{error:#}").to_ascii_lowercase();
    if message.contains("__json_error_printed__") {
        1
    } else if message.contains("does not exist") || message.contains("not found") {
        3
    } else if message.contains("duplicate") || message.contains("already exists") {
        4
    } else if message.contains("failed to parse")
        || message.contains("invalid workdeck config")
        || message.contains("config")
    {
        5
    } else if message.contains("unknown ")
        || message.contains("invalid ")
        || message.contains("cannot ")
        || message.contains("requires --yes")
        || message.contains("must ")
    {
        2
    } else {
        1
    }
}

fn is_json_error_already_printed(error: &anyhow::Error) -> bool {
    format!("{error:#}").contains("__json_error_printed__")
}

fn json_success(kind: &str, action: Option<&str>, data: impl serde::Serialize) -> Result<()> {
    let mut payload = json!({
        "ok": true,
        "kind": kind,
        "data": data,
    });
    if let Some(action) = action {
        payload["action"] = json!(action);
    }
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

fn print_json_error(error: &anyhow::Error) -> Result<()> {
    let code = classify_exit_code(error);
    let code_name = match code {
        2 => "validation_error",
        3 => "not_found",
        4 => "conflict",
        5 => "config_or_store_error",
        _ => "error",
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": false,
            "error": {
                "code": code_name,
                "message": format!("{error:#}"),
            }
        }))?
    );
    Ok(())
}

fn handle_export(
    repo_root: &Path,
    store: &WorkdeckStore,
    json_output: bool,
    jsonl: bool,
) -> Result<()> {
    let issues = store.load_issues()?;
    let reference_data = store.load_reference_data()?;
    let sessions = store.load_agent_sessions()?;
    let events = store.load_events()?;

    if jsonl {
        print_jsonl_record("repo", json!({ "root": repo_root }))?;
        for issue in issues {
            print_jsonl_record("issue", serde_json::to_value(issue)?)?;
        }
        for project in reference_data.projects {
            print_jsonl_record("project", serde_json::to_value(project)?)?;
        }
        for cycle in reference_data.cycles {
            print_jsonl_record("cycle", serde_json::to_value(cycle)?)?;
        }
        for label in reference_data.labels {
            print_jsonl_record("label", serde_json::to_value(label)?)?;
        }
        for session in sessions {
            print_jsonl_record("agent_session", serde_json::to_value(session)?)?;
        }
        for event in events {
            print_event_jsonl_record(event)?;
        }
    } else {
        let payload = json!({
            "repo_root": repo_root,
            "issues": issues,
            "projects": reference_data.projects,
            "cycles": reference_data.cycles,
            "labels": reference_data.labels,
            "agent_sessions": sessions,
            "events": events,
        });
        if json_output {
            json_success("export", None, payload)?;
        } else {
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
    }

    Ok(())
}

fn print_status(repo_root: &Path, json_output: bool) -> Result<()> {
    let snapshot = git::scan_repo(repo_root)?;
    let payload = status_payload(&snapshot);
    if json_output {
        json_success("status", None, payload)?;
    } else if snapshot.changes.is_empty() {
        println!("clean worktree");
    } else {
        for change in &snapshot.changes {
            println!(
                "{:<12} {:<18} {:>5} {:>5} {}",
                change.kind.label(),
                change.stage_label(),
                format!("+{}", change.additions),
                format!("-{}", change.deletions),
                change.path.display()
            );
        }
    }
    Ok(())
}

fn status_payload(snapshot: &git::RepoSnapshot) -> Value {
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    for change in &snapshot.changes {
        *counts.entry(change.kind.label()).or_default() += 1;
    }
    json!({
        "repo_root": snapshot.root,
        "counts": counts,
        "groups": snapshot.groups.iter().map(|group| {
            json!({
                "path": group.path,
                "files": group.files.len(),
                "additions": group.total_additions,
                "deletions": group.total_deletions,
                "changes": group.files.iter().map(change_payload).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
        "changes": snapshot.changes.iter().map(change_payload).collect::<Vec<_>>(),
    })
}

fn change_payload(change: &git::ChangeEntry) -> Value {
    json!({
        "path": change.path,
        "kind": change.kind.label(),
        "stage": change.stage_label(),
        "staged": change.staged,
        "unstaged": change.unstaged,
        "additions": change.additions,
        "deletions": change.deletions,
    })
}

fn handle_files_command(repo_root: &Path, command: FilesCommand) -> Result<()> {
    match command {
        FilesCommand::List { path, json } => {
            let files = git::list_repo_files(repo_root, 20_000)?;
            let entries = file_entries_for_path(&files, path.as_deref().unwrap_or(Path::new("")));
            if json {
                json_success("file_list", None, entries)?;
            } else if entries.is_empty() {
                println!("no files");
            } else {
                for entry in entries {
                    println!(
                        "{:<9} {}",
                        entry["kind"].as_str().unwrap_or("unknown"),
                        entry["path"].as_str().unwrap_or("")
                    );
                }
            }
        }
        FilesCommand::Show { path, json } => {
            let preview = git::read_file_preview(repo_root, &path, 80_000)?;
            if json {
                json_success("file_preview", None, file_preview_payload(&preview))?;
            } else {
                print!("{}", preview.content);
            }
        }
    }
    Ok(())
}

fn file_entries_for_path(files: &[PathBuf], cwd: &Path) -> Vec<Value> {
    let mut dirs = BTreeSet::<PathBuf>::new();
    let mut direct_files = Vec::<PathBuf>::new();
    for path in files {
        let relative = if cwd.as_os_str().is_empty() {
            path.as_path()
        } else {
            match path.strip_prefix(cwd) {
                Ok(relative) if !relative.as_os_str().is_empty() => relative,
                _ => continue,
            }
        };
        let mut components = relative.components();
        let Some(first) = components.next() else {
            continue;
        };
        let child = cwd.join(first.as_os_str());
        if components.next().is_some() {
            dirs.insert(child);
        } else {
            direct_files.push(child);
        }
    }
    direct_files.sort();

    dirs.into_iter()
        .map(|path| json!({ "kind": "directory", "path": path, "name": file_name(&path) }))
        .chain(
            direct_files
                .into_iter()
                .map(|path| json!({ "kind": "file", "path": path, "name": file_name(&path) })),
        )
        .collect()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn handle_changes_command(repo_root: &Path, command: ChangesCommand) -> Result<()> {
    match command {
        ChangesCommand::List { group, json } => {
            let snapshot = git::scan_repo(repo_root)?;
            if json {
                let payload = match group.as_str() {
                    "directory" | "dir" => status_payload(&snapshot),
                    "status" => json!({
                        "repo_root": snapshot.root,
                        "groups": changes_grouped_by_status(&snapshot.changes),
                    }),
                    value => bail!("unknown change group {value}"),
                };
                json_success("change_list", None, payload)?;
            } else if snapshot.changes.is_empty() {
                println!("clean worktree");
            } else {
                match group.as_str() {
                    "directory" | "dir" => {
                        for group in snapshot.groups {
                            println!(
                                "{} {} +{} -{}",
                                group.path.display(),
                                group.files.len(),
                                group.total_additions,
                                group.total_deletions
                            );
                            for change in group.files {
                                println!("  {:<10} {}", change.kind.label(), change.path.display());
                            }
                        }
                    }
                    "status" => {
                        for group in changes_grouped_by_status(&snapshot.changes) {
                            println!("{}", group["status"].as_str().unwrap_or("unknown"));
                            for change in group["changes"].as_array().into_iter().flatten() {
                                println!("  {}", change["path"].as_str().unwrap_or_default());
                            }
                        }
                    }
                    value => bail!("unknown change group {value}"),
                }
            }
        }
        ChangesCommand::Diff { path, json } => {
            let preview = git::diff_for_path(repo_root, &path)?;
            if json {
                json_success("change_diff", None, file_preview_payload(&preview))?;
            } else {
                print!("{}", preview.content);
            }
        }
    }
    Ok(())
}

fn file_preview_payload(preview: &git::FilePreview) -> Value {
    json!({
        "title": preview.title,
        "content": preview.content,
        "truncated": preview.truncated,
        "binary": preview.binary,
    })
}

fn changes_grouped_by_status(changes: &[git::ChangeEntry]) -> Vec<Value> {
    let mut grouped = std::collections::BTreeMap::<&str, Vec<Value>>::new();
    for change in changes {
        grouped
            .entry(change.kind.label())
            .or_default()
            .push(change_payload(change));
    }
    grouped
        .into_iter()
        .map(|(status, changes)| json!({ "status": status, "changes": changes }))
        .collect()
}

fn handle_search_command(
    repo_root: &Path,
    store: &WorkdeckStore,
    query: String,
    targets: Vec<String>,
    json_output: bool,
) -> Result<()> {
    let snapshot = git::scan_repo(repo_root)?;
    let files = git::list_repo_files(repo_root, 20_000)?;
    let issues = store.load_issues()?;
    let sessions = store.load_agent_sessions()?;
    let references = store.load_reference_data()?;
    let symbols = workdeck_cli::search::extract_symbols(repo_root, &files);
    let index = workdeck_cli::search::SearchIndex::rebuild(
        &files,
        &snapshot.changes,
        &issues,
        &sessions,
        &references,
        &symbols,
        None,
    );
    let target_filter = targets
        .into_iter()
        .map(|target| target.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let results = index
        .query(&query, 100)
        .into_iter()
        .filter(|result| {
            target_filter.is_empty()
                || target_filter.contains(search_target_group(&result.record.target))
        })
        .map(|result| {
            json!({
                "score": result.score,
                "label": result.record.label,
                "detail": result.record.detail,
                "target": search_target_payload(&result.record.target),
            })
        })
        .collect::<Vec<_>>();

    if json_output {
        json_success("search_results", None, results)?;
    } else if results.is_empty() {
        println!("no results");
    } else {
        for result in results {
            println!(
                "{:<6} {:<10} {}",
                result["score"].as_i64().unwrap_or_default(),
                result["target"]["kind"].as_str().unwrap_or("unknown"),
                result["label"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn search_target_group(target: &workdeck_cli::search::SearchTarget) -> &'static str {
    match target {
        workdeck_cli::search::SearchTarget::File(_) => "files",
        workdeck_cli::search::SearchTarget::Change(_) => "changes",
        workdeck_cli::search::SearchTarget::Issue(_) => "issues",
        workdeck_cli::search::SearchTarget::AgentSession(_) => "agents",
        workdeck_cli::search::SearchTarget::GitCommit(_)
        | workdeck_cli::search::SearchTarget::GitBranch(_)
        | workdeck_cli::search::SearchTarget::GitStash(_)
        | workdeck_cli::search::SearchTarget::GitTag(_) => "git",
        workdeck_cli::search::SearchTarget::Project(_)
        | workdeck_cli::search::SearchTarget::Cycle(_)
        | workdeck_cli::search::SearchTarget::Label(_) => "issues",
        workdeck_cli::search::SearchTarget::Symbol { .. } => "files",
    }
}

fn search_target_payload(target: &workdeck_cli::search::SearchTarget) -> Value {
    match target {
        workdeck_cli::search::SearchTarget::File(path) => {
            json!({ "kind": "file", "path": path })
        }
        workdeck_cli::search::SearchTarget::Change(path) => {
            json!({ "kind": "change", "path": path })
        }
        workdeck_cli::search::SearchTarget::Issue(key) => json!({ "kind": "issue", "key": key }),
        workdeck_cli::search::SearchTarget::AgentSession(id) => {
            json!({ "kind": "agent", "id": id })
        }
        workdeck_cli::search::SearchTarget::GitCommit(sha) => {
            json!({ "kind": "git_commit", "sha": sha })
        }
        workdeck_cli::search::SearchTarget::GitBranch(name) => {
            json!({ "kind": "git_branch", "name": name })
        }
        workdeck_cli::search::SearchTarget::GitStash(name) => {
            json!({ "kind": "git_stash", "name": name })
        }
        workdeck_cli::search::SearchTarget::GitTag(name) => {
            json!({ "kind": "git_tag", "name": name })
        }
        workdeck_cli::search::SearchTarget::Project(id) => {
            json!({ "kind": "project", "id": id })
        }
        workdeck_cli::search::SearchTarget::Cycle(id) => json!({ "kind": "cycle", "id": id }),
        workdeck_cli::search::SearchTarget::Label(id) => json!({ "kind": "label", "id": id }),
        workdeck_cli::search::SearchTarget::Symbol { path, line, name } => {
            json!({ "kind": "symbol", "path": path, "line": line, "name": name })
        }
    }
}

fn print_jsonl_record(kind: &str, payload: Value) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&json!({
            "kind": kind,
            "payload": payload,
        }))?
    );
    Ok(())
}

fn print_event_jsonl_record(event: StoreEvent) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&json!({
            "kind": "event",
            "payload": {
                "kind": event.kind,
                "payload": event.payload,
                "created_at": event.created_at,
            },
        }))?
    );
    Ok(())
}

fn handle_config_command(
    repo_root: &Path,
    store: &WorkdeckStore,
    command: ConfigCommand,
) -> Result<()> {
    let path = repo_root.join(".agents/workdeck/config.toml");
    match command {
        ConfigCommand::Path { json } => {
            if json {
                json_success("config_path", None, json!({ "path": path }))?;
            } else {
                println!("{}", path.display());
            }
        }
        ConfigCommand::Show { json } => {
            let config = Config::load(repo_root)?;
            if json {
                json_success("config", None, config)?;
            } else {
                println!("{}", toml::to_string_pretty(&config)?);
            }
        }
        ConfigCommand::Init { json } => {
            store.init()?;
            if json {
                json_success(
                    "config",
                    Some("init"),
                    json!({ "initialized": true, "path": store.root() }),
                )?;
            } else {
                println!("initialized {}", store.root().display());
            }
        }
        ConfigCommand::Validate { json } => {
            Config::load(repo_root)?;
            if json {
                json_success("config_validation", None, json!({ "ok": true }))?;
            } else {
                println!("config ok");
            }
        }
        ConfigCommand::Get { key, json } => {
            let value = load_repo_config_value(&path)?;
            let Some(value) = get_config_value(&value, &key) else {
                bail!("config key {key} does not exist");
            };
            if json {
                json_success("config_value", None, json!({ "key": key, "value": value }))?;
            } else {
                println!("{value}");
            }
        }
        ConfigCommand::Set { key, value, json } => {
            let mut root = load_repo_config_value(&path)?;
            let storage_key = config_storage_key(&key);
            set_toml_path(&mut root, &storage_key, parse_config_value(&value))?;
            let raw = toml::to_string_pretty(&root)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, raw)?;
            Config::load(repo_root)?;
            if json {
                json_success(
                    "config",
                    Some("set"),
                    json!({ "key": storage_key, "set": true }),
                )?;
            } else {
                println!("set {storage_key}");
            }
        }
    }
    Ok(())
}

fn load_repo_config_value(path: &Path) -> Result<toml::Value> {
    if !path.exists() {
        return Ok(toml::Value::Table(Default::default()));
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn get_toml_path<'a>(value: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    let mut current = value;
    for part in key.split('.') {
        current = current.as_table()?.get(part)?;
    }
    Some(current)
}

fn get_config_value<'a>(value: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    get_toml_path(value, key).or_else(|| {
        if key == "keys.issues" {
            get_toml_path(value, "keys.tasks")
        } else {
            None
        }
    })
}

fn config_storage_key(key: &str) -> String {
    if key == "keys.tasks" {
        "keys.issues".to_string()
    } else {
        key.to_string()
    }
}

fn set_toml_path(root: &mut toml::Value, key: &str, value: toml::Value) -> Result<()> {
    let parts = key
        .split('.')
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        bail!("config key cannot be empty");
    }
    let mut current = root;
    for part in &parts[..parts.len() - 1] {
        let table = current
            .as_table_mut()
            .with_context(|| format!("config path {key} is not a table"))?;
        current = table
            .entry((*part).to_string())
            .or_insert_with(|| toml::Value::Table(Default::default()));
    }
    current
        .as_table_mut()
        .with_context(|| format!("config path {key} is not a table"))?
        .insert(parts[parts.len() - 1].to_string(), value);
    Ok(())
}

fn parse_config_value(value: &str) -> toml::Value {
    match value {
        "true" => toml::Value::Boolean(true),
        "false" => toml::Value::Boolean(false),
        _ => toml::Value::String(value.to_string()),
    }
}

fn handle_events_command(store: &WorkdeckStore, command: EventsCommand) -> Result<()> {
    match command {
        EventsCommand::List { json } => {
            let events = store.load_events()?;
            if json {
                json_success("event_list", None, events)?;
            } else if events.is_empty() {
                println!("no events");
            } else {
                for event in events {
                    println!("{:<24} {}", event.created_at, event.kind);
                }
            }
        }
    }
    Ok(())
}

fn handle_import_command(
    store: &WorkdeckStore,
    path: PathBuf,
    _merge: bool,
    replace: bool,
    dry_run: bool,
    json_output: bool,
) -> Result<()> {
    let file = File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    let value: Value = serde_json::from_reader(file)
        .with_context(|| format!("failed to parse JSON {}", path.display()))?;
    let issues: Vec<Issue> = serde_json::from_value(
        value
            .get("issues")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .with_context(|| "failed to parse issues")?;
    let projects: Vec<Project> = serde_json::from_value(
        value
            .get("projects")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .with_context(|| "failed to parse projects")?;
    let cycles: Vec<Cycle> = serde_json::from_value(
        value
            .get("cycles")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .with_context(|| "failed to parse cycles")?;
    let labels: Vec<Label> = serde_json::from_value(
        value
            .get("labels")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .with_context(|| "failed to parse labels")?;
    let sessions: Vec<AgentSession> = serde_json::from_value(
        value
            .get("agent_sessions")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .with_context(|| "failed to parse agent sessions")?;

    if !dry_run {
        if replace && store.root().exists() {
            std::fs::remove_dir_all(store.root())
                .with_context(|| format!("failed to replace {}", store.root().display()))?;
        }
        store.init()?;
        for issue in &issues {
            store.save_issue(issue)?;
        }
        for project in &projects {
            store.upsert_project(
                Some(project.id.clone()),
                project.name.clone(),
                Some(project.description.clone()),
                Some(project.status.clone()),
            )?;
        }
        for cycle in &cycles {
            store.upsert_cycle(
                Some(cycle.id.clone()),
                cycle.name.clone(),
                Some(cycle.starts_at.clone()),
                Some(cycle.ends_at.clone()),
                Some(cycle.status.clone()),
            )?;
        }
        for label in &labels {
            store.upsert_label(
                Some(label.id.clone()),
                label.name.clone(),
                Some(label.color.clone()),
            )?;
        }
        for session in &sessions {
            store.save_agent_session(session)?;
        }
        store.append_event(
            "import_completed",
            json!({
                "path": path,
                "replace": replace,
            }),
        )?;
    }

    let payload = json!({
        "dry_run": dry_run,
        "issues": issues.len(),
        "projects": projects.len(),
        "cycles": cycles.len(),
        "labels": labels.len(),
        "agent_sessions": sessions.len(),
    });
    if json_output {
        json_success(
            "import",
            Some(if dry_run { "dry-run" } else { "import" }),
            payload,
        )?;
    } else if dry_run {
        println!("import dry-run ok");
    } else {
        println!("imported Workdeck data");
    }
    Ok(())
}

fn handle_doctor(repo_root: &std::path::Path, as_json: bool) -> Result<()> {
    let config_result = Config::load(repo_root);
    let (data_dir, config_check) = match config_result {
        Ok(config) => (
            config.data_dir(repo_root),
            json!({
                "name": "config",
                "ok": true,
                "message": "config is valid",
            }),
        ),
        Err(error) => (
            Config::default().data_dir(repo_root),
            json!({
                "name": "config",
                "ok": false,
                "message": format!("{error:#}"),
            }),
        ),
    };
    let store = WorkdeckStore::new(data_dir.clone());

    let checks = vec![
        json!({
            "name": "repo",
            "ok": repo_root.join(".git").exists(),
            "message": repo_root.display().to_string(),
        }),
        config_check,
        json!({
            "name": "data_dir",
            "ok": true,
            "message": data_dir.display().to_string(),
            "exists": data_dir.exists(),
        }),
        doctor_check("issues", store.load_issues(), |issues| {
            format!("{} issue(s)", issues.len())
        }),
        doctor_check("agents", store.load_agent_sessions(), |sessions| {
            format!("{} agent session(s)", sessions.len())
        }),
        doctor_check(
            "references",
            store.load_reference_data(),
            |reference_data| reference_summary(&reference_data),
        ),
        doctor_check("events", store.load_events(), |events| {
            format!("{} event(s)", events.len())
        }),
    ];
    let ok = checks
        .iter()
        .all(|check| check["ok"].as_bool().unwrap_or(false));

    if as_json {
        if ok {
            json_success("doctor", None, json!({ "ok": ok, "checks": checks }))?;
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "ok": false,
                    "kind": "doctor",
                    "error": {
                        "code": "doctor_failed",
                        "message": "doctor found failed checks",
                    },
                    "data": {
                        "checks": checks,
                    }
                }))?
            );
            bail!("__json_error_printed__: doctor found failed checks");
        }
    } else {
        for check in checks {
            let mark = if check["ok"].as_bool().unwrap_or(false) {
                "ok"
            } else {
                "fail"
            };
            println!(
                "{:<5} {:<10} {}",
                mark,
                check["name"].as_str().unwrap_or("unknown"),
                check["message"].as_str().unwrap_or("")
            );
        }
    }

    if !ok {
        bail!("doctor found failed checks");
    }
    Ok(())
}

fn doctor_check<T>(
    name: &str,
    result: Result<T>,
    message: impl FnOnce(T) -> String,
) -> serde_json::Value {
    match result {
        Ok(value) => json!({
            "name": name,
            "ok": true,
            "message": message(value),
        }),
        Err(error) => json!({
            "name": name,
            "ok": false,
            "message": format!("{error:#}"),
        }),
    }
}

fn handle_project_command(store: &WorkdeckStore, command: ProjectCommand) -> Result<()> {
    match command {
        ProjectCommand::List { status, json } => {
            let mut projects = store.load_reference_data()?.projects;
            if let Some(status) = status {
                projects.retain(|project| project.status == status);
            }
            if json {
                json_success("project_list", None, projects)?;
            } else if projects.is_empty() {
                println!("no projects");
            } else {
                for project in projects {
                    println!("{:<24} {:<10} {}", project.id, project.status, project.name);
                }
            }
        }
        ProjectCommand::Save {
            name,
            id,
            description,
            status,
            json,
        } => {
            let project = store.upsert_project(id, name, description, status)?;
            if json {
                json_success("project", Some("save"), project)?;
            } else {
                println!("{} {}", project.id, project.name);
            }
        }
        ProjectCommand::Show { id, json } => {
            let project = store
                .load_reference_data()?
                .projects
                .into_iter()
                .find(|project| project.id == id)
                .with_context(|| format!("project {id} does not exist"))?;
            if json {
                json_success("project", None, project)?;
            } else {
                println!("{} {}", project.id, project.name);
            }
        }
        ProjectCommand::Delete {
            id,
            yes,
            force,
            json,
        } => {
            if !yes {
                bail!("delete requires --yes");
            }
            let project = store.delete_project(&id, force)?;
            if json {
                json_success("project", Some("delete"), project)?;
            } else {
                println!("deleted {}", project.id);
            }
        }
    }
    Ok(())
}

fn handle_cycle_command(store: &WorkdeckStore, command: CycleCommand) -> Result<()> {
    match command {
        CycleCommand::List { status, json } => {
            let mut cycles = store.load_reference_data()?.cycles;
            if let Some(status) = status {
                cycles.retain(|cycle| cycle.status == status);
            }
            if json {
                json_success("cycle_list", None, cycles)?;
            } else if cycles.is_empty() {
                println!("no cycles");
            } else {
                for cycle in cycles {
                    println!("{:<24} {:<10} {}", cycle.id, cycle.status, cycle.name);
                }
            }
        }
        CycleCommand::Save {
            name,
            id,
            starts_at,
            ends_at,
            status,
            json,
        } => {
            let cycle = store.upsert_cycle(id, name, starts_at, ends_at, status)?;
            if json {
                json_success("cycle", Some("save"), cycle)?;
            } else {
                println!("{} {}", cycle.id, cycle.name);
            }
        }
        CycleCommand::Show { id, json } => {
            let cycle = store
                .load_reference_data()?
                .cycles
                .into_iter()
                .find(|cycle| cycle.id == id)
                .with_context(|| format!("cycle {id} does not exist"))?;
            if json {
                json_success("cycle", None, cycle)?;
            } else {
                println!("{} {}", cycle.id, cycle.name);
            }
        }
        CycleCommand::Delete {
            id,
            yes,
            force,
            json,
        } => {
            if !yes {
                bail!("delete requires --yes");
            }
            let cycle = store.delete_cycle(&id, force)?;
            if json {
                json_success("cycle", Some("delete"), cycle)?;
            } else {
                println!("deleted {}", cycle.id);
            }
        }
    }
    Ok(())
}

fn handle_label_command(store: &WorkdeckStore, command: LabelCommand) -> Result<()> {
    match command {
        LabelCommand::List { color, json } => {
            let mut labels = store.load_reference_data()?.labels;
            if let Some(color) = color {
                labels.retain(|label| label.color == color);
            }
            if json {
                json_success("label_list", None, labels)?;
            } else if labels.is_empty() {
                println!("no labels");
            } else {
                for label in labels {
                    println!("{:<24} {:<10} {}", label.id, label.color, label.name);
                }
            }
        }
        LabelCommand::Save {
            name,
            id,
            color,
            json,
        } => {
            let label = store.upsert_label(id, name, color)?;
            if json {
                json_success("label", Some("save"), label)?;
            } else {
                println!("{} {}", label.id, label.name);
            }
        }
        LabelCommand::Show { id, json } => {
            let label = store
                .load_reference_data()?
                .labels
                .into_iter()
                .find(|label| label.id == id)
                .with_context(|| format!("label {id} does not exist"))?;
            if json {
                json_success("label", None, label)?;
            } else {
                println!("{} {}", label.id, label.name);
            }
        }
        LabelCommand::Delete {
            id,
            yes,
            force,
            json,
        } => {
            if !yes {
                bail!("delete requires --yes");
            }
            let label = store.delete_label(&id, force)?;
            if json {
                json_success("label", Some("delete"), label)?;
            } else {
                println!("deleted {}", label.id);
            }
        }
    }
    Ok(())
}

fn reference_summary(reference_data: &ReferenceData) -> String {
    format!(
        "{} project(s), {} cycle(s), {} label(s)",
        reference_data.projects.len(),
        reference_data.cycles.len(),
        reference_data.labels.len()
    )
}

fn handle_agent_command(store: &WorkdeckStore, command: AgentCommand) -> Result<()> {
    match command {
        AgentCommand::List { json } => {
            let sessions = store.load_agent_sessions()?;
            if json {
                json_success("agent_session_list", None, sessions)?;
            } else if sessions.is_empty() {
                println!("no agent sessions");
            } else {
                for session in sessions {
                    println!(
                        "{:<32} {:<10} {:<10} {}",
                        session.id, session.agent, session.status, session.title
                    );
                }
            }
        }
        AgentCommand::Record {
            title,
            id,
            agent,
            status,
            goal,
            summary,
            cwd,
            plan_item,
            touched_file,
            command_run,
            test_run,
            handoff_note,
            json,
        } => {
            let mut session = AgentSession::new(title);
            if let Some(id) = id {
                session.id = id;
            }
            if let Some(agent) = agent {
                session.agent = agent;
            }
            if let Some(status) = status {
                session.status = status;
            }
            if let Some(goal) = goal {
                session.goal = goal;
            }
            if let Some(summary) = summary {
                session.summary = summary;
            }
            if let Some(cwd) = cwd {
                session.cwd = cwd.display().to_string();
            }
            session.plan = plan_item;
            session.touched_files = touched_file
                .into_iter()
                .map(|path| AgentTouchedFile {
                    path,
                    change_type: String::new(),
                })
                .collect();
            session.commands_run = command_run;
            session.tests_run = test_run;
            session.handoff_notes = handoff_note;
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("record"))?;
        }
        AgentCommand::Show { id, json } => {
            let session = load_agent_session(store, &id)?;
            print_agent_session(session, json)?;
        }
        AgentCommand::Update {
            id,
            title,
            agent,
            status,
            goal,
            summary,
            cwd,
            json,
        } => {
            let mut session = load_agent_session(store, &id)?;
            if let Some(title) = title {
                session.title = title;
            }
            if let Some(agent) = agent {
                session.agent = agent;
            }
            if let Some(status) = status {
                session.status = status;
            }
            if let Some(goal) = goal {
                session.goal = goal;
            }
            if let Some(summary) = summary {
                session.summary = summary;
            }
            if let Some(cwd) = cwd {
                session.cwd = cwd.display().to_string();
            }
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("update"))?;
        }
        AgentCommand::Finish { id, summary, json } => {
            let mut session = load_agent_session(store, &id)?;
            session.status = "done".to_string();
            session.ended_at = Utc::now().to_rfc3339();
            if let Some(summary) = summary {
                session.summary = summary;
            }
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("finish"))?;
        }
        AgentCommand::AppendPlan { id, text, json } => {
            let mut session = load_agent_session(store, &id)?;
            session.plan.push(text);
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("append-plan"))?;
        }
        AgentCommand::AddFile {
            id,
            path,
            change_type,
            json,
        } => {
            let mut session = load_agent_session(store, &id)?;
            session
                .touched_files
                .push(AgentTouchedFile { path, change_type });
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("add-file"))?;
        }
        AgentCommand::AddCommand { id, text, json } => {
            let mut session = load_agent_session(store, &id)?;
            session.commands_run.push(text);
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("add-command"))?;
        }
        AgentCommand::AddTest { id, text, json } => {
            let mut session = load_agent_session(store, &id)?;
            session.tests_run.push(text);
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("add-test"))?;
        }
        AgentCommand::AddNote { id, text, json } => {
            let mut session = load_agent_session(store, &id)?;
            session.handoff_notes.push(text);
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("add-note"))?;
        }
        AgentCommand::Delete { id, yes, json } => {
            if !yes {
                bail!("delete requires --yes");
            }
            let path = store.agent_session_file_path(&id);
            if !path.exists() {
                bail!("agent session {id} does not exist");
            }
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to delete {}", path.display()))?;
            store.append_event("agent_session_deleted", json!({ "id": id }))?;
            if json {
                json_success(
                    "agent_session",
                    Some("delete"),
                    json!({ "deleted": true, "id": id }),
                )?;
            } else {
                println!("deleted {id}");
            }
        }
        AgentCommand::Import { path, json } => {
            let sessions = read_agent_sessions(&path)?;
            let imported = sessions
                .into_iter()
                .map(|session| {
                    store.save_agent_session(&session)?;
                    Ok(session)
                })
                .collect::<Result<Vec<_>>>()?;
            if json {
                json_success("agent_session_list", Some("import"), imported)?;
            } else {
                println!("imported {} agent session(s)", imported.len());
            }
        }
    }
    Ok(())
}

fn load_agent_session(store: &WorkdeckStore, id: &str) -> Result<AgentSession> {
    store
        .load_agent_sessions()?
        .into_iter()
        .find(|session| session.id == id)
        .with_context(|| format!("agent session {id} does not exist"))
}

fn read_agent_sessions(path: &Path) -> Result<Vec<AgentSession>> {
    let extension = path.extension().and_then(|value| value.to_str());
    let sessions = if extension == Some("jsonl") {
        let file =
            File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let mut sessions = Vec::new();
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line = line.with_context(|| format!("failed to read line {}", index + 1))?;
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(&line)
                .with_context(|| format!("failed to parse JSONL line {}", index + 1))?;
            sessions.push(agent_session_from_json_value(value).with_context(|| {
                format!("line {} does not contain an agent session", index + 1)
            })?);
        }
        sessions
    } else {
        let file =
            File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let value: Value = serde_json::from_reader(file)
            .with_context(|| format!("failed to parse JSON {}", path.display()))?;
        match value {
            Value::Array(values) => values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    agent_session_from_json_value(value).with_context(|| {
                        format!("item {} does not contain an agent session", index + 1)
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            value => vec![
                agent_session_from_json_value(value)
                    .with_context(|| "JSON file does not contain an agent session")?,
            ],
        }
    };

    if sessions.is_empty() {
        bail!("no agent sessions found in {}", path.display());
    }
    Ok(sessions)
}

fn agent_session_from_json_value(value: Value) -> Result<AgentSession> {
    if let Some(session) = value.get("session") {
        return serde_json::from_value(session.clone()).map_err(Into::into);
    }
    if let Some(session) = value.pointer("/payload/session") {
        return serde_json::from_value(session.clone()).map_err(Into::into);
    }
    serde_json::from_value(value).map_err(Into::into)
}

fn handle_issue_command(store: &WorkdeckStore, command: IssueCommand) -> Result<()> {
    match command {
        IssueCommand::List {
            status,
            priority,
            project,
            cycle,
            label,
            assignee,
            due_at,
            json,
        } => {
            let issues = filter_issues(
                store.load_issues()?,
                status,
                priority,
                project,
                cycle,
                label,
                assignee,
                due_at,
            )?;
            if json {
                json_success("issue_list", None, issues)?;
            } else if issues.is_empty() {
                println!("no issues");
            } else {
                for issue in issues {
                    println!(
                        "{:<7} {:<12} {:<7} {}",
                        issue.key,
                        issue.status.label(),
                        issue.priority.label(),
                        issue.title
                    );
                }
            }
        }
        IssueCommand::Create {
            title,
            from_json,
            description,
            status,
            priority,
            project,
            cycle,
            assignee,
            due_at,
            label,
            linked_commit,
            linked_file,
            json,
        } => {
            let input = issue_create_input(
                title,
                from_json,
                description,
                status,
                priority,
                project,
                cycle,
                assignee,
                due_at,
                label,
                linked_commit,
                linked_file,
            )?;
            let mut issue = store.create_issue(input.title)?;
            let update = issue_update(
                None,
                input.description,
                input.status,
                input.priority,
                input.project,
                input.cycle,
                input.assignee,
                input.due_at,
                input.labels,
                input.linked_commits,
            )?;
            issue = store.update_issue(&issue.key, update)?;
            for path in input.linked_files {
                issue = store.link_issue_file(&issue.key, &path)?;
            }
            print_issue_action(issue, json, Some("create"))?;
        }
        IssueCommand::Update {
            key,
            title,
            description,
            status,
            priority,
            project,
            cycle,
            assignee,
            due_at,
            label,
            linked_commit,
            json,
        } => {
            let issue = store.update_issue(
                &key,
                issue_update(
                    title,
                    description,
                    status,
                    priority,
                    project,
                    cycle,
                    assignee,
                    due_at,
                    label,
                    linked_commit,
                )?,
            )?;
            print_issue_action(issue, json, Some("update"))?;
        }
        IssueCommand::Link { key, path, json } => {
            let issue = store.link_issue_file(&key, &path)?;
            print_issue_action(issue, json, Some("link-file"))?;
        }
        IssueCommand::LinkFile { key, path, json } => {
            let issue = store.link_issue_file(&key, &path)?;
            print_issue_action(issue, json, Some("link-file"))?;
        }
        IssueCommand::UnlinkFile { key, path, json } => {
            let mut issue = load_issue(store, &key)?;
            issue.linked_files.retain(|linked| linked != &path);
            issue.touch();
            store.save_issue(&issue)?;
            store.append_event("issue_file_unlinked", json!({ "key": key, "path": path }))?;
            print_issue_action(issue, json, Some("unlink-file"))?;
        }
        IssueCommand::LinkCommit { key, sha, json } => {
            let issue = store.update_issue(
                &key,
                IssueUpdate {
                    linked_commits: Some(vec![sha]),
                    ..IssueUpdate::default()
                },
            )?;
            print_issue_action(issue, json, Some("link-commit"))?;
        }
        IssueCommand::UnlinkCommit { key, sha, json } => {
            let mut issue = load_issue(store, &key)?;
            issue.linked_commits.retain(|linked| linked != &sha);
            issue.touch();
            store.save_issue(&issue)?;
            store.append_event("issue_commit_unlinked", json!({ "key": key, "sha": sha }))?;
            print_issue_action(issue, json, Some("unlink-commit"))?;
        }
        IssueCommand::Close { key, json } => {
            let issue = store.update_issue(
                &key,
                IssueUpdate {
                    status: Some(IssueStatus::Done),
                    ..IssueUpdate::default()
                },
            )?;
            print_issue_action(issue, json, Some("close"))?;
        }
        IssueCommand::Reopen { key, json } => {
            let issue = store.update_issue(
                &key,
                IssueUpdate {
                    status: Some(IssueStatus::Todo),
                    ..IssueUpdate::default()
                },
            )?;
            print_issue_action(issue, json, Some("reopen"))?;
        }
        IssueCommand::Move { key, status, json } => {
            let issue = store.update_issue(
                &key,
                IssueUpdate {
                    status: Some(status.parse().map_err(anyhow::Error::msg)?),
                    ..IssueUpdate::default()
                },
            )?;
            print_issue_action(issue, json, Some("move"))?;
        }
        IssueCommand::Assign {
            key,
            assignee,
            json,
        } => {
            let issue = store.update_issue(
                &key,
                IssueUpdate {
                    assignee: Some(assignee),
                    ..IssueUpdate::default()
                },
            )?;
            print_issue_action(issue, json, Some("assign"))?;
        }
        IssueCommand::Unassign { key, json } => {
            let issue = store.update_issue(
                &key,
                IssueUpdate {
                    assignee: Some(String::new()),
                    ..IssueUpdate::default()
                },
            )?;
            print_issue_action(issue, json, Some("unassign"))?;
        }
        IssueCommand::Label { command } => match command {
            IssueLabelCommand::Add { key, label, json } => {
                let mut issue = load_issue(store, &key)?;
                if !issue.labels.iter().any(|value| value == &label) {
                    issue.labels.push(label);
                    issue.labels.sort();
                    issue.touch();
                    store.save_issue(&issue)?;
                    store.append_event("issue_label_added", json!({ "key": key }))?;
                }
                print_issue_action(issue, json, Some("label-add"))?;
            }
            IssueLabelCommand::Remove { key, label, json } => {
                let mut issue = load_issue(store, &key)?;
                issue.labels.retain(|value| value != &label);
                issue.touch();
                store.save_issue(&issue)?;
                store.append_event("issue_label_removed", json!({ "key": key, "label": label }))?;
                print_issue_action(issue, json, Some("label-remove"))?;
            }
        },
        IssueCommand::Delete { key, yes, json } => {
            if !yes {
                bail!("delete requires --yes");
            }
            let path = store.issue_file_path(&key);
            if !path.exists() {
                bail!("issue {key} does not exist");
            }
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to delete {}", path.display()))?;
            store.append_event("issue_deleted", json!({ "key": key }))?;
            if json {
                json_success(
                    "issue",
                    Some("delete"),
                    json!({ "deleted": true, "key": key }),
                )?;
            } else {
                println!("deleted {key}");
            }
        }
        IssueCommand::Show { key, json } => {
            let issue = store
                .load_issues()?
                .into_iter()
                .find(|issue| issue.key == key)
                .with_context(|| format!("issue {key} does not exist"))?;
            print_issue(issue, json)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn filter_issues(
    issues: Vec<workdeck_cli::store::Issue>,
    status: Option<String>,
    priority: Option<String>,
    project: Option<String>,
    cycle: Option<String>,
    label: Option<String>,
    assignee: Option<String>,
    due_at: Option<String>,
) -> Result<Vec<workdeck_cli::store::Issue>> {
    let status = status
        .map(|value| value.parse::<IssueStatus>())
        .transpose()
        .map_err(anyhow::Error::msg)?;
    let priority = priority
        .map(|value| value.parse::<Priority>())
        .transpose()
        .map_err(anyhow::Error::msg)?;
    Ok(issues
        .into_iter()
        .filter(|issue| status.is_none_or(|status| issue.status == status))
        .filter(|issue| priority.is_none_or(|priority| issue.priority == priority))
        .filter(|issue| {
            project
                .as_ref()
                .is_none_or(|project| &issue.project == project)
        })
        .filter(|issue| cycle.as_ref().is_none_or(|cycle| &issue.cycle == cycle))
        .filter(|issue| {
            label
                .as_ref()
                .is_none_or(|label| issue.labels.iter().any(|value| value == label))
        })
        .filter(|issue| {
            assignee
                .as_ref()
                .is_none_or(|assignee| &issue.assignee == assignee)
        })
        .filter(|issue| due_at.as_ref().is_none_or(|due_at| &issue.due_at == due_at))
        .collect())
}

fn load_issue(store: &WorkdeckStore, key: &str) -> Result<workdeck_cli::store::Issue> {
    store
        .load_issues()?
        .into_iter()
        .find(|issue| issue.key == key)
        .with_context(|| format!("issue {key} does not exist"))
}

#[derive(Debug)]
struct IssueCreateInput {
    title: String,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    project: Option<String>,
    cycle: Option<String>,
    assignee: Option<String>,
    due_at: Option<String>,
    labels: Vec<String>,
    linked_commits: Vec<String>,
    linked_files: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
fn issue_create_input(
    title: Option<String>,
    from_json: Option<PathBuf>,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    project: Option<String>,
    cycle: Option<String>,
    assignee: Option<String>,
    due_at: Option<String>,
    labels: Vec<String>,
    linked_commits: Vec<String>,
    linked_files: Vec<String>,
) -> Result<IssueCreateInput> {
    let mut input = if let Some(path) = from_json {
        issue_create_input_from_json(&path)?
    } else {
        IssueCreateInput {
            title: title
                .clone()
                .with_context(|| "issue title is required unless --from-json is used")?,
            description: None,
            status: None,
            priority: None,
            project: None,
            cycle: None,
            assignee: None,
            due_at: None,
            labels: Vec::new(),
            linked_commits: Vec::new(),
            linked_files: Vec::new(),
        }
    };

    if let Some(title) = title {
        input.title = title;
    }
    input.description = description.or(input.description);
    input.status = status.or(input.status);
    input.priority = priority.or(input.priority);
    input.project = project.or(input.project);
    input.cycle = cycle.or(input.cycle);
    input.assignee = assignee.or(input.assignee);
    input.due_at = due_at.or(input.due_at);
    if !labels.is_empty() {
        input.labels = labels;
    }
    if !linked_commits.is_empty() {
        input.linked_commits = linked_commits;
    }
    if !linked_files.is_empty() {
        input.linked_files = linked_files;
    }
    Ok(input)
}

fn issue_create_input_from_json(path: &Path) -> Result<IssueCreateInput> {
    let mut raw = String::new();
    if path == Path::new("-") {
        std::io::stdin()
            .read_to_string(&mut raw)
            .with_context(|| "failed to read issue JSON from stdin")?;
    } else {
        raw = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
    }
    let value: Value = serde_json::from_str(&raw).with_context(|| "failed to parse issue JSON")?;
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .with_context(|| "issue JSON requires title")?
        .to_string();
    Ok(IssueCreateInput {
        title,
        description: json_string(&value, "description"),
        status: json_string(&value, "status"),
        priority: json_string(&value, "priority"),
        project: json_string(&value, "project"),
        cycle: json_string(&value, "cycle"),
        assignee: json_string(&value, "assignee"),
        due_at: json_string(&value, "due_at"),
        labels: json_string_array(&value, "labels"),
        linked_commits: json_string_array(&value, "linked_commits"),
        linked_files: json_string_array(&value, "linked_files"),
    })
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn json_string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn issue_update(
    title: Option<String>,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    project: Option<String>,
    cycle: Option<String>,
    assignee: Option<String>,
    due_at: Option<String>,
    labels: Vec<String>,
    linked_commits: Vec<String>,
) -> Result<IssueUpdate> {
    Ok(IssueUpdate {
        title,
        description,
        status: status
            .map(|value| value.parse())
            .transpose()
            .map_err(anyhow::Error::msg)?,
        priority: priority
            .map(|value| value.parse::<Priority>())
            .transpose()
            .map_err(anyhow::Error::msg)?,
        project,
        cycle,
        assignee,
        due_at,
        labels: (!labels.is_empty()).then_some(labels),
        linked_commits: (!linked_commits.is_empty()).then_some(linked_commits),
    })
}

fn print_issue(issue: workdeck_cli::store::Issue, json: bool) -> Result<()> {
    print_issue_action(issue, json, None)
}

fn print_issue_action(
    issue: workdeck_cli::store::Issue,
    json: bool,
    action: Option<&str>,
) -> Result<()> {
    if json {
        json_success("issue", action, issue)?;
    } else {
        println!("{} {}", issue.key, issue.title);
    }
    Ok(())
}

fn print_agent_session(session: AgentSession, json: bool) -> Result<()> {
    print_agent_session_action(session, json, None)
}

fn print_agent_session_action(
    session: AgentSession,
    json: bool,
    action: Option<&str>,
) -> Result<()> {
    if json {
        json_success("agent_session", action, session)?;
    } else {
        println!("{} {}", session.id, session.title);
    }
    Ok(())
}
