//! Native PTY/VT terminal keyframe capture and deterministic scene driving.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use qwertty_term_vt::screen::cursor::CursorStyle;
use qwertty_term_vt::snapshot::{CellWidth, Snapshot, SnapshotColor, SnapshotUnderline};
use qwertty_term_vt::stream::{Stream, TerminalHandler};
use qwertty_term_vt::terminal::{Options as TerminalOptions, Terminal};
use regex::Regex;
use serde::{Deserialize, Serialize};
use swash::scale::image::{Content as GlyphContent, Image as GlyphImage};
use swash::scale::{Render as GlyphRender, ScaleContext, Source, StrikeWith};
use swash::{CacheKey, FontRef};

use super::{required_path, resolve};

const DEFAULT_FONT_SIZE: f32 = 16.0;
const DEFAULT_LINE_HEIGHT: f32 = 1.5;
const DEFAULT_DEVICE_PIXEL_RATIO: u16 = 2;
const DEFAULT_TYPE_DELAY_MS: u64 = 30;
const DEFAULT_WAIT_TIMEOUT_MS: u64 = 10_000;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RenderOptions {
    #[serde(default = "default_font_size")]
    font_size: f32,
    #[serde(default = "default_line_height")]
    line_height: f32,
    #[serde(default = "default_device_pixel_ratio")]
    device_pixel_ratio: u16,
    #[serde(default = "default_background")]
    background: [u8; 3],
    #[serde(default = "default_foreground")]
    foreground: [u8; 3],
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            font_size: default_font_size(),
            line_height: default_line_height(),
            device_pixel_ratio: default_device_pixel_ratio(),
            background: default_background(),
            foreground: default_foreground(),
        }
    }
}

const fn default_font_size() -> f32 {
    DEFAULT_FONT_SIZE
}

const fn default_line_height() -> f32 {
    DEFAULT_LINE_HEIGHT
}

const fn default_device_pixel_ratio() -> u16 {
    DEFAULT_DEVICE_PIXEL_RATIO
}

const fn default_background() -> [u8; 3] {
    [0, 0, 0]
}

