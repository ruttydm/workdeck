//! Deterministic terminal-video storyboard planning.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

mod capture;
mod compose;
mod launch;

pub use capture::capture_file;
pub(crate) use compose::capture_card_documents;
pub use compose::compose_file;
pub(crate) use compose::validate_card_png_bytes;
pub use launch::launch_file;

const DEFAULT_FPS: f64 = 30.0;
const DEFAULT_CAPTION_ANIMATION_SECONDS: f64 = 0.45;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShotKind {
    Card,
    Term,
}

/// One declarative storyboard beat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shot {
    pub kind: ShotKind,
    pub dur: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub img: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap_key: Option<String>,
    #[serde(default)]
    pub enter: bool,
}

/// The complete render state handed to the native compositor for one frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameState {
    pub kind: ShotKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub img: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    pub shot_t: f64,
    pub cap_t: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedFrame {
    pub state: FrameState,
    pub duration: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FramePlan {
    pub frames: Vec<PlannedFrame>,
    pub total_seconds: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanOptions {
    pub fps: f64,
    pub caption_animation_seconds: f64,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            fps: DEFAULT_FPS,
            caption_animation_seconds: DEFAULT_CAPTION_ANIMATION_SECONDS,
        }
    }
}

/// Expand a storyboard into its exact animated frames and compressed hold frames.
#[must_use]
pub fn plan_frames(shots: &[Shot], options: PlanOptions) -> FramePlan {
    let mut frames = Vec::new();
    // JavaScript distinguishes the initial `null` from an absent (`undefined`) capKey. Retain
    // that distinction so a first caption without a key still animates exactly once.
    let mut previous_cap_key_initialized = false;
    let mut previous_cap_key: Option<String> = None;
    let mut previous_caption: Option<String> = None;

    for shot in shots {
        let caption = shot.caption.clone().or_else(|| {
            shot.cap_key
                .as_deref()
                .filter(|key| !key.is_empty())
                .filter(|key| previous_cap_key.as_deref() == Some(*key))
                .and(previous_caption.clone())
        });
        let mut base = FrameState {
            kind: shot.kind,
            html: None,
            img: None,
            title: None,
            caption: None,
            shot_t: 1.0,
            cap_t: 1.0,
        };
        match shot.kind {
            ShotKind::Card => base.html.clone_from(&shot.html),
            ShotKind::Term => {
                base.img.clone_from(&shot.img);
                base.title.clone_from(&shot.title);
                base.caption = caption;
            }
        }
        let authored_caption = shot
            .caption
            .as_deref()
            .is_some_and(|caption| !caption.is_empty());
        let cap_key_changed = !previous_cap_key_initialized || shot.cap_key != previous_cap_key;
        let caption_changes = shot.kind == ShotKind::Term && authored_caption && cap_key_changed;
        let animation_seconds = if shot.enter || caption_changes {
            options.caption_animation_seconds.min(shot.dur * 0.6)
        } else {
            0.0
        };
        let animation_frames = (animation_seconds * options.fps).round() as usize;

        for index in 0..animation_frames {
            let progress = (index + 1) as f64 / animation_frames as f64;
            let mut state = base.clone();
            state.shot_t = if shot.enter { progress } else { 1.0 };
            state.cap_t = if caption_changes { progress } else { 1.0 };
            frames.push(PlannedFrame {
                state,
                duration: 1.0 / options.fps,
            });
        }
        frames.push(PlannedFrame {
            state: base,
            duration: (shot.dur - animation_frames as f64 / options.fps).max(1.0 / options.fps),
        });

        if shot.kind == ShotKind::Term && authored_caption {
            previous_cap_key.clone_from(&shot.cap_key);
            previous_cap_key_initialized = true;
            previous_caption.clone_from(&shot.caption);
        } else if shot.kind == ShotKind::Card {
            previous_cap_key = None;
            previous_cap_key_initialized = false;
            previous_caption = None;
        }
    }

    FramePlan {
        total_seconds: frames.iter().map(|frame| frame.duration).sum(),
        frames,
    }
}

