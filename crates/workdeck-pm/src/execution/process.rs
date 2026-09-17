use super::*;
use crate::{ErrorCode, PmError, Result, RunBounds};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Instant,
};
#[cfg(unix)]
use std::{sync::atomic::Ordering, time::Duration};

pub(super) fn execute(
    planning: &Path,
    directory: &Path,
    input: &ResolvedInvocation,
    environment: &BTreeMap<OsString, OsString>,
    bounds: &RunBounds,
    control: &RunControl,
    fault: &mut impl FnMut(RunFaultPoint) -> Result<()>,
) -> Result<ProcessObservation> {
    #[cfg(not(unix))]
    {
        let _ = (
            planning,
            directory,
            input,
            environment,
            bounds,
            control,
            fault,
        );
        Err(PmError::new(
            ErrorCode::Unsupported,
            "this platform has no qualified foreground process-group runner",
        ))
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::{CommandExt, ExitStatusExt};
        if input.argv.is_empty()
            || bounds.timeout_seconds == 0
            || bounds.timeout_seconds > 86_400
            || bounds.stdout_bytes as u64 > MAX_LOCAL_LOG_BYTES
            || bounds.stderr_bytes as u64 > MAX_LOCAL_LOG_BYTES
        {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "execution argv or process bounds are invalid",
            ));
        }
        local::safe(planning, directory)?;
        let mut stdout = Output::new(planning, &directory.join("stdout.log"), bounds.stdout_bytes)?;
        let mut stderr = Output::new(planning, &directory.join("stderr.log"), bounds.stderr_bytes)?;
        let start = Instant::now();
        if control.cancellation_requested() {
            return observation(
                None,
                start,
                ProcessTermination::NotRun,
                None,
                None,
                true,
                stdout,
                stderr,
                vec!["canceled_before_spawn".into()],
            );
        }
        let mut command = std::process::Command::new(&input.argv[0]);
        command
            .args(&input.argv[1..])
            .current_dir(&input.cwd)
            .env_clear()
            .envs(environment)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .process_group(0);
        let started_at = chrono::Utc::now();
        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return observation(
                    Some(started_at),
                    start,
                    ProcessTermination::SpawnFailed,
                    None,
                    None,
                    true,
                    stdout,
                    stderr,
                    vec![format!("spawn_failed:{}", error.kind())],
                );
            }
        };
        let mut owned = Owned {
            group: child.id() as libc::pid_t,
            child,
            cleaned: false,
            control: control.clone(),
        };
        let mut out = owned.child.stdout.take().expect("piped stdout");
        let mut err = owned.child.stderr.take().expect("piped stderr");
        nonblocking(&out)?;
        nonblocking(&err)?;
        fault(RunFaultPoint::AfterSpawn)?;
        let mut ending = None;
        let mut termination = ProcessTermination::Exited;
        let status = loop {
            stdout.drain(&mut out)?;
            stderr.drain(&mut err)?;
            if let Some(status) = owned
                .child
                .try_wait()
                .map_err(|e| PmError::io("execution child", e))?
            {
                break Some(status);
            }
            if ending.is_none()
                && (control.cancellation_requested()
                    || start.elapsed() >= Duration::from_secs(bounds.timeout_seconds))
            {
                termination = if control.cancellation_requested() {
                    ProcessTermination::Canceled
                } else {
                    ProcessTermination::TimedOut
                };
                owned.signal(libc::SIGTERM)?;
                ending = Some(Instant::now());
            }
            if let Some(ending) = ending {
                if control.cancellation.load(Ordering::SeqCst) >= 2
                    || ending.elapsed() >= Duration::from_millis(250)
                {
                    owned.signal(libc::SIGKILL)?;
                }
                if ending.elapsed() > Duration::from_secs(2) {
                    termination = ProcessTermination::CleanupFailed;
                    break None;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let cleanup = owned.cleanup();
        // The entire group is terminated before draining residual pipe data. A
        // descendant retaining descriptors cannot turn a completed run into a hang.
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            let progress = stdout.drain(&mut out)? | stderr.drain(&mut err)?;
            if !progress {
                break;
            }
        }
        if !cleanup {
            termination = ProcessTermination::CleanupFailed;
        }
        observation(
            Some(started_at),
            start,
            termination,
            status.as_ref().and_then(|s| s.code()),
            status.as_ref().and_then(|s| s.signal()),
            cleanup,
            stdout,
            stderr,
            Vec::new(),
        )
    }
}

