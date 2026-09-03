//! Workdeck's canonical product-video scenes, storyboard wrapper, and encoders.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use cargo_metadata::MetadataCommand;
use tempfile::{Builder as TempBuilder, TempDir};

use super::capture::{
    CaptureAction, CaptureScene, CaptureScript, LaunchSpec, RenderOptions, SceneFilter,
    WrapperSpec, run_capture,
};
use super::{compose_file, required_path, resolve};

const COLS: u16 = 140;
const ROWS: u16 = 32;
const DEFAULT_WORK_DIR: &str = ".video-work";
const STORYBOARD: &str = "media/launch/storyboard.json";
const CLI_NOTE_STML: &str = r#"<h2>Cache rework</h2>
<row gap="1">
  <box border border-color="accent" padding-x="1">lookup</box>
  <text width="3"><br/>&rarr;</text>
  <box border border-color="warning" padding-x="1">miss?</box>
  <text width="3"><br/>&rarr;</text>
  <box border border-color="success" padding-x="1">rebuild once</box>
</row>
<spacer/>
<text><c fg="success">██████████████</c><c fg="subtle">░░░░░░</c> hit rate 70%</text>
<text><badge color="success">OK</badge> single-flight, <b>no stampede</b></text>
"#;

pub fn launch_file(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    match args.next().as_deref() {
        Some("capture") => capture_launch(repo, args),
        Some("compose") => compose_launch(repo, args),
        Some("encode") => encode_launch(repo, args),
        _ => bail!("media launch requires the capture, compose, or encode command"),
    }
}

fn capture_launch(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut work_dir = PathBuf::from(DEFAULT_WORK_DIR);
    let mut font = None;
    let mut binary = None;
    let mut scenes = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--work-dir" => work_dir = required_path(&mut args, "--work-dir")?,
            "--font" => font = Some(required_path(&mut args, "--font")?),
            "--binary" => binary = Some(required_path(&mut args, "--binary")?),
            "--scenes" => {
                scenes = Some(
                    args.next()
                        .context("--scenes requires a comma-separated value")?,
                );
            }
            _ => bail!("unknown media launch capture option {argument:?}"),
        }
    }

    let work_dir = resolve(repo, work_dir);
    let font = resolve(
        repo,
        font.context("media launch capture requires --font FILE")?,
    );
    let binary = binary
        .map(|path| resolve(repo, path))
        .map(Ok)
        .unwrap_or_else(|| build_workdeck(repo))?;
    if !binary.is_file() {
        bail!(
            "Workdeck capture binary does not exist: {}",
            binary.display()
        );
    }

    let selected = scenes.or_else(|| std::env::var("SCENES").ok());
    let wants = SceneFilter::new(selected.as_deref());
    let mut temporary = Vec::new();
    let config_home = new_temp_dir(&mut temporary, "workdeck-video-config-")?;
    let environment = capture_environment(&config_home);
    let mut capture_scenes = Vec::new();

    if wants.matches("review") {
        let repo_dir = create_demo_repo(repo, &mut temporary)?;
        capture_scenes.push(review_scene(&binary, &repo_dir, &environment));
    }
    if wants.matches("stml") {
        capture_scenes.push(stml_scene(repo, &binary, &environment));
    }
    if wants.matches("cli") {
        let cli_dir = new_temp_dir(&mut temporary, "workdeck-video-cli-")?;
        fs::write(cli_dir.join("note.stml"), CLI_NOTE_STML)
            .with_context(|| format!("write launch-video STML fixture in {}", cli_dir.display()))?;
        let bin_dir = new_temp_dir(&mut temporary, "workdeck-video-bin-")?.join("bin");
        capture_scenes.push(markup_scene(&binary, &cli_dir, &bin_dir, &environment));
    }
    if wants.matches("pager") {
        let repo_dir = create_demo_repo(repo, &mut temporary)?;
        let bin_dir = new_temp_dir(&mut temporary, "workdeck-video-bin-")?.join("bin");
        capture_scenes.push(pager_scene(&binary, &repo_dir, &bin_dir, &environment));
    }
    if wants.matches("triage") {
        let extension = crate::prepare_extension_example(repo, "review-triage")?;
        let repo_dir = create_demo_repo(repo, &mut temporary)?;
        capture_scenes.push(triage_scene(&binary, &repo_dir, &extension, &environment));
    }
    if wants.matches("fileview") {
        let extension = crate::prepare_extension_example(repo, "file-view-gallery")?;
        capture_scenes.push(file_view_scene(
            repo,
            &binary,
            &extension,
            &environment,
            FileViewFixture::Palette,
        ));
        capture_scenes.push(file_view_scene(
            repo,
            &binary,
            &extension,
            &environment,
            FileViewFixture::Dependencies,
        ));
    }

    if capture_scenes.is_empty() {
        bail!(
            "no launch-video scenes selected; choose review, stml, cli, pager, triage, or fileview"
        );
    }
    let script = CaptureScript {
        cols: COLS,
        rows: ROWS,
        frames_dir: work_dir.join("frames"),
        font,
        render_options: RenderOptions::default(),
        manifest: Some(work_dir.join("manifest.json")),
        scenes: capture_scenes,
    };
    let result = run_capture(&script, &SceneFilter::new(None))?;
    println!(
        "captured {} Workdeck launch keyframes across {} sessions -> {}",
        result.frames,
        result.scenes,
        result.manifest.display()
    );
    Ok(())
}

