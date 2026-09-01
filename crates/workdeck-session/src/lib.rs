//! Authenticated, loopback-only control of live Workdeck review sessions.

mod broker_auth;
mod broker_config;
mod broker_validation;
mod canonical_json;
mod daemon_http;
mod selectors;

pub use broker_auth::*;
pub use broker_config::*;
pub use broker_validation::*;
pub use canonical_json::{CanonicalJsonError, canonical_json_bytes, canonicalize_json};
pub use daemon_http::*;
pub use selectors::{
    SelectableSession, SessionSelector, describe_session_selector, matches_session_selector,
    normalize_session_selector, repo_selector_distance, resolve_session_selector_boundary,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use workdeck_core::{ReviewSide, ReviewSnapshot};
use workdeck_review::{ReviewComment, ReviewError, ReviewState};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_ENVELOPE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("session protocol JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("secure random token generation failed: {0}")]
    Random(String),
    #[error("session response exceeded {MAX_ENVELOPE_BYTES} bytes")]
    Oversized,
    #[error("session {0} did not respond")]
    Unavailable(String),
    #[error("session request failed [{code}]: {message}")]
    Remote { code: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDescriptor {
    pub protocol_version: u32,
    pub id: String,
    pub repo: PathBuf,
    pub title: String,
    pub address: SocketAddr,
    pub token: String,
    pub process_id: u32,
    pub started_at_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub protocol_version: u32,
    pub id: u64,
    pub token: String,
    pub actor: Actor,
    pub action: SessionAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub kind: ActorKind,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActorKind {
    Terminal,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SessionAction {
    Health,
    Snapshot,
    Review {
        include_patch: bool,
        include_source: bool,
        include_agent_context: bool,
    },
    NavigateFile {
        file_index: usize,
    },
    NavigateHunk {
        file_index: usize,
        hunk_index: usize,
    },
    RevealLine {
        file_index: usize,
        side: ReviewSide,
        line: u32,
    },
    CommentAdd {
        comment: Box<ReviewComment>,
    },
    CommentList,
    CommentRemove {
        id: String,
    },
    Reload,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub protocol_version: u32,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
}

#[derive(Debug)]
pub struct ReviewSessionServer {
    descriptor: SessionDescriptor,
    discovery_path: PathBuf,
    stop: Arc<AtomicBool>,
    reload: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ReviewSessionServer {
    pub fn spawn(
        state: Arc<Mutex<ReviewState>>,
        repo: PathBuf,
        discovery_directory: PathBuf,
    ) -> Result<Self, SessionError> {
        fs::create_dir_all(&discovery_directory)?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let token = random_hex(32)?;
        let id = random_hex(12)?;
        let title = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .changeset()
            .title
            .clone();
        let descriptor = SessionDescriptor {
            protocol_version: PROTOCOL_VERSION,
            id: id.clone(),
            repo,
            title,
            address,
            token,
            process_id: std::process::id(),
            started_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        };
        let discovery_path = discovery_directory.join(format!("{id}.json"));
        write_private_json(&discovery_path, &descriptor)?;
        let stop = Arc::new(AtomicBool::new(false));
        let reload = Arc::new(AtomicBool::new(false));
        let server_stop = Arc::clone(&stop);
        let server_reload = Arc::clone(&reload);
        let server_descriptor = descriptor.clone();
        let thread = thread::spawn(move || {
            while !server_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = handle_connection(
                            stream,
                            &server_descriptor,
                            &state,
                            &server_stop,
                            &server_reload,
                        );
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(25));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            descriptor,
            discovery_path,
            stop,
            reload,
            thread: Some(thread),
        })
    }

    pub fn descriptor(&self) -> &SessionDescriptor {
        &self.descriptor
    }

    pub fn stop_signal(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }

    pub fn reload_signal(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.reload)
    }
}

impl Drop for ReviewSessionServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect_timeout(&self.descriptor.address, Duration::from_millis(50));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.discovery_path);
    }
}

pub struct SessionClient;

impl SessionClient {
    pub fn discover(directory: &Path) -> Vec<SessionDescriptor> {
        let Ok(entries) = fs::read_dir(directory) else {
            return Vec::new();
        };
        let mut sessions = entries
            .flatten()
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .filter_map(|source| serde_json::from_str::<SessionDescriptor>(&source).ok())
            .filter(|session| Self::request(session, SessionAction::Health).is_ok())
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| session.started_at_unix_ms);
        sessions
    }

    pub fn request(
        descriptor: &SessionDescriptor,
        action: SessionAction,
    ) -> Result<Value, SessionError> {
        let mut stream = TcpStream::connect_timeout(&descriptor.address, Duration::from_secs(1))
            .map_err(|_| SessionError::Unavailable(descriptor.id.clone()))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: 1,
            token: descriptor.token.clone(),
            actor: Actor {
                kind: ActorKind::Agent,
                name: None,
            },
            action,
        };
        let mut encoded = serde_json::to_vec(&request)?;
        if encoded.len() > MAX_ENVELOPE_BYTES {
            return Err(SessionError::Oversized);
        }
        encoded.push(b'\n');
        stream.write_all(&encoded)?;
        stream.flush()?;
        let mut response = String::new();
        BufReader::new(stream).read_line(&mut response)?;
        if response.len() > MAX_ENVELOPE_BYTES {
            return Err(SessionError::Oversized);
        }
        let response: ResponseEnvelope = serde_json::from_str(&response)?;
        if let Some(error) = response.error {
            return Err(SessionError::Remote {
                code: error.code,
                message: error.message,
            });
        }
        Ok(response.result.unwrap_or(Value::Null))
    }
}

