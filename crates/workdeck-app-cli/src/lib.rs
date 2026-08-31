//! Stable, JSON-friendly command surface for the Workdeck desktop catalog.

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    thread,
    time::Duration,
};
use workdeck_api::{
    ActivityTarget, RequestId, WorkdeckClient, WorkdeckRequest, WorkdeckResponse, WorktreeId,
};
use workdeck_artifacts::ArtifactStore;
use workdeck_core::{ApplicationPaths, WorkdeckService};
use workdeck_domain::WorkspaceProject;
use workdeck_presenter::{LocalWorkdeckClient, RuntimeHandle};

#[derive(Debug, Parser)]
#[command(
    name = "workdeck-app",
    version,
    about = "Follow commits, pull requests, CI, and artifacts across your projects"
)]
pub struct Cli {
    #[arg(long, global = true, help = "Emit a stable JSON envelope")]
    json: bool,
    #[command(subcommand)]
    command: RootCommand,
}

#[derive(Debug, Subcommand)]
enum RootCommand {
    Catalog(CatalogArgs),
    Updates(UpdatesArgs),
    Git(GitArgs),
    Search(SearchArgs),
    Github(GitHubArgs),
    Artifact(ArtifactArgs),
    Doctor,
    Fixture(FixtureArgs),
}

#[derive(Debug, Args)]
struct CatalogArgs {
    #[command(subcommand)]
    command: CatalogCommand,
}

#[derive(Debug, Subcommand)]
enum CatalogCommand {
    Scan {
        #[arg(value_name = "ROOT")]
        roots: Vec<PathBuf>,
    },
    Refresh,
    List,
    Show {
        id: String,
    },
}

#[derive(Debug, Args)]
struct UpdatesArgs {
    #[command(subcommand)]
    command: UpdatesCommand,
}

#[derive(Debug, Subcommand)]
enum UpdatesCommand {
    List,
    ReadCommit {
        worktree_id: String,
        revision: String,
    },
    ReadPr {
        repository: String,
        number: u64,
        revision: String,
    },
}

#[derive(Debug, Args)]
struct GitArgs {
    #[arg(long, global = true, value_name = "PATH")]
    path: Option<PathBuf>,
    #[command(subcommand)]
    command: GitCommand,
}

