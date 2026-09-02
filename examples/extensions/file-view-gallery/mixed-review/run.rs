//! Build the five-file native gallery review in a disposable Git repository.

use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use tempfile::TempDir;

struct DemoFile {
    target: &'static str,
    before: &'static str,
    after: &'static str,
}

const FILES: &[DemoFile] = &[
    DemoFile {
        target: "README.md",
        before: "mixed-review/fixtures/before/README.md",
        after: "mixed-review/fixtures/after/README.md",
    },
    DemoFile {
        target: "Cargo.toml",
        before: "fixtures/package-dependencies/before/Cargo.toml",
        after: "fixtures/package-dependencies/after/Cargo.toml",
    },
    DemoFile {
        target: "scripts/deploy.py",
        before: "mixed-review/fixtures/before/scripts/deploy.py",
        after: "mixed-review/fixtures/after/scripts/deploy.py",
    },
    DemoFile {
        target: "src/invoice.rs",
        before: "fixtures/change-atlas/before.rs",
        after: "fixtures/change-atlas/after.rs",
    },
    DemoFile {
        target: "styles/theme.css",
        before: "fixtures/css-palette/before.css",
        after: "fixtures/css-palette/after.css",
    },
];

fn run_checked<I, S>(args: I, program: &str, cwd: &Path) -> Result<(), Box<dyn Error>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = Command::new(program).args(args).current_dir(cwd).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} failed with {status}").into())
    }
}

fn install_side(gallery_root: &Path, demo_root: &Path, after: bool) -> std::io::Result<()> {
    for file in FILES {
        let source = gallery_root.join(if after { file.after } else { file.before });
        let destination = demo_root.join(file.target);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, destination)?;
    }
    Ok(())
}

fn changed_paths(demo_root: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let output = Command::new("git")
        .args(["diff", "--name-only"])
        .current_dir(demo_root)
        .output()?;
    if !output.status.success() {
        return Err(format!("git diff --name-only failed with {}", output.status).into());
    }
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .map(str::to_owned)
        .collect())
}

fn main() -> Result<ExitCode, Box<dyn Error>> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments
        .iter()
        .any(|argument| argument != "--prepare-only")
    {
        return Err(
            "usage: workdeck-example-file-view-gallery-mixed-review [--prepare-only]".into(),
        );
    }
    let prepare_only = arguments
        .iter()
        .any(|argument| argument == "--prepare-only");
    let examples_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = examples_root
        .parent()
        .ok_or("examples crate has no repository parent")?;
    let gallery_root = examples_root.join("extensions/file-view-gallery");
    let demo = TempDir::new()?;
    install_side(&gallery_root, demo.path(), false)?;
    run_checked(["init", "--quiet"], "git", demo.path())?;
    run_checked(["config", "user.name", "Workdeck Demo"], "git", demo.path())?;
    run_checked(
        ["config", "user.email", "demo@workdeck.local"],
        "git",
        demo.path(),
    )?;
    run_checked(["add", "."], "git", demo.path())?;
    run_checked(
        [
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            "demo baseline",
        ],
        "git",
        demo.path(),
    )?;
    install_side(&gallery_root, demo.path(), true)?;

    let changed = changed_paths(demo.path())?;
    let mut expected = FILES
        .iter()
        .map(|file| file.target.to_owned())
        .collect::<Vec<_>>();
    expected.sort();
    if changed != expected {
        return Err(format!(
            "prepared review paths differ: expected {expected:?}, got {changed:?}"
        )
        .into());
    }
    if prepare_only {
        println!("Prepared native gallery review: {}", changed.join(", "));
        return Ok(ExitCode::SUCCESS);
    }

    run_checked(
        [
            "run",
            "--quiet",
            "--manifest-path",
            repo_root
                .join("Cargo.toml")
                .to_str()
                .ok_or("non-UTF-8 root")?,
            "-p",
            "xtask",
            "--",
            "extension",
            "stage-example",
            "file-view-gallery",
        ],
        "cargo",
        repo_root,
    )?;

    println!("Opening a five-file working-tree review.");
    println!("Enable previews with F8 on Cargo.toml, src/invoice.rs, and styles/theme.css.");
    println!("README.md and scripts/deploy.py intentionally remain raw diffs.\n");

    let extension = repo_root.join("target/workdeck-extension-examples/file-view-gallery");
    let status = Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--manifest-path",
            repo_root
                .join("Cargo.toml")
                .to_str()
                .ok_or("non-UTF-8 root")?,
            "-p",
            "workdeck-cli",
            "--",
            "--cwd",
            demo.path().to_str().ok_or("non-UTF-8 temporary path")?,
            "--extension",
            extension.to_str().ok_or("non-UTF-8 extension path")?,
            "diff",
            "--mode",
            "stack",
        ])
        .current_dir(repo_root)
        .status()?;
    Ok(ExitCode::from(
        status.code().unwrap_or(1).clamp(0, 255) as u8
    ))
}