/// List terminal image names in first-use order, ignoring cards and duplicates.
#[must_use]
pub fn required_keyframes(shots: &[Shot]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    shots
        .iter()
        .filter(|shot| shot.kind == ShotKind::Term)
        .filter_map(|shot| shot.img.clone())
        .filter(|image| seen.insert(image.clone()))
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StoryboardInput {
    Shots(Vec<Shot>),
    Object { shots: Vec<Shot> },
}

pub fn plan_file(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut storyboard = None;
    let mut output = None;
    let mut options = PlanOptions::default();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--storyboard" => storyboard = Some(required_path(&mut args, "--storyboard")?),
            "--output" => output = Some(required_path(&mut args, "--output")?),
            "--fps" => options.fps = required_number(&mut args, "--fps")?,
            "--caption-animation-seconds" => {
                options.caption_animation_seconds =
                    required_number(&mut args, "--caption-animation-seconds")?;
            }
            _ => bail!("unknown media plan option {argument:?}"),
        }
    }
    if !options.fps.is_finite() || options.fps <= 0.0 {
        bail!("--fps must be a positive finite number");
    }
    if !options.caption_animation_seconds.is_finite() || options.caption_animation_seconds < 0.0 {
        bail!("--caption-animation-seconds must be a non-negative finite number");
    }
    let storyboard = resolve(
        repo,
        storyboard.context("media plan requires --storyboard FILE")?,
    );
    let output = resolve(repo, output.context("media plan requires --output FILE")?);
    let source = fs::read_to_string(&storyboard)
        .with_context(|| format!("read storyboard {}", storyboard.display()))?;
    let input: StoryboardInput = serde_json::from_str(&source)
        .with_context(|| format!("parse storyboard {}", storyboard.display()))?;
    let shots = match input {
        StoryboardInput::Shots(shots) | StoryboardInput::Object { shots } => shots,
    };
    let keyframes = required_keyframes(&shots);
    let plan = plan_frames(&shots, options);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create media plan directory {}", parent.display()))?;
    }
    let encoded = serde_json::to_vec_pretty(&plan).context("encode terminal-video frame plan")?;
    fs::write(&output, encoded)
        .with_context(|| format!("write terminal-video frame plan {}", output.display()))?;
    println!(
        "planned {} frames across {:.6} seconds from {} terminal keyframes -> {}",
        plan.frames.len(),
        plan.total_seconds,
        keyframes.len(),
        output.display()
    );
    Ok(())
}

fn resolve(repo: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        repo.join(path)
    }
}

fn required_path(args: &mut impl Iterator<Item = String>, option: &str) -> Result<PathBuf> {
    args.next()
        .map(PathBuf::from)
        .with_context(|| format!("{option} requires a path"))
}