#[cfg(unix)]
fn nonblocking(value: &impl std::os::fd::AsRawFd) -> Result<()> {
    let fd = value.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(PmError::io(
            "execution pipe",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}
#[cfg(unix)]
struct Owned {
    child: std::process::Child,
    group: libc::pid_t,
    cleaned: bool,
    control: RunControl,
}
#[cfg(unix)]
impl Owned {
    fn signal(&self, signal: i32) -> Result<()> {
        if unsafe { libc::kill(-self.group, signal) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(PmError::io("owned execution group", error))
        }
    }
    fn cleanup(&mut self) -> bool {
        if self.cleaned {
            return true;
        }
        let mut okay = self.signal(libc::SIGKILL).is_ok();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    self.cleaned = okay;
                    return okay;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(_) => {
                    okay = false;
                    break;
                }
            }
        }
        self.cleaned = okay && self.child.try_wait().is_ok_and(|s| s.is_some());
        self.cleaned
    }
}
#[cfg(unix)]
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.cleanup() {
            self.control.cleanup_failed.store(true, Ordering::SeqCst);
        }
    }
}
struct Output {
    file: File,
    path: PathBuf,
    limit: usize,
    observed: u64,
    retained: u64,
    hash: Sha256,
}
impl Output {
    fn new(planning: &Path, path: &Path, limit: usize) -> Result<Self> {
        Ok(Self {
            file: local::create_file(planning, path)?,
            path: path
                .strip_prefix(planning)
                .expect("checked local path")
                .into(),
            limit,
            observed: 0,
            retained: 0,
            hash: Sha256::new(),
        })
    }
    fn drain(&mut self, pipe: &mut impl Read) -> Result<bool> {
        let mut progress = false;
        let mut bytes = [0u8; 8192];
        for _ in 0..32 {
            match pipe.read(&mut bytes) {
                Ok(0) => break,
                Ok(count) => {
                    progress = true;
                    self.observed = self.observed.saturating_add(count as u64);
                    let keep = count.min(self.limit.saturating_sub(self.retained as usize));
                    self.file
                        .write_all(&bytes[..keep])
                        .map_err(|e| PmError::io(&self.path, e))?;
                    self.hash.update(&bytes[..keep]);
                    self.retained += keep as u64;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(PmError::io(&self.path, e)),
            }
        }
        Ok(progress)
    }
    fn finish(self) -> Result<LogDescriptor> {
        self.file
            .sync_all()
            .map_err(|e| PmError::io(&self.path, e))?;
        Ok(LogDescriptor {
            path: self.path,
            content: format!("{:x}", self.hash.finalize()).parse()?,
            retained_bytes: self.retained,
            observed_bytes: self.observed,
            truncated: self.observed > self.retained,
        })
    }
}
#[allow(clippy::too_many_arguments)]
fn observation(
    started_at: Option<crate::Timestamp>,
    start: Instant,
    termination: ProcessTermination,
    exit_code: Option<i32>,
    signal: Option<i32>,
    cleanup_complete: bool,
    stdout: Output,
    stderr: Output,
    mut reason_codes: Vec<String>,
) -> Result<ProcessObservation> {
    let reason = match termination {
        ProcessTermination::TimedOut => Some("process_timeout"),
        ProcessTermination::Canceled => Some("process_canceled"),
        ProcessTermination::CleanupFailed => Some("owned_process_cleanup_failed"),
        ProcessTermination::SpawnFailed => Some("process_spawn_failed"),
        ProcessTermination::NotRun => Some("process_not_run"),
        ProcessTermination::Exited if signal.is_some() => Some("process_signaled"),
        ProcessTermination::Exited if exit_code.is_some_and(|code| code != 0) => {
            Some("process_exit_nonzero")
        }
        ProcessTermination::Exited => None,
    };
    if let Some(reason) = reason {
        reason_codes.push(reason.into());
    }
    reason_codes.sort();
    reason_codes.dedup();
    Ok(ProcessObservation {
        started_at,
        finished_at: chrono::Utc::now(),
        elapsed_millis: start.elapsed().as_millis().min(u64::MAX as u128) as u64,
        termination,
        exit_code,
        signal,
        cleanup_complete,
        stdout: stdout.finish()?,
        stderr: stderr.finish()?,
        reason_codes,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{ContentHash, Repository};
    fn fixture(script: &str) -> (tempfile::TempDir, std::path::PathBuf, ResolvedInvocation) {
        let temp = tempfile::tempdir().unwrap();
        let repo = Repository::init(temp.path(), "WD").unwrap();
        let directory = local::prepare(repo.root(), &LocalRunId::new()).unwrap();
        let input = ResolvedInvocation {
            argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
            cwd: temp.path().into(),
            fingerprint: ContentHash::of(script.as_bytes()),
        };
        (temp, directory, input)
    }
    #[test]
    fn captures_separate_bounded_streams_and_actual_exit_without_shell_interpolation() {
        let (temp, directory, input) = fixture("printf stdout; printf stderr >&2; exit 7");
        let output = execute(
            &temp.path().canonicalize().unwrap().join(".workdeck"),
            &directory,
            &input,
            &BTreeMap::new(),
            &RunBounds::default(),
            &RunControl::default(),
            &mut |_| Ok(()),
        )
        .unwrap();
        assert_eq!(output.exit_code, Some(7));
        assert_eq!(output.termination, ProcessTermination::Exited);
        assert_eq!(
            std::fs::read(temp.path().join(".workdeck").join(output.stdout.path)).unwrap(),
            b"stdout"
        );
        assert_eq!(
            std::fs::read(temp.path().join(".workdeck").join(output.stderr.path)).unwrap(),
            b"stderr"
        );
    }
    #[test]
    fn timeout_drains_flooded_output_and_retains_only_requested_bound() {
        let (temp, directory, input) =
            fixture("while :; do printf abcdefghijklmnopqrstuvwxyz; done");
        let bounds = RunBounds {
            timeout_seconds: 1,
            stdout_bytes: 1024,
            stderr_bytes: 1024,
        };
        let start = std::time::Instant::now();
        let output = execute(
            &temp.path().canonicalize().unwrap().join(".workdeck"),
            &directory,
            &input,
            &BTreeMap::new(),
            &bounds,
            &RunControl::default(),
            &mut |_| Ok(()),
        )
        .unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(4));
        assert_eq!(output.termination, ProcessTermination::TimedOut);
        assert!(output.cleanup_complete);
        assert_eq!(output.stdout.retained_bytes, 1024);
        assert!(output.stdout.observed_bytes > 1024);
        assert!(output.stdout.truncated);
    }
    #[test]
    fn canceled_owned_group_cannot_leave_a_descendant_writing_later() {
        let (temp, directory, input) =
            fixture("(/bin/sleep 2; printf escaped > escaped.txt) & wait");
        let control = RunControl::default();
        let output = execute(
            &temp.path().canonicalize().unwrap().join(".workdeck"),
            &directory,
            &input,
            &BTreeMap::new(),
            &RunBounds::default(),
            &control,
            &mut |point| {
                if point == RunFaultPoint::AfterSpawn {
                    control.force_cancel();
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(output.termination, ProcessTermination::Canceled);
        assert!(output.cleanup_complete);
        std::thread::sleep(std::time::Duration::from_millis(2200));
        assert!(!temp.path().join("escaped.txt").exists());
    }
    #[test]
    fn callback_failure_after_spawn_cleans_up_owned_group() {
        let (temp, directory, input) =
            fixture("(/bin/sleep 2; printf escaped > escaped.txt) & wait");
        let mut spawned = false;
        assert!(
            execute(
                &temp.path().canonicalize().unwrap().join(".workdeck"),
                &directory,
                &input,
                &BTreeMap::new(),
                &RunBounds::default(),
                &RunControl::default(),
                &mut |point| {
                    if point == RunFaultPoint::AfterSpawn {
                        spawned = true;
                        Err(PmError::new(ErrorCode::Io, "lost caller"))
                    } else {
                        Ok(())
                    }
                }
            )
            .is_err()
        );
        assert!(spawned);
        std::thread::sleep(std::time::Duration::from_millis(2200));
        assert!(!temp.path().join("escaped.txt").exists());
    }
}