#[derive(Debug, Subcommand)]
enum GitCommand {
    Status,
    Changes,
    Commits {
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Graph {
        #[arg(long, default_value_t = 300)]
        limit: usize,
    },
}

#[derive(Debug, Args)]
struct SearchArgs {
    query: String,
}

#[derive(Debug, Args)]
struct GitHubArgs {
    #[command(subcommand)]
    command: GitHubCommand,
}

#[derive(Debug, Subcommand)]
enum GitHubCommand {
    Prs {
        repository: String,
    },
    Pr {
        repository: String,
        number: u64,
    },
    Runs {
        repository: String,
    },
    Run {
        repository: String,
        run_id: String,
    },
    Jobs {
        repository: String,
        run_id: String,
    },
    Logs {
        repository: String,
        job_id: u64,
    },
    Artifacts {
        repository: String,
        run_id: Option<String>,
    },
}

#[derive(Debug, Args)]
struct ArtifactArgs {
    #[command(subcommand)]
    command: ArtifactCommand,
}

#[derive(Debug, Subcommand)]
enum ArtifactCommand {
    Import {
        zip: PathBuf,
        name: Option<String>,
    },
    List,
    Inspect {
        id: String,
    },
    Open {
        id: String,
        #[arg(long, default_value_t = 30)]
        seconds: u64,
    },
}

#[derive(Debug, Args)]
struct FixtureArgs {
    #[arg(default_value = "polished")]
    scenario: String,
}

pub fn main_entry(arguments: impl IntoIterator<Item = OsString>) -> ExitCode {
    match Cli::try_parse_from(arguments) {
        Ok(cli) => match run(cli) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("workdeck-app: {error:#}");
                ExitCode::from(1)
            }
        },
        Err(error) => {
            let code = if error.use_stderr() { 2 } else { 0 };
            let _ = error.print();
            ExitCode::from(code)
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let paths = ApplicationPaths::discover()?;
    match cli.command {
        RootCommand::Catalog(arguments) => catalog_command(&paths, arguments.command, cli.json),
        RootCommand::Updates(arguments) => updates_command(&paths, arguments.command, cli.json),
        RootCommand::Git(arguments) => git_command(arguments, cli.json),
        RootCommand::Search(arguments) => {
            let response = api_request(
                &paths,
                WorkdeckRequest::Search {
                    request_id: RequestId::new(),
                    query: arguments.query,
                },
            )?;
            emit("search", response, cli.json)
        }
        RootCommand::Github(arguments) => github_command(arguments.command, cli.json),
        RootCommand::Artifact(arguments) => artifact_command(&paths, arguments.command, cli.json),
        RootCommand::Doctor => doctor(&paths, cli.json),
        RootCommand::Fixture(arguments) => fixture(&arguments.scenario, cli.json),
    }
}

fn catalog_command(paths: &ApplicationPaths, command: CatalogCommand, json: bool) -> Result<()> {
    let service = WorkdeckService::open(paths.clone())?;
    match command {
        CatalogCommand::Scan { roots } => {
            let roots = if roots.is_empty() {
                default_portfolio_roots()
            } else {
                roots
            };
            emit("catalog_scan", scan_roots(&service, &roots)?, json)
        }
        CatalogCommand::Refresh => emit("catalog_refresh", refresh_catalog(&service)?, json),
        CatalogCommand::List => emit(
            "catalog",
            json!({
                "projects": service.catalog.list_projects(true)?,
                "repositories": service.catalog.all_repositories()?,
                "checkouts": service.catalog.all_checkouts()?,
                "worktrees": service.catalog.all_worktrees()?,
            }),
            json,
        ),
        CatalogCommand::Show { id } => {
            let value = catalog_object(&service, &id)?;
            emit("catalog_object", value, json)
        }
    }
}

#[derive(Debug, Serialize)]
struct ScanReport {
    roots: Vec<PathBuf>,
    projects: usize,
    repositories: usize,
    worktrees: usize,
    skipped: Vec<String>,
}

fn scan_roots(service: &WorkdeckService, roots: &[PathBuf]) -> Result<ScanReport> {
    let mut skipped = Vec::new();
    for root in roots {
        if !root.is_dir() {
            skipped.push(format!("{} is unavailable", root.display()));
            continue;
        }
        for (project_name, repository_path) in repository_candidates(root)? {
            let project = project_for_repository(service, &project_name, &repository_path)?;
            match service.add_repository(&project, &repository_path) {
                Ok(_) => {}
                Err(error) => skipped.push(format!("{}: {error:#}", repository_path.display())),
            }
        }
    }
    Ok(ScanReport {
        roots: roots.to_vec(),
        projects: service.catalog.list_projects(true)?.len(),
        repositories: service.catalog.all_repositories()?.len(),
        worktrees: service.catalog.all_worktrees()?.len(),
        skipped,
    })
}

fn repository_candidates(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    if workdeck_git::discover(root).is_ok() {
        return Ok(vec![(path_label(root), root.to_path_buf())]);
    }
    let mut candidates = Vec::new();
    let mut children = readable_directories(root)?;
    children.sort();
    for child in children {
        let project_name = path_label(&child);
        if workdeck_git::discover(&child).is_ok() {
            candidates.push((project_name, child));
            continue;
        }
        let mut grandchildren = readable_directories(&child).unwrap_or_default();
        grandchildren.sort();
        for repository in grandchildren {
            if workdeck_git::discover(&repository).is_ok() {
                candidates.push((project_name.clone(), repository));
            }
        }
    }
    Ok(candidates)
}

fn readable_directories(root: &Path) -> Result<Vec<PathBuf>> {
    Ok(fs::read_dir(root)
        .with_context(|| format!("could not scan {}", root.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir() && !kind.is_symlink())
                .map(|_| entry.path())
        })
        .filter(|path| {
            !path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.starts_with('.'))
        })
        .collect::<Vec<_>>())
}

fn get_or_create_project(service: &WorkdeckService, name: &str) -> Result<WorkspaceProject> {
    service
        .catalog
        .project_by_name(name)?
        .map_or_else(|| service.create_project(name), Ok)
}

fn project_for_repository(
    service: &WorkdeckService,
    suggested_name: &str,
    repository_path: &Path,
) -> Result<WorkspaceProject> {
    let discovery = workdeck_git::discover(repository_path)?;
    let existing_checkout = service
        .catalog
        .all_checkouts()?
        .into_iter()
        .find(|checkout| checkout.git_common_dir == discovery.git_common_dir);
    if let Some(checkout) = existing_checkout {
        let repository = service
            .catalog
            .repository(&checkout.repository_id)?
            .with_context(|| format!("checkout {} has no repository", checkout.id))?;
        return service
            .catalog
            .list_projects(true)?
            .into_iter()
            .find(|project| project.id == repository.project_id)
            .with_context(|| format!("repository {} has no project", repository.id));
    }
    get_or_create_project(service, suggested_name)
}