fn compose_launch(repo: &Path, args: impl Iterator<Item = String>) -> Result<()> {
    let forwarded = args.collect::<Vec<_>>();
    reject_forwarded_option(&forwarded, "--storyboard")?;
    reject_forwarded_option(&forwarded, "--root-dir")?;
    let mut composed = vec![
        "--storyboard".into(),
        STORYBOARD.into(),
        "--root-dir".into(),
        repo.display().to_string(),
    ];
    if !has_option(&forwarded, "--work-dir") {
        composed.extend(["--work-dir".into(), DEFAULT_WORK_DIR.into()]);
    }
    composed.extend(forwarded);
    compose_file(repo, composed.into_iter())
}

fn encode_launch(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut work_dir = PathBuf::from(DEFAULT_WORK_DIR);
    let mut ffmpeg = PathBuf::from("ffmpeg");
    let mut mp4 = PathBuf::from("launch.mp4");
    let mut webm = PathBuf::from("launch.webm");
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--work-dir" => work_dir = required_path(&mut args, "--work-dir")?,
            "--ffmpeg" => ffmpeg = required_path(&mut args, "--ffmpeg")?,
            "--mp4" => mp4 = required_path(&mut args, "--mp4")?,
            "--webm" => webm = required_path(&mut args, "--webm")?,
            _ => bail!("unknown media launch encode option {argument:?}"),
        }
    }
    let work_dir = resolve(repo, work_dir);
    let concat = work_dir.join("concat.txt");
    if !concat.is_file() {
        bail!(
            "launch-video concat list does not exist: {}",
            concat.display()
        );
    }
    let mp4 = resolve_output(&work_dir, mp4);
    let webm = resolve_output(&work_dir, webm);
    if mp4 == webm {
        bail!("launch-video MP4 and WebM outputs must be different paths");
    }
    if let Some(parent) = mp4.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = webm.parent() {
        fs::create_dir_all(parent)?;
    }
    let (mp4_args, webm_args) = encode_arguments(&concat, &mp4, &webm);
    run_encoder(repo, &ffmpeg, &mp4_args)?;
    run_encoder(repo, &ffmpeg, &webm_args)?;
    println!("encoded {} and {}", mp4.display(), webm.display());
    Ok(())
}

fn build_workdeck(repo: &Path) -> Result<PathBuf> {
    crate::run_checked(
        repo,
        "cargo",
        &["build", "-p", "workdeck-cli", "--bin", "workdeck"],
    )?;
    let metadata = MetadataCommand::new()
        .current_dir(repo)
        .no_deps()
        .exec()
        .context("resolve Cargo target directory for launch-video capture")?;
    Ok(metadata
        .target_directory
        .join("debug")
        .join(format!("workdeck{}", std::env::consts::EXE_SUFFIX))
        .into_std_path_buf())
}

fn capture_environment(config_home: &Path) -> BTreeMap<String, Option<String>> {
    BTreeMap::from([
        (
            "XDG_CONFIG_HOME".into(),
            Some(config_home.display().to_string()),
        ),
        ("WORKDECK_MCP_DISABLE".into(), Some("1".into())),
        ("WORKDECK_DISABLE_UPDATE_NOTICE".into(), Some("1".into())),
    ])
}

