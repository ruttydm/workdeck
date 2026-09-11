//! Disposable, validated source generations for Zola's live preview server.
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::{Child, Command},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

type Snapshot = BTreeMap<PathBuf, Vec<u8>>;

fn collect(root: &Path, directory: &Path, output: &mut Snapshot) -> Result<()> {
    ensure!(
        fs::symlink_metadata(directory)?.file_type().is_dir(),
        "preview source must be a real directory"
    );
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root)?;
        if relative == Path::new("site/public") {
            continue;
        }
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "preview source symlink: {}",
            relative.display()
        );
        if kind.is_dir() {
            collect(root, &path, output)?;
        } else {
            ensure!(kind.is_file(), "preview source is not a regular file");
            output.insert(relative.to_owned(), fs::read(path)?);
        }
    }
    Ok(())
}

fn capture(repo: &Path) -> Result<Snapshot> {
    let mut snapshot = Snapshot::new();
    collect(repo, &repo.join("site"), &mut snapshot)?;
    let skill = Path::new("skills/workdeck-review/SKILL.md");
    let mut path = repo.to_owned();
    for part in skill.components() {
        path.push(part);
        ensure!(
            !fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "preview skill is a symlink"
        );
    }
    snapshot.insert(skill.to_owned(), fs::read(repo.join(skill))?);
    Ok(snapshot)
}

fn write_snapshot(root: &Path, snapshot: &Snapshot) -> Result<()> {
    for (relative, bytes) in snapshot {
        ensure!(
            !relative.as_os_str().is_empty()
                && relative
                    .components()
                    .all(|c| matches!(c, Component::Normal(_))),
            "unsafe preview path"
        );
        let path = root.join(relative);
        fs::create_dir_all(path.parent().context("preview parent")?)?;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?
            .write_all(bytes)?;
    }
    Ok(())
}

fn prepare(source: &Snapshot) -> Result<Snapshot> {
    let generation = tempfile::tempdir()?;
    let root = generation.path();
    write_snapshot(root, source)?;
    crate::site_assets::sbom(root).context("validate preview asset inventory")?;
    crate::skill::check_generated_skills(root).context("validate preview generated skills")?;
    let output = root.join("rendered");
    crate::run_checked(
        &root.join("site"),
        "zola",
        &[
            "build",
            "--output-dir",
            output.to_str().context("preview output UTF-8")?,
        ],
    )
    .context("build preview site")?;
    crate::site_markdown::stage_install_script(root, &output).context("stage preview installer")?;
    crate::site_markdown::emit(root, &output).context("emit preview Markdown exports")?;
    let mut prepared = source.clone();
    for (relative, content) in
        crate::site_markdown::plan(root).context("plan preview Markdown exports")?
    {
        let path = Path::new("site/static").join(relative);
        ensure!(
            prepared.insert(path, content.into_bytes()).is_none(),
            "authored asset collides with preview export"
        );
    }
    Ok(prepared)
}

