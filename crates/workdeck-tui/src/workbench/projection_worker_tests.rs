use super::projection_worker::*;
use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

type Response = mpsc::Sender<Result<String, String>>;
fn controlled() -> (
    ProjectionWorker<String, String>,
    mpsc::Receiver<(String, Response)>,
) {
    let (send, receive) = mpsc::channel();
    // A mutable Send, !Sync session models the core SQLite ownership contract.
    let count = Cell::new(0);
    let worker = ProjectionWorker::new(move |input: String| {
        count.set(count.get() + 1);
        let (reply, response) = mpsc::channel();
        send.send((input, reply)).unwrap();
        response.recv_timeout(Duration::from_secs(3)).unwrap()
    })
    .unwrap();
    (worker, receive)
}
fn next(reads: &mpsc::Receiver<(String, Response)>) -> (String, Response) {
    reads.recv_timeout(Duration::from_secs(3)).unwrap()
}
fn poll_until(
    worker: &mut ProjectionWorker<String, String>,
    mut ready: impl FnMut(&mut ProjectionWorker<String, String>, Option<ReadCompletion<String>>) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let completion = worker.poll();
        if ready(worker, completion) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "projection reader timed out: {worker:?}"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn page_bursts_coalesce_and_obsolete_generations_never_replace_current_rows() {
    let (mut worker, reads) = controlled();
    worker.request(ReadLane::Page, "old page".into()).unwrap();
    let (_, first) = next(&reads);
    for page in 0..40_000 {
        worker
            .request(ReadLane::Page, format!("page {page}"))
            .unwrap();
    }
    assert_eq!(worker.pending_count(), 1);
    first.send(Ok("must never publish".into())).unwrap();
    poll_until(&mut worker, |worker, completion| {
        assert!(completion.is_none());
        worker.pending_count() == 0
    });
    let (request, last) = next(&reads);
    assert_eq!(request, "page 39999");
    last.send(Ok("current page".into())).unwrap();
    poll_until(&mut worker, |_, completion| {
        completion.is_some_and(|value| {
            assert_eq!(value.result.unwrap(), "current page");
            true
        })
    });
    assert!(worker.is_idle());
}

#[test]
fn adopting_a_source_discards_all_old_lanes() {
    let (mut worker, reads) = controlled();
    worker.request(ReadLane::Detail, "A detail".into()).unwrap();
    let (_, first) = next(&reads);
    worker.request(ReadLane::Locate, "A locate".into()).unwrap();
    worker.request(ReadLane::Page, "A page".into()).unwrap();
    worker.invalidate().unwrap();
    let wanted = worker.request(ReadLane::Query, "B query".into()).unwrap();
    first.send(Ok("late A detail".into())).unwrap();
    poll_until(&mut worker, |worker, completion| {
        assert!(completion.is_none());
        worker.pending_count() == 0
    });
    let (request, reply) = next(&reads);
    assert_eq!(request, "B query");
    reply.send(Ok("B query handle".into())).unwrap();
    poll_until(&mut worker, |_, completion| {
        completion.is_some_and(|value| {
            assert_eq!(value.ticket, wanted);
            assert_eq!(value.result.unwrap(), "B query handle");
            true
        })
    });
}

#[test]
fn failed_refresh_is_explicit_and_the_read_session_can_serve_its_old_view() {
    let (mut worker, reads) = controlled();
    worker.request(ReadLane::Refresh, "refresh".into()).unwrap();
    let (_, reply) = next(&reads);
    reply
        .send(Err("source malformed; view generation 1 retained".into()))
        .unwrap();
    poll_until(&mut worker, |_, completion| {
        completion.is_some_and(|value| {
            assert!(value.result.unwrap_err().contains("generation 1 retained"));
            true
        })
    });
    assert!(worker.failure().is_none());
    worker
        .request(ReadLane::Detail, "generation 1 exact row token".into())
        .unwrap();
    let (request, reply) = next(&reads);
    assert_eq!(request, "generation 1 exact row token");
    reply.send(Ok("original source bytes".into())).unwrap();
    poll_until(&mut worker, |_, completion| {
        completion.is_some_and(|value| {
            assert_eq!(value.result.unwrap(), "original source bytes");
            true
        })
    });
}

#[test]
fn shutdown_discards_pending_jobs_and_acknowledges_only_after_owned_read_joins() {
    let (mut worker, reads) = controlled();
    worker
        .request(ReadLane::Query, "in progress".into())
        .unwrap();
    let (_, reply) = next(&reads);
    worker.request(ReadLane::Page, "never run".into()).unwrap();
    worker.begin_shutdown();
    assert_eq!(worker.pending_count(), 0);
    assert!(!worker.finish_shutdown());
    assert!(worker.request(ReadLane::Refresh, "closed".into()).is_err());
    reply.send(Ok("completed read".into())).unwrap();
    poll_until(&mut worker, |worker, completion| {
        assert!(completion.is_none());
        worker.finish_shutdown()
    });
    assert!(reads.try_recv().is_err());
    assert!(worker.is_idle());
}

#[test]
fn panic_is_terminal_and_does_not_reuse_the_mutable_session() {
    let (entered, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let mut worker = ProjectionWorker::new(move |_: String| -> Result<String, String> {
        entered.send(()).unwrap();
        blocked.recv_timeout(Duration::from_secs(3)).unwrap();
        panic!("opaque session failed")
    })
    .unwrap();
    worker.request(ReadLane::Refresh, "panic".into()).unwrap();
    waiting.recv_timeout(Duration::from_secs(3)).unwrap();
    worker
        .request(ReadLane::Page, "never reused".into())
        .unwrap();
    release.send(()).unwrap();
    poll_until(&mut worker, |_, completion| {
        completion.is_some_and(|value| {
            assert!(value.result.unwrap_err().contains("panicked"));
            true
        })
    });
    assert!(worker.failure().unwrap().contains("stale"));
    assert_eq!(worker.pending_count(), 0);
    assert!(worker.request(ReadLane::Detail, "blocked".into()).is_err());
}

#[test]
fn drop_joins_and_releases_the_worker_owned_non_sync_session() {
    struct Released(Arc<AtomicBool>);
    impl Drop for Released {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let released = Arc::new(AtomicBool::new(false));
    let session = Released(released.clone());
    let (entered, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let mut worker = ProjectionWorker::new(move |input: String| {
        let _session = &session;
        entered.send(()).unwrap();
        blocked.recv_timeout(Duration::from_secs(3)).unwrap();
        Ok(input)
    })
    .unwrap();
    worker.request(ReadLane::Query, "read".into()).unwrap();
    waiting.recv_timeout(Duration::from_secs(3)).unwrap();
    let (joined, finished) = mpsc::channel();
    let owner = thread::spawn(move || {
        drop(worker);
        joined.send(()).unwrap();
    });
    assert!(finished.try_recv().is_err());
    assert!(!released.load(Ordering::Acquire));
    release.send(()).unwrap();
    finished.recv_timeout(Duration::from_secs(3)).unwrap();
    owner.join().unwrap();
    assert!(released.load(Ordering::Acquire));
}
