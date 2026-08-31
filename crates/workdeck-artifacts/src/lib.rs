use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tiny_http::{Header, Response, Server, StatusCode};
use unicode_normalization::UnicodeNormalization;
use workdeck_domain::ArtifactId;

const DEFAULT_MAX_FILES: usize = 10_000;
const DEFAULT_MAX_BYTES: u64 = 1_000_000_000;
const MAX_COMPRESSION_RATIO: u64 = 200;
const COMPRESSION_RATIO_FLOOR: u64 = 1_048_576;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub id: ArtifactId,
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub series_key: String,
    #[serde(default = "default_artifact_version")]
    pub version: u64,
    pub imported_at: DateTime<Utc>,
    pub file_count: usize,
    pub total_bytes: u64,
    #[serde(default)]
    pub content_sha256: String,
    #[serde(default)]
    pub archive_bytes: Option<u64>,
    #[serde(default)]
    pub archive_sha256: Option<String>,
    pub entrypoint: Option<PathBuf>,
    pub files: Vec<ArtifactFile>,
}

fn default_artifact_version() -> u64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactFile {
    pub path: PathBuf,
    pub size: u64,
    pub media_type: String,
    #[serde(default)]
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct ArtifactStore {
    root: PathBuf,
    max_files: usize,
    max_bytes: u64,
}

impl ArtifactStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            max_files: DEFAULT_MAX_FILES,
            max_bytes: DEFAULT_MAX_BYTES,
        })
    }

    pub fn with_limits(mut self, max_files: usize, max_bytes: u64) -> Self {
        self.max_files = max_files;
        self.max_bytes = max_bytes;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn import_zip(
        &self,
        zip_path: &Path,
        name: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<ArtifactManifest> {
        let (archive_bytes, archive_sha256) = hash_file(zip_path)?;
        let file = File::open(zip_path)
            .with_context(|| format!("failed to open artifact ZIP {}", zip_path.display()))?;
        self.import_zip_reader_with_archive(
            file,
            name.into(),
            source.into(),
            Some((archive_bytes, archive_sha256)),
        )
    }

    pub fn import_zip_reader<R: Read + Seek>(
        &self,
        reader: R,
        name: String,
        source: String,
    ) -> Result<ArtifactManifest> {
        self.import_zip_reader_with_archive(reader, name, source, None)
    }

    fn import_zip_reader_with_archive<R: Read + Seek>(
        &self,
        reader: R,
        name: String,
        source: String,
        archive_metadata: Option<(u64, String)>,
    ) -> Result<ArtifactManifest> {
        let series_key = artifact_series_key(&name, &source);
        let version = self
            .list()?
            .into_iter()
            .filter(|artifact| artifact.series_key == series_key)
            .map(|artifact| artifact.version.max(1))
            .max()
            .unwrap_or_default()
            .saturating_add(1);
        let id = ArtifactId::new();
        let temporary = tempfile::Builder::new()
            .prefix("artifact-")
            .tempdir_in(&self.root)?;
        let files_root = temporary.path().join("files");
        fs::create_dir(&files_root)?;
        let mut archive = zip::ZipArchive::new(reader).context("invalid artifact ZIP")?;
        if archive.len() > self.max_files {
            bail!(
                "artifact contains {} entries; limit is {}",
                archive.len(),
                self.max_files
            );
        }
        let mut files = Vec::new();
        let mut normalized_paths = BTreeMap::<String, ArtifactPathKind>::new();
        let mut total_bytes = 0u64;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let enclosed = entry
                .enclosed_name()
                .with_context(|| format!("artifact entry {} escapes its root", entry.name()))?;
            validate_relative_path(&enclosed)?;
            if is_platform_metadata(&enclosed) {
                continue;
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                bail!("artifact symlinks are not allowed: {}", enclosed.display());
            }
            if entry.unix_mode().is_some_and(|mode| {
                let kind = mode & 0o170000;
                kind != 0 && kind != 0o100000 && !(kind == 0o040000 && entry.is_dir())
            }) {
                bail!(
                    "artifact contains a non-regular entry: {}",
                    enclosed.display()
                );
            }
            let output = files_root.join(&enclosed);
            if entry.is_dir() {
                register_artifact_path(
                    &mut normalized_paths,
                    &enclosed,
                    ArtifactPathKind::Directory,
                )?;
                fs::create_dir_all(&output)?;
                continue;
            }
            register_artifact_path(&mut normalized_paths, &enclosed, ArtifactPathKind::File)?;
            let compressed_size = entry.compressed_size();
            if entry.size() >= COMPRESSION_RATIO_FLOOR
                && (compressed_size == 0
                    || entry.size() > compressed_size.saturating_mul(MAX_COMPRESSION_RATIO))
            {
                bail!(
                    "artifact entry {} exceeds the compression ratio limit",
                    enclosed.display()
                );
            }
            total_bytes = total_bytes
                .checked_add(entry.size())
                .context("artifact expanded size overflow")?;
            if total_bytes > self.max_bytes {
                bail!("artifact expands to more than {} bytes", self.max_bytes);
            }
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut target = File::create(&output)?;
            let expected_size = entry.size();
            let mut copied = 0u64;
            let mut file_hasher = Sha256::new();
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let count = entry.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                copied = copied
                    .checked_add(count as u64)
                    .context("artifact entry size overflow")?;
                if copied > expected_size || copied > self.max_bytes {
                    bail!("artifact entry changed size while extracting");
                }
                std::io::Write::write_all(&mut target, &buffer[..count])?;
                file_hasher.update(&buffer[..count]);
            }
            if copied != expected_size {
                bail!("artifact entry changed size while extracting");
            }
            files.push(ArtifactFile {
                media_type: mime_guess::from_path(&enclosed)
                    .first_or_octet_stream()
                    .essence_str()
                    .to_string(),
                path: enclosed,
                size: copied,
                sha256: format!("{:x}", file_hasher.finalize()),
            });
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let content_sha256 = artifact_content_hash(&files)?;
        let entrypoint = choose_entrypoint(&files);
        let (archive_bytes, archive_sha256) = archive_metadata
            .map(|(bytes, checksum)| (Some(bytes), Some(checksum)))
            .unwrap_or((None, None));
        let manifest = ArtifactManifest {
            id: id.clone(),
            name,
            source,
            series_key,
            version,
            imported_at: Utc::now(),
            file_count: files.len(),
            total_bytes,
            content_sha256,
            archive_bytes,
            archive_sha256,
            entrypoint,
            files,
        };
        fs::write(
            temporary.path().join("manifest.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        let destination = self.root.join(id.as_str());
        fs::rename(temporary.keep(), &destination)
            .with_context(|| format!("failed to persist artifact {}", destination.display()))?;
        Ok(manifest)
    }

    pub fn list(&self) -> Result<Vec<ArtifactManifest>> {
        let mut manifests = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path().join("manifest.json");
            if path.is_file() {
                let bytes = fs::read(&path)?;
                let mut manifest: ArtifactManifest =
                    serde_json::from_slice(&bytes).with_context(|| {
                        format!("failed to parse artifact manifest {}", path.display())
                    })?;
                if manifest.series_key.is_empty() {
                    manifest.series_key = artifact_series_key(&manifest.name, &manifest.source);
                }
                manifest.version = manifest.version.max(1);
                validate_manifest(&entry.file_name(), &manifest).with_context(|| {
                    format!("artifact manifest {} failed validation", path.display())
                })?;
                manifests.push(manifest);
            }
        }
        manifests
            .sort_by_key(|manifest: &ArtifactManifest| std::cmp::Reverse(manifest.imported_at));
        Ok(manifests)
    }

    pub fn find(&self, value: &str) -> Result<Option<ArtifactManifest>> {
        Ok(self
            .list()?
            .into_iter()
            .find(|manifest| manifest.id.as_str() == value || manifest.name == value))
    }

    pub fn files_root(&self, id: &ArtifactId) -> Result<PathBuf> {
        validate_artifact_id(id.as_str())?;
        let store_root = fs::canonicalize(&self.root)
            .with_context(|| format!("artifact store {} does not exist", self.root.display()))?;
        let files_root = fs::canonicalize(self.root.join(id.as_str()).join("files"))
            .with_context(|| format!("artifact {id} has no extracted files"))?;
        if !files_root.starts_with(&store_root) {
            bail!("artifact {id} resolves outside the Workdeck artifact store");
        }
        Ok(files_root)
    }

    pub fn start_preview(&self, manifest: &ArtifactManifest) -> Result<ArtifactPreview> {
        let entrypoint = manifest
            .entrypoint
            .clone()
            .context("artifact has no safely previewable file")?;
        validate_manifest(std::ffi::OsStr::new(manifest.id.as_str()), manifest)?;
        ArtifactPreview::start(self.files_root(&manifest.id)?, entrypoint)
    }
}

fn artifact_series_key(name: &str, source: &str) -> String {
    let normalized_source = source
        .rsplit_once(":artifact:")
        .filter(|(_, id)| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
        .map_or(source, |(prefix, _)| prefix);
    workdeck_domain::content_hash(
        format!(
            "{}\n{}",
            name.trim().to_ascii_lowercase(),
            normalized_source.trim().to_ascii_lowercase()
        )
        .as_bytes(),
    )
}

fn hash_file(path: &Path) -> Result<(u64, String)> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to open artifact ZIP {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .context("artifact archive size overflow")?;
        hasher.update(&buffer[..count]);
    }
    Ok((bytes, format!("{:x}", hasher.finalize())))
}

fn artifact_content_hash(files: &[ArtifactFile]) -> Result<String> {
    let mut hasher = Sha256::new();
    for file in files {
        let path = normalized_artifact_path(&file.path)?;
        hasher.update((path.len() as u64).to_be_bytes());
        hasher.update(path.as_bytes());
        hasher.update(file.size.to_be_bytes());
        hasher.update(file.sha256.as_bytes());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn validate_artifact_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("invalid artifact identifier");
    }
    Ok(())
}

fn validate_manifest(directory_name: &std::ffi::OsStr, manifest: &ArtifactManifest) -> Result<()> {
    let directory_name = directory_name
        .to_str()
        .context("artifact directory name is not valid UTF-8")?;
    validate_artifact_id(manifest.id.as_str())?;
    if directory_name != manifest.id.as_str() {
        bail!("artifact manifest identifier does not match its store directory");
    }
    if manifest.file_count != manifest.files.len() {
        bail!("artifact manifest file count does not match its file list");
    }
    let mut paths = BTreeMap::new();
    let mut total_bytes = 0u64;
    for file in &manifest.files {
        validate_relative_path(&file.path)?;
        register_artifact_path(&mut paths, &file.path, ArtifactPathKind::File)?;
        total_bytes = total_bytes
            .checked_add(file.size)
            .context("artifact manifest size overflow")?;
    }
    if total_bytes != manifest.total_bytes {
        bail!("artifact manifest size does not match its file list");
    }
    if let Some(entrypoint) = manifest.entrypoint.as_ref() {
        validate_relative_path(entrypoint)?;
        if !manifest.files.iter().any(|file| file.path == *entrypoint) {
            bail!("artifact entrypoint is absent from its file list");
        }
    }
    Ok(())
}

/// Owns a loopback-only artifact server. Dropping the handle synchronously
/// stops the helper thread, which prevents preview servers from leaking after
/// tabs or the application close.
pub struct ArtifactPreview {
    address: String,
    shutdown: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for ArtifactPreview {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ArtifactPreview")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl ArtifactPreview {
    pub fn start(root: PathBuf, entrypoint: PathBuf) -> Result<Self> {
        let server = ArtifactServer::bind(root, entrypoint, 0)?;
        let address = server.address();
        let (shutdown, receiver) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("workdeck-artifact-preview".into())
            .spawn(move || {
                if let Err(error) = server.serve_until(receiver) {
                    eprintln!("artifact preview stopped with an error: {error:#}");
                }
            })
            .context("failed to start artifact preview")?;
        Ok(Self {
            address,
            shutdown: Some(shutdown),
            thread: Some(thread),
        })
    }

    pub fn url(&self) -> &str {
        &self.address
    }

    pub fn url_for(&self, path: &Path) -> Result<String> {
        validate_relative_path(path)?;
        let encoded = path
            .components()
            .map(|component| {
                let Component::Normal(segment) = component else {
                    return Err(anyhow::anyhow!("artifact path has an ambiguous component"));
                };
                let segment = segment
                    .to_str()
                    .context("artifact preview paths must be valid UTF-8")?;
                Ok(percent_encode_path_segment(segment))
            })
            .collect::<Result<Vec<_>>>()?
            .join("/");
        Ok(format!("{}/{encoded}", self.address))
    }

    pub fn stop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn percent_encode_path_segment(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                vec![byte as char]
            } else {
                format!("%{byte:02X}").chars().collect()
            }
        })
        .collect()
}

