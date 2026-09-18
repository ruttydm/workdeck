//! Native terminal-video compositor backed by an external WebDriver and Chromium.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail, ensure};
use base64::Engine;
use serde_json::{Value, json};

use super::{
    FrameState, PlanOptions, Shot, ShotKind, StoryboardInput, plan_frames, required_keyframes,
    required_number, required_path, resolve,
};

const DEFAULT_STAGE: &str = include_str!("../../assets/terminal-stage.html");
const DEFAULT_WIDTH: u32 = 1920;
const DEFAULT_HEIGHT: u32 = 1080;
const DRIVER_START_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_WEBDRIVER_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
struct ComposeOptions {
    shots: Vec<Shot>,
    work_dir: PathBuf,
    root_dir: PathBuf,
    frames_dir: PathBuf,
    stage_path: Option<PathBuf>,
    font_path: Option<PathBuf>,
    chromium_path: Option<PathBuf>,
    webdriver_path: PathBuf,
    fps: f64,
    caption_animation_seconds: f64,
    viewport: Viewport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Viewport {
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, PartialEq)]
struct ComposeResult {
    unique_frames: usize,
    total_seconds: f64,
    concat_path: PathBuf,
    out_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
struct ConcatEntry {
    file: String,
    duration: f64,
}

pub fn compose_file(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut storyboard = None;
    let mut work_dir = None;
    let mut root_dir = None;
    let mut frames_dir = None;
    let mut stage_path = None;
    let mut font_path = None;
    let mut chromium_path = None;
    let mut webdriver_path = None;
    let mut fps = 30.0;
    let mut caption_animation_seconds = 0.45;
    let mut width = DEFAULT_WIDTH;
    let mut height = DEFAULT_HEIGHT;

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--storyboard" => storyboard = Some(required_path(&mut args, "--storyboard")?),
            "--work-dir" => work_dir = Some(required_path(&mut args, "--work-dir")?),
            "--root-dir" => root_dir = Some(required_path(&mut args, "--root-dir")?),
            "--frames-dir" => frames_dir = Some(required_path(&mut args, "--frames-dir")?),
            "--stage" => stage_path = Some(required_path(&mut args, "--stage")?),
            "--font" => font_path = Some(required_path(&mut args, "--font")?),
            "--chromium" => chromium_path = Some(required_path(&mut args, "--chromium")?),
            "--webdriver" => webdriver_path = Some(required_path(&mut args, "--webdriver")?),
            "--fps" => fps = required_number(&mut args, "--fps")?,
            "--caption-animation-seconds" => {
                caption_animation_seconds =
                    required_number(&mut args, "--caption-animation-seconds")?;
            }
            "--width" => width = required_u32(&mut args, "--width")?,
            "--height" => height = required_u32(&mut args, "--height")?,
            _ => bail!("unknown media compose option {argument:?}"),
        }
    }

    if !fps.is_finite() || fps <= 0.0 {
        bail!("--fps must be a positive finite number");
    }
    if !caption_animation_seconds.is_finite() || caption_animation_seconds < 0.0 {
        bail!("--caption-animation-seconds must be a non-negative finite number");
    }
    let storyboard = resolve(
        repo,
        storyboard.context("media compose requires --storyboard FILE")?,
    );
    let work_dir = resolve(
        repo,
        work_dir.context("media compose requires --work-dir DIR")?,
    );
    let source = fs::read_to_string(&storyboard)
        .with_context(|| format!("read storyboard {}", storyboard.display()))?;
    let input: StoryboardInput = serde_json::from_str(&source)
        .with_context(|| format!("parse storyboard {}", storyboard.display()))?;
    let shots = match input {
        StoryboardInput::Shots(shots) | StoryboardInput::Object { shots } => shots,
    };
    let root_dir = resolve(repo, root_dir.unwrap_or_else(|| repo.to_path_buf()));
    let frames_dir = resolve(repo, frames_dir.unwrap_or_else(|| work_dir.join("frames")));
    let options = ComposeOptions {
        shots,
        work_dir,
        root_dir,
        frames_dir,
        stage_path: stage_path.map(|path| resolve(repo, path)),
        font_path: font_path.map(|path| resolve(repo, path)),
        chromium_path: resolve_chromium(chromium_path.map(|path| resolve(repo, path))),
        webdriver_path: webdriver_path
            .map(|path| resolve_executable(repo, path))
            .or_else(|| std::env::var_os("WEBDRIVER_PATH").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("chromedriver")),
        fps,
        caption_animation_seconds,
        viewport: Viewport { width, height },
    };

    ensure_output_and_keyframes(&options)?;
    let mut renderer = WebDriverRenderer::launch(
        &options.webdriver_path,
        options.chromium_path.as_deref(),
        options.viewport,
    )?;
    let result = compose_storyboard(&options, &mut renderer, |message| println!("{message}"))?;
    renderer.close()?;
    println!(
        "composited {} unique frames across {:.6} seconds in {} -> {}",
        result.unique_frames,
        result.total_seconds,
        result.out_dir.display(),
        result.concat_path.display()
    );
    Ok(())
}

fn required_u32(args: &mut impl Iterator<Item = String>, option: &str) -> Result<u32> {
    let value = args
        .next()
        .with_context(|| format!("{option} requires an integer"))?;
    let parsed: u32 = value
        .parse()
        .with_context(|| format!("{option} requires a positive integer, got {value:?}"))?;
    if parsed == 0 {
        bail!("{option} requires a positive integer, got {value:?}");
    }
    Ok(parsed)
}