fn required_number(args: &mut impl Iterator<Item = String>, option: &str) -> Result<f64> {
    let value = args
        .next()
        .with_context(|| format!("{option} requires a number"))?;
    value
        .parse()
        .with_context(|| format!("{option} requires a number, got {value:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term() -> Shot {
        Shot {
            kind: ShotKind::Term,
            dur: 2.0,
            html: None,
            img: Some("frame-a".into()),
            title: Some("t".into()),
            caption: None,
            cap_key: None,
            enter: false,
        }
    }

    #[test]
    fn captioned_shot_animates_its_caption_then_holds() {
        let mut shot = term();
        shot.caption = Some("hello".into());
        shot.cap_key = Some("a".into());
        let plan = plan_frames(&[shot], PlanOptions::default());
        let animation_frames = (0.45_f64 * 30.0).round() as usize;
        assert_eq!(plan.frames.len(), animation_frames + 1);
        assert_eq!(plan.frames[0].state.caption.as_deref(), Some("hello"));
        assert_eq!(plan.frames[0].state.cap_t, 1.0 / animation_frames as f64);
        assert_eq!(plan.frames.last().unwrap().state.cap_t, 1.0);
        assert!(
            (plan.frames.last().unwrap().duration - (2.0 - animation_frames as f64 / 30.0)).abs()
                < 0.000_01
        );
    }

    #[test]
    fn continuation_with_the_same_caption_key_carries_without_reanimating() {
        let mut first = term();
        first.dur = 1.0;
        first.caption = Some("walking".into());
        first.cap_key = Some("walk".into());
        let mut continuation = term();
        continuation.img = Some("frame-b".into());
        continuation.dur = 0.2;
        continuation.cap_key = Some("walk".into());
        let plan = plan_frames(&[first, continuation], PlanOptions::default());
        let matching = plan
            .frames
            .iter()
            .filter(|frame| frame.state.img.as_deref() == Some("frame-b"))
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].state.caption.as_deref(), Some("walking"));
        assert_eq!(matching[0].state.cap_t, 1.0);
    }

    #[test]
    fn changed_caption_key_animates_the_new_caption() {
        let mut first = term();
        first.dur = 1.0;
        first.caption = Some("first".into());
        first.cap_key = Some("one".into());
        let mut second = term();
        second.dur = 1.0;
        second.img = Some("frame-b".into());
        second.caption = Some("second".into());
        second.cap_key = Some("two".into());
        let plan = plan_frames(&[first, second], PlanOptions::default());
        let matching = plan
            .frames
            .iter()
            .filter(|frame| frame.state.img.as_deref() == Some("frame-b"))
            .collect::<Vec<_>>();
        assert!(matching.len() > 1);
        assert!(matching[0].state.cap_t < 1.0);
    }

    #[test]
    fn cards_reset_caption_state() {
        let mut before = term();
        before.dur = 1.0;
        before.caption = Some("before".into());
        before.cap_key = Some("same".into());
        let card = Shot {
            kind: ShotKind::Card,
            dur: 1.0,
            html: Some("<h1>x</h1>".into()),
            img: None,
            title: None,
            caption: None,
            cap_key: None,
            enter: true,
        };
        let mut after = term();
        after.dur = 1.0;
        after.img = Some("frame-b".into());
        after.caption = Some("before".into());
        after.cap_key = Some("same".into());
        let plan = plan_frames(&[before, card, after], PlanOptions::default());
        let first_after = plan
            .frames
            .iter()
            .find(|frame| frame.state.img.as_deref() == Some("frame-b"))
            .unwrap();
        assert!(first_after.state.cap_t < 1.0);
    }

    #[test]
    fn enter_animates_shot_progress_and_clamps_short_windows() {
        let mut shot = term();
        shot.enter = true;
        shot.dur = 0.3;
        let plan = plan_frames(&[shot], PlanOptions::default());
        let animation_frames = (0.3_f64 * 0.6 * 30.0).round() as usize;
        assert_eq!(plan.frames.len(), animation_frames + 1);
        assert!(plan.frames[0].state.shot_t < 1.0);
    }

    #[test]
    fn holds_last_at_least_one_frame_and_totals_never_undercut_shots() {
        let mut first = term();
        first.dur = 1.5;
        first.caption = Some("a".into());
        first.cap_key = Some("a".into());
        let mut second = term();
        second.dur = 0.01;
        second.img = Some("frame-b".into());
        second.cap_key = Some("a".into());
        let plan = plan_frames(&[first, second], PlanOptions::default());
        assert!(
            plan.frames
                .iter()
                .all(|frame| frame.duration >= 1.0 / 30.0 - 1e-9)
        );
        assert!(plan.total_seconds >= 1.5 + 0.01 - 1e-9);
    }

    #[test]
    fn required_keyframes_lists_each_terminal_image_once_and_ignores_cards() {
        let mut one = term();
        one.img = Some("one".into());
        let mut two = term();
        two.img = Some("two".into());
        let card = Shot {
            kind: ShotKind::Card,
            dur: 1.0,
            html: Some("<h1>x</h1>".into()),
            img: None,
            title: None,
            caption: None,
            cap_key: None,
            enter: false,
        };
        assert_eq!(
            required_keyframes(&[one.clone(), two, one, card]),
            ["one", "two"]
        );
    }

    #[test]
    fn frozen_hunk_oracle_records_both_identical_pinned_planners() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../port/hunk/oracles/terminal-video-plan.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["baselineOracle"]["passed"], 7);
        assert_eq!(oracle["baselineOracle"]["expectations"], 29);
        assert_eq!(oracle["stableOracle"]["passed"], 7);
        assert_eq!(oracle["stableOracle"]["expectations"], 29);
    }

    #[test]
    fn cli_writes_a_consumable_camel_case_plan() {
        let directory = tempfile::TempDir::new().unwrap();
        let storyboard = directory.path().join("storyboard.json");
        fs::write(
            &storyboard,
            r#"[{"kind":"term","dur":0.1,"img":"one","title":"demo"}]"#,
        )
        .unwrap();
        let output = directory.path().join("nested/plan.json");
        plan_file(
            directory.path(),
            [
                "--storyboard".into(),
                storyboard.to_string_lossy().into_owned(),
                "--output".into(),
                output.to_string_lossy().into_owned(),
                "--fps".into(),
                "24".into(),
            ]
            .into_iter(),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
        assert!(value.get("totalSeconds").is_some());
        assert_eq!(value["frames"][0]["state"]["shotT"], 1.0);
        assert!(value["frames"][0]["state"].get("shot_t").is_none());
    }
}
