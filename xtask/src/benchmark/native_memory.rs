//! Native process diagnostics, not aliases for JavaScript heapUsed or peak RSS.
use super::*;
#[cfg(target_os = "linux")]
use anyhow::Context;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Snapshot {
    pub rss_bytes: u64,
    /// None when the platform does not expose comparable allocator-zone usage.
    pub malloc_in_use_bytes: Option<u64>,
}

#[cfg(target_os = "macos")]
pub(super) fn snapshot() -> Result<Snapshot> {
    let mut task = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_taskinfo>();
    // SAFETY: writable correctly sized output buffer; querying this process only.
    let copied = unsafe {
        libc::proc_pidinfo(
            std::process::id() as i32,
            libc::PROC_PIDTASKINFO,
            0,
            task.as_mut_ptr().cast(),
            size as i32,
        )
    };
    if copied != size as i32 {
        bail!("proc_pidinfo did not return complete task statistics: {copied}");
    }
    // SAFETY: the kernel returned the full initialized structure above.
    let task = unsafe { task.assume_init() };
    let mut malloc = std::mem::MaybeUninit::<libc::malloc_statistics_t>::uninit();
    // SAFETY: the SDK specifies null zone means all zones; stats is writable.
    unsafe { libc::malloc_zone_statistics(std::ptr::null_mut(), malloc.as_mut_ptr()) };
    // SAFETY: malloc_zone_statistics fills the entire statistics structure.
    let malloc = unsafe { malloc.assume_init() };
    Ok(Snapshot {
        rss_bytes: task.pti_resident_size,
        malloc_in_use_bytes: Some(malloc.size_in_use as u64),
    })
}

// Kernel documentation: https://docs.kernel.org/filesystems/proc.html
// smaps_rollup aggregates page-table accounting, avoiding asynchronous statm RSS.
#[cfg(any(test, target_os = "linux"))]
fn parse_rollup_rss(contents: &str) -> Result<u64> {
    let mut rss = None;
    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some("Rss:") {
            continue;
        }
        if rss.is_some() {
            bail!("duplicate Rss field in smaps_rollup");
        }
        let kib = fields
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing smaps_rollup Rss value"))?
            .parse::<u64>()?;
        if fields.next() != Some("kB") || fields.next().is_some() {
            bail!("invalid smaps_rollup Rss units or trailing fields");
        }
        rss = Some(
            kib.checked_mul(1024)
                .ok_or_else(|| anyhow::anyhow!("smaps_rollup Rss overflow"))?,
        );
    }
    rss.ok_or_else(|| anyhow::anyhow!("missing Rss field in smaps_rollup"))
}

#[cfg(target_os = "linux")]
pub(super) fn snapshot() -> Result<Snapshot> {
    let rollup = std::fs::read_to_string("/proc/self/smaps_rollup")
        .context("reading Linux process resident memory")?;
    Ok(Snapshot {
        rss_bytes: parse_rollup_rss(&rollup)?,
        malloc_in_use_bytes: None,
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(super) fn snapshot() -> Result<Snapshot> {
    bail!("native memory snapshot backend is not implemented on this platform")
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark memory-snapshot accepts no arguments");
    }
    println!("{}", serde_json::to_string(&snapshot()?)?);
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    fn live_process_reports_resident_and_allocator_bytes() {
        let first = snapshot().unwrap();
        assert!(first.rss_bytes > 0);
        assert!(first.malloc_in_use_bytes.unwrap() > 0);
        let allocation = vec![7u8; 4 * 1024 * 1024];
        std::hint::black_box(&allocation);
        let second = snapshot().unwrap();
        assert!(
            second.rss_bytes > 0 && second.malloc_in_use_bytes.unwrap() >= allocation.len() as u64
        );
        assert!(run(["unexpected".into()].into_iter()).is_err());
    }
}

#[test]
fn linux_rollup_rss_is_checked_and_not_confused_with_peak_or_pss() {
    assert_eq!(
        parse_rollup_rss("001-fff ---p [rollup]\nRss: 123 kB\nPss: 99 kB\n").unwrap(),
        123 * 1024
    );
    assert_eq!(parse_rollup_rss("Rss:\t0 kB\n").unwrap(), 0);
    for input in [
        "",
        "Pss: 42 kB",
        "Rss:",
        "Rss: -1 kB",
        "Rss: 1 MB",
        "Rss: 1 kB extra",
        "Rss: 1 kB\nRss: 2 kB",
        "Rss: 18446744073709551615 kB",
    ] {
        assert!(parse_rollup_rss(input).is_err(), "{input}");
    }
    let snapshot = Snapshot {
        rss_bytes: 1024,
        malloc_in_use_bytes: None,
    };
    assert!(serde_json::to_value(snapshot).unwrap()["mallocInUseBytes"].is_null());
}

#[cfg(all(test, target_os = "linux"))]
#[test]
fn linux_live_snapshot_reports_rss_without_inventing_allocator_usage() {
    let sample = snapshot().unwrap();
    assert!(sample.rss_bytes > 0);
    assert!(sample.malloc_in_use_bytes.is_none());
}