fn resolve_executable(repo: &Path, path: PathBuf) -> PathBuf {
    if path.components().count() == 1 {
        path
    } else {
        resolve(repo, path)
    }
}

fn resolve_chromium(explicit: Option<PathBuf>) -> Option<PathBuf> {
    resolve_chromium_from(
        explicit,
        std::env::var_os("CHROMIUM_PATH").map(PathBuf::from),
        Path::new("/opt/pw-browsers/chromium").is_file(),
    )
}

fn resolve_chromium_from(
    explicit: Option<PathBuf>,
    environment: Option<PathBuf>,
    sandbox_chromium_exists: bool,
) -> Option<PathBuf> {
    explicit
        .or(environment)
        .or_else(|| sandbox_chromium_exists.then(|| PathBuf::from("/opt/pw-browsers/chromium")))
}

fn find_caption_font(root_dir: &Path) -> Result<PathBuf> {
    let candidates = [
        root_dir.join("share/workdeck/fonts/jetbrains-mono-nerd.ttf"),
        root_dir.join("assets/fonts/jetbrains-mono-nerd.ttf"),
    ];
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "caption font not found; searched:\n{}",
                candidates
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        })
}

trait FrameRenderer {
    fn screenshot(&mut self, stage_url: &str, output: &Path) -> Result<()>;
}

pub(crate) fn capture_card_documents(
    driver: &Path,
    chromium: &Path,
    documents: &[String],
) -> Result<tempfile::TempDir> {
    ensure!(!documents.is_empty(), "no social-card documents to capture");
    let viewport = Viewport {
        width: 1200,
        height: 630,
    };
    let mut renderer = WebDriverRenderer::launch(driver, Some(chromium), viewport)?;
    let staged = stage_card_documents(documents, &mut renderer)?;
    renderer.close()?;
    Ok(staged)
}

fn stage_card_documents(
    documents: &[String],
    renderer: &mut impl FrameRenderer,
) -> Result<tempfile::TempDir> {
    let staged = tempfile::Builder::new()
        .prefix("workdeck-social-cards-")
        .tempdir()?;
    for (index, html) in documents.iter().enumerate() {
        let document = staged.path().join(format!("{index:04}.html"));
        let image = staged.path().join(format!("{index:04}.png"));
        fs::write(&document, html)?;
        renderer.screenshot(&file_url(&document)?, &image)?;
        validate_card_png(&image)?;
        fs::remove_file(document)?;
    }
    Ok(staged)
}

pub(crate) fn validate_card_png(path: &Path) -> Result<()> {
    validate_card_png_bytes(&fs::read(path)?)
}

pub(crate) fn validate_card_png_bytes(bytes: &[u8]) -> Result<()> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info()?;
    ensure!(
        reader.info().width == 1200 && reader.info().height == 630,
        "social-card screenshot must be 1200x630"
    );
    let mut pixels = vec![
        0;
        reader
            .output_buffer_size()
            .context("social-card PNG too large")?
    ];
    reader.next_frame(&mut pixels)?;
    Ok(())
}

fn compose_storyboard(
    options: &ComposeOptions,
    renderer: &mut impl FrameRenderer,
    mut log: impl FnMut(String),
) -> Result<ComposeResult> {
    ensure_output_and_keyframes(options)?;
    let out_dir = options.work_dir.join("out");

    let font_path = options
        .font_path
        .clone()
        .map(Ok)
        .unwrap_or_else(|| find_caption_font(&options.root_dir))?;
    if !font_path.is_file() {
        bail!("caption font does not exist: {}", font_path.display());
    }
    let template = options
        .stage_path
        .as_ref()
        .map(|path| {
            fs::read_to_string(path)
                .with_context(|| format!("read terminal-video stage {}", path.display()))
        })
        .transpose()?
        .unwrap_or_else(|| DEFAULT_STAGE.to_owned());
    validate_stage_template(&template)?;

    let plan = plan_frames(
        &options.shots,
        PlanOptions {
            fps: options.fps,
            caption_animation_seconds: options.caption_animation_seconds,
        },
    );
    if plan.frames.is_empty() {
        bail!("terminal-video storyboard must contain at least one shot");
    }
    let stage_path = options.work_dir.join("stage-built.html");
    let font_url = file_url(&font_path)?;
    let mut backgrounds: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut entries = Vec::with_capacity(plan.frames.len());

    for (index, frame) in plan.frames.iter().enumerate() {
        let (image_url, background) = if frame.state.kind == ShotKind::Term {
            let image_name = frame
                .state
                .img
                .as_deref()
                .context("terminal frame state is missing its image name")?;
            let image_path = options.frames_dir.join(format!("{image_name}.png"));
            let background = if let Some(background) = backgrounds.get(&image_path) {
                background.clone()
            } else {
                let background = sample_background(&image_path)?;
                backgrounds.insert(image_path.clone(), background.clone());
                background
            };
            (Some(file_url(&image_path)?), background)
        } else {
            (None, "#10141f".to_owned())
        };
        let html = render_stage(
            &template,
            &frame.state,
            &font_url,
            image_url.as_deref(),
            &background,
        )?;
        fs::write(&stage_path, html)
            .with_context(|| format!("write built stage {}", stage_path.display()))?;
        let mut stage_url = url::Url::from_file_path(&stage_path).map_err(|()| {
            anyhow!(
                "cannot convert stage path to file URL: {}",
                stage_path.display()
            )
        })?;
        stage_url.set_query(Some(&format!("frame={index}")));
        let file = format!("f{index:04}.png");
        renderer.screenshot(stage_url.as_str(), &out_dir.join(&file))?;
        entries.push(ConcatEntry {
            file,
            duration: frame.duration,
        });
        if index % 25 == 0 || index == plan.frames.len() - 1 {
            log(format!("frame {}/{}", index + 1, plan.frames.len()));
        }
    }

    let concat_path = options.work_dir.join("concat.txt");
    fs::write(&concat_path, render_concat(&out_dir, &entries)?)
        .with_context(|| format!("write ffconcat list {}", concat_path.display()))?;
    Ok(ComposeResult {
        unique_frames: entries.len(),
        total_seconds: plan.total_seconds,
        concat_path,
        out_dir,
    })
}