fn refresh_catalog(service: &WorkdeckService) -> Result<Value> {
    let projects = service.catalog.list_projects(true)?;
    let repositories = service.catalog.all_repositories()?;
    let checkouts = service.catalog.all_checkouts()?;
    let mut refreshed = Vec::new();
    let mut unavailable = Vec::new();
    for checkout in checkouts {
        let Some(repository) = repositories
            .iter()
            .find(|repository| repository.id == checkout.repository_id)
        else {
            continue;
        };
        let Some(project) = projects
            .iter()
            .find(|project| project.id == repository.project_id)
        else {
            continue;
        };
        let worktrees = service.catalog.list_worktrees(&checkout.id)?;
        let path = std::iter::once(&checkout.path)
            .chain(worktrees.iter().map(|worktree| &worktree.path))
            .find(|path| path.exists())
            .cloned();
        if let Some(path) = path {
            let (_, discovery) = service.add_repository(project, &path)?;
            refreshed.push(
                json!({ "repository": repository.id, "worktrees": discovery.worktrees.len() }),
            );
        } else {
            unavailable.push(repository.id.to_string());
        }
    }
    Ok(json!({ "refreshed": refreshed, "unavailable": unavailable }))
}

fn catalog_object(service: &WorkdeckService, id: &str) -> Result<Value> {
    if let Some(project) = service
        .catalog
        .list_projects(true)?
        .into_iter()
        .find(|value| value.id.as_str() == id || value.name == id)
    {
        return Ok(serde_json::to_value(project)?);
    }
    if let Some(repository) = service
        .catalog
        .all_repositories()?
        .into_iter()
        .find(|value| value.id.as_str() == id || value.name == id)
    {
        return Ok(serde_json::to_value(repository)?);
    }
    if let Some(worktree) = service
        .catalog
        .all_worktrees()?
        .into_iter()
        .find(|value| value.id.as_str() == id || value.path == Path::new(id))
    {
        return Ok(serde_json::to_value(worktree)?);
    }
    bail!("catalog object {id} does not exist")
}

fn updates_command(paths: &ApplicationPaths, command: UpdatesCommand, json: bool) -> Result<()> {
    let client = local_client(paths)?;
    match command {
        UpdatesCommand::List => {
            let bootstrap = bootstrap(&client)?;
            emit("updates", bootstrap.inbox, json)
        }
        UpdatesCommand::ReadCommit {
            worktree_id,
            revision,
        } => emit(
            "activity_read",
            request(
                &client,
                WorkdeckRequest::MarkActivityRead {
                    request_id: RequestId::new(),
                    target: ActivityTarget::CommitBranch {
                        worktree_id: WorktreeId(worktree_id),
                    },
                    revision,
                },
            )?,
            json,
        ),
        UpdatesCommand::ReadPr {
            repository,
            number,
            revision,
        } => emit(
            "activity_read",
            request(
                &client,
                WorkdeckRequest::MarkActivityRead {
                    request_id: RequestId::new(),
                    target: ActivityTarget::PullRequest { repository, number },
                    revision,
                },
            )?,
            json,
        ),
    }
}

fn local_client(paths: &ApplicationPaths) -> Result<WorkdeckClient> {
    LocalWorkdeckClient::spawn(RuntimeHandle::spawn(paths.clone())?)
}

fn api_request(
    paths: &ApplicationPaths,
    request_value: WorkdeckRequest,
) -> Result<WorkdeckResponse> {
    request(&local_client(paths)?, request_value)
}

fn request(client: &WorkdeckClient, request_value: WorkdeckRequest) -> Result<WorkdeckResponse> {
    futures::executor::block_on(client.request(request_value))
        .map(|response| response.payload)
        .map_err(|error| anyhow!(error))
}

fn bootstrap(client: &WorkdeckClient) -> Result<workdeck_api::BootstrapSnapshot> {
    match request(
        client,
        WorkdeckRequest::Bootstrap {
            request_id: RequestId::new(),
        },
    )? {
        WorkdeckResponse::Bootstrap(snapshot) => Ok(snapshot),
        _ => bail!("native runtime returned the wrong bootstrap response"),
    }
}

