//! Native CLI composition for Hunk's MIT release benchmark comparison script.

use super::*;
use std::io::Write;

#[derive(Debug)]
struct Options {
    release_dir: PathBuf,
    version: String,
    head: Option<PathBuf>,
    base: Option<PathBuf>,
    out: Option<PathBuf>,
    summary: Option<PathBuf>,
}

fn resolve(cwd: &Path, value: &str) -> Result<PathBuf> {
    let mut result = PathBuf::new();
    for component in std::path::absolute(cwd.join(value))?.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    Ok(result)
}

fn parse(
    root: &Path,
    cwd: &Path,
    version: String,
    mut args: impl Iterator<Item = String>,
) -> Result<Options> {
    let mut options = Options {
        release_dir: root.join("benchmarks/release"),
        version,
        head: None,
        base: None,
        out: None,
        summary: None,
    };
    while let Some(arg) = args.next() {
        if !matches!(
            arg.as_str(),
            "--release-dir" | "--version" | "--head" | "--base" | "--out" | "--summary"
        ) {
            bail!("Unknown release benchmark comparison argument: {arg}");
        }
        let value = args
            .next()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing value for {arg}"))?;
        match arg.as_str() {
            "--version" => options.version = value,
            "--release-dir" => options.release_dir = resolve(cwd, &value)?,
            "--head" => options.head = Some(resolve(cwd, &value)?),
            "--base" => options.base = Some(resolve(cwd, &value)?),
            "--out" => options.out = Some(resolve(cwd, &value)?),
            "--summary" => options.summary = Some(resolve(cwd, &value)?),
            _ => unreachable!(),
        }
    }
    release_version(&options.version)?;
    Ok(options)
}

fn load(path: &Path) -> Result<Run> {
    let run: Run = serde_json::from_slice(&std::fs::read(path)?)?;
    if run.version != 1 {
        bail!("Invalid benchmark result file: {}", path.display());
    }
    Ok(run)
}

fn execute(options: &Options, output: &mut impl Write) -> Result<()> {
    let head_path = options.head.clone().unwrap_or_else(|| {
        options
            .release_dir
            .join(format!("bench-{}.json", options.version))
    });
    if !head_path.exists() {
        bail!(
            "Missing release benchmark {}. Capture the native release benchmark before tagging this release.",
            head_path.display()
        );
    }
    let (base_label, base_path) = if let Some(base) = &options.base {
        (base.display().to_string(), base.clone())
    } else {
        previous_release(&options.version,&options.release_dir)?.ok_or_else(|| anyhow::anyhow!("Missing previous release benchmark in {}. Backfill at least one lower stable release benchmark before releasing.",options.release_dir.display()))?
    };
    let comparison = compare(
        &load(&base_path)?,
        &load(&head_path)?,
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    );
    if let Some(path) = &options.out {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            path,
            format!("{}\n", serde_json::to_string_pretty(&comparison)?),
        )?;
    }
    let head_label = head_path.file_name().unwrap_or_default().to_string_lossy();
    let report = markdown(&comparison, &base_label, &head_label);
    output.write_all(report.as_bytes())?;
    if let Some(path) = &options.summary {
        let mut summary = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(summary, "\n{}", report.trim_end_matches('\n'))?;
    }
    if comparison.failed {
        bail!(
            "Release benchmark gate failed. This historical report cannot waive the strict semantic-port performance gate."
        );
    }
    let platform = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(windows) {
        "win32"
    } else {
        std::env::consts::OS
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        "x86" => "ia32",
        other => other,
    };
    writeln!(
        output,
        "Release benchmark gate passed on {platform}/{arch}."
    )?;
    Ok(())
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let root = crate::repo_root()?;
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(&root)
        .no_deps()
        .exec()?;
    let version = metadata
        .packages
        .iter()
        .find(|p| p.name.as_str() == "workdeck-cli")
        .ok_or_else(|| anyhow::anyhow!("workspace does not contain workdeck-cli"))?
        .version
        .to_string();
    let options = parse(&root, &std::env::current_dir()?, version, args)?;
    execute(&options, &mut std::io::stdout().lock())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_cli_selects_default_baseline_writes_report_and_appends_summary() {
        let root = tempfile::tempdir().unwrap();
        let releases = root.path().join("benchmarks/release");
        std::fs::create_dir_all(&releases).unwrap();
        for (version, value) in [("0.1.0", 100.0), ("0.2.0", 110.0)] {
            std::fs::write(releases.join(format!("bench-{version}.json")),serde_json::to_vec(&serde_json::json!({"version":1,"results":[aggregate("fixture","render_ms",vec![value])]})).unwrap()).unwrap();
        }
        let options = parse(
            root.path(),
            root.path(),
            "0.2.0".into(),
            [
                "--out",
                "reports/comparison.json",
                "--summary",
                "summary.md",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        let mut output = Vec::new();
        execute(&options, &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Base: `0.1.0`"));
        assert!(output.contains("Head: `bench-0.2.0.json`"));
        assert!(output.contains("Release benchmark gate passed on"));
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(options.out.as_ref().unwrap()).unwrap()).unwrap();
        assert_eq!(report["failed"], false);
        let first = std::fs::read_to_string(options.summary.as_ref().unwrap()).unwrap();
        execute(&options, &mut Vec::new()).unwrap();
        assert_eq!(
            std::fs::read_to_string(options.summary.as_ref().unwrap()).unwrap(),
            format!("{first}{first}")
        );
        assert!(!root.path().join(".agents").exists());
    }

    #[test]
    fn comparison_cli_reports_failure_after_writing_evidence_and_rejects_missing_inputs() {
        let root = tempfile::tempdir().unwrap();
        let parse = |args: &[&str]| {
            parse(
                root.path(),
                root.path(),
                "0.2.0".into(),
                args.iter().map(|s| (*s).to_owned()),
            )
        };
        let options = parse(&[
            "--base",
            "base.json",
            "--head",
            "head.json",
            "--out",
            "out.json",
        ])
        .unwrap();
        assert!(
            execute(&options, &mut Vec::new())
                .unwrap_err()
                .to_string()
                .contains("Missing release benchmark")
        );
        for (name, value) in [("base.json", 100.0), ("head.json", 150.0)] {
            std::fs::write(root.path().join(name),serde_json::to_vec(&serde_json::json!({"version":1,"results":[aggregate("fixture","render_ms",vec![value])]})).unwrap()).unwrap();
        }
        let mut output = Vec::new();
        assert!(execute(&options, &mut output).is_err());
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("1 material benchmark regression found")
        );
        assert!(options.out.unwrap().exists());
        let automatic = parse(&["--head", "head.json"]).unwrap();
        assert!(
            execute(&automatic, &mut Vec::new())
                .unwrap_err()
                .to_string()
                .contains("Missing previous release benchmark")
        );
        for args in [
            vec!["--version", "invalid"],
            vec!["--out"],
            vec!["--unknown"],
        ] {
            assert!(parse(&args).is_err());
        }
    }
}