const fn default_foreground() -> [u8; 3] {
    [208, 208, 208]
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CaptureScript {
    pub(super) cols: u16,
    pub(super) rows: u16,
    pub(super) frames_dir: PathBuf,
    pub(super) font: PathBuf,
    #[serde(default)]
    pub(super) render_options: RenderOptions,
    #[serde(default)]
    pub(super) manifest: Option<PathBuf>,
    pub(super) scenes: Vec<CaptureScene>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CaptureScene {
    pub(super) name: String,
    #[serde(default)]
    pub(super) wrappers: Vec<WrapperSpec>,
    pub(super) launch: LaunchSpec,
    pub(super) actions: Vec<CaptureAction>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct WrapperSpec {
    pub(super) bin_dir: PathBuf,
    pub(super) name: String,
    pub(super) exec: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum LaunchSpec {
    App {
        command: PathBuf,
        #[serde(default)]
        args: Vec<String>,
        cwd: PathBuf,
        #[serde(default)]
        env: BTreeMap<String, Option<String>>,
    },
    Shell {
        cwd: PathBuf,
        #[serde(default)]
        path_prepend: Vec<PathBuf>,
        #[serde(default)]
        prompt: Option<String>,
        #[serde(default)]
        env: BTreeMap<String, Option<String>>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum CaptureAction {
    Sleep {
        ms: u64,
    },
    WaitText {
        pattern: String,
        #[serde(default = "default_wait_timeout_ms")]
        timeout_ms: u64,
    },
    Probe {
        key: String,
        pattern: String,
        dismiss_key: String,
        #[serde(default = "default_probe_attempts")]
        attempts: usize,
        #[serde(default = "default_probe_timeout_ms")]
        timeout_ms: u64,
        #[serde(default = "default_probe_settle_ms")]
        settle_ms: u64,
    },
    Press {
        key: String,
    },
    Write {
        text: String,
    },
    Type {
        text: String,
        #[serde(default)]
        delay_ms: u64,
    },
    TypeCommand {
        command: String,
        #[serde(default, deserialize_with = "deserialize_snap_at")]
        snap_at: BTreeMap<usize, String>,
        #[serde(default = "default_type_delay_ms")]
        delay_ms: u64,
    },
    Snap {
        name: String,
    },
}

fn deserialize_snap_at<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<usize, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let wire = BTreeMap::<String, String>::deserialize(deserializer)?;
    Ok(wire
        .into_iter()
        .filter_map(|(key, value)| {
            let index = key.parse::<usize>().ok()?;
            (key == index.to_string()).then_some((index, value))
        })
        .collect())
}

const fn default_wait_timeout_ms() -> u64 {
    DEFAULT_WAIT_TIMEOUT_MS
}

const fn default_probe_attempts() -> usize {
    5
}

const fn default_probe_timeout_ms() -> u64 {
    2_000
}

const fn default_probe_settle_ms() -> u64 {
    300
}

const fn default_type_delay_ms() -> u64 {
    DEFAULT_TYPE_DELAY_MS
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct KeyframeEntry {
    name: String,
    file: String,
    cols: u16,
    rows: u16,
}

pub fn capture_file(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut script_path = None;
    let mut explicit_scenes = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--script" => script_path = Some(required_path(&mut args, "--script")?),
            "--scenes" => {
                explicit_scenes = Some(
                    args.next()
                        .context("--scenes requires a comma-separated value")?,
                );
            }
            _ => bail!("unknown media capture option {argument:?}"),
        }
    }
    let script_path = resolve(
        repo,
        script_path.context("media capture requires --script FILE")?,
    );
    let source = fs::read_to_string(&script_path)
        .with_context(|| format!("read capture script {}", script_path.display()))?;
    let mut script: CaptureScript = serde_json::from_str(&source)
        .with_context(|| format!("parse capture script {}", script_path.display()))?;
    validate_script(&script)?;
    resolve_script_paths(repo, &mut script);
    let selected = explicit_scenes.or_else(|| std::env::var("SCENES").ok());
    let wants = SceneFilter::new(selected.as_deref());
    let captured = run_capture(&script, &wants)?;
    println!(
        "captured {} keyframes across {} selected scenes -> {}",
        captured.frames,
        captured.scenes,
        captured.manifest.display()
    );
    Ok(())
}

fn validate_script(script: &CaptureScript) -> Result<()> {
    if script.cols == 0 || script.rows == 0 {
        bail!("capture geometry requires nonzero cols and rows");
    }
    let render = script.render_options;
    if !render.font_size.is_finite() || render.font_size <= 0.0 {
        bail!("capture fontSize must be a positive finite number");
    }
    if !render.line_height.is_finite() || render.line_height <= 0.0 {
        bail!("capture lineHeight must be a positive finite number");
    }
    if render.device_pixel_ratio == 0 {
        bail!("capture devicePixelRatio must be nonzero");
    }
    if script.scenes.is_empty() {
        bail!("capture script requires at least one scene");
    }
    let mut names = std::collections::BTreeSet::new();
    for scene in &script.scenes {
        if scene.name.trim().is_empty() {
            bail!("capture scene name cannot be empty");
        }
        if !names.insert(&scene.name) {
            bail!("duplicate capture scene name {:?}", scene.name);
        }
        for wrapper in &scene.wrappers {
            validate_frame_name(&wrapper.name).context("invalid wrapper name")?;
            if wrapper.exec.is_empty() {
                bail!("capture wrapper {:?} requires an executable", wrapper.name);
            }
        }
        for action in &scene.actions {
            match action {
                CaptureAction::Snap { name } => validate_frame_name(name)?,
                CaptureAction::TypeCommand { snap_at, .. } => {
                    for name in snap_at.values() {
                        validate_frame_name(name)?;
                    }
                }
                CaptureAction::Probe { attempts, .. } if *attempts == 0 => {
                    bail!("capture keyboard probe attempts must be nonzero");
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn validate_frame_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_control)
    {
        bail!("invalid capture frame name {name:?}");
    }
    Ok(())
}

fn resolve_script_paths(repo: &Path, script: &mut CaptureScript) {
    script.frames_dir = resolve(repo, script.frames_dir.clone());
    script.font = resolve(repo, script.font.clone());
    script.manifest = script.manifest.take().map(|path| resolve(repo, path));
    for scene in &mut script.scenes {
        for wrapper in &mut scene.wrappers {
            wrapper.bin_dir = resolve(repo, wrapper.bin_dir.clone());
        }
        match &mut scene.launch {
            LaunchSpec::App { command, cwd, .. } => {
                if command.components().count() > 1 {
                    *command = resolve(repo, command.clone());
                }
                *cwd = resolve(repo, cwd.clone());
            }
            LaunchSpec::Shell {
                cwd, path_prepend, ..
            } => {
                *cwd = resolve(repo, cwd.clone());
                for path in path_prepend {
                    *path = resolve(repo, path.clone());
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CaptureResult {
    pub(super) frames: usize,
    pub(super) scenes: usize,
    pub(super) manifest: PathBuf,
}

pub(super) fn run_capture(script: &CaptureScript, wants: &SceneFilter) -> Result<CaptureResult> {
    let mut keyframer = Keyframer::new(
        script.frames_dir.clone(),
        script.font.clone(),
        script.cols,
        script.rows,
        script.render_options,
    )?;
    let mut selected_scenes = 0;
    for scene in &script.scenes {
        if !wants.matches(&scene.name) {
            continue;
        }
        selected_scenes += 1;
        println!("scene: {}", scene.name);
        for wrapper in &scene.wrappers {
            create_command_wrapper(&wrapper.bin_dir, &wrapper.name, &wrapper.exec)?;
        }
        let mut session = TerminalSession::launch(&scene.launch, script.cols, script.rows)?;
        let result = execute_actions(&mut session, &mut keyframer, &scene.actions);
        session.close()?;
        result.with_context(|| format!("capture scene {:?}", scene.name))?;
    }
    let manifest = script
        .manifest
        .clone()
        .unwrap_or_else(|| script.frames_dir.join("manifest.json"));
    keyframer.write_manifest(&manifest)?;
    Ok(CaptureResult {
        frames: keyframer.manifest.len(),
        scenes: selected_scenes,
        manifest,
    })
}

fn execute_actions(
    session: &mut TerminalSession,
    keyframer: &mut Keyframer,
    actions: &[CaptureAction],
) -> Result<()> {
    for action in actions {
        match action {
            CaptureAction::Sleep { ms } => thread::sleep(Duration::from_millis(*ms)),
            CaptureAction::WaitText {
                pattern,
                timeout_ms,
            } => {
                let pattern = Regex::new(pattern)
                    .with_context(|| format!("compile capture wait pattern {pattern:?}"))?;
                session.wait_for_text(&pattern, Duration::from_millis(*timeout_ms))?;
            }
            CaptureAction::Probe {
                key,
                pattern,
                dismiss_key,
                attempts,
                timeout_ms,
                settle_ms,
            } => {
                let probe = KeyboardProbe {
                    probe_key: key,
                    expect: Regex::new(pattern)
                        .with_context(|| format!("compile capture probe pattern {pattern:?}"))?,
                    dismiss_key,
                    attempts: *attempts,
                    timeout: Duration::from_millis(*timeout_ms),
                    settle: Duration::from_millis(*settle_ms),
                };
                ensure_keyboard_is_live(session, &probe)?;
            }
            CaptureAction::Press { key } => session.press(key)?,
            CaptureAction::Write { text } => session.write_raw(text)?,
            CaptureAction::Type { text, delay_ms } => {
                type_text(session, text, Duration::from_millis(*delay_ms))?;
            }
            CaptureAction::TypeCommand {
                command,
                snap_at,
                delay_ms,
            } => type_command(
                session,
                keyframer,
                command,
                snap_at,
                Duration::from_millis(*delay_ms),
            )?,
            CaptureAction::Snap { name } => keyframer.snap(session, name)?,
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SceneFilter {
    names: Option<Vec<String>>,
}

impl SceneFilter {
    pub(super) fn new(value: Option<&str>) -> Self {
        Self {
            names: value.map(|value| {
                value
                    .split(',')
                    .map(|scene| scene.trim().to_owned())
                    .collect()
            }),
        }
    }

    pub(super) fn matches(&self, name: &str) -> bool {
        self.names
            .as_ref()
            .is_none_or(|names| names.iter().any(|candidate| candidate == name))
    }
}

struct TerminalSession {
    writer: Option<Box<dyn Write + Send>>,
    child: Box<dyn Child + Send + Sync>,
    parser: Stream<TerminalHandler>,
    output: Receiver<Vec<u8>>,
    reader: Option<thread::JoinHandle<()>>,
    closed: bool,
}

impl TerminalSession {
    fn launch(spec: &LaunchSpec, cols: u16, rows: u16) -> Result<Self> {
        let system = native_pty_system();
        let pair = system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("open capture PTY")?;
        let mut command = command_builder(spec)?;
        command.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(command)
            .context("launch capture process in PTY")?;
        drop(pair.slave);
        let mut source = pair
            .master
            .try_clone_reader()
            .context("clone capture PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("take capture PTY writer")?;
        drop(pair.master);
        let terminal = Terminal::new(TerminalOptions {
            cols,
            rows,
            ..TerminalOptions::default()
        });
        let parser = Stream::new(TerminalHandler::new(terminal));
        let (output_tx, output) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut buffer = [0_u8; 16 * 1024];
            while let Ok(count) = source.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                if output_tx.send(buffer[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            writer: Some(writer),
            child,
            parser,
            output,
            reader: Some(reader),
            closed: false,
        })
    }

    fn write_raw(&mut self, text: &str) -> Result<()> {
        let writer = self
            .writer
            .as_mut()
            .context("capture PTY is already closed")?;
        writer
            .write_all(text.as_bytes())
            .context("write capture PTY input")?;
        writer.flush().context("flush capture PTY input")
    }

    fn press(&mut self, key: &str) -> Result<()> {
        let bytes = key_sequence(key)?;
        let writer = self
            .writer
            .as_mut()
            .context("capture PTY is already closed")?;
        writer.write_all(&bytes).context("press capture PTY key")?;
        writer.flush().context("flush capture PTY key")
    }

    fn wait_for_text(&mut self, pattern: &Regex, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if pattern.is_match(&self.contents()?) {
                return Ok(());
            }
            if let Some(status) = self.child.try_wait().context("poll capture process")? {
                thread::sleep(Duration::from_millis(20));
                if pattern.is_match(&self.contents()?) {
                    return Ok(());
                }
                bail!(
                    "capture process exited with {status:?} before terminal matched /{}/",
                    pattern.as_str()
                );
            }
            if Instant::now() >= deadline {
                bail!(
                    "terminal did not match /{}/ within {timeout:?}; screen:\n{}",
                    pattern.as_str(),
                    self.contents()?
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn drain_output(&mut self) -> Result<()> {
        while let Ok(bytes) = self.output.try_recv() {
            self.parser.feed(&bytes);
        }
        let replies = self.parser.handler.take_output();
        if !replies.is_empty()
            && let Some(writer) = self.writer.as_mut()
        {
            writer
                .write_all(&replies)
                .context("write terminal protocol reply to capture PTY")?;
            writer
                .flush()
                .context("flush terminal protocol reply to capture PTY")?;
        }
        Ok(())
    }

    fn contents(&mut self) -> Result<String> {
        self.drain_output()?;
        Ok(self.parser.terminal().plain_string())
    }

    fn screen(&mut self) -> Result<Snapshot> {
        self.drain_output()?;
        Ok(self.parser.terminal().snapshot())
    }

    fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        let reply_result = self.drain_output();
        self.closed = true;
        self.writer.take();
        let process_result = (|| {
            if self
                .child
                .try_wait()
                .context("poll capture process")?
                .is_none()
            {
                self.child.kill().context("terminate capture process")?;
                self.child.wait().context("reap capture process")?;
            }
            Ok(())
        })();
        let reader_result = self.reader.take().map_or(Ok(()), |reader| {
            reader
                .join()
                .map_err(|_| anyhow!("capture PTY reader thread panicked"))
        });
        let final_drain = self.drain_output();
        reply_result
            .and(process_result)
            .and(reader_result)
            .and(final_drain)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn command_builder(spec: &LaunchSpec) -> Result<CommandBuilder> {
    match spec {
        LaunchSpec::App {
            command,
            args,
            cwd,
            env,
        } => {
            let mut builder = CommandBuilder::new(command);
            builder.args(args);
            builder.cwd(cwd);
            apply_environment(&mut builder, env);
            Ok(builder)
        }
        LaunchSpec::Shell {
            cwd,
            path_prepend,
            prompt,
            env,
        } => {
            #[cfg(unix)]
            let mut builder = {
                let mut builder = CommandBuilder::new("/bin/bash");
                builder.args(["--noprofile", "--norc", "-i"]);
                builder.env(
                    "PS1",
                    prompt
                        .as_deref()
                        .unwrap_or("\\[\\e[38;5;213m\\]❯\\[\\e[0m\\] "),
                );
                builder
            };
            #[cfg(windows)]
            let mut builder = {
                let mut builder = CommandBuilder::new("cmd.exe");
                builder.args(["/Q", "/K"]);
                if let Some(prompt) = prompt {
                    builder.env("PROMPT", prompt);
                }
                builder
            };
            builder.cwd(cwd);
            if !path_prepend.is_empty() {
                let current = std::env::var_os("PATH").unwrap_or_default();
                let mut paths = path_prepend.clone();
                paths.extend(std::env::split_paths(&current));
                let joined = std::env::join_paths(paths).context("compose capture shell PATH")?;
                builder.env("PATH", joined);
            }
            apply_environment(&mut builder, env);
            Ok(builder)
        }
    }
}

fn apply_environment(builder: &mut CommandBuilder, environment: &BTreeMap<String, Option<String>>) {
    for (name, value) in environment {
        if let Some(value) = value {
            builder.env(name, value);
        } else {
            builder.env_remove(name);
        }
    }
}

fn key_sequence(key: &str) -> Result<Vec<u8>> {
    let normalized = key.trim().to_ascii_lowercase();
    let sequence: &[u8] = match normalized.as_str() {
        "enter" | "return" => b"\r",
        "escape" | "esc" => b"\x1b",
        "tab" => b"\t",
        "backspace" => b"\x7f",
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        "home" => b"\x1b[H",
        "end" => b"\x1b[F",
        "pageup" | "page-up" => b"\x1b[5~",
        "pagedown" | "page-down" => b"\x1b[6~",
        "delete" => b"\x1b[3~",
        "insert" => b"\x1b[2~",
        "f1" => b"\x1bOP",
        "f2" => b"\x1bOQ",
        "f3" => b"\x1bOR",
        "f4" => b"\x1bOS",
        "f5" => b"\x1b[15~",
        "f6" => b"\x1b[17~",
        "f7" => b"\x1b[18~",
        "f8" => b"\x1b[19~",
        "f9" => b"\x1b[20~",
        "f10" => b"\x1b[21~",
        "f11" => b"\x1b[23~",
        "f12" => b"\x1b[24~",
        _ => {
            if let Some(letter) = normalized.strip_prefix("ctrl+") {
                let mut chars = letter.chars();
                let character = chars
                    .next()
                    .filter(|_| chars.next().is_none())
                    .filter(char::is_ascii)
                    .context("control key requires one ASCII character")?;
                return Ok(vec![(character.to_ascii_uppercase() as u8) & 0x1f]);
            }
            if key.chars().count() == 1 {
                return Ok(key.as_bytes().to_vec());
            }
            bail!("unsupported capture key {key:?}");
        }
    };
    Ok(sequence.to_vec())
}

fn type_text(session: &mut TerminalSession, text: &str, delay: Duration) -> Result<()> {
    for character in text.chars() {
        let mut encoded = [0; 4];
        session.write_raw(character.encode_utf8(&mut encoded))?;
        if !delay.is_zero() {
            thread::sleep(delay);
        }
    }
    Ok(())
}

fn type_command(
    session: &mut TerminalSession,
    keyframer: &mut Keyframer,
    command: &str,
    snap_at: &BTreeMap<usize, String>,
    delay: Duration,
) -> Result<()> {
    for (character, snap_name) in command_steps(command, snap_at) {
        let mut encoded = [0; 4];
        session.write_raw(character.encode_utf8(&mut encoded))?;
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        if let Some(name) = snap_name {
            keyframer.snap(session, name)?;
        }
    }
    Ok(())
}

fn command_steps<'a>(
    command: &'a str,
    snap_at: &'a BTreeMap<usize, String>,
) -> impl Iterator<Item = (char, Option<&'a str>)> + 'a {
    command
        .chars()
        .enumerate()
        .map(|(index, character)| (character, snap_at.get(&index).map(String::as_str)))
}

struct KeyboardProbe<'a> {
    probe_key: &'a str,
    expect: Regex,
    dismiss_key: &'a str,
    attempts: usize,
    timeout: Duration,
    settle: Duration,
}

trait ProbeSession {
    fn press_probe_key(&mut self, key: &str) -> Result<()>;
    fn wait_for_probe_text(&mut self, pattern: &Regex, timeout: Duration) -> Result<()>;
}

impl ProbeSession for TerminalSession {
    fn press_probe_key(&mut self, key: &str) -> Result<()> {
        self.press(key)
    }

    fn wait_for_probe_text(&mut self, pattern: &Regex, timeout: Duration) -> Result<()> {
        self.wait_for_text(pattern, timeout)
    }
}

fn ensure_keyboard_is_live<S: ProbeSession>(
    session: &mut S,
    probe: &KeyboardProbe<'_>,
) -> Result<()> {
    for _ in 0..probe.attempts {
        session.press_probe_key(probe.probe_key)?;
        if session
            .wait_for_probe_text(&probe.expect, probe.timeout)
            .is_ok()
        {
            session.press_probe_key(probe.dismiss_key)?;
            if !probe.settle.is_zero() {
                thread::sleep(probe.settle);
            }
            return Ok(());
        }
    }
    bail!("The app never reacted to a keypress.")
}

fn create_command_wrapper(bin_dir: &Path, name: &str, exec: &[String]) -> Result<PathBuf> {
    validate_frame_name(name).context("invalid command wrapper name")?;
    if exec.is_empty() {
        bail!("command wrapper requires an executable");
    }
    fs::create_dir_all(bin_dir)
        .with_context(|| format!("create wrapper directory {}", bin_dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let wrapper = bin_dir.join(name);
        let quoted = exec
            .iter()
            .map(|part| format!("\"{part}\""))
            .collect::<Vec<_>>()
            .join(" ");
        fs::write(&wrapper, format!("#!/bin/bash\nexec {quoted} \"$@\"\n"))
            .with_context(|| format!("write command wrapper {}", wrapper.display()))?;
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("make command wrapper executable {}", wrapper.display()))?;
    }
    #[cfg(windows)]
    {
        let wrapper = bin_dir.join(format!("{name}.cmd"));
        let quoted = exec
            .iter()
            .map(|part| format!("\"{}\"", part.replace('%', "%%")))
            .collect::<Vec<_>>()
            .join(" ");
        fs::write(&wrapper, format!("@echo off\r\n{quoted} %*\r\n"))
            .with_context(|| format!("write command wrapper {}", wrapper.display()))?;
    }
    Ok(bin_dir.to_path_buf())
}

struct Keyframer {
    frames_dir: PathBuf,
    cols: u16,
    rows: u16,
    render_options: RenderOptions,
    font: RasterFont,
    manifest: Vec<KeyframeEntry>,
}

impl Keyframer {
    fn new(
        frames_dir: PathBuf,
        font_path: PathBuf,
        cols: u16,
        rows: u16,
        render_options: RenderOptions,
    ) -> Result<Self> {
        fs::create_dir_all(&frames_dir)
            .with_context(|| format!("create keyframe directory {}", frames_dir.display()))?;
        let font_bytes = fs::read(&font_path)
            .with_context(|| format!("read capture font {}", font_path.display()))?;
        let font = RasterFont::new(font_bytes)
            .with_context(|| format!("parse capture font {}", font_path.display()))?;
        Ok(Self {
            frames_dir,
            cols,
            rows,
            render_options,
            font,
            manifest: Vec::new(),
        })
    }

    fn snap(&mut self, session: &mut TerminalSession, name: &str) -> Result<()> {
        validate_frame_name(name)?;
        let screen = session.screen()?;
        let png = render_terminal_png(
            &screen,
            &self.font,
            self.cols,
            self.rows,
            self.render_options,
        )?;
        let file = format!("{name}.png");
        fs::write(self.frames_dir.join(&file), png)
            .with_context(|| format!("write terminal keyframe {file}"))?;
        self.manifest.push(KeyframeEntry {
            name: name.to_owned(),
            file,
            cols: self.cols,
            rows: self.rows,
        });
        println!("  snap {name}");
        Ok(())
    }

    fn write_manifest(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create manifest directory {}", parent.display()))?;
        }
        let json = serde_json::to_vec_pretty(&self.manifest).context("encode keyframe manifest")?;
        fs::write(path, json).with_context(|| format!("write keyframe manifest {}", path.display()))
    }
}

struct RasterFont {
    data: Vec<u8>,
    offset: u32,
    key: CacheKey,
}

impl RasterFont {
    fn new(data: Vec<u8>) -> Result<Self> {
        let font = FontRef::from_index(&data, 0).context("font data has no OpenType face 0")?;
        Ok(Self {
            offset: font.offset,
            key: font.key,
            data,
        })
    }

    fn as_ref(&self) -> FontRef<'_> {
        FontRef {
            data: &self.data,
            offset: self.offset,
            key: self.key,
        }
    }
}

fn render_terminal_png(
    screen: &Snapshot,
    font: &RasterFont,
    cols: u16,
    rows: u16,
    options: RenderOptions,
) -> Result<Vec<u8>> {
    if screen.cols != usize::from(cols) || screen.rows != usize::from(rows) {
        bail!(
            "capture snapshot is {}x{}, expected {cols}x{rows}",
            screen.cols,
            screen.rows
        );
    }
    let px = options.font_size * f32::from(options.device_pixel_ratio);
    let font_ref = font.as_ref();
    let charmap = font_ref.charmap();
    let glyph_metrics = font_ref.glyph_metrics(&[]).scale(px);
    let cell_width = glyph_metrics
        .advance_width(charmap.map('M'))
        .ceil()
        .max(1.0) as usize;
    let cell_height = (px * options.line_height).round().max(1.0) as usize;
    let width = cell_width
        .checked_mul(usize::from(cols))
        .context("capture image width overflow")?;
    let height = cell_height
        .checked_mul(usize::from(rows))
        .context("capture image height overflow")?;
    let pixel_count = width
        .checked_mul(height)
        .context("capture image pixel count overflow")?;
    let byte_count = pixel_count
        .checked_mul(3)
        .context("capture image byte count overflow")?;
    let mut pixels = vec![0; byte_count];
    for pixel in pixels.chunks_exact_mut(3) {
        pixel.copy_from_slice(&options.background);
    }
    let metrics = font_ref.metrics(&[]).scale(px);
    let line_size = metrics.ascent + metrics.descent + metrics.leading;
    let baseline_offset = ((cell_height as f32 - line_size) / 2.0 + metrics.ascent).round() as i32;
    let mut scale_context = ScaleContext::new();
    let mut scaler = scale_context.builder(font_ref).size(px).hint(true).build();
    let glyph_sources = [
        Source::ColorOutline(0),
        Source::ColorBitmap(StrikeWith::BestFit),
        Source::Outline,
    ];
    let default_foreground = screen
        .default_fg
        .map_or(options.foreground, |color| [color.r, color.g, color.b]);
    let default_background = screen
        .default_bg
        .map_or(options.background, |color| [color.r, color.g, color.b]);

    for (row, snapshot_row) in screen.visible_window(0).iter().enumerate() {
        for (col, cell) in snapshot_row.cells.iter().enumerate() {
            if cell.is_spacer() {
                continue;
            }
            let style = cell.style;
            let mut foreground = terminal_color(style.fg, default_foreground, &screen.palette);
            let mut background = terminal_color(style.bg, default_background, &screen.palette);
            if style.inverse {
                std::mem::swap(&mut foreground, &mut background);
            }
            let cursor_here =
                screen.cursor.visible && screen.cursor.row == row && screen.cursor.col == col;
            let block_cursor = cursor_here && matches!(screen.cursor.style, CursorStyle::Block);
            if block_cursor {
                let cursor_color = default_foreground;
                foreground = background;
                background = cursor_color;
            }
            if style.faint {
                foreground = blend(foreground, background, 0.58);
            } else if style.bold {
                foreground = brighten(foreground);
            }
            let cell_x = col * cell_width;
            let cell_y = row * cell_height;
            let clip_width = if matches!(cell.width, CellWidth::Wide) {
                cell_width.saturating_mul(2)
            } else {
                cell_width
            };
            fill_rect(
                &mut pixels,
                width,
                cell_x,
                cell_y,
                clip_width,
                cell_height,
                background,
            );
            if !style.invisible {
                for character in std::iter::once(&cell.ch).chain(cell.combining.iter()) {
                    if let Some(glyph) = GlyphRender::new(&glyph_sources)
                        .render(&mut scaler, charmap.map(*character))
                    {
                        draw_glyph(
                            &mut pixels,
                            width,
                            height,
                            (cell_x, cell_y, clip_width, cell_height),
                            (cell_x as i32, cell_y as i32 + baseline_offset),
                            &glyph,
                            foreground,
                            style.bold,
                            style.italic,
                        );
                    }
                }
            }
            let underline_color = match style.underline_color {
                SnapshotColor::Default => foreground,
                color => terminal_color(color, foreground, &screen.palette),
            };
            draw_decorations(
                &mut pixels,
                width,
                cell_x,
                cell_y,
                clip_width,
                cell_height,
                foreground,
                underline_color,
                style.underline,
                style.strikethrough,
                style.overline,
            );
            if cursor_here && !block_cursor {
                draw_cursor(
                    &mut pixels,
                    width,
                    cell_x,
                    cell_y,
                    clip_width,
                    cell_height,
                    default_foreground,
                    screen.cursor.style,
                );
            }
        }
    }

    encode_rgb_png(width, height, &pixels)
}

fn encode_rgb_png(width: usize, height: usize, pixels: &[u8]) -> Result<Vec<u8>> {
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(3))
        .context("capture PNG dimensions overflow")?;
    if pixels.len() != expected {
        bail!(
            "capture RGB buffer has {} bytes, expected {expected}",
            pixels.len()
        );
    }
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(
            BufWriter::new(&mut encoded),
            u32::try_from(width).context("capture image is too wide")?,
            u32::try_from(height).context("capture image is too tall")?,
        );
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .context("write keyframe PNG header")?;
        writer
            .write_image_data(pixels)
            .context("write keyframe PNG pixels")?;
    }
    Ok(encoded)
}

#[allow(clippy::too_many_arguments)]
fn draw_decorations(
    pixels: &mut [u8],
    image_width: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    foreground: [u8; 3],
    underline_color: [u8; 3],
    underline: SnapshotUnderline,
    strikethrough: bool,
    overline: bool,
) {
    if overline {
        fill_rect(pixels, image_width, x, y, width, 1, foreground);
    }
    if strikethrough {
        fill_rect(pixels, image_width, x, y + height / 2, width, 1, foreground);
    }
    let bottom = y + height.saturating_sub(2);
    match underline {
        SnapshotUnderline::None => {}
        SnapshotUnderline::Single => {
            fill_rect(pixels, image_width, x, bottom, width, 1, underline_color);
        }
        SnapshotUnderline::Double => {
            fill_rect(
                pixels,
                image_width,
                x,
                bottom.saturating_sub(2),
                width,
                1,
                underline_color,
            );
            fill_rect(pixels, image_width, x, bottom, width, 1, underline_color);
        }
        SnapshotUnderline::Curly => {
            for offset in 0..width {
                let wave_y = bottom.saturating_sub(usize::from(offset % 4 >= 2));
                fill_rect(
                    pixels,
                    image_width,
                    x + offset,
                    wave_y,
                    1,
                    1,
                    underline_color,
                );
            }
        }
        SnapshotUnderline::Dotted => {
            for offset in (0..width).step_by(2) {
                fill_rect(
                    pixels,
                    image_width,
                    x + offset,
                    bottom,
                    1,
                    1,
                    underline_color,
                );
            }
        }
        SnapshotUnderline::Dashed => {
            for offset in (0..width).step_by(5) {
                fill_rect(
                    pixels,
                    image_width,
                    x + offset,
                    bottom,
                    3.min(width.saturating_sub(offset)),
                    1,
                    underline_color,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_cursor(
    pixels: &mut [u8],
    image_width: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    color: [u8; 3],
    style: CursorStyle,
) {
    match style {
        CursorStyle::Block => {}
        CursorStyle::BlockHollow => {
            fill_rect(pixels, image_width, x, y, width, 1, color);
            fill_rect(
                pixels,
                image_width,
                x,
                y + height.saturating_sub(1),
                width,
                1,
                color,
            );
            fill_rect(pixels, image_width, x, y, 1, height, color);
            fill_rect(
                pixels,
                image_width,
                x + width.saturating_sub(1),
                y,
                1,
                height,
                color,
            );
        }
        CursorStyle::Bar => {
            fill_rect(pixels, image_width, x, y, 2.min(width), height, color);
        }
        CursorStyle::Underline => {
            fill_rect(
                pixels,
                image_width,
                x,
                y + height.saturating_sub(2),
                width,
                2.min(height),
                color,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_glyph(
    pixels: &mut [u8],
    image_width: usize,
    image_height: usize,
    clip: (usize, usize, usize, usize),
    baseline: (i32, i32),
    glyph: &GlyphImage,
    color: [u8; 3],
    bold: bool,
    italic: bool,
) {
    let (clip_x, clip_y, clip_width, clip_height) = clip;
    let glyph_width = glyph.placement.width as usize;
    let glyph_height = glyph.placement.height as usize;
    let origin = (
        baseline.0 + glyph.placement.left,
        baseline.1 - glyph.placement.top,
    );
    for glyph_y in 0..glyph_height {
        let shear = if italic {
            (glyph_height.saturating_sub(glyph_y) / 5) as i32
        } else {
            0
        };
        for glyph_x in 0..glyph_width {
            let source_offset = glyph_y * glyph_width + glyph_x;
            let (glyph_color, alpha) = match glyph.content {
                GlyphContent::Mask => (color, glyph.data[source_offset]),
                GlyphContent::Color => {
                    let offset = source_offset * 4;
                    (
                        [
                            glyph.data[offset],
                            glyph.data[offset + 1],
                            glyph.data[offset + 2],
                        ],
                        glyph.data[offset + 3],
                    )
                }
                GlyphContent::SubpixelMask => {
                    let offset = source_offset * 4;
                    let alpha = glyph.data[offset..offset + 3]
                        .iter()
                        .copied()
                        .max()
                        .unwrap_or(0);
                    (color, alpha)
                }
            };
            if alpha == 0 {
                continue;
            }
            for bold_offset in 0..=usize::from(bold) {
                let x = origin.0 + glyph_x as i32 + shear + bold_offset as i32;
                let y = origin.1 + glyph_y as i32;
                if x < 0 || y < 0 {
                    continue;
                }
                let (x, y) = (x as usize, y as usize);
                if x >= image_width
                    || y >= image_height
                    || x < clip_x
                    || y < clip_y
                    || x >= clip_x + clip_width
                    || y >= clip_y + clip_height
                {
                    continue;
                }
                let offset = (y * image_width + x) * 3;
                for channel in 0..3 {
                    pixels[offset + channel] =
                        alpha_blend(pixels[offset + channel], glyph_color[channel], alpha);
                }
            }
        }
    }
}

fn alpha_blend(background: u8, foreground: u8, alpha: u8) -> u8 {
    let alpha = u16::from(alpha);
    let value = u16::from(foreground) * alpha + u16::from(background) * (255 - alpha);
    ((value + 127) / 255) as u8
}

fn fill_rect(
    pixels: &mut [u8],
    image_width: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    color: [u8; 3],
) {
    for row in y..y.saturating_add(height) {
        for col in x..x.saturating_add(width) {
            let offset = (row * image_width + col) * 3;
            if let Some(pixel) = pixels.get_mut(offset..offset + 3) {
                pixel.copy_from_slice(&color);
            }
        }
    }
}

fn blend(foreground: [u8; 3], background: [u8; 3], amount: f32) -> [u8; 3] {
    std::array::from_fn(|index| {
        (f32::from(foreground[index]) * amount + f32::from(background[index]) * (1.0 - amount))
            .round() as u8
    })
}

fn brighten(color: [u8; 3]) -> [u8; 3] {
    std::array::from_fn(|index| ((u16::from(color[index]) * 6 / 5).min(255)) as u8)
}

fn terminal_color(
    color: SnapshotColor,
    default: [u8; 3],
    palette: &qwertty_term_vt::color::Palette,
) -> [u8; 3] {
    match color {
        SnapshotColor::Default => default,
        SnapshotColor::Rgb { r, g, b } => [r, g, b],
        SnapshotColor::Palette(index) => {
            let color = palette[usize::from(index)];
            [color.r, color.g, color.b]
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;

    use anyhow::{Result, bail};
    use qwertty_term_vt::color::{DEFAULT as DEFAULT_PALETTE, Rgb};
    use qwertty_term_vt::screen::cursor::CursorStyle;
    use qwertty_term_vt::snapshot::{CellWidth, SnapshotColor, SnapshotUnderline};
    use qwertty_term_vt::stream::{Stream, TerminalHandler};
    use qwertty_term_vt::terminal::{Options, Terminal};
    use regex::Regex;
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::{
        CaptureAction, CaptureScript, KeyboardProbe, LaunchSpec, ProbeSession, SceneFilter,
        TerminalSession, command_steps, create_command_wrapper, draw_cursor, draw_decorations,
        encode_rgb_png, ensure_keyboard_is_live, key_sequence, terminal_color, validate_frame_name,
    };

    fn oracle() -> Value {
        serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/terminal-video-capture.json"
        ))
        .unwrap()
    }

    #[test]
    fn scene_filter_matches_the_pinned_capture_oracle() {
        let fixture = oracle();
        let all = SceneFilter::new(None);
        let selected = SceneFilter::new(Some(" review, pager ,,"));
        assert_eq!(
            json!([all.matches("review"), all.matches("anything")]),
            fixture["baselineOracle"]["allScenes"]
        );
        assert_eq!(
            json!([
                selected.matches("review"),
                selected.matches("pager"),
                selected.matches(""),
                selected.matches("other")
            ]),
            fixture["baselineOracle"]["selectedScenes"]
        );
        assert_eq!(fixture["baselineOracle"], fixture["stableOracle"]);
    }

    #[test]
    fn capture_script_uses_documented_camel_case_fields_for_every_nested_union() {
        let script: CaptureScript = serde_json::from_value(json!({
            "cols": 80,
            "rows": 24,
            "framesDir": "frames",
            "font": "font.ttf",
            "renderOptions": { "fontSize": 18.0, "devicePixelRatio": 2 },
            "scenes": [{
                "name": "shell",
                "launch": {
                    "kind": "shell",
                    "cwd": ".",
                    "pathPrepend": ["bin"]
                },
                "actions": [
                    { "kind": "waitText", "pattern": "ready", "timeoutMs": 12 },
                    { "kind": "typeCommand", "command": "workdeck", "snapAt": { "2": "mid" }, "delayMs": 4 }
                ]
            }]
        }))
        .unwrap();
        let LaunchSpec::Shell { path_prepend, .. } = &script.scenes[0].launch else {
            panic!("expected shell launch")
        };
        assert_eq!(path_prepend, &[PathBuf::from("bin")]);
        assert!(matches!(
            script.scenes[0].actions[0],
            CaptureAction::WaitText { timeout_ms: 12, .. }
        ));
        assert!(matches!(
            script.scenes[0].actions[1],
            CaptureAction::TypeCommand { delay_ms: 4, .. }
        ));
    }

    #[test]
    fn unicode_command_steps_and_snap_indexes_match_the_pinned_capture_oracle() {
        let fixture = oracle();
        let snaps = BTreeMap::from([(0, "a".to_owned()), (1, "emoji".to_owned())]);
        let steps = command_steps("a😀b", &snaps).collect::<Vec<_>>();
        let typed = steps
            .iter()
            .map(|(ch, _)| ch.to_string())
            .collect::<Vec<_>>();
        let snapped = steps
            .iter()
            .filter_map(|(_, snap)| snap.map(str::to_owned))
            .collect::<Vec<_>>();
        assert_eq!(json!(typed), fixture["baselineOracle"]["typed"]);
        assert_eq!(json!(snapped), fixture["baselineOracle"]["snapped"]);
    }

    #[derive(Default)]
    struct FakeProbe {
        presses: Vec<String>,
        waits: usize,
        succeeds_on: usize,
    }

    impl ProbeSession for FakeProbe {
        fn press_probe_key(&mut self, key: &str) -> Result<()> {
            self.presses.push(key.to_owned());
            Ok(())
        }

        fn wait_for_probe_text(&mut self, pattern: &Regex, timeout: Duration) -> Result<()> {
            assert_eq!(pattern.as_str(), "Help");
            assert_eq!(timeout, Duration::from_secs(2));
            self.waits += 1;
            if self.waits < self.succeeds_on {
                bail!("not yet")
            }
            Ok(())
        }
    }

    #[test]
    fn keyboard_liveness_retries_and_dismisses_like_the_pinned_capture_oracle() {
        let fixture = oracle();
        let mut session = FakeProbe {
            succeeds_on: 3,
            ..FakeProbe::default()
        };
        let probe = KeyboardProbe {
            probe_key: "?",
            expect: Regex::new("Help").unwrap(),
            dismiss_key: "escape",
            attempts: 5,
            timeout: Duration::from_secs(2),
            settle: Duration::ZERO,
        };
        ensure_keyboard_is_live(&mut session, &probe).unwrap();
        assert_eq!(json!(session.presses), fixture["baselineOracle"]["presses"]);
        assert_eq!(json!(session.waits), fixture["baselineOracle"]["waits"]);

        let mut never_live = FakeProbe {
            succeeds_on: usize::MAX,
            ..FakeProbe::default()
        };
        assert_eq!(
            ensure_keyboard_is_live(&mut never_live, &probe)
                .unwrap_err()
                .to_string(),
            "The app never reacted to a keypress."
        );
        assert_eq!(never_live.waits, 5);
    }

    #[test]
    fn unix_command_wrapper_matches_the_pinned_capture_oracle() {
        let fixture = oracle();
        let directory = tempdir().unwrap();
        let returned = create_command_wrapper(
            directory.path(),
            "demo",
            &["cargo".to_owned(), "run".to_owned(), "two words".to_owned()],
        )
        .unwrap();
        assert_eq!(returned, directory.path());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let wrapper = directory.path().join("demo");
            assert_eq!(
                fs::read_to_string(&wrapper).unwrap(),
                fixture["baselineOracle"]["wrapperText"].as_str().unwrap()
            );
            assert_eq!(
                fs::metadata(wrapper).unwrap().permissions().mode() & 0o777,
                fixture["baselineOracle"]["wrapperMode"].as_u64().unwrap() as u32
            );
        }

        #[cfg(windows)]
        assert_eq!(
            fs::read_to_string(directory.path().join("demo.cmd")).unwrap(),
            "@echo off\r\n\"cargo\" \"run\" \"two words\" %*\r\n"
        );
    }

    #[test]
    fn terminal_stream_preserves_graphemes_width_styles_palette_and_cursor() {
        let terminal = Terminal::new(Options {
            cols: 8,
            rows: 2,
            ..Options::default()
        });
        let mut stream = Stream::new(TerminalHandler::new(terminal));
        stream.feed("A\u{301}好".as_bytes());
        stream.feed(b"\x1b[1;2;3;4:3;7;8;9;53;38;2;1;2;3;48;5;4mZ\x1b[6 q");
        let snapshot = stream.terminal().snapshot();
        let row = &snapshot.visible_window(0)[0];
        assert_eq!(row.cells[0].ch, 'A');
        assert_eq!(row.cells[0].combining, vec!['\u{301}']);
        assert_eq!(row.cells[1].ch, '好');
        assert_eq!(row.cells[1].width, CellWidth::Wide);
        assert_eq!(row.cells[2].width, CellWidth::Spacer);
        let styled = &row.cells[3].style;
        assert!(styled.bold && styled.faint && styled.italic);
        assert!(styled.inverse && styled.invisible && styled.strikethrough && styled.overline);
        assert_eq!(styled.underline, SnapshotUnderline::Curly);
        assert_eq!(styled.fg, SnapshotColor::Rgb { r: 1, g: 2, b: 3 });
        assert_eq!(styled.bg, SnapshotColor::Palette(4));
        assert_eq!(snapshot.cursor.col, 4);
        assert_eq!(
            snapshot.cursor.style,
            qwertty_term_vt::screen::cursor::CursorStyle::Bar
        );
    }

    #[cfg(unix)]
    #[test]
    fn real_pty_round_trip_streams_input_and_output_into_owned_snapshots() {
        let working = tempdir().unwrap();
        let launch = LaunchSpec::App {
            command: PathBuf::from("/bin/sh"),
            args: vec![
                "-c".to_owned(),
                "printf 'READY '; IFS= read -r line; printf '\\nGOT:%s\\n' \"$line\"".to_owned(),
            ],
            cwd: working.path().to_path_buf(),
            env: BTreeMap::new(),
        };
        let mut session = TerminalSession::launch(&launch, 24, 4).unwrap();
        session
            .wait_for_text(&Regex::new("READY").unwrap(), Duration::from_secs(2))
            .unwrap();
        session.write_raw("héllo 😀\n").unwrap();
        session
            .wait_for_text(&Regex::new("GOT:héllo 😀").unwrap(), Duration::from_secs(2))
            .unwrap();
        let snapshot = session.screen().unwrap();
        assert_eq!(snapshot.cols, 24);
        assert_eq!(snapshot.rows, 4);
        assert!(
            snapshot
                .visible_window(0)
                .iter()
                .flat_map(|row| &row.cells)
                .any(|cell| cell.ch == '😀' && cell.width == CellWidth::Wide)
        );
        session.close().unwrap();
    }

    #[test]
    fn key_mapping_and_frame_names_reject_ambiguous_input() {
        assert_eq!(key_sequence("escape").unwrap(), b"\x1b");
        assert_eq!(key_sequence("ctrl+c").unwrap(), vec![3]);
        assert_eq!(key_sequence("😀").unwrap(), "😀".as_bytes());
        assert_eq!(
            key_sequence("ctrl+😀").unwrap_err().to_string(),
            "control key requires one ASCII character"
        );
        assert!(validate_frame_name("frame-01").is_ok());
        assert!(validate_frame_name("../frame").is_err());
        assert!(validate_frame_name("a/b").is_err());
    }

    #[test]
    fn native_pixel_helpers_honor_live_palettes_decorations_and_cursor_shapes() {
        let mut palette = DEFAULT_PALETTE;
        palette[42] = Rgb::new(7, 8, 9);
        assert_eq!(
            terminal_color(SnapshotColor::Palette(42), [1, 2, 3], &palette),
            [7, 8, 9]
        );
        assert_eq!(
            terminal_color(SnapshotColor::Rgb { r: 4, g: 5, b: 6 }, [1, 2, 3], &palette),
            [4, 5, 6]
        );

        let mut pixels = vec![0; 8 * 8 * 3];
        draw_decorations(
            &mut pixels,
            8,
            1,
            1,
            5,
            6,
            [11, 12, 13],
            [21, 22, 23],
            SnapshotUnderline::Double,
            true,
            true,
        );
        let pixel = |x: usize, y: usize| {
            let offset = (y * 8 + x) * 3;
            &pixels[offset..offset + 3]
        };
        assert_eq!(pixel(1, 1), [11, 12, 13]);
        assert_eq!(pixel(1, 3), [21, 22, 23]);
        assert_eq!(pixel(1, 5), [21, 22, 23]);

        pixels.fill(0);
        draw_cursor(
            &mut pixels,
            8,
            3,
            2,
            3,
            4,
            [31, 32, 33],
            CursorStyle::BlockHollow,
        );
        let offset = (2 * 8 + 3) * 3;
        assert_eq!(&pixels[offset..offset + 3], [31, 32, 33]);
        let offset = (3 * 8 + 4) * 3;
        assert_eq!(&pixels[offset..offset + 3], [0, 0, 0]);
    }

    #[test]
    fn rgb_png_encoder_checks_geometry_and_emits_a_decodable_png() {
        let png = encode_rgb_png(2, 1, &[1, 2, 3, 4, 5, 6]).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(
            encode_rgb_png(2, 1, &[1, 2, 3]).unwrap_err().to_string(),
            "capture RGB buffer has 3 bytes, expected 6"
        );
    }
}