fn ensure_output_and_keyframes(options: &ComposeOptions) -> Result<()> {
    let out_dir = options.work_dir.join("out");
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("create compositor output {}", out_dir.display()))?;
    let missing = required_keyframes(&options.shots)
        .into_iter()
        .filter(|name| !options.frames_dir.join(format!("{name}.png")).is_file())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(
            "missing keyframes in {}:\n{}\nrun the capture command first (optionally --scenes NAME for a partial recapture)",
            options.frames_dir.display(),
            missing
                .iter()
                .map(|name| format!("  {name}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    Ok(())
}

fn validate_stage_template(template: &str) -> Result<()> {
    const TOKENS: [&str; 11] = [
        "__WORKDECK_FONT_URL__",
        "__WORKDECK_TERM_DISPLAY__",
        "__WORKDECK_SHOT_OPACITY__",
        "__WORKDECK_TERM_SCALE__",
        "__WORKDECK_TERM_TITLE__",
        "__WORKDECK_TERM_BACKGROUND__",
        "__WORKDECK_TERM_IMAGE__",
        "__WORKDECK_CAPTION_OPACITY__",
        "__WORKDECK_CAPTION_Y__",
        "__WORKDECK_CAPTION__",
        "__WORKDECK_CARD_DISPLAY__",
    ];
    for token in TOKENS {
        if !template.contains(token) {
            bail!("terminal-video stage is missing required marker {token}");
        }
    }
    if !template.contains("__WORKDECK_CARD_SCALE__") || !template.contains("__WORKDECK_CARD__") {
        bail!("terminal-video stage is missing required card markers");
    }
    Ok(())
}

fn render_stage(
    template: &str,
    state: &FrameState,
    font_url: &str,
    image_url: Option<&str>,
    background: &str,
) -> Result<String> {
    validate_stage_template(template)?;
    let shot_t = ease_out(state.shot_t);
    let cap_t = ease_out(state.cap_t);
    let is_term = state.kind == ShotKind::Term;
    let caption = state
        .caption
        .as_deref()
        .filter(|caption| !caption.is_empty())
        .map(|caption| format!("<span class=\"line\">{caption}</span>"))
        .unwrap_or_default();
    let values = [
        ("__WORKDECK_FONT_URL__", escape_html(font_url)),
        (
            "__WORKDECK_TERM_DISPLAY__",
            if is_term { "block" } else { "none" }.to_owned(),
        ),
        ("__WORKDECK_SHOT_OPACITY__", shot_t.to_string()),
        (
            "__WORKDECK_TERM_SCALE__",
            (0.975 + 0.025 * shot_t).to_string(),
        ),
        (
            "__WORKDECK_TERM_TITLE__",
            escape_html(state.title.as_deref().unwrap_or("workdeck")),
        ),
        ("__WORKDECK_TERM_BACKGROUND__", background.to_owned()),
        (
            "__WORKDECK_TERM_IMAGE__",
            escape_html(image_url.unwrap_or_default()),
        ),
        ("__WORKDECK_CAPTION_OPACITY__", cap_t.to_string()),
        ("__WORKDECK_CAPTION_Y__", ((1.0 - cap_t) * 26.0).to_string()),
        ("__WORKDECK_CAPTION__", caption),
        (
            "__WORKDECK_CARD_DISPLAY__",
            if is_term { "none" } else { "flex" }.to_owned(),
        ),
        (
            "__WORKDECK_CARD_SCALE__",
            (0.965 + 0.035 * shot_t).to_string(),
        ),
        ("__WORKDECK_CARD__", state.html.clone().unwrap_or_default()),
    ];
    let mut rendered = template.to_owned();
    for (marker, value) in values {
        rendered = rendered.replace(marker, &value);
    }
    if rendered.contains("__WORKDECK_") {
        bail!("terminal-video stage contains an unknown Workdeck marker");
    }
    Ok(rendered)
}

fn ease_out(value: f64) -> f64 {
    1.0 - (1.0 - value.clamp(0.0, 1.0)).powi(4)
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn file_url(path: &Path) -> Result<String> {
    url::Url::from_file_path(path)
        .map(String::from)
        .map_err(|()| anyhow!("cannot convert path to file URL: {}", path.display()))
}

fn sample_background(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("open keyframe {}", path.display()))?;
    let mut decoder = png::Decoder::new(BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .with_context(|| format!("decode PNG header {}", path.display()))?;
    let size = reader
        .output_buffer_size()
        .context("decoded PNG exceeds addressable memory")?;
    let mut buffer = vec![0; size];
    let info = reader
        .next_frame(&mut buffer)
        .with_context(|| format!("decode PNG pixels {}", path.display()))?;
    if info.width == 0 || info.height == 0 {
        bail!("keyframe PNG has empty geometry: {}", path.display());
    }
    let x = 6_u32.min(info.width - 1) as usize;
    let y = info.height.saturating_sub(6) as usize;
    let samples = info.color_type.samples();
    let offset = y
        .checked_mul(info.line_size)
        .and_then(|row| row.checked_add(x * samples))
        .context("keyframe PNG sample offset overflow")?;
    let pixels = &buffer[..info.buffer_size()];
    let (red, green, blue) = match info.color_type {
        png::ColorType::Rgb | png::ColorType::Rgba => (
            *pixels.get(offset).context("missing red PNG sample")?,
            *pixels.get(offset + 1).context("missing green PNG sample")?,
            *pixels.get(offset + 2).context("missing blue PNG sample")?,
        ),
        png::ColorType::Grayscale | png::ColorType::GrayscaleAlpha => {
            let value = *pixels.get(offset).context("missing grayscale PNG sample")?;
            (value, value, value)
        }
        png::ColorType::Indexed => bail!("indexed PNG remained unexpanded: {}", path.display()),
    };
    Ok(format!("rgb({red}, {green}, {blue})"))
}

fn render_concat(out_dir: &Path, entries: &[ConcatEntry]) -> Result<String> {
    let last = entries
        .last()
        .context("cannot write ffconcat list without frames")?;
    let mut lines = vec!["ffconcat version 1.0".to_owned()];
    for entry in entries {
        lines.push(format!("file '{}'", out_dir.join(&entry.file).display()));
        lines.push(format!("duration {:.5}", entry.duration));
    }
    lines.push(format!("file '{}'", out_dir.join(&last.file).display()));
    Ok(format!("{}\n", lines.join("\n")))
}

struct WebDriverRenderer {
    child: Child,
    port: u16,
    session_id: String,
    closed: bool,
}

impl WebDriverRenderer {
    fn launch(driver: &Path, chromium: Option<&Path>, viewport: Viewport) -> Result<Self> {
        let port = reserve_loopback_port()?;
        let mut child = Command::new(driver)
            .arg(format!("--port={port}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("launch WebDriver {}", driver.display()))?;
        let setup: Result<String> = (|| {
            wait_for_driver(&mut child, port, DRIVER_START_TIMEOUT)?;
            let capabilities = chrome_capabilities(chromium, viewport);
            let response = webdriver_request(port, "POST", "/session", Some(&capabilities))?;
            let session_id = response
                .pointer("/value/sessionId")
                .or_else(|| response.get("sessionId"))
                .and_then(Value::as_str)
                .context("WebDriver session response has no sessionId")?
                .to_owned();
            let path = format!("/session/{session_id}/window/rect");
            webdriver_request(
                port,
                "POST",
                &path,
                Some(&json!({ "width": viewport.width, "height": viewport.height })),
            )?;
            let metrics = format!("/session/{session_id}/goog/cdp/execute");
            webdriver_request(
                port,
                "POST",
                &metrics,
                Some(&device_metrics_override(viewport)),
            )?;
            Ok(session_id)
        })();
        match setup {
            Ok(session_id) => Ok(Self {
                child,
                port,
                session_id,
                closed: false,
            }),
            Err(error) => {
                terminate_child(&mut child);
                Err(error)
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let path = format!("/session/{}", self.session_id);
        let request_result = webdriver_request(self.port, "DELETE", &path, None).map(|_| ());
        terminate_child(&mut self.child);
        request_result.context("close WebDriver browser session")
    }
}

impl FrameRenderer for WebDriverRenderer {
    fn screenshot(&mut self, stage_url: &str, output: &Path) -> Result<()> {
        let navigate = format!("/session/{}/url", self.session_id);
        webdriver_request(
            self.port,
            "POST",
            &navigate,
            Some(&json!({ "url": stage_url })),
        )?;
        let paint = format!("/session/{}/execute/async", self.session_id);
        let painted = webdriver_request(
            self.port,
            "POST",
            &paint,
            Some(&json!({
                "script": "const done = arguments[arguments.length - 1]; document.fonts.ready.then(() => requestAnimationFrame(() => requestAnimationFrame(() => done(true))), (error) => done(String(error)));",
                "args": []
            })),
        )?;
        require_paint_ready(&painted)?;
        let screenshot = format!("/session/{}/screenshot", self.session_id);
        let response = webdriver_request(self.port, "GET", &screenshot, None)?;
        let encoded = response
            .get("value")
            .and_then(Value::as_str)
            .context("WebDriver screenshot response is not base64 text")?;
        let png = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .context("decode WebDriver screenshot")?;
        fs::write(output, png)
            .with_context(|| format!("write composited frame {}", output.display()))
    }
}

fn require_paint_ready(response: &Value) -> Result<()> {
    ensure!(
        response.get("value") == Some(&Value::Bool(true)),
        "browser did not confirm font readiness and paint completion: {}",
        response.get("value").unwrap_or(&Value::Null)
    );
    Ok(())
}

impl Drop for WebDriverRenderer {
    fn drop(&mut self) {
        if !self.closed {
            let path = format!("/session/{}", self.session_id);
            let _ = webdriver_request(self.port, "DELETE", &path, None);
            terminate_child(&mut self.child);
            self.closed = true;
        }
    }
}

fn terminate_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
}

fn reserve_loopback_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).context("reserve WebDriver port")?;
    listener
        .local_addr()
        .map(|address| address.port())
        .context("read reserved WebDriver port")
}

fn wait_for_driver(child: &mut Child, port: u16, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().context("poll WebDriver process")? {
            bail!("WebDriver exited during startup with {status}");
        }
        if webdriver_request(port, "GET", "/status", None).is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            terminate_child(child);
            bail!("WebDriver did not become ready within {timeout:?}");
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn chrome_capabilities(chromium: Option<&Path>, viewport: Viewport) -> Value {
    let mut options = serde_json::Map::new();
    options.insert(
        "args".into(),
        json!([
            "--headless=new",
            "--allow-file-access-from-files",
            "--hide-scrollbars",
            format!("--window-size={},{}", viewport.width, viewport.height),
            "--force-device-scale-factor=1"
        ]),
    );
    if let Some(chromium) = chromium {
        options.insert("binary".into(), json!(chromium));
    }
    json!({
        "capabilities": {
            "alwaysMatch": {
                "browserName": "chrome",
                "goog:chromeOptions": options
            }
        }
    })
}

fn device_metrics_override(viewport: Viewport) -> Value {
    json!({
        "cmd": "Emulation.setDeviceMetricsOverride",
        "params": {
            "width": viewport.width,
            "height": viewport.height,
            "deviceScaleFactor": 1,
            "mobile": false
        }
    })
}

fn webdriver_request(port: u16, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
    let encoded = body
        .map(serde_json::to_vec)
        .transpose()
        .context("encode WebDriver request")?
        .unwrap_or_default();
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .with_context(|| format!("connect to WebDriver on port {port}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .context("set WebDriver read timeout")?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        encoded.len()
    )
    .context("write WebDriver request headers")?;
    stream
        .write_all(&encoded)
        .context("write WebDriver request body")?;
    let response = read_webdriver_response(&mut stream)?;
    parse_http_json(&response)
}

fn read_webdriver_response(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut response = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = stream
            .read(&mut buffer)
            .context("read WebDriver response")?;
        if count == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..count]);
        if response.len() > MAX_WEBDRIVER_RESPONSE_BYTES {
            bail!(
                "WebDriver response exceeds the {} byte limit",
                MAX_WEBDRIVER_RESPONSE_BYTES
            );
        }
        if let Some(length) = webdriver_response_length(&response)? {
            response.truncate(length);
            return Ok(response);
        }
    }
    if response.is_empty() {
        bail!("WebDriver closed the connection without a response");
    }
    Ok(response)
}

fn webdriver_response_length(response: &[u8]) -> Result<Option<usize>> {
    let Some(separator) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Ok(None);
    };
    let headers = std::str::from_utf8(&response[..separator]).context("WebDriver HTTP headers")?;
    let body_start = separator + 4;
    if headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("transfer-encoding")
                && value
                    .split(',')
                    .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
        })
    }) {
        return chunked_message_length(&response[body_start..])
            .map(|length| length.map(|length| body_start + length));
    }
    let content_length = headers.lines().find_map(|line| {
        line.split_once(':').and_then(|(name, value)| {
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim())
        })
    });
    let Some(content_length) = content_length else {
        return Ok(None);
    };
    let content_length = content_length
        .parse::<usize>()
        .context("WebDriver response has invalid Content-Length")?;
    let total = body_start
        .checked_add(content_length)
        .context("WebDriver response length overflow")?;
    Ok((response.len() >= total).then_some(total))
}