impl Drop for ArtifactPreview {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct ArtifactServer {
    server: Server,
    root: PathBuf,
    entrypoint: PathBuf,
    token: String,
}

impl ArtifactServer {
    pub fn bind(root: PathBuf, entrypoint: PathBuf, port: u16) -> Result<Self> {
        let root = fs::canonicalize(&root)
            .with_context(|| format!("artifact root {} does not exist", root.display()))?;
        validate_relative_path(&entrypoint)?;
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        let server = Server::http(address)
            .map_err(|error| anyhow::anyhow!("failed to bind artifact viewer: {error}"))?;
        Ok(Self {
            server,
            root,
            entrypoint,
            token: ArtifactId::new().as_str().to_string(),
        })
    }

    pub fn address(&self) -> String {
        format!("http://{}/{}", self.server.server_addr(), self.token)
    }

    pub fn serve(self) -> Result<()> {
        for request in self.server.incoming_requests() {
            let response = self.response_for(request.url());
            request.respond(response)?;
        }
        Ok(())
    }

    fn serve_until(self, shutdown: mpsc::Receiver<()>) -> Result<()> {
        loop {
            if shutdown.try_recv().is_ok() {
                break;
            }
            if let Some(request) = self.server.recv_timeout(Duration::from_millis(50))? {
                let response = self.response_for(request.url());
                request.respond(response)?;
            }
        }
        Ok(())
    }