fn handle_connection(
    mut stream: TcpStream,
    descriptor: &SessionDescriptor,
    state: &Arc<Mutex<ReviewState>>,
    stop: &Arc<AtomicBool>,
    reload: &Arc<AtomicBool>,
) -> Result<(), SessionError> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let response = if line.len() > MAX_ENVELOPE_BYTES {
        failure(0, "too-large", "request exceeded the protocol size limit")
    } else {
        match serde_json::from_str::<RequestEnvelope>(&line) {
            Ok(request) => dispatch(request, descriptor, state, stop, reload),
            Err(error) => failure(0, "invalid-request", &error.to_string()),
        }
    };
    let mut encoded = serde_json::to_vec(&response)?;
    encoded.push(b'\n');
    stream.write_all(&encoded)?;
    stream.flush()?;
    Ok(())
}

fn dispatch(
    request: RequestEnvelope,
    descriptor: &SessionDescriptor,
    state: &Arc<Mutex<ReviewState>>,
    stop: &Arc<AtomicBool>,
    reload: &Arc<AtomicBool>,
) -> ResponseEnvelope {
    if request.protocol_version != PROTOCOL_VERSION {
        return failure(request.id, "version", "unsupported review protocol version");
    }
    if !constant_time_eq(request.token.as_bytes(), descriptor.token.as_bytes()) {
        return failure(request.id, "unauthorized", "invalid session token");
    }
    let mut state = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let result = match request.action {
        SessionAction::Health => serde_json::json!({ "ok": true, "session": descriptor.id }),
        SessionAction::Snapshot => {
            serde_json::to_value(project_snapshot(state.snapshot(), false, false, false))
                .unwrap_or(Value::Null)
        }
        SessionAction::Review {
            include_patch,
            include_source,
            include_agent_context,
        } => serde_json::to_value(project_snapshot(
            state.snapshot(),
            include_patch,
            include_source,
            include_agent_context,
        ))
        .unwrap_or(Value::Null),
        SessionAction::NavigateFile { file_index } => match state.select_file(file_index) {
            Ok(()) => selection_value(&state),
            Err(error) => return review_failure(request.id, error),
        },
        SessionAction::NavigateHunk {
            file_index,
            hunk_index,
        } => match state.select_hunk(file_index, hunk_index) {
            Ok(()) => selection_value(&state),
            Err(error) => return review_failure(request.id, error),
        },
        SessionAction::RevealLine {
            file_index,
            side,
            line,
        } => match state.reveal_line(file_index, side, line) {
            Ok(()) => selection_value(&state),
            Err(error) => return review_failure(request.id, error),
        },
        SessionAction::CommentAdd { comment } => match state.add_comment(*comment) {
            Ok(()) => serde_json::json!({ "count": state.comments().len() }),
            Err(error) => return review_failure(request.id, error),
        },
        SessionAction::CommentList => serde_json::to_value(state.comments()).unwrap_or(Value::Null),
        SessionAction::CommentRemove { id } => match state.remove_comment(&id) {
            Some(comment) => serde_json::json!({
                "removed": comment.id,
                "count": state.comments().len()
            }),
            None => return failure(request.id, "missing-comment", "comment does not exist"),
        },
        SessionAction::Reload => {
            reload.store(true, Ordering::Relaxed);
            serde_json::json!({ "reloadQueued": true })
        }
        SessionAction::Quit => {
            stop.store(true, Ordering::Relaxed);
            serde_json::json!({ "quitting": true })
        }
    };
    success(request.id, result)
}