fn chunked_message_length(body: &[u8]) -> Result<Option<usize>> {
    let mut offset = 0;
    loop {
        let Some(line_end) = body[offset..]
            .windows(2)
            .position(|window| window == b"\r\n")
        else {
            return Ok(None);
        };
        let size_text = std::str::from_utf8(&body[offset..offset + line_end])
            .context("invalid chunk size text")?
            .split(';')
            .next()
            .unwrap_or_default();
        let size = usize::from_str_radix(size_text.trim(), 16).context("invalid chunk size")?;
        offset += line_end + 2;
        if size == 0 {
            if body.get(offset..offset + 2) == Some(b"\r\n") {
                return Ok(Some(offset + 2));
            }
            let trailer_end = body[offset..]
                .windows(4)
                .position(|window| window == b"\r\n\r\n");
            return Ok(trailer_end.map(|end| offset + end + 4));
        }
        let Some(data_end) = offset.checked_add(size) else {
            bail!("WebDriver chunk length overflow");
        };
        if body.len() < data_end + 2 {
            return Ok(None);
        }
        if &body[data_end..data_end + 2] != b"\r\n" {
            bail!("invalid chunked WebDriver response delimiter");
        }
        offset = data_end + 2;
    }
}

fn parse_http_json(response: &[u8]) -> Result<Value> {
    let separator = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .context("WebDriver response has no HTTP header terminator")?;
    let headers = std::str::from_utf8(&response[..separator]).context("WebDriver HTTP headers")?;
    let status: u16 = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .context("WebDriver response has no HTTP status")?
        .parse()
        .context("WebDriver response has invalid HTTP status")?;
    let raw_body = &response[separator + 4..];
    let body = if headers.lines().any(|line| {
        line.eq_ignore_ascii_case("transfer-encoding: chunked")
            || line
                .to_ascii_lowercase()
                .starts_with("transfer-encoding: chunked;")
    }) {
        decode_chunked(raw_body)?
    } else {
        raw_body.to_vec()
    };
    let value: Value = serde_json::from_slice(&body).context("decode WebDriver JSON response")?;
    if !(200..300).contains(&status) {
        let message = value
            .pointer("/value/message")
            .and_then(Value::as_str)
            .unwrap_or("unknown WebDriver error");
        bail!("WebDriver HTTP {status}: {message}");
    }
    Ok(value)
}

