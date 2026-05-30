use crate::config::Config;
use crate::git;
use crate::payload::{
    file_preview_payload, search_target_group, search_target_payload, status_payload,
};
use crate::search::{self, SearchIndex};
use crate::store::WorkdeckStore;
use anyhow::{Context, Result, bail};
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, broadcast};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

const INDEX_HTML: &str = include_str!("assets/index.html");
const APP_CSS: &str = include_str!("assets/app.css");
const APP_JS: &str = include_str!("assets/app.js");
const HIGHLIGHT_JS: &str = include_str!("assets/vendor/highlight.js");

#[derive(Debug, Clone, Copy)]
pub struct WebOptions {
    pub host: IpAddr,
    pub port: u16,
    pub live: bool,
}

#[derive(Debug, Clone)]
struct WebState {
    repo_root: PathBuf,
    config: Config,
    cache: Arc<RwLock<SnapshotCache>>,
    events: broadcast::Sender<u64>,
}

#[derive(Debug, Clone)]
struct SnapshotCache {
    generation: u64,
    payload: Value,
}

#[derive(Debug, Deserialize)]
struct PreviewQuery {
    kind: String,
    path: Option<PathBuf>,
    sha: Option<String>,
    stash: Option<String>,
    branch: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: Option<String>,
    target: Option<String>,
}

pub fn run(repo_root: PathBuf, config: Config, options: WebOptions) -> Result<()> {
    if let Some(warning) = non_loopback_warning(options.host) {
        eprintln!("{warning}");
    }

    let runtime = tokio::runtime::Runtime::new().context("failed to start web runtime")?;
    runtime.block_on(async move { run_async(repo_root, config, options).await })
}

async fn run_async(repo_root: PathBuf, config: Config, options: WebOptions) -> Result<()> {
    let initial_payload = snapshot_payload(&repo_root, &config, 1)?;
    let (events, _) = broadcast::channel(32);
    let state = WebState {
        repo_root: repo_root.clone(),
        config: config.clone(),
        cache: Arc::new(RwLock::new(SnapshotCache {
            generation: 1,
            payload: initial_payload,
        })),
        events,
    };

    if options.live && config.refresh.auto {
        spawn_refresh_loop(state.clone());
    }

    let address = SocketAddr::new(options.host, options.port);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("failed to bind workdeck web server on {address}"))?;
    let local_addr = listener
        .local_addr()
        .context("failed to read web server address")?;
    println!("workdeck web listening on http://{local_addr}");

    axum::serve(listener, router(state))
        .await
        .context("workdeck web server failed")
}

pub fn router_for_tests(repo_root: PathBuf, config: Config) -> Result<Router> {
    let payload = snapshot_payload(&repo_root, &config, 1)?;
    let (events, _) = broadcast::channel(32);
    Ok(router(WebState {
        repo_root,
        config,
        cache: Arc::new(RwLock::new(SnapshotCache {
            generation: 1,
            payload,
        })),
        events,
    }))
}

fn router(state: WebState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.css", get(app_css))
        .route("/app.js", get(app_js))
        .route("/vendor/highlight.js", get(highlight_js))
        .route("/api/snapshot", get(api_snapshot))
        .route("/api/preview", get(api_preview))
        .route("/api/search", get(api_search))
        .route("/events", get(events))
        .with_state(state)
}

fn spawn_refresh_loop(state: WebState) {
    tokio::spawn(async move {
        let interval_ms = state.config.refresh.interval_ms.max(250);
        let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
        loop {
            interval.tick().await;
            let next_generation = state.cache.read().await.generation.saturating_add(1);
            match snapshot_payload(&state.repo_root, &state.config, next_generation) {
                Ok(payload) => {
                    {
                        let mut cache = state.cache.write().await;
                        cache.generation = next_generation;
                        cache.payload = payload;
                    }
                    let _ = state.events.send(next_generation);
                }
                Err(error) => eprintln!("workdeck web refresh failed: {error:#}"),
            }
        }
    });
}

async fn index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

async fn app_css() -> impl IntoResponse {
    static_asset(APP_CSS, "text/css; charset=utf-8")
}

async fn app_js() -> impl IntoResponse {
    static_asset(APP_JS, "text/javascript; charset=utf-8")
}

async fn highlight_js() -> impl IntoResponse {
    static_asset(HIGHLIGHT_JS, "text/javascript; charset=utf-8")
}

fn static_asset(content: &'static str, content_type: &'static str) -> Response {
    ([(header::CONTENT_TYPE, content_type)], content).into_response()
}

