//! MIT translation of Hunk's deterministic benchmark fixture generators (Modem Labs Inc.).

use super::*;
use std::fs;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Options {
    pub file_count: f64,
    pub lines: f64,
    pub changed_start: Option<f64>,
    pub changed_lines: Option<f64>,
    #[serde(default = "default_extension")]
    pub extension: String,
    #[serde(default = "default_prefix")]
    pub prefix: String,
}

fn default_extension() -> String {
    "ts".into()
}
fn default_prefix() -> String {
    "src/bench".into()
}

pub(super) fn source(index: usize, changed: bool, options: &Options) -> String {
    let start = options
        .changed_start
        .unwrap_or((options.lines / 3.0).floor());
    let end = start
        + options
            .changed_lines
            .unwrap_or((options.lines / 6.0).floor().max(4.0));
    (0..array_length(options.lines)).map(|line_index| {
        let line = line_index+1;
        if changed && (start..end).contains(&(line_index as f64)) {
            format!("export function bench{index}_{line}(value: number) {{ return value * {line} + {index}; }}\n")
        } else {
            format!("export function bench{index}_{line}(value: number) {{ return value + {line}; }}\n")
        }
    }).collect()
}

fn array_length(value: f64) -> usize {
    value.floor().clamp(0.0, 9_007_199_254_740_991.0) as usize
}

pub(super) fn patch(options: &Options) -> String {
    let banner =
        regex::Regex::new(r"^Index: [^\r\n\u{2028}\u{2029}]*\n=+\n").expect("literal banner regex");
    (1..=array_length(options.file_count))
        .map(|index| {
            let path = format!("{}{index}.{}", options.prefix, options.extension);
            let patch = workdeck_diff::create_two_files_patch(
                &path,
                &source(index, false, options),
                &source(index, true, options),
                3,
            );
            let stripped = banner.replace(&patch, "");
            stripped
                .trim_end_matches(|c: char| {
                    crate::release_channel::trim_source_whitespace(&c.to_string()).is_empty()
                })
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn temporary(prefix: &str) -> Result<tempfile::TempDir> {
    Ok(tempfile::Builder::new().prefix(prefix).tempdir()?)
}

pub(super) fn git(root: &Path, args: &[&str]) -> Result<String> {
    let result = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let stderr = crate::release_channel::trim_source_whitespace(&stderr);
        bail!(
            "{}",
            if stderr.is_empty() {
                format!("git {} failed", args.join(" "))
            } else {
                stderr.into()
            }
        );
    }
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

pub(super) fn changed_repo(options: &Options) -> Result<tempfile::TempDir> {
    let root = temporary("workdeck-benchmark-repo-")?;
    git(root.path(), &["init"])?;
    git(root.path(), &["config", "user.name", "Benchmark User"])?;
    git(
        root.path(),
        &["config", "user.email", "benchmark@example.com"],
    )?;
    git(root.path(), &["config", "commit.gpgsign", "false"])?;
    for index in 1..=array_length(options.file_count) {
        let path = root
            .path()
            .join(format!("src/bench{index}.{}", options.extension));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, source(index, false, options))?;
    }
    git(root.path(), &["add", "."])?;
    git(root.path(), &["commit", "-m", "initial benchmark fixture"])?;
    for index in 1..=array_length(options.file_count) {
        fs::write(
            root.path()
                .join(format!("src/bench{index}.{}", options.extension)),
            source(index, true, options),
        )?;
    }
    Ok(root)
}

pub(super) fn add_untracked(root: &Path, files: f64, lines: f64) -> Result<()> {
    let options = Options {
        file_count: files,
        lines,
        changed_start: None,
        changed_lines: None,
        extension: default_extension(),
        prefix: default_prefix(),
    };
    for index in 1..=array_length(files) {
        let path = root.join(format!("untracked/new{index}.ts"));
        fs::create_dir_all(path.parent().expect("untracked directory"))?;
        fs::write(path, source(index, true, &options))?;
    }
    Ok(())
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let Some(options) = args.next() else {
        bail!("benchmark synthetic-patch requires OPTIONS_JSON");
    };
    if args.next().is_some() {
        bail!("unexpected synthetic-patch argument");
    }
    let options: Options = serde_json::from_str(&options)?;
    print!("{}", patch(&options));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_dual_pin_sources_and_patches_match_every_byte() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-fixtures.json"
        ))
        .unwrap();
        for case in oracle["cases"]
            .as_array()
            .unwrap()
            .iter()
            .chain(oracle["numeric_cases"].as_array().unwrap())
        {
            let options: Options = serde_json::from_value(case["options"].clone()).unwrap();
            assert_eq!(
                source(1, false, &options),
                case["before"].as_str().unwrap(),
                "{options:?}"
            );
            assert_eq!(
                source(1, true, &options),
                case["after"].as_str().unwrap(),
                "{options:?}"
            );
            assert_eq!(
                patch(&options),
                case["patch"].as_str().unwrap(),
                "{options:?}"
            );
        }
    }

    #[test]
    fn custom_repository_options_preserve_commit_identity_and_untracked_sources() {
        let options = Options {
            file_count: 2.0,
            lines: 8.0,
            changed_start: Some(0.0),
            changed_lines: Some(2.0),
            extension: "rs".into(),
            prefix: "ignored-for-repos".into(),
        };
        let root = changed_repo(&options).unwrap();
        assert_eq!(
            git(root.path(), &["log", "-1", "--format=%an <%ae>:%s"]).unwrap(),
            "Benchmark User <benchmark@example.com>:initial benchmark fixture\n"
        );
        for index in 1..=2 {
            let path = format!("src/bench{index}.rs");
            assert_eq!(
                git(root.path(), &["show", &format!("HEAD:{path}")]).unwrap(),
                source(index, false, &options)
            );
            assert_eq!(
                fs::read_to_string(root.path().join(path)).unwrap(),
                source(index, true, &options)
            );
        }
        add_untracked(root.path(), 2.0, 8.0).unwrap();
        assert_eq!(
            git(root.path(), &["ls-files", "--others", "--exclude-standard"]).unwrap(),
            "untracked/new1.ts\nuntracked/new2.ts\n"
        );
        assert!(!root.path().join("ignored-for-repos").exists());
        assert!(!root.path().join(".agents").exists());
        assert!(git(root.path(), &["show", "nonexistent-revision"]).is_err());
        let path = root.path().to_owned();
        drop(root);
        assert!(!path.exists());
    }

    #[test]
    fn temporary_and_patch_cli_preserve_read_only_and_empty_input_behavior() {
        let root = temporary("workdeck-native-fixture-test-").unwrap();
        assert!(
            root.path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("workdeck-native-fixture-test-")
        );
        assert!(fs::read_dir(root.path()).unwrap().next().is_none());
        assert!(run(std::iter::empty()).is_err());
        assert!(run(["{}".into()].into_iter()).is_err());
        run(["{\"fileCount\":0,\"lines\":0}".into()].into_iter()).unwrap();
    }
}