fn decode_chunked(mut body: &[u8]) -> Result<Vec<u8>> {
    let mut decoded = Vec::new();
    loop {
        let line_end = body
            .windows(2)
            .position(|window| window == b"\r\n")
            .context("invalid chunked WebDriver response")?;
        let size_text = std::str::from_utf8(&body[..line_end])
            .context("invalid chunk size text")?
            .split(';')
            .next()
            .unwrap_or_default();
        let size = usize::from_str_radix(size_text.trim(), 16).context("invalid chunk size")?;
        body = &body[line_end + 2..];
        if size == 0 {
            break;
        }
        if body.len() < size + 2 || &body[size..size + 2] != b"\r\n" {
            bail!("truncated chunked WebDriver response");
        }
        decoded.extend_from_slice(&body[..size]);
        body = &body[size + 2..];
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use std::io::{BufWriter, Read as _, Write as _};
    use std::sync::{Arc, Mutex};

    use super::*;

    const ORACLE: &str = include_str!("../../../port/hunk/oracles/terminal-video-compose.json");

    fn term(name: &str) -> Shot {
        Shot {
            kind: ShotKind::Term,
            dur: 0.5,
            html: None,
            img: Some(name.into()),
            title: Some("demo".into()),
            caption: None,
            cap_key: None,
            enter: false,
        }
    }

    fn options(directory: &tempfile::TempDir, shots: Vec<Shot>) -> ComposeOptions {
        ComposeOptions {
            shots,
            work_dir: directory.path().join("work"),
            root_dir: directory.path().to_path_buf(),
            frames_dir: directory.path().join("frames"),
            stage_path: None,
            font_path: Some(directory.path().join("font.ttf")),
            chromium_path: None,
            webdriver_path: PathBuf::from("unused-chromedriver"),
            fps: 2.0,
            caption_animation_seconds: 0.0,
            viewport: Viewport {
                width: DEFAULT_WIDTH,
                height: DEFAULT_HEIGHT,
            },
        }
    }

    fn write_png(path: &Path, width: u32, height: u32, color: [u8; 3]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = File::create(path).unwrap();
        let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let pixels = color.repeat((width * height) as usize);
        writer.write_image_data(&pixels).unwrap();
    }

    #[test]
    fn capture_requires_explicit_font_and_paint_success() {
        require_paint_ready(&json!({"value":true})).unwrap();
        for response in [
            json!({}),
            json!({"value":null}),
            json!({"value":false}),
            json!({"value":"true"}),
            json!({"value":"font loading failed"}),
        ] {
            assert!(require_paint_ready(&response).is_err(), "{response}");
        }
    }

    #[test]
    fn social_card_staging_validates_geometry_and_cleans_partial_capture() {
        struct Renderer {
            outputs: Vec<PathBuf>,
            wrong_size: bool,
            fail_second: bool,
        }
        impl FrameRenderer for Renderer {
            fn screenshot(&mut self, url: &str, output: &Path) -> Result<()> {
                assert!(url.starts_with("file://"));
                self.outputs.push(output.to_owned());
                if self.fail_second && self.outputs.len() == 2 {
                    bail!("injected capture failure");
                }
                write_png(
                    output,
                    if self.wrong_size { 1199 } else { 1200 },
                    630,
                    [10, 20, 30],
                );
                Ok(())
            }
        }
        let documents = vec!["<html>one</html>".into(), "<html>two</html>".into()];
        for (wrong_size, fail_second) in [(false, false), (true, false), (false, true)] {
            let mut renderer = Renderer {
                outputs: vec![],
                wrong_size,
                fail_second,
            };
            let result = stage_card_documents(&documents, &mut renderer);
            if wrong_size || fail_second {
                assert!(result.is_err());
                for path in renderer.outputs {
                    assert!(!path.parent().unwrap().exists());
                }
            } else {
                let staged = result.unwrap();
                assert_eq!(fs::read_dir(staged.path()).unwrap().count(), 2);
                assert!(staged.path().join("0000.png").is_file());
                assert!(staged.path().join("0001.png").is_file());
                assert!(!staged.path().join("0000.html").exists());
            }
        }
    }

    #[test]
    fn frozen_hunk_oracle_covers_baseline_stable_helpers_and_fail_fast_order() {
        let oracle: Value = serde_json::from_str(ORACLE).unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["baselineOracle"], oracle["stableOracle"]);
        assert_eq!(oracle["baselineOracle"]["stageBasename"], "stage.html");
        assert_eq!(oracle["baselineOracle"]["firstFontIsIsolated"], true);
        assert_eq!(
            oracle["baselineOracle"]["outCreatedBeforeMissingCheck"],
            true
        );
        assert_eq!(oracle["baselineOracle"]["missingFrames"][1], "  one");
        assert_eq!(oracle["baselineOracle"]["missingFrames"][2], "  two");
    }

    #[test]
    fn chromium_resolution_preserves_explicit_environment_and_sandbox_precedence() {
        assert_eq!(
            resolve_chromium_from(
                Some(PathBuf::from("explicit")),
                Some(PathBuf::from("environment")),
                true,
            ),
            Some(PathBuf::from("explicit"))
        );
        assert_eq!(
            resolve_chromium_from(None, Some(PathBuf::from("environment")), true),
            Some(PathBuf::from("environment"))
        );
        assert_eq!(
            resolve_chromium_from(None, None, true),
            Some(PathBuf::from("/opt/pw-browsers/chromium"))
        );
        assert_eq!(resolve_chromium_from(None, None, false), None);
    }

    #[test]
    fn native_caption_font_discovery_is_packaged_first_and_names_all_failures() {
        let directory = tempfile::TempDir::new().unwrap();
        let packaged = directory
            .path()
            .join("share/workdeck/fonts/jetbrains-mono-nerd.ttf");
        let source = directory
            .path()
            .join("assets/fonts/jetbrains-mono-nerd.ttf");
        fs::create_dir_all(packaged.parent().unwrap()).unwrap();
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, "source").unwrap();
        assert_eq!(find_caption_font(directory.path()).unwrap(), source);
        fs::write(&packaged, "packaged").unwrap();
        assert_eq!(find_caption_font(directory.path()).unwrap(), packaged);

        let missing = tempfile::TempDir::new().unwrap();
        let error = find_caption_font(missing.path()).unwrap_err().to_string();
        assert!(error.starts_with("caption font not found; searched:\n"));
        assert!(error.contains("share/workdeck/fonts/jetbrains-mono-nerd.ttf"));
        assert!(error.contains("assets/fonts/jetbrains-mono-nerd.ttf"));
    }

    #[test]
    fn missing_keyframes_are_deduplicated_and_fail_before_external_processes() {
        let directory = tempfile::TempDir::new().unwrap();
        let options = options(&directory, vec![term("one"), term("two"), term("one")]);
        let error = ensure_output_and_keyframes(&options)
            .unwrap_err()
            .to_string();
        assert!(options.work_dir.join("out").is_dir());
        assert_eq!(error.matches("  one").count(), 1);
        assert_eq!(error.matches("  two").count(), 1);
        assert!(error.contains("run the capture command first"));
    }

    #[test]
    fn compose_cli_reports_missing_frames_without_launching_its_declared_driver() {
        let directory = tempfile::TempDir::new().unwrap();
        let storyboard = directory.path().join("storyboard.json");
        fs::write(
            &storyboard,
            r#"[{"kind":"term","dur":1,"img":"absent","title":"demo"}]"#,
        )
        .unwrap();
        let work_dir = directory.path().join("work");
        let error = compose_file(
            directory.path(),
            [
                "--storyboard".into(),
                storyboard.to_string_lossy().into_owned(),
                "--work-dir".into(),
                work_dir.to_string_lossy().into_owned(),
                "--webdriver".into(),
                "definitely-not-a-real-webdriver".into(),
            ]
            .into_iter(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("missing keyframes"));
        assert!(!error.contains("launch WebDriver"));
        assert!(work_dir.join("out").is_dir());
    }

    #[test]
    fn stage_is_static_and_preserves_easing_raw_card_caption_and_escaped_text() {
        assert!(!DEFAULT_STAGE.contains("<script"));
        assert!(DEFAULT_STAGE.contains("radial-gradient(1200px 800px"));
        let state = FrameState {
            kind: ShotKind::Term,
            html: None,
            img: Some("frame".into()),
            title: Some("a<&\"'".into()),
            caption: Some("<span class=\"badge\">NEW</span> hello".into()),
            shot_t: 0.0,
            cap_t: 0.5,
        };
        let rendered = render_stage(
            DEFAULT_STAGE,
            &state,
            "file:///font.ttf",
            Some("file:///frame.png?x=1&y=2"),
            "rgb(1, 2, 3)",
        )
        .unwrap();
        assert!(rendered.contains("display: block; opacity: 0;"));
        assert!(rendered.contains("scale(0.975)"));
        assert!(rendered.contains("a&lt;&amp;&quot;&#39;"));
        assert!(rendered.contains("file:///frame.png?x=1&amp;y=2"));
        assert!(rendered.contains("rgb(1, 2, 3)"));
        assert!(
            rendered.contains("<span class=\"line\"><span class=\"badge\">NEW</span> hello</span>")
        );
        assert!(rendered.contains("opacity: 0.9375"));
        assert!(rendered.contains("translateY(1.625px)"));
        assert!(!rendered.contains("__WORKDECK_"));

        let card = FrameState {
            kind: ShotKind::Card,
            html: Some("<h1>Raw card</h1>".into()),
            img: None,
            title: None,
            caption: None,
            shot_t: 1.0,
            cap_t: 1.0,
        };
        let rendered = render_stage(DEFAULT_STAGE, &card, "font", None, "#10141f").unwrap();
        assert!(rendered.contains("display: none; opacity: 1;"));
        assert!(rendered.contains("display: flex; opacity: 1;"));
        assert!(rendered.contains("<h1>Raw card</h1>"));
    }

    #[test]
    fn png_background_sampling_matches_the_bottom_left_canvas_coordinate() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("frame.png");
        write_png(&path, 12, 12, [17, 34, 51]);
        assert_eq!(sample_background(&path).unwrap(), "rgb(17, 34, 51)");
    }

    #[test]
    fn ffconcat_repeats_the_last_file_and_rounds_durations_to_five_decimals() {
        let rendered = render_concat(
            Path::new("/tmp/out"),
            &[
                ConcatEntry {
                    file: "f0000.png".into(),
                    duration: 1.0 / 30.0,
                },
                ConcatEntry {
                    file: "f0001.png".into(),
                    duration: 1.25,
                },
            ],
        )
        .unwrap();
        assert_eq!(
            rendered,
            "ffconcat version 1.0\nfile '/tmp/out/f0000.png'\nduration 0.03333\nfile '/tmp/out/f0001.png'\nduration 1.25000\nfile '/tmp/out/f0001.png'\n"
        );
        assert!(render_concat(Path::new("/tmp/out"), &[]).is_err());
    }

    #[derive(Default)]
    struct FakeRenderer {
        html: Vec<String>,
        outputs: Vec<PathBuf>,
    }

    impl FrameRenderer for FakeRenderer {
        fn screenshot(&mut self, stage_url: &str, output: &Path) -> Result<()> {
            let url = url::Url::parse(stage_url)?;
            let path = url
                .to_file_path()
                .map_err(|()| anyhow!("fake stage URL is not a file"))?;
            self.html.push(fs::read_to_string(path)?);
            fs::write(output, b"fake png")?;
            self.outputs.push(output.to_path_buf());
            Ok(())
        }
    }

    #[test]
    fn compositor_writes_every_unique_frame_progress_and_consumable_concat() {
        let directory = tempfile::TempDir::new().unwrap();
        let mut card = term("unused");
        card.kind = ShotKind::Card;
        card.img = None;
        card.html = Some("<h1>Done</h1>".into());
        let options = options(&directory, vec![term("one"), card]);
        fs::create_dir_all(&options.work_dir).unwrap();
        fs::write(options.font_path.as_ref().unwrap(), "font").unwrap();
        write_png(&options.frames_dir.join("one.png"), 12, 12, [1, 2, 3]);
        let mut renderer = FakeRenderer::default();
        let mut logs = Vec::new();
        let result =
            compose_storyboard(&options, &mut renderer, |message| logs.push(message)).unwrap();
        assert_eq!(result.unique_frames, 2);
        assert_eq!(result.total_seconds, 1.0);
        assert_eq!(renderer.outputs.len(), 2);
        assert_eq!(logs, ["frame 1/2", "frame 2/2"]);
        assert!(renderer.outputs.iter().all(|path| path.is_file()));
        assert!(renderer.html[0].contains("rgb(1, 2, 3)"));
        assert!(renderer.html[1].contains("<h1>Done</h1>"));
        let concat = fs::read_to_string(result.concat_path).unwrap();
        assert!(concat.starts_with("ffconcat version 1.0\n"));
        assert_eq!(concat.matches("duration 0.50000").count(), 2);
        assert_eq!(concat.matches("f0001.png").count(), 2);
    }

    #[test]
    fn chrome_capabilities_pin_headless_file_access_binary_and_geometry() {
        let value = chrome_capabilities(
            Some(Path::new("/opt/chromium")),
            Viewport {
                width: 800,
                height: 600,
            },
        );
        assert_eq!(
            value.pointer("/capabilities/alwaysMatch/browserName"),
            Some(&json!("chrome"))
        );
        assert_eq!(
            value.pointer("/capabilities/alwaysMatch/goog:chromeOptions/binary"),
            Some(&json!("/opt/chromium"))
        );
        let args = value
            .pointer("/capabilities/alwaysMatch/goog:chromeOptions/args")
            .and_then(Value::as_array)
            .unwrap();
        assert!(args.contains(&json!("--allow-file-access-from-files")));
        assert!(args.contains(&json!("--window-size=800,600")));
        assert_eq!(
            device_metrics_override(Viewport {
                width: 800,
                height: 600,
            }),
            json!({
                "cmd": "Emulation.setDeviceMetricsOverride",
                "params": {
                    "width": 800,
                    "height": 600,
                    "deviceScaleFactor": 1,
                    "mobile": false
                }
            })
        );
    }

    #[test]
    fn http_parser_accepts_content_length_and_chunked_webdriver_json() {
        assert_eq!(
            parse_http_json(b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\n{\"value\":42}")
                .unwrap()["value"],
            42
        );
        assert_eq!(
            parse_http_json(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"value\r\n5\r\n\":42}\r\n0\r\n\r\n"
            )
            .unwrap()["value"],
            42
        );
        let error = parse_http_json(
            b"HTTP/1.1 500 Error\r\nContent-Length: 28\r\n\r\n{\"value\":{\"message\":\"bad\"}}",
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "WebDriver HTTP 500: bad");
    }

    #[test]
    fn http_framing_finishes_keep_alive_responses_without_waiting_for_eof() {
        let fixed = b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: keep-alive\r\n\r\n{\"value\":42}";
        assert_eq!(
            webdriver_response_length(&fixed[..fixed.len() - 1]).unwrap(),
            None
        );
        assert_eq!(webdriver_response_length(fixed).unwrap(), Some(fixed.len()));

        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\nConnection: keep-alive\r\n\r\n7\r\n{\"value\r\n5\r\n\":42}\r\n0\r\n\r\n";
        assert_eq!(
            webdriver_response_length(&chunked[..chunked.len() - 1]).unwrap(),
            None
        );
        assert_eq!(
            webdriver_response_length(chunked).unwrap(),
            Some(chunked.len())
        );
    }

    #[test]
    fn webdriver_request_uses_loopback_literal_http_and_decodes_the_response() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let server_capture = Arc::clone(&captured);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let count = stream.read(&mut buffer).unwrap();
                bytes.extend_from_slice(&buffer[..count]);
                if count == 0 || bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            *server_capture.lock().unwrap() = bytes;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 14\r\nConnection: close\r\n\r\n{\"value\":true}",
                )
                .unwrap();
        });
        assert_eq!(
            webdriver_request(port, "GET", "/status", None).unwrap()["value"],
            true
        );
        server.join().unwrap();
        let request = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
        assert!(request.starts_with("GET /status HTTP/1.1\r\n"));
        assert!(request.contains(&format!("Host: 127.0.0.1:{port}\r\n")));
        assert!(request.contains("Content-Length: 0\r\n"));
    }
}