    fn response_for(&self, url: &str) -> Response<std::io::Cursor<Vec<u8>>> {
        let raw_path = url.split('?').next().unwrap_or("/");
        let decoded = match percent_decode_str(raw_path).decode_utf8() {
            Ok(value) => value,
            Err(_) => return error_response(StatusCode(400), "invalid URL encoding"),
        };
        let token_root = format!("/{}", self.token);
        let Some(route) = decoded.strip_prefix(&token_root) else {
            return error_response(StatusCode(403), "invalid artifact preview token");
        };
        if !route.is_empty() && !route.starts_with('/') {
            return error_response(StatusCode(403), "invalid artifact preview token");
        }
        let relative = if route.is_empty() || route == "/" {
            self.entrypoint.clone()
        } else {
            PathBuf::from(route.trim_start_matches('/'))
        };
        if validate_relative_path(&relative).is_err() {
            return error_response(StatusCode(403), "path is outside artifact");
        }
        let requested = self.root.join(&relative);
        let resolved = match fs::canonicalize(&requested) {
            Ok(path) if path.starts_with(&self.root) => path,
            _ => return error_response(StatusCode(404), "artifact file not found"),
        };
        let resolved = if resolved.is_dir() {
            resolved.join("index.html")
        } else {
            resolved
        };
        let bytes = match fs::read(&resolved) {
            Ok(bytes) => bytes,
            Err(_) => return error_response(StatusCode(404), "artifact file not found"),
        };
        let media_type = mime_guess::from_path(&resolved)
            .first_or_octet_stream()
            .essence_str()
            .to_string();
        secure_response(
            Response::from_data(bytes).with_header(
                Header::from_bytes("Content-Type", media_type).expect("static header"),
            ),
        )
    }
}

fn secure_response<R: Read + Send + 'static>(response: Response<R>) -> Response<R> {
    response
        .with_header(
            Header::from_bytes(
                "Content-Security-Policy",
                "sandbox allow-scripts; default-src 'self' data: blob:; script-src 'self' 'unsafe-inline' blob:; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
            )
            .expect("static CSP"),
        )
        .with_header(
            Header::from_bytes("X-Content-Type-Options", "nosniff").expect("static header"),
        )
        .with_header(
            Header::from_bytes("Referrer-Policy", "no-referrer").expect("static header"),
        )
        .with_header(
            Header::from_bytes("Cache-Control", "no-store").expect("static header"),
        )
}