fn new_temp_dir(temporary: &mut Vec<TempDir>, prefix: &str) -> Result<PathBuf> {
    let directory = TempBuilder::new()
        .prefix(prefix)
        .tempdir()
        .with_context(|| format!("create {prefix} temporary directory"))?;
    let path = directory.path().to_owned();
    temporary.push(directory);
    Ok(path)
}

fn create_demo_repo(repo: &Path, temporary: &mut Vec<TempDir>) -> Result<PathBuf> {
    let destination = new_temp_dir(temporary, "workdeck-video-repo-")?;
    run_git(&destination, ["init", "--quiet"])?;
    run_git(&destination, ["config", "user.name", "Demo"])?;
    run_git(&destination, ["config", "user.email", "demo@example.com"])?;
    copy_tree(
        &repo.join("examples/2-mini-app-refactor/before"),
        &destination,
    )?;
    run_git(&destination, ["add", "."])?;
    run_git(&destination, ["commit", "--quiet", "-m", "before"])?;
    copy_tree(
        &repo.join("examples/2-mini-app-refactor/after"),
        &destination,
    )?;
    Ok(destination)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(source)
        .with_context(|| format!("read launch-video fixture directory {}", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&destination_path)?;
            copy_tree(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "copy launch-video fixture {} -> {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn run_git<const N: usize>(cwd: &Path, args: [&str; N]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("launch git while preparing product-video fixture")?;
    if !output.status.success() {
        bail!(
            "git fixture setup failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn review_scene(
    binary: &Path,
    cwd: &Path,
    environment: &BTreeMap<String, Option<String>>,
) -> CaptureScene {
    let mut actions = vec![wait("src/", 60_000), keyboard_probe(), sleep(500)];
    for index in 0..10 {
        actions.extend([
            press("j"),
            sleep(120),
            snap(&format!("review-walk-{index:02}")),
        ]);
    }
    for offset in 0..4 {
        actions.extend([
            press("k"),
            sleep(120),
            snap(&format!("review-walk-{:02}", 10 + offset)),
        ]);
    }
    actions.extend([
        press("c"),
        wait("Draft note", 10_000),
        sleep(300),
        snap("review-draft"),
        type_text("edge case: empty task list renders a blank summary"),
        sleep(300),
        snap("review-typed"),
        press("ctrl+s"),
        wait("Your note", 10_000),
        sleep(500),
        snap("review-note"),
    ]);
    app_scene(
        "review",
        binary,
        cwd,
        ["diff", "--mode", "stack"],
        environment,
        actions,
    )
}

fn stml_scene(
    repo: &Path,
    binary: &Path,
    environment: &BTreeMap<String, Option<String>>,
) -> CaptureScene {
    let patch = repo.join("examples/9-agent-markup-notes/change.patch");
    let context = repo.join("examples/9-agent-markup-notes/agent-context.json");
    let mut actions = vec![
        wait("retry\\.rs", 60_000),
        keyboard_probe(),
        sleep(500),
        snap("stml-review"),
        press("a"),
        sleep(900),
        snap("stml-notes"),
    ];
    for step in 0..10 {
        actions.extend([press("down"), sleep(60)]);
        if step == 4 || step == 9 {
            actions.push(snap(&format!("stml-scroll-{step}")));
        }
    }
    actions.extend([sleep(400), snap("stml-note-2")]);
    app_scene(
        "stml",
        binary,
        repo,
        [
            "patch".into(),
            path_text(&patch),
            "--agent-context".into(),
            path_text(&context),
            "--mode".into(),
            "stack".into(),
        ],
        environment,
        actions,
    )
}

fn markup_scene(
    binary: &Path,
    cwd: &Path,
    bin_dir: &Path,
    environment: &BTreeMap<String, Option<String>>,
) -> CaptureScene {
    shell_scene(
        "cli",
        binary,
        cwd,
        bin_dir,
        environment,
        vec![
            wait("❯", 15_000),
            sleep(300),
            CaptureAction::TypeCommand {
                command: "workdeck markup render note.stml --width 72".into(),
                snap_at: BTreeMap::from([
                    (10, "cli-typing-10".into()),
                    (22, "cli-typing-22".into()),
                    (34, "cli-typing-34".into()),
                ]),
                delay_ms: 30,
            },
            sleep(200),
            snap("cli-typed"),
            press("enter"),
            wait("hit rate", 60_000),
            sleep(400),
            snap("cli-rendered"),
        ],
    )
}

fn pager_scene(
    binary: &Path,
    cwd: &Path,
    bin_dir: &Path,
    environment: &BTreeMap<String, Option<String>>,
) -> CaptureScene {
    shell_scene(
        "pager",
        binary,
        cwd,
        bin_dir,
        environment,
        vec![
            wait("❯", 15_000),
            sleep(300),
            CaptureAction::TypeCommand {
                command: "git diff | workdeck pager".into(),
                snap_at: BTreeMap::from([(9, "pager-typing-9".into()), (24, "pager-typed".into())]),
                delay_ms: 30,
            },
            sleep(200),
            press("enter"),
            wait("src/format\\.rs", 60_000),
            sleep(900),
            snap("pager-review"),
            press("s"),
            sleep(700),
            snap("pager-sidebar"),
        ],
    )
}

fn triage_scene(
    binary: &Path,
    cwd: &Path,
    extension: &Path,
    environment: &BTreeMap<String, Option<String>>,
) -> CaptureScene {
    app_scene(
        "triage",
        binary,
        cwd,
        [
            "diff".into(),
            "--extension".into(),
            path_text(extension),
            "--mode".into(),
            "stack".into(),
        ],
        environment,
        vec![
            wait("src/", 60_000),
            keyboard_probe(),
            sleep(500),
            snap("triage-review"),
            press("y"),
            wait("Review triage", 10_000),
            sleep(500),
            snap("triage-sidebar"),
            press("x"),
            wait("Triage ", 10_000),
            sleep(400),
            snap("triage-select"),
            press("enter"),
            wait("optional rationale", 10_000),
            sleep(300),
            type_text("bounded retries look right"),
            sleep(300),
            snap("triage-rationale"),
            press("enter"),
            sleep(700),
            snap("triage-marked"),
            press("]"),
            sleep(400),
            press("x"),
            wait("Triage ", 10_000),
            press("down"),
            sleep(200),
            press("enter"),
            wait("optional rationale", 10_000),
            press("enter"),
            sleep(700),
            snap("triage-board"),
        ],
    )
}

#[derive(Debug, Clone, Copy)]
enum FileViewFixture {
    Palette,
    Dependencies,
}

fn file_view_scene(
    repo: &Path,
    binary: &Path,
    extension: &Path,
    environment: &BTreeMap<String, Option<String>>,
    fixture: FileViewFixture,
) -> CaptureScene {
    let (label, before, after, ready, rendered) = match fixture {
        FileViewFixture::Palette => (
            "palette",
            "examples/extensions/file-view-gallery/fixtures/css-palette/before.css",
            "examples/extensions/file-view-gallery/fixtures/css-palette/after.css",
            "--canvas",
            "#|palette|swatch",
        ),
        FileViewFixture::Dependencies => (
            "deps",
            "examples/extensions/file-view-gallery/fixtures/package-dependencies/before/Cargo.toml",
            "examples/extensions/file-view-gallery/fixtures/package-dependencies/after/Cargo.toml",
            "Cargo\\.toml",
            "dependencies",
        ),
    };
    app_scene(
        &format!("fileview-{label}"),
        binary,
        repo,
        [
            "difftool".into(),
            path_text(&repo.join(before)),
            path_text(&repo.join(after)),
            "--extension".into(),
            path_text(extension),
            "--mode".into(),
            "stack".into(),
        ],
        environment,
        vec![
            wait(ready, 60_000),
            keyboard_probe(),
            sleep(500),
            snap(&format!("fileview-{label}-raw")),
            press("f8"),
            wait(rendered, 10_000),
            sleep(600),
            snap(&format!("fileview-{label}-rendered")),
        ],
    )
}

fn app_scene<I, S>(
    name: &str,
    binary: &Path,
    cwd: &Path,
    args: I,
    environment: &BTreeMap<String, Option<String>>,
    actions: Vec<CaptureAction>,
) -> CaptureScene
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    CaptureScene {
        name: name.into(),
        wrappers: Vec::new(),
        launch: LaunchSpec::App {
            command: binary.to_owned(),
            args: args.into_iter().map(Into::into).collect(),
            cwd: cwd.to_owned(),
            env: environment.clone(),
        },
        actions,
    }
}

fn shell_scene(
    name: &str,
    binary: &Path,
    cwd: &Path,
    bin_dir: &Path,
    environment: &BTreeMap<String, Option<String>>,
    actions: Vec<CaptureAction>,
) -> CaptureScene {
    CaptureScene {
        name: name.into(),
        wrappers: vec![WrapperSpec {
            bin_dir: bin_dir.to_owned(),
            name: "workdeck".into(),
            exec: vec![path_text(binary)],
        }],
        launch: LaunchSpec::Shell {
            cwd: cwd.to_owned(),
            path_prepend: vec![bin_dir.to_owned()],
            prompt: None,
            env: environment.clone(),
        },
        actions,
    }
}

fn wait(pattern: &str, timeout_ms: u64) -> CaptureAction {
    CaptureAction::WaitText {
        pattern: pattern.into(),
        timeout_ms,
    }
}

fn keyboard_probe() -> CaptureAction {
    CaptureAction::Probe {
        key: "?".into(),
        pattern: "Workdeck help".into(),
        dismiss_key: "escape".into(),
        attempts: 5,
        timeout_ms: 2_000,
        settle_ms: 300,
    }
}

fn sleep(ms: u64) -> CaptureAction {
    CaptureAction::Sleep { ms }
}

fn press(key: &str) -> CaptureAction {
    CaptureAction::Press { key: key.into() }
}

fn snap(name: &str) -> CaptureAction {
    CaptureAction::Snap { name: name.into() }
}

fn type_text(text: &str) -> CaptureAction {
    CaptureAction::Type {
        text: text.into(),
        delay_ms: 0,
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn reject_forwarded_option(arguments: &[String], option: &str) -> Result<()> {
    if has_option(arguments, option) {
        bail!("media launch compose owns {option}; edit {STORYBOARD} for launch-video changes");
    }
    Ok(())
}

fn has_option(arguments: &[String], option: &str) -> bool {
    arguments
        .iter()
        .any(|argument| argument == option || argument.starts_with(&format!("{option}=")))
}

fn resolve_output(work_dir: &Path, output: PathBuf) -> PathBuf {
    if output.is_absolute() {
        output
    } else {
        work_dir.join(output)
    }
}

fn encode_arguments(concat: &Path, mp4: &Path, webm: &Path) -> (Vec<String>, Vec<String>) {
    let common = [
        "-y",
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
        concat.to_str().unwrap_or_default(),
        "-vf",
        "fps=30,format=yuv420p",
    ];
    let mut mp4_args = common.map(str::to_owned).to_vec();
    mp4_args.extend(
        [
            "-c:v",
            "libx264",
            "-preset",
            "slow",
            "-crf",
            "18",
            "-movflags",
            "+faststart",
            mp4.to_str().unwrap_or_default(),
        ]
        .map(str::to_owned),
    );
    let mut webm_args = common.map(str::to_owned).to_vec();
    webm_args.extend(
        [
            "-c:v",
            "libvpx-vp9",
            "-b:v",
            "0",
            "-crf",
            "32",
            "-row-mt",
            "1",
            webm.to_str().unwrap_or_default(),
        ]
        .map(str::to_owned),
    );
    (mp4_args, webm_args)
}

fn run_encoder(repo: &Path, ffmpeg: &Path, arguments: &[String]) -> Result<()> {
    let status = Command::new(ffmpeg)
        .args(arguments)
        .current_dir(repo)
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("launch video encoder {}", ffmpeg.display()))?;
    if !status.success() {
        bail!("{} exited with {status}", executable_name(ffmpeg));
    }
    Ok(())
}

fn executable_name(path: &Path) -> &str {
    path.file_name().and_then(OsStr::to_str).unwrap_or("ffmpeg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_launch_capture_scene_inventory_matches_hunk_oracle() {
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../../../port/hunk/oracles/launch-video.json"))
                .unwrap();
        let filter = SceneFilter::new(None);
        assert!(filter.matches("review"));
        assert_eq!(u64::from(COLS), oracle["capture"]["geometry"]["cols"]);
        assert_eq!(u64::from(ROWS), oracle["capture"]["geometry"]["rows"]);
        let scene_groups = oracle["capture"]["sceneGroups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            scene_groups,
            ["review", "stml", "cli", "pager", "triage", "fileview"]
        );

        let environment = BTreeMap::new();
        let binary = Path::new("workdeck");
        let root = Path::new("repo");
        let extension = Path::new("extension");
        let scenes = [
            review_scene(binary, root, &environment),
            stml_scene(root, binary, &environment),
            markup_scene(binary, root, Path::new("bin-cli"), &environment),
            pager_scene(binary, root, Path::new("bin-pager"), &environment),
            triage_scene(binary, root, extension, &environment),
            file_view_scene(
                root,
                binary,
                extension,
                &environment,
                FileViewFixture::Palette,
            ),
            file_view_scene(
                root,
                binary,
                extension,
                &environment,
                FileViewFixture::Dependencies,
            ),
        ];
        let keyframes = scenes
            .iter()
            .flat_map(|scene| scene.actions.iter())
            .flat_map(|action| match action {
                CaptureAction::Snap { name } => vec![name.as_str()],
                CaptureAction::TypeCommand { snap_at, .. } => {
                    snap_at.values().map(String::as_str).collect()
                }
                _ => Vec::new(),
            })
            .collect::<Vec<_>>();
        let expected_keyframes = oracle["capture"]["keyframes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(keyframes, expected_keyframes);
    }

    #[test]
    fn canonical_storyboard_preserves_hunk_timing_and_keyframe_inventory() {
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../../../port/hunk/oracles/launch-video.json"))
                .unwrap();
        let source = include_str!("../../../media/launch/storyboard.json");
        let input: super::super::StoryboardInput = serde_json::from_str(source).unwrap();
        let shots = match input {
            super::super::StoryboardInput::Shots(shots)
            | super::super::StoryboardInput::Object { shots } => shots,
        };
        assert_eq!(shots.len() as u64, oracle["storyboard"]["shots"]);
        assert_eq!(
            super::super::required_keyframes(&shots).len() as u64,
            oracle["storyboard"]["requiredKeyframes"]
        );
        let plan = super::super::plan_frames(&shots, super::super::PlanOptions::default());
        assert_eq!(
            plan.frames.len() as u64,
            oracle["storyboard"]["plannedFramesAt30Fps"]
        );
        assert!(
            (plan.total_seconds - oracle["storyboard"]["totalSeconds"].as_f64().unwrap()).abs()
                < 0.000_001
        );
    }

    #[test]
    fn launch_scene_filter_preserves_trimmed_exact_match_semantics() {
        let filter = SceneFilter::new(Some(" review, fileview,unknown "));
        assert!(filter.matches("review"));
        assert!(filter.matches("fileview"));
        assert!(!filter.matches("pager"));
    }

    #[test]
    fn encoder_arguments_preserve_both_hunk_release_recipes() {
        let (mp4, webm) = encode_arguments(
            Path::new("work/concat.txt"),
            Path::new("work/launch.mp4"),
            Path::new("work/launch.webm"),
        );
        assert_eq!(
            &mp4[9..],
            [
                "-c:v",
                "libx264",
                "-preset",
                "slow",
                "-crf",
                "18",
                "-movflags",
                "+faststart",
                "work/launch.mp4"
            ]
        );
        assert_eq!(
            &webm[9..],
            [
                "-c:v",
                "libvpx-vp9",
                "-b:v",
                "0",
                "-crf",
                "32",
                "-row-mt",
                "1",
                "work/launch.webm"
            ]
        );
    }

    #[test]
    fn demo_repo_contains_a_real_working_tree_diff() {
        let source = TempBuilder::new()
            .prefix("launch-source-")
            .tempdir()
            .unwrap();
        let before = source.path().join("examples/2-mini-app-refactor/before");
        let after = source.path().join("examples/2-mini-app-refactor/after");
        fs::create_dir_all(before.join("src")).unwrap();
        fs::create_dir_all(after.join("src")).unwrap();
        fs::write(before.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            after.join("src/main.rs"),
            "fn main() { println!(\"ok\"); }\n",
        )
        .unwrap();
        let mut temporary = Vec::new();
        let demo = create_demo_repo(source.path(), &mut temporary).unwrap();
        let output = Command::new("git")
            .args(["diff", "--", "src/main.rs"])
            .current_dir(demo)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("println!")
        );
    }

    #[test]
    fn launch_compose_cannot_replace_the_canonical_storyboard() {
        let error = reject_forwarded_option(
            &["--storyboard".into(), "elsewhere.json".into()],
            "--storyboard",
        )
        .unwrap_err();
        assert!(error.to_string().contains(STORYBOARD));
    }
}