async fn api_snapshot(State(state): State<WebState>) -> impl IntoResponse {
    let cache = state.cache.read().await;
    Json(cache.payload.clone())
}

async fn api_preview(State(state): State<WebState>, Query(query): Query<PreviewQuery>) -> Response {
    match preview_payload(&state.repo_root, &state.config, query) {
        Ok(payload) => Json(payload).into_response(),
        Err(error) => api_error(error),
    }
}

async fn api_search(State(state): State<WebState>, Query(query): Query<SearchQuery>) -> Response {
    match search_payload(&state.repo_root, &state.config, query) {
        Ok(payload) => Json(payload).into_response(),
        Err(error) => api_error(error),
    }
}

async fn events(
    State(state): State<WebState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(state.events.subscribe()).filter_map(|event| match event {
        Ok(generation) => Some(Ok(Event::default()
            .event("snapshot_updated")
            .data(json!({ "generation": generation }).to_string()))),
        Err(_) => None,
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn api_error(error: anyhow::Error) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "ok": false,
            "error": error.to_string(),
        })),
    )
        .into_response()
}

fn preview_payload(repo_root: &Path, config: &Config, query: PreviewQuery) -> Result<Value> {
    let preview = match query.kind.as_str() {
        "diff" => {
            let path = query.path.context("missing path for diff preview")?;
            git::diff_for_path(repo_root, &path)?
        }
        "file" => {
            let path = query.path.context("missing path for file preview")?;
            git::read_file_preview(repo_root, &path, 160_000)?
        }
        "git_commit" => {
            let sha = query.sha.context("missing sha for commit preview")?;
            git::git_commit_preview(repo_root, &sha)?
        }
        "git_stash" => {
            let stash = query
                .stash
                .or(query.name)
                .context("missing stash for stash preview")?;
            git::git_stash_preview(repo_root, &stash)?
        }
        "git_branch" => {
            let branch = query
                .branch
                .or(query.name)
                .context("missing branch for branch preview")?;
            git::git_branch_preview(repo_root, &branch, config.git.recent_commits)?
        }
        "git_summary" => git::git_summary_preview(repo_root, non_empty(&config.git.base_branch))?,
        "git_tag" => generated_preview(
            "Git tag",
            query
                .name
                .or(query.sha)
                .unwrap_or_else(|| "tag selected".to_string()),
        ),
        "git_remote" => generated_preview(
            "Git remote",
            query
                .name
                .or(query.branch)
                .unwrap_or_else(|| "remote selected".to_string()),
        ),
        value => bail!("unknown preview kind {value}"),
    };
    Ok(file_preview_payload(&preview))
}

fn generated_preview(title: &str, content: String) -> git::FilePreview {
    git::FilePreview {
        title: title.to_string(),
        content,
        truncated: false,
        binary: false,
    }
}

fn snapshot_payload(repo_root: &Path, config: &Config, generation: u64) -> Result<Value> {
    let store = WorkdeckStore::new(config.data_dir(repo_root));
    let snapshot = git::scan_repo(repo_root)?;
    let git_overview = git::scan_git_overview(
        repo_root,
        non_empty(&config.git.base_branch),
        config.git.recent_commits,
    )?;
    let files = git::list_repo_files(repo_root, 20_000)?;
    let issues = store.load_issues()?;
    let sessions = store.load_agent_sessions()?;
    let reference_data = store.load_reference_data()?;
    let symbols = search::extract_symbols(repo_root, &files);

    Ok(json!({
        "ok": true,
        "generation": generation,
        "repo_root": repo_root,
        "tabs": ["changes", "git", "files", "issues", "agents", "search"],
        "status": status_payload(&snapshot),
        "git": git_overview_payload(&git_overview),
        "files": files,
        "issues": issues,
        "agents": sessions,
        "references": reference_data,
        "symbols": symbols.iter().map(|symbol| json!({
            "path": symbol.path,
            "line": symbol.line,
            "name": symbol.name,
            "kind": symbol.kind,
        })).collect::<Vec<_>>(),
    }))
}

fn git_overview_payload(overview: &git::GitOverview) -> Value {
    json!({
        "current_branch": overview.current_branch,
        "upstream": overview.upstream,
        "ahead": overview.ahead,
        "behind": overview.behind,
        "base_branch": overview.base_branch,
        "remotes": overview.remotes.iter().map(|remote| json!({
            "name": remote.name,
            "fetch_url": remote.fetch_url,
            "push_url": remote.push_url,
        })).collect::<Vec<_>>(),
        "branches": overview.branches.iter().map(|branch| json!({
            "name": branch.name,
            "is_current": branch.is_current,
            "is_remote": branch.is_remote,
            "upstream": branch.upstream,
        })).collect::<Vec<_>>(),
        "recent_commits": overview.recent_commits.iter().map(|commit| json!({
            "sha": commit.sha,
            "short_sha": commit.short_sha,
            "summary": commit.summary,
            "author": commit.author,
            "date": commit.date,
        })).collect::<Vec<_>>(),
        "stashes": overview.stashes.iter().map(|stash| json!({
            "name": stash.name,
            "summary": stash.summary,
        })).collect::<Vec<_>>(),
        "tags": overview.tags.iter().map(|tag| json!({ "name": tag.name })).collect::<Vec<_>>(),
    })
}