fn git_command(arguments: GitArgs, json: bool) -> Result<()> {
    let path = arguments.path.unwrap_or(std::env::current_dir()?);
    let root = workdeck_git::discover(&path)?.root;
    match arguments.command {
        GitCommand::Status => emit(
            "git_status",
            workdeck_git::load_graph(&root, 1)?.status,
            json,
        ),
        GitCommand::Changes => {
            let paths = ApplicationPaths::discover()?;
            let mut service = WorkdeckService::open(paths)?;
            let snapshot = workdeck_git::capture_worktree(&root, &mut service.content)?;
            emit("git_changes", snapshot.files, json)
        }
        GitCommand::Commits { limit } => {
            let graph = workdeck_git::load_graph_without_status(&root, bounded_limit(limit)?)?;
            emit(
                "git_commits",
                graph
                    .rows
                    .into_iter()
                    .map(|row| row.commit)
                    .collect::<Vec<_>>(),
                json,
            )
        }
        GitCommand::Graph { limit } => emit(
            "git_graph",
            workdeck_git::load_graph(&root, bounded_limit(limit)?)?,
            json,
        ),
    }
}

fn bounded_limit(limit: usize) -> Result<usize> {
    if !(1..=20_000).contains(&limit) {
        bail!("limit must be between 1 and 20000");
    }
    Ok(limit)
}

fn github_command(command: GitHubCommand, json: bool) -> Result<()> {
    let client = workdeck_github::GitHubClient::default();
    match command {
        GitHubCommand::Prs { repository } => emit(
            "github_pull_requests",
            gh_json(&format!("repos/{repository}/pulls?state=all&per_page=100"))?,
            json,
        ),
        GitHubCommand::Pr { repository, number } => emit(
            "github_pull_request",
            client.pull_request(&repository, number)?,
            json,
        ),
        GitHubCommand::Runs { repository } => emit(
            "github_runs",
            gh_json(&format!("repos/{repository}/actions/runs?per_page=100"))?,
            json,
        ),
        GitHubCommand::Run { repository, run_id } => emit(
            "github_run",
            client.workflow_run(&repository, &run_id)?,
            json,
        ),
        GitHubCommand::Jobs { repository, run_id } => {
            let run = client.workflow_run(&repository, &run_id)?;
            emit("github_jobs", run.jobs, json)
        }
        GitHubCommand::Logs { repository, job_id } => {
            let log = client.job_log(&repository, job_id)?;
            emit(
                "github_job_log",
                json!({ "repository": repository, "job_id": job_id, "text": String::from_utf8_lossy(&log) }),
                json,
            )
        }
        GitHubCommand::Artifacts { repository, run_id } => {
            let endpoint = run_id.map_or_else(
                || format!("repos/{repository}/actions/artifacts?per_page=100"),
                |run_id| format!("repos/{repository}/actions/runs/{run_id}/artifacts?per_page=100"),
            );
            emit("github_artifacts", gh_json(&endpoint)?, json)
        }
    }
}