fn error_response(status: StatusCode, message: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    secure_response(Response::from_string(message).with_status_code(status))
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.is_absolute() || path.as_os_str().is_empty() {
        bail!("artifact paths must be non-empty and relative");
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::CurDir | Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("artifact path escapes its root: {}", path.display());
    }
    Ok(())
}

fn normalized_artifact_path(path: &Path) -> Result<String> {
    validate_relative_path(path)?;
    let mut segments = Vec::new();
    for component in path.components() {
        let Component::Normal(segment) = component else {
            bail!(
                "artifact path has an ambiguous component: {}",
                path.display()
            );
        };
        let segment = segment
            .to_str()
            .with_context(|| format!("artifact path is not valid UTF-8: {}", path.display()))?;
        if segment.contains('\\') || segment.chars().any(char::is_control) {
            bail!(
                "artifact path contains an unsafe character: {}",
                path.display()
            );
        }
        segments.push(segment.nfc().collect::<String>().to_lowercase());
    }
    let normalized = segments.join("/");
    if normalized.is_empty() {
        bail!("artifact path is invalid after normalization");
    }
    Ok(normalized)
}

fn normalized_ancestors(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/')
        .map(|(index, _)| &path[..index])
        .filter(|ancestor| !ancestor.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactPathKind {
    File,
    Directory,
}

fn register_artifact_path(
    paths: &mut BTreeMap<String, ArtifactPathKind>,
    path: &Path,
    kind: ArtifactPathKind,
) -> Result<()> {
    let normalized = normalized_artifact_path(path)?;
    let normalized_prefix = format!("{normalized}/");
    if paths.contains_key(&normalized)
        || (kind == ArtifactPathKind::File
            && paths
                .keys()
                .any(|existing| existing.starts_with(&normalized_prefix)))
        || normalized_ancestors(&normalized)
            .any(|ancestor| paths.get(ancestor) == Some(&ArtifactPathKind::File))
    {
        bail!(
            "artifact contains duplicate or colliding path: {}",
            path.display()
        );
    }
    paths.insert(normalized, kind);
    Ok(())
}

fn choose_entrypoint(files: &[ArtifactFile]) -> Option<PathBuf> {
    files
        .iter()
        .find(|file| file.path == Path::new("index.html"))
        .or_else(|| {
            files
                .iter()
                .filter(|file| {
                    file.path
                        .file_name()
                        .is_some_and(|name| name == "index.html")
                })
                .min_by_key(|file| file.path.components().count())
        })
        .map(|file| file.path.clone())
        .or_else(|| {
            files
                .iter()
                .find(|file| file.media_type == "text/html")
                .map(|file| file.path.clone())
        })
        .or_else(|| {
            files
                .iter()
                .find(|file| {
                    file.media_type.starts_with("text/")
                        || file.media_type.starts_with("image/")
                        || matches!(
                            file.path
                                .extension()
                                .and_then(|extension| extension.to_str()),
                            Some("log" | "md" | "markdown" | "json" | "xml" | "yaml" | "yml")
                        )
                })
                .map(|file| file.path.clone())
        })
}

fn is_platform_metadata(path: &Path) -> bool {
    path.components().next().is_some_and(
        |component| matches!(component, std::path::Component::Normal(name) if name == "__MACOSX"),
    ) || path
        .file_name()
        .is_some_and(|name| name == ".DS_Store" || name.to_string_lossy().starts_with("._"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};
    use std::net::TcpStream;

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        for (name, bytes) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn compressed_archive(name: &str, bytes: &[u8]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        writer
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn imports_html_and_selects_root_entrypoint() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(temporary.path()).unwrap();
        let bytes = archive(&[("assets/app.js", b"ok"), ("index.html", b"<h1>Hi</h1>")]);
        let manifest = store
            .import_zip_reader(Cursor::new(bytes), "report".into(), "test".into())
            .unwrap();
        assert_eq!(manifest.file_count, 2);
        assert_eq!(
            manifest.entrypoint.as_deref(),
            Some(Path::new("index.html"))
        );
        assert!(
            store
                .files_root(&manifest.id)
                .unwrap()
                .join("index.html")
                .is_file()
        );
        assert_eq!(manifest.content_sha256.len(), 64);
        assert_eq!(manifest.files[0].sha256.len(), 64);
    }

    #[test]
    fn path_import_records_archive_and_content_integrity_metadata() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(temporary.path().join("store")).unwrap();
        let archive_path = temporary.path().join("report.zip");
        let bytes = archive(&[("index.html", b"<main>evidence</main>")]);
        fs::write(&archive_path, &bytes).unwrap();

        let manifest = store
            .import_zip(&archive_path, "report", "fixture")
            .unwrap();

        assert_eq!(manifest.archive_bytes, Some(bytes.len() as u64));
        assert_eq!(manifest.archive_sha256.as_deref().map(str::len), Some(64));
        assert_eq!(manifest.content_sha256.len(), 64);
        assert_eq!(manifest.files[0].sha256.len(), 64);
    }

    #[test]
    fn artifact_versions_are_monotonic_and_legacy_manifests_migrate_in_memory() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(temporary.path()).unwrap();
        let first = store
            .import_zip_reader(
                Cursor::new(archive(&[("report.md", b"one")])),
                "quality".into(),
                "github:owner/repo:artifact:41".into(),
            )
            .unwrap();
        let second = store
            .import_zip_reader(
                Cursor::new(archive(&[("report.md", b"two")])),
                "quality".into(),
                "github:owner/repo:artifact:42".into(),
            )
            .unwrap();
        assert_eq!(first.series_key, second.series_key);
        assert_eq!((first.version, second.version), (1, 2));

        let manifest_path = temporary
            .path()
            .join(first.id.as_str())
            .join("manifest.json");
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        legacy.as_object_mut().unwrap().remove("series_key");
        legacy.as_object_mut().unwrap().remove("version");
        fs::write(&manifest_path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
        let restored = store.find(first.id.as_str()).unwrap().unwrap();
        assert_eq!(restored.version, 1);
        assert!(!restored.series_key.is_empty());
    }

    #[test]
    fn selects_text_markdown_log_and_image_previews_without_html() {
        for (name, bytes) in [
            ("report.md", b"# Report".as_slice()),
            ("test.log", b"PASS".as_slice()),
            ("screenshot.png", b"not-a-real-image".as_slice()),
        ] {
            let temporary = tempfile::tempdir().unwrap();
            let store = ArtifactStore::new(temporary.path()).unwrap();
            let manifest = store
                .import_zip_reader(
                    Cursor::new(archive(&[(name, bytes)])),
                    "preview".into(),
                    "test".into(),
                )
                .unwrap();
            assert_eq!(manifest.entrypoint.as_deref(), Some(Path::new(name)));
        }
    }

    #[test]
    fn rejects_zip_slip_paths() {
        assert!(validate_relative_path(Path::new("../secret")).is_err());
        assert!(validate_relative_path(Path::new("safe/index.html")).is_ok());
    }

    #[test]
    fn ignores_platform_metadata() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(temporary.path()).unwrap();
        let bytes = archive(&[
            ("__MACOSX/report/._index.html", b"metadata"),
            ("report/.DS_Store", b"metadata"),
            ("report/index.html", b"<h1>Report</h1>"),
        ]);
        let manifest = store
            .import_zip_reader(Cursor::new(bytes), "report".into(), "test".into())
            .unwrap();

        assert_eq!(manifest.file_count, 1);
        assert_eq!(
            manifest.entrypoint.as_deref(),
            Some(Path::new("report/index.html"))
        );
    }

    #[test]
    fn preview_is_loopback_only_hardened_and_stoppable() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(temporary.path()).unwrap();
        let bytes = archive(&[("index.html", b"<h1>Preview</h1>")]);
        let manifest = store
            .import_zip_reader(Cursor::new(bytes), "report".into(), "test".into())
            .unwrap();
        let mut preview = store.start_preview(&manifest).unwrap();
        assert!(preview.url().starts_with("http://127.0.0.1:"));
        assert!(
            preview
                .url_for(Path::new("assets/quality report.html"))
                .unwrap()
                .ends_with("/assets/quality%20report.html")
        );
        assert!(preview.url_for(Path::new("../outside")).is_err());

        let address = preview.url().trim_start_matches("http://");
        let (socket, route) = address.split_once('/').unwrap();
        let mut stream = TcpStream::connect(socket).unwrap();
        stream
            .write_all(
                format!("GET /{route} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("200 OK"));
        assert!(response.contains("Content-Security-Policy"));
        assert!(response.contains("<h1>Preview</h1>"));

        preview.stop();
        assert!(preview.shutdown.is_none());
        assert!(preview.thread.is_none());
    }

    #[test]
    fn enforces_expanded_size_limit() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(temporary.path())
            .unwrap()
            .with_limits(10, 3);
        let bytes = archive(&[("index.html", b"four")]);
        assert!(
            store
                .import_zip_reader(Cursor::new(bytes), "large".into(), "test".into())
                .is_err()
        );
    }

    #[test]
    fn rejects_symlinks_and_excessive_entry_counts() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(temporary.path()).unwrap();
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        writer
            .add_symlink(
                "outside",
                "../secret",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        let symlink = writer.finish().unwrap().into_inner();
        assert!(
            store
                .import_zip_reader(Cursor::new(symlink), "links".into(), "test".into())
                .is_err()
        );

        let bounded = ArtifactStore::new(temporary.path().join("bounded"))
            .unwrap()
            .with_limits(1, 1024);
        let two_files = archive(&[("one.txt", b"1"), ("two.txt", b"2")]);
        assert!(
            bounded
                .import_zip_reader(Cursor::new(two_files), "many".into(), "test".into())
                .is_err()
        );
    }

    #[test]
    fn rejects_duplicate_unicode_normalized_and_colliding_paths() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(temporary.path()).unwrap();
        let mut paths = BTreeMap::new();
        register_artifact_path(&mut paths, Path::new("index.html"), ArtifactPathKind::File)
            .unwrap();
        assert!(
            register_artifact_path(&mut paths, Path::new("index.html"), ArtifactPathKind::File)
                .is_err()
        );
        register_artifact_path(&mut paths, Path::new("café.html"), ArtifactPathKind::File).unwrap();
        assert!(
            register_artifact_path(
                &mut paths,
                Path::new("cafe\u{301}.html"),
                ArtifactPathKind::File,
            )
            .is_err()
        );
        register_artifact_path(&mut paths, Path::new("assets"), ArtifactPathKind::File).unwrap();
        assert!(
            register_artifact_path(
                &mut paths,
                Path::new("assets/app.js"),
                ArtifactPathKind::File,
            )
            .is_err()
        );

        let unicode_collision = archive(&[("café.html", b"one"), ("cafe\u{301}.html", b"two")]);
        assert!(
            store
                .import_zip_reader(
                    Cursor::new(unicode_collision),
                    "unicode".into(),
                    "test".into(),
                )
                .is_err()
        );

        let file_directory_collision = archive(&[("bundle", b"file"), ("bundle/app.js", b"js")]);
        assert!(
            store
                .import_zip_reader(
                    Cursor::new(file_directory_collision),
                    "collision".into(),
                    "test".into(),
                )
                .is_err()
        );
    }

    #[test]
    fn rejects_suspicious_compression_ratios() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(temporary.path()).unwrap();
        let payload = vec![b'A'; (COMPRESSION_RATIO_FLOOR + 1) as usize];
        let bytes = compressed_archive("report.log", &payload);
        assert!(
            store
                .import_zip_reader(Cursor::new(bytes), "bomb".into(), "test".into())
                .is_err()
        );
    }

    #[test]
    fn sandbox_server_is_loopback_only_and_blocks_traversal() {
        let temporary = tempfile::tempdir().unwrap();
        fs::write(temporary.path().join("index.html"), "<h1>safe</h1>").unwrap();
        let server = ArtifactServer::bind(
            temporary.path().to_path_buf(),
            PathBuf::from("index.html"),
            0,
        )
        .unwrap();
        assert!(server.address().starts_with("http://127.0.0.1:"));
        assert!(!server.address().ends_with(":0"));
        assert!(!server.address().ends_with('/'));

        let response = server.response_for(&format!("/{}/", server.token));
        assert_eq!(response.status_code(), StatusCode(200));
        assert!(response.headers().iter().any(|header| {
            header.field.equiv("Content-Security-Policy")
                && header.value.as_str().contains("connect-src 'none'")
        }));
        assert_eq!(
            server
                .response_for(&format!("/{}/%2e%2e/secret", server.token))
                .status_code(),
            StatusCode(403)
        );
        assert_eq!(
            server.response_for("/wrong/index.html").status_code(),
            StatusCode(403)
        );
    }

    #[test]
    fn rejects_case_folded_and_ambiguous_zip_paths() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(temporary.path()).unwrap();
        let case_collision = archive(&[("Assets/app.js", b"one"), ("assets/app.js", b"two")]);
        assert!(
            store
                .import_zip_reader(
                    Cursor::new(case_collision),
                    "case collision".into(),
                    "test".into(),
                )
                .is_err()
        );
        assert!(normalized_artifact_path(Path::new("./index.html")).is_err());
        assert!(normalized_artifact_path(Path::new("assets\\app.js")).is_err());
    }

    #[test]
    fn tampered_manifest_id_cannot_redirect_preview_outside_the_store() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(temporary.path()).unwrap();
        let manifest = store
            .import_zip_reader(
                Cursor::new(archive(&[("index.html", b"safe")])),
                "report".into(),
                "test".into(),
            )
            .unwrap();
        let manifest_path = temporary
            .path()
            .join(manifest.id.as_str())
            .join("manifest.json");
        let mut tampered: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        tampered["id"] = serde_json::Value::String("../../outside".into());
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&tampered).unwrap(),
        )
        .unwrap();

        assert!(store.list().is_err());
        assert!(store.find("report").is_err());
    }
}