fn search_payload(repo_root: &Path, config: &Config, query: SearchQuery) -> Result<Value> {
    let text = query.q.unwrap_or_default();
    let filters = query
        .target
        .unwrap_or_default()
        .split(',')
        .filter(|target| !target.trim().is_empty())
        .map(|target| target.trim().to_ascii_lowercase())
        .collect::<BTreeSet<_>>();

    if text.trim().is_empty() {
        return Ok(json!({ "ok": true, "query": text, "results": [] }));
    }

    let store = WorkdeckStore::new(config.data_dir(repo_root));
    let snapshot = git::scan_repo(repo_root)?;
    let git_overview = git::scan_git_overview(
        repo_root,
        non_empty(&config.git.base_branch),
        config.git.recent_commits,
    )?;
    let files = git::list_repo_files(repo_root, 20_000)?;
    let issues = store.load_issues()?;
    let sessions = store.load_agent_sessions()?;
    let references = store.load_reference_data()?;
    let symbols = search::extract_symbols(repo_root, &files);
    let index = SearchIndex::rebuild(
        &files,
        &snapshot.changes,
        &issues,
        &sessions,
        &references,
        &symbols,
        Some(&git_overview),
    );
    let results = index
        .query(&text, 100)
        .into_iter()
        .filter(|result| {
            filters.is_empty() || filters.contains(search_target_group(&result.record.target))
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

    Ok(json!({ "ok": true, "query": text, "results": results }))
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

pub fn non_loopback_warning(host: IpAddr) -> Option<String> {
    (!host.is_loopback()).then(|| {
        format!(
            "Warning: workdeck web is read-only, but {host} is not a loopback address; only bind non-local hosts on trusted networks."
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use std::fs;
    use std::process::Command;
    use tempfile::tempdir;
    use tower::ServiceExt;

    #[tokio::test]
    async fn assets_are_embedded_without_external_cdn() {
        assert!(!INDEX_HTML.contains("https://"));
        assert!(!INDEX_HTML.contains("http://"));
        assert!(!APP_JS.contains("https://"));
        assert!(HIGHLIGHT_JS.contains("highlightVue"));
        assert!(APP_CSS.contains(".hl-tag"));

        let dir = tempdir().unwrap();
        init_repo(dir.path());
        let app = router_for_tests(dir.path().to_path_buf(), Config::default()).unwrap();
        for path in ["/", "/app.css", "/app.js", "/vendor/highlight.js"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
        }
    }

    #[tokio::test]
    async fn snapshot_preview_search_and_events_routes_work() {
        let dir = tempdir().unwrap();
        init_repo(dir.path());
        fs::write(dir.path().join("src.txt"), "one\ntwo\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "initial"]);
        fs::write(dir.path().join("src.txt"), "one\ntwo\nthree\n").unwrap();

        let app = router_for_tests(dir.path().to_path_buf(), Config::default()).unwrap();
        let snapshot = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/snapshot")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(snapshot.status(), StatusCode::OK);

        let preview = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/preview?kind=diff&path=src.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(preview.status(), StatusCode::OK);

        let search = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/search?q=src&target=files,changes")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(search.status(), StatusCode::OK);

        let events = app
            .oneshot(
                Request::builder()
                    .uri("/events")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(events.status(), StatusCode::OK);
    }

    #[test]
    fn non_local_host_warning_is_actionable() {
        let warning = non_loopback_warning("0.0.0.0".parse().unwrap()).unwrap();
        assert!(warning.contains("not a loopback address"));
        assert!(warning.contains("trusted networks"));
        assert!(non_loopback_warning("127.0.0.1".parse().unwrap()).is_none());
    }

    fn init_repo(path: &Path) {
        git(path, &["init"]);
        git(path, &["config", "user.email", "workdeck@example.test"]);
        git(path, &["config", "user.name", "Workdeck Test"]);
        fs::write(path.join("README.md"), "workdeck\n").unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-m", "initial"]);
    }

    fn git(path: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }
}