fn gh_json(endpoint: &str) -> Result<Value> {
    if endpoint.chars().any(|character| character.is_control()) || !endpoint.starts_with("repos/") {
        bail!("invalid GitHub endpoint");
    }
    let output = Command::new("gh")
        .args(["api", "--allow-escape-sequences", endpoint])
        .output()
        .context("could not execute GitHub CLI")?;
    if !output.status.success() {
        bail!(
            "GitHub request failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    if output.stdout.len() > 16 * 1024 * 1024 {
        bail!("GitHub response exceeded 16 MiB");
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn artifact_command(paths: &ApplicationPaths, command: ArtifactCommand, json: bool) -> Result<()> {
    paths.ensure()?;
    let store = ArtifactStore::new(&paths.artifacts)?;
    match command {
        ArtifactCommand::Import { zip, name } => {
            let name = name.unwrap_or_else(|| path_label(&zip));
            let artifact = store.import_zip(&zip, name, zip.display().to_string())?;
            emit("artifact_import", artifact, json)
        }
        ArtifactCommand::List => emit("artifacts", store.list()?, json),
        ArtifactCommand::Inspect { id } => emit(
            "artifact",
            store
                .find(&id)?
                .with_context(|| format!("artifact {id} does not exist"))?,
            json,
        ),
        ArtifactCommand::Open { id, seconds } => {
            if !(1..=3_600).contains(&seconds) {
                bail!("--seconds must be between 1 and 3600");
            }
            let artifact = store
                .find(&id)?
                .with_context(|| format!("artifact {id} does not exist"))?;
            let mut preview = store.start_preview(&artifact)?;
            emit(
                "artifact_preview",
                json!({ "id": id, "url": preview.url(), "lifetime_seconds": seconds }),
                json,
            )?;
            thread::sleep(Duration::from_secs(seconds));
            preview.stop();
            Ok(())
        }
    }
}

fn doctor(paths: &ApplicationPaths, json: bool) -> Result<()> {
    let service = WorkdeckService::open(paths.clone())?;
    let projects = service.catalog.list_projects(true)?;
    let repositories = service.catalog.all_repositories()?;
    let worktrees = service.catalog.all_worktrees()?;
    let integrity = service.catalog.verify_integrity()?;
    let github = workdeck_github::GitHubClient::default().status();
    emit(
        "doctor",
        json!({
            "ok": true,
            "data_dir": paths.root,
            "database": paths.database,
            "catalog": {
                "projects": projects.len(),
                "repositories": repositories.len(),
                "worktrees": worktrees.len(),
                "integrity": integrity,
            },
            "github": github,
            "repository_policy": "read_only",
        }),
        json,
    )
}

fn fixture(scenario: &str, json: bool) -> Result<()> {
    let snapshot = match scenario {
        "polished" => workdeck_api::fixtures::polished(),
        "empty" => workdeck_api::fixtures::empty(),
        "offline" => workdeck_api::fixtures::offline(),
        _ => bail!("fixture must be polished, empty, or offline"),
    };
    emit("fixture", snapshot, json)
}

fn default_portfolio_roots() -> Vec<PathBuf> {
    let Some(user_dirs) = directories::UserDirs::new() else {
        return Vec::new();
    };

    ["Projects", "Sites"]
        .map(|name| user_dirs.home_dir().join(name))
        .into_iter()
        .filter(|path| path.is_dir())
        .collect()
}

fn path_label(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("repository")
        .to_owned()
}

fn emit(kind: &str, value: impl Serialize, json_output: bool) -> Result<()> {
    let value = serde_json::to_value(value)?;
    let output = if json_output {
        json!({ "ok": true, "kind": kind, "data": value })
    } else {
        value
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scan_is_empty_when_machine_roots_are_not_assumed_in_tests() {
        let roots = default_portfolio_roots();
        assert!(roots.iter().all(|root| root.is_absolute()));
    }

    #[test]
    fn candidate_scan_discovers_only_temporary_git_repositories() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("sample");
        fs::create_dir(&repository).unwrap();
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&repository)
            .status()
            .unwrap();
        assert!(status.success());
        let candidates = repository_candidates(root.path()).unwrap();
        assert_eq!(candidates, vec![("sample".into(), repository)]);
    }

    #[test]
    fn git_limits_are_bounded() {
        assert_eq!(bounded_limit(300).unwrap(), 300);
        assert!(bounded_limit(0).is_err());
        assert!(bounded_limit(20_001).is_err());
    }

    #[test]
    fn repeated_scans_reuse_a_linked_worktrees_repository_and_project() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("primary");
        let linked = root.path().join("linked");
        fs::create_dir(&repository).unwrap();
        let git = |arguments: &[&str], current_dir: &Path| {
            let status = Command::new("git")
                .args(arguments)
                .current_dir(current_dir)
                .status()
                .unwrap();
            assert!(status.success(), "git {arguments:?}");
        };
        git(&["init", "--quiet", "--initial-branch=main"], &repository);
        git(&["config", "user.name", "Workdeck Fixture"], &repository);
        git(
            &["config", "user.email", "workdeck@example.invalid"],
            &repository,
        );
        fs::write(repository.join("README.md"), "# fixture\n").unwrap();
        git(&["add", "--", "README.md"], &repository);
        git(&["commit", "--quiet", "-m", "fixture"], &repository);
        git(
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "feat/linked",
                linked.to_str().unwrap(),
            ],
            &repository,
        );

        let data = tempfile::tempdir().unwrap();
        let service = WorkdeckService::open(ApplicationPaths::at(data.path())).unwrap();
        for _ in 0..2 {
            let report = scan_roots(&service, &[root.path().to_path_buf()]).unwrap();
            assert_eq!(report.projects, 1);
            assert_eq!(report.repositories, 1);
            assert_eq!(report.worktrees, 2);
            assert!(report.skipped.is_empty());
        }
        assert_eq!(service.catalog.list_projects(true).unwrap().len(), 1);
    }
}