fn selection_value(state: &ReviewState) -> Value {
    serde_json::to_value(state.selection()).unwrap_or(Value::Null)
}

fn project_snapshot(
    mut snapshot: ReviewSnapshot,
    include_patch: bool,
    include_source: bool,
    include_agent_context: bool,
) -> ReviewSnapshot {
    for file in &mut snapshot.changeset.files {
        if !include_patch {
            file.patch.clear();
        }
        if !include_source {
            file.sources = workdeck_core::FileSourceSnapshots::default();
        }
        if !include_agent_context {
            file.agent = None;
        }
    }
    snapshot
}

fn review_failure(id: u64, error: ReviewError) -> ResponseEnvelope {
    failure(id, "invalid-target", &error.to_string())
}

fn success(id: u64, result: Value) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id,
        result: Some(result),
        error: None,
    }
}

fn failure(id: u64, code: &str, message: &str) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id,
        result: None,
        error: Some(ProtocolError {
            code: code.to_owned(),
            message: message.to_owned(),
        }),
    }
}

fn random_hex(bytes: usize) -> Result<String, SessionError> {
    let mut data = vec![0_u8; bytes];
    getrandom::fill(&mut data).map_err(|error| SessionError::Random(error.to_string()))?;
    Ok(data.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn write_private_json(path: &Path, value: &impl Serialize) -> Result<(), SessionError> {
    let temporary = path.with_extension("json.tmp");
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
}

pub fn default_discovery_directory() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| path.join("workdeck/sessions"))
        .or_else(|| {
            std::env::temp_dir()
                .join(format!("workdeck-{}", current_user_key()))
                .into()
        })
}

fn current_user_key() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".into())
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .collect()
}

pub fn decode_snapshot(value: Value) -> Result<ReviewSnapshot, SessionError> {
    serde_json::from_value(value).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use workdeck_core::ChangesetSource;
    use workdeck_diff::parse_patch;

    #[test]
    fn serves_authenticated_snapshot_and_navigation() {
        let changeset = parse_patch(
            "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-a\n+b\n",
            "test",
            "Test",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        let state = Arc::new(Mutex::new(ReviewState::new(changeset)));
        let directory = TempDir::new().unwrap();
        let server = ReviewSessionServer::spawn(
            Arc::clone(&state),
            directory.path().into(),
            directory.path().join("sessions"),
        )
        .unwrap();
        let snapshot = decode_snapshot(
            SessionClient::request(server.descriptor(), SessionAction::Snapshot).unwrap(),
        )
        .unwrap();
        assert_eq!(snapshot.changeset.files[0].path, "a");
        assert!(snapshot.changeset.files[0].patch.is_empty());
        let review = decode_snapshot(
            SessionClient::request(
                server.descriptor(),
                SessionAction::Review {
                    include_patch: true,
                    include_source: false,
                    include_agent_context: false,
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert!(review.changeset.files[0].patch.contains("@@ -1 +1 @@"));
        let mut wrong = server.descriptor().clone();
        wrong.token = "wrong".into();
        assert!(matches!(
            SessionClient::request(&wrong, SessionAction::Health),
            Err(SessionError::Remote { code, .. }) if code == "unauthorized"
        ));
    }

    #[test]
    fn discovery_ignores_stale_records() {
        let directory = TempDir::new().unwrap();
        fs::write(
            directory.path().join("stale.json"),
            r#"{"protocol_version":1,"id":"stale","repo":"/tmp","title":"stale","address":"127.0.0.1:1","token":"x","process_id":1,"started_at_unix_ms":0}"#,
        )
        .unwrap();
        assert!(SessionClient::discover(directory.path()).is_empty());
    }
}