fn synchronize(root: &Path, previous: &Snapshot, next: &Snapshot) -> Result<()> {
    // Only mutate owned staging files, never the user's source tree. Detect an
    // outside edit before replacing or removing any member of the generation.
    for relative in previous.keys().chain(next.keys()) {
        let mut parent = root.to_owned();
        ensure!(
            fs::symlink_metadata(&parent)?.file_type().is_dir(),
            "preview staging root changed"
        );
        if let Some(directory) = relative.parent() {
            for component in directory.components() {
                parent.push(component);
                match fs::symlink_metadata(&parent) {
                    Ok(metadata) => ensure!(
                        metadata.file_type().is_dir(),
                        "preview parent is not a real directory"
                    ),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        if !previous.contains_key(relative) {
            ensure!(
                fs::symlink_metadata(root.join(relative)).is_err(),
                "unowned preview destination: {}",
                relative.display()
            );
        }
    }
    for (relative, bytes) in previous {
        let path = root.join(relative);
        ensure!(
            fs::symlink_metadata(&path)?.file_type().is_file() && fs::read(&path)? == *bytes,
            "preview staging file changed outside the synchronizer: {}",
            relative.display()
        );
    }
    for relative in previous.keys().filter(|path| !next.contains_key(*path)) {
        let path = root.join(relative);
        fs::remove_file(&path)?;
        let mut parent = path.parent();
        while let Some(directory) = parent.filter(|directory| *directory != root) {
            if fs::remove_dir(directory).is_err() {
                break;
            }
            parent = directory.parent();
        }
    }
    for (relative, bytes) in next {
        if previous.get(relative) == Some(bytes) {
            continue;
        }
        let path = root.join(relative);
        if !previous.contains_key(relative) {
            ensure!(
                fs::symlink_metadata(&path).is_err(),
                "unowned preview destination: {}",
                relative.display()
            );
        }
        let parent = path.parent().context("preview parent")?;
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(bytes)?;
        temporary.persist(&path).map_err(|error| error.error)?;
    }
    Ok(())
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn retire_static_outputs(output: &Path, previous: &Snapshot, next: &Snapshot) -> Result<()> {
    // Zola 0.23.4's static watcher can retain deleted files in its output.
    // Retire only exact former static copies in our disposable output tree.
    for (path, expected) in previous
        .iter()
        .filter(|(path, _)| !next.contains_key(*path))
    {
        let Ok(relative) = path.strip_prefix("site/static") else {
            continue;
        };
        let destination = output.join(relative);
        let mut parent = output.to_owned();
        ensure!(
            fs::symlink_metadata(&parent)?.file_type().is_dir(),
            "preview output root changed"
        );
        if let Some(directory) = relative.parent() {
            for component in directory.components() {
                parent.push(component);
                match fs::symlink_metadata(&parent) {
                    Ok(metadata) => ensure!(
                        metadata.file_type().is_dir(),
                        "preview output parent changed"
                    ),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                    Err(error) => return Err(error.into()),
                }
            }
        }
        match fs::symlink_metadata(&destination) {
            Ok(metadata) => {
                ensure!(
                    metadata.file_type().is_file() && fs::read(&destination)? == *expected,
                    "retired preview output was modified"
                );
                fs::remove_file(destination)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Native HTTP checks of generated exports and deletion/refresh behavior.
pub(crate) fn check(repo: &Path) -> Result<()> {
    let original = capture(repo)?;
    let mut source = original.clone();
    let page = PathBuf::from("site/content/docs/workdeck-preview-check.md");
    ensure!(
        !source.contains_key(&page),
        "preview fixture collides with authored content"
    );
    source.insert(
        page.clone(),
        b"+++\ntitle = 'Preview fixture'\n+++\nPREVIEW_BEFORE\n".to_vec(),
    );
    let first = prepare(&source)?;
    let staging = tempfile::tempdir()?;
    write_snapshot(staging.path(), &first)?;
    let reservation = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let port = reservation.local_addr()?.port();
    drop(reservation);
    let mut server = Server(
        Command::new("zola")
            .args([
                "serve",
                "--force",
                "--interface",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--output-dir",
            ])
            .arg(staging.path().join("public"))
            .current_dir(staging.path().join("site"))
            .spawn()?,
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(1)))
        .build()
        .into();
    let mut await_body = |route: &str, expected: Option<&str>| -> Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            ensure!(
                server.0.try_wait()?.is_none(),
                "preview server exited during HTTP check"
            );
            let response = agent.get(format!("http://127.0.0.1:{port}/{route}")).call();
            let ready = match (response, expected) {
                (Ok(mut response), Some(text)) => {
                    response.status() == 200 && response.body_mut().read_to_string()?.contains(text)
                }
                (Err(ureq::Error::StatusCode(404)), None) => true,
                _ => false,
            };
            if ready {
                return Ok(());
            }
            ensure!(
                std::time::Instant::now() < deadline,
                "preview route did not settle: {route}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    await_body("docs/workdeck-preview-check.md", Some("PREVIEW_BEFORE"))?;
    await_body("llms-full.txt", Some("PREVIEW_BEFORE"))?;
    source.insert(
        page.clone(),
        b"+++\ntitle = 'Preview fixture'\nslug = 'workdeck-preview-renamed'\n+++\nPREVIEW_AFTER\n"
            .to_vec(),
    );
    let second = prepare(&source)?;
    synchronize(staging.path(), &first, &second)?;
    retire_static_outputs(&staging.path().join("public"), &first, &second)?;
    await_body("docs/workdeck-preview-renamed.md", Some("PREVIEW_AFTER"))?;
    await_body("docs/workdeck-preview-renamed/", Some("PREVIEW_AFTER"))?;
    await_body("llms-full.txt", Some("PREVIEW_AFTER"))?;
    await_body("docs/workdeck-preview-check.md", None)?;
    source.insert(page, b"invalid frontmatter".to_vec());
    ensure!(
        prepare(&source).is_err(),
        "invalid preview source was accepted"
    );
    await_body("docs/workdeck-preview-renamed.md", Some("PREVIEW_AFTER"))?;
    ensure!(
        capture(repo)? == original,
        "preview check altered authored sources"
    );
    eprintln!(
        "Workdeck preview HTTP checks passed: export refresh, renamed-route removal, invalid-edit preservation, and source isolation."
    );
    Ok(())
}

pub(crate) fn serve(repo: &Path) -> Result<()> {
    let stopped = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&stopped);
    ctrlc::set_handler(move || signal.store(true, Ordering::SeqCst))?;
    let mut source = capture(repo)?;
    let mut prepared = prepare(&source)?;
    let staging = tempfile::tempdir()?;
    write_snapshot(staging.path(), &prepared)?;
    let output = staging.path().join("public");
    let mut server = Server(
        Command::new("zola")
            .args(["serve", "--force", "--output-dir"])
            .arg(&output)
            .current_dir(staging.path().join("site"))
            .spawn()
            .context("start Zola preview")?,
    );
    eprintln!(
        "Workdeck preview: watching authored site; validated exports are staged outside the repository."
    );
    let mut last_error = None;
    loop {
        if stopped.load(Ordering::SeqCst) {
            return Ok(());
        }
        if let Some(status) = server.0.try_wait()? {
            ensure!(status.success(), "Zola preview exited with {status}");
            return Ok(());
        }
        let update = (|| -> Result<Option<Snapshot>> {
            let candidate = capture(repo)?;
            if candidate == source {
                return Ok(None);
            }
            // Remember failed snapshots so a persistent syntax error is not
            // rebuilt every polling interval. Any source change retries it.
            source = candidate;
            prepare(&source).map(Some)
        })();
        match update {
            Ok(Some(next)) => {
                // A staging I/O failure is fatal: do not describe a partially
                // synchronized generation as the previous valid generation.
                synchronize(staging.path(), &prepared, &next)?;
                retire_static_outputs(&output, &prepared, &next)?;
                prepared = next;
                last_error = None;
                eprintln!("Workdeck preview: validated source and export generation refreshed.");
            }
            Ok(None) => {}
            Err(error) => {
                let message = format!("{error:#}");
                if last_error.as_ref() != Some(&message) {
                    eprintln!("Workdeck preview: keeping last valid generation: {message}");
                    last_error = Some(message);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retiring_static_outputs_checks_ownership_and_leaves_current_files() {
        let output = tempfile::tempdir().unwrap();
        let previous = Snapshot::from([
            (PathBuf::from("site/static/old.md"), b"old".to_vec()),
            (PathBuf::from("site/static/current.md"), b"current".to_vec()),
        ]);
        let next = Snapshot::from([(PathBuf::from("site/static/current.md"), b"current".to_vec())]);
        fs::write(output.path().join("old.md"), "outside edit").unwrap();
        fs::write(output.path().join("current.md"), "current").unwrap();
        assert!(retire_static_outputs(output.path(), &previous, &next).is_err());
        assert_eq!(
            fs::read(output.path().join("old.md")).unwrap(),
            b"outside edit"
        );
        fs::write(output.path().join("old.md"), "old").unwrap();
        retire_static_outputs(output.path(), &previous, &next).unwrap();
        assert!(!output.path().join("old.md").exists());
        assert_eq!(
            fs::read(output.path().join("current.md")).unwrap(),
            b"current"
        );
        retire_static_outputs(output.path(), &previous, &next).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn staging_parent_symlinks_cannot_redirect_sync_or_retirement() {
        let staging = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("page.md"), "same bytes").unwrap();
        fs::create_dir(staging.path().join("site")).unwrap();
        std::os::unix::fs::symlink(outside.path(), staging.path().join("site/static")).unwrap();
        let previous =
            Snapshot::from([(PathBuf::from("site/static/page.md"), b"same bytes".to_vec())]);
        assert!(synchronize(staging.path(), &previous, &Snapshot::new()).is_err());
        assert!(
            retire_static_outputs(
                &staging.path().join("site/static"),
                &previous,
                &Snapshot::new()
            )
            .is_err()
        );
        assert_eq!(
            fs::read(outside.path().join("page.md")).unwrap(),
            b"same bytes"
        );
    }

    #[test]
    fn snapshot_sync_updates_removes_and_preserves_source_bytes() {
        let staged = tempfile::tempdir().unwrap();
        let previous = Snapshot::from([
            (PathBuf::from("site/static/old.md"), b"old".to_vec()),
            (PathBuf::from("site/content/page.md"), b"before".to_vec()),
        ]);
        write_snapshot(staged.path(), &previous).unwrap();
        let next = Snapshot::from([
            (PathBuf::from("site/static/new.md"), b"new".to_vec()),
            (PathBuf::from("site/content/page.md"), b"after".to_vec()),
        ]);
        synchronize(staged.path(), &previous, &next).unwrap();
        assert!(!staged.path().join("site/static/old.md").exists());
        for (path, bytes) in &next {
            assert_eq!(fs::read(staged.path().join(path)).unwrap(), *bytes);
        }
        assert_eq!(previous[Path::new("site/content/page.md")], b"before");
        synchronize(staged.path(), &next, &next).unwrap();
    }

    #[test]
    fn outside_edits_abort_before_removal() {
        let staged = tempfile::tempdir().unwrap();
        let previous = Snapshot::from([
            (PathBuf::from("site/a"), b"owned".to_vec()),
            (PathBuf::from("site/b"), b"remove".to_vec()),
        ]);
        write_snapshot(staged.path(), &previous).unwrap();
        fs::write(staged.path().join("site/a"), "outside edit").unwrap();
        assert!(synchronize(staged.path(), &previous, &Snapshot::new()).is_err());
        assert_eq!(fs::read(staged.path().join("site/b")).unwrap(), b"remove");
    }

    #[cfg(unix)]
    #[test]
    fn capture_rejects_authored_symlinks_but_ignores_public_output() {
        let repo = tempfile::tempdir().unwrap();
        fs::create_dir(repo.path().join("site")).unwrap();
        std::os::unix::fs::symlink("/missing", repo.path().join("site/public")).unwrap();
        let mut snapshot = Snapshot::new();
        collect(repo.path(), &repo.path().join("site"), &mut snapshot).unwrap();
        assert!(snapshot.is_empty());
        std::os::unix::fs::symlink("/missing", repo.path().join("site/content")).unwrap();
        assert!(collect(repo.path(), &repo.path().join("site"), &mut snapshot).is_err());
    }
}
