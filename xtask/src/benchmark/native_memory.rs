//! Native process diagnostics, not aliases for JavaScript heapUsed or peak RSS.
use super::*;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Snapshot {
    pub rss_bytes: u64,
    pub malloc_in_use_bytes: u64,
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
        malloc_in_use_bytes: malloc.size_in_use as u64,
    })
}

#[cfg(not(target_os = "macos"))]
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
        assert!(first.malloc_in_use_bytes > 0);
        let allocation = vec![7u8; 4 * 1024 * 1024];
        std::hint::black_box(&allocation);
        let second = snapshot().unwrap();
        assert!(second.rss_bytes > 0 && second.malloc_in_use_bytes >= allocation.len() as u64);
        assert!(run(["unexpected".into()].into_iter()).is_err());
    }
}
