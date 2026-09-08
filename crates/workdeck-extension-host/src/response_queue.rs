//! Bounded transport buffering for serialized native extension operations.

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

/// At most eight complete frames await the serialized consumer. Each frame is
/// independently limited by the protocol reader before entering this queue.
pub const MAX_LEGACY_RESPONSE_FRAMES: usize = 8;

type Frame = Result<String, io::Error>;

enum FrameOwner {
    Response(u64),
    Cli(u64),
    Unknown,
}
impl FrameOwner {
    fn parse(frame: &Frame) -> Self {
        let Ok(line) = frame else {
            return Self::Unknown;
        };
        if let Some(id) = super::json_rpc_response_id(line) {
            return Self::Response(id);
        }
        if let Some(output) = super::parse_cli_output_notification(line) {
            return Self::Cli(output.request_id);
        }
        if let Some(input) = super::parse_cli_stdin_read_notification(line) {
            return Self::Cli(input.request_id);
        }
        Self::Unknown
    }
    fn is_revoked(&self, active: u64) -> bool {
        match *self {
            Self::Response(id) => id < active,
            Self::Cli(id) => id != active,
            Self::Unknown => false,
        }
    }
}

#[derive(Debug)]
struct State {
    frames: VecDeque<Frame>,
    active: Option<u64>,
    writer_alive: bool,
    reader_alive: bool,
}

#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Debug)]
pub(crate) struct ResponseSender(Arc<Shared>);
#[derive(Debug)]
pub(crate) struct ResponseReceiver(Arc<Shared>);
#[derive(Debug)]
pub(crate) struct ResponseLease {
    shared: Arc<Shared>,
    id: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LeaseError {
    Busy,
    Closed,
}

pub(crate) fn response_channel() -> (ResponseSender, ResponseReceiver) {
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            frames: VecDeque::new(),
            active: None,
            writer_alive: true,
            reader_alive: true,
        }),
        changed: Condvar::new(),
    });
    (
        ResponseSender(Arc::clone(&shared)),
        ResponseReceiver(shared),
    )
}

impl ResponseSender {
    pub(crate) fn send(&self, frame: Frame) -> Result<(), mpsc::SendError<Frame>> {
        let frame_owner = FrameOwner::parse(&frame);
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let owner = state.active;
        if owner.is_none() && matches!(&frame_owner, FrameOwner::Unknown) {
            return Err(mpsc::SendError(frame));
        }
        if state.reader_alive && owner.is_some_and(|id| frame_owner.is_revoked(id)) {
            return Ok(());
        }
        while state.reader_alive
            && owner.is_some()
            && state.active == owner
            && state.frames.len() >= MAX_LEGACY_RESPONSE_FRAMES
        {
            state = self
                .0
                .changed
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
        if !state.reader_alive {
            return Err(mpsc::SendError(frame));
        }
        // Unowned or revoked serialized output cannot block routed requests.
        if owner.is_none() || state.active != owner {
            return Ok(());
        }
        state.frames.push_back(frame);
        self.0.changed.notify_all();
        Ok(())
    }

    #[cfg(test)]
    fn try_send(&self, frame: Frame) -> Result<(), mpsc::TrySendError<Frame>> {
        let frame_owner = FrameOwner::parse(&frame);
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !state.reader_alive {
            return Err(mpsc::TrySendError::Disconnected(frame));
        }
        if state.active.is_none() {
            return if matches!(&frame_owner, FrameOwner::Unknown) {
                Err(mpsc::TrySendError::Disconnected(frame))
            } else {
                Ok(())
            };
        }
        if state.active.is_some_and(|id| frame_owner.is_revoked(id)) {
            return Ok(());
        }
        if state.frames.len() >= MAX_LEGACY_RESPONSE_FRAMES {
            return Err(mpsc::TrySendError::Full(frame));
        }
        state.frames.push_back(frame);
        self.0.changed.notify_all();
        Ok(())
    }
}

impl ResponseReceiver {
    pub(crate) fn begin(&self, id: u64) -> Result<ResponseLease, LeaseError> {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !state.writer_alive {
            return Err(LeaseError::Closed);
        }
        if state.active.is_some() {
            return Err(LeaseError::Busy);
        }
        state.active = Some(id);
        Ok(ResponseLease {
            shared: Arc::clone(&self.0),
            id,
        })
    }

    pub(crate) fn try_recv(&self) -> Result<Frame, mpsc::TryRecvError> {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(frame) = state.frames.pop_front() {
            self.0.changed.notify_all();
            return Ok(frame);
        }
        Err(if state.writer_alive {
            mpsc::TryRecvError::Empty
        } else {
            mpsc::TryRecvError::Disconnected
        })
    }

    pub(crate) fn recv_timeout(&self, timeout: Duration) -> Result<Frame, mpsc::RecvTimeoutError> {
        let deadline = Instant::now() + timeout;
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        loop {
            if let Some(frame) = state.frames.pop_front() {
                self.0.changed.notify_all();
                return Ok(frame);
            }
            if !state.writer_alive {
                return Err(mpsc::RecvTimeoutError::Disconnected);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(mpsc::RecvTimeoutError::Timeout);
            }
            state = self
                .0
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
    }

    #[cfg(test)]
    fn recv(&self) -> Result<Frame, mpsc::RecvTimeoutError> {
        self.recv_timeout(Duration::from_secs(2))
    }
}

impl Drop for ResponseSender {
    fn drop(&mut self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .writer_alive = false;
        self.0.changed.notify_all();
    }
}

impl Drop for ResponseReceiver {
    fn drop(&mut self) {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.reader_alive = false;
        state.active = None;
        state.frames.clear();
        self.0.changed.notify_all();
    }
}

impl Drop for ResponseLease {
    fn drop(&mut self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.active == Some(self.id) {
            state.active = None;
            state.frames.clear();
            self.shared.changed.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn exact_queue_limit_preserves_every_frame_and_reports_backpressure() {
        let (sender, receiver) = response_channel();
        let _lease = receiver.begin(1).unwrap();
        for index in 0..MAX_LEGACY_RESPONSE_FRAMES {
            sender.try_send(Ok(index.to_string())).unwrap();
        }
        assert!(matches!(
            sender.try_send(Ok("next".into())),
            Err(mpsc::TrySendError::Full(_))
        ));
        assert_eq!(receiver.try_recv().unwrap().unwrap(), "0");
        sender.try_send(Ok("next".into())).unwrap();
        drop(sender);
        for index in 1..MAX_LEGACY_RESPONSE_FRAMES {
            assert_eq!(receiver.try_recv().unwrap().unwrap(), index.to_string());
        }
        assert_eq!(receiver.try_recv().unwrap().unwrap(), "next");
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn receiver_drop_releases_reader_waiting_on_full_queue() {
        let (sender, receiver) = response_channel();
        let _lease = receiver.begin(1).unwrap();
        for _ in 0..MAX_LEGACY_RESPONSE_FRAMES {
            sender.send(Ok("buffered".into())).unwrap();
        }
        let (started, start) = mpsc::channel();
        let (finished, finish) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            let result = sender.send(Ok("pending".into()));
            finished.send(result.is_err()).unwrap();
        });
        start.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(finish.try_recv(), Err(mpsc::TryRecvError::Empty)));
        drop(receiver);
        assert!(finish.recv_timeout(Duration::from_secs(2)).unwrap());
        worker.join().unwrap();
    }

    #[test]
    fn ending_a_request_releases_backpressure_and_allows_a_new_owner() {
        let (sender, receiver) = response_channel();
        let lease = receiver.begin(1).unwrap();
        assert_eq!(receiver.begin(2).unwrap_err(), LeaseError::Busy);
        for _ in 0..MAX_LEGACY_RESPONSE_FRAMES {
            sender.send(Ok("queued".into())).unwrap();
        }
        let (started, start) = mpsc::channel();
        let (finished, finish) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            sender
                .send(Ok(r#"{"jsonrpc":"2.0","id":1,"result":null}"#.into()))
                .unwrap();
            finished.send(()).unwrap();
            sender
        });
        start.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(lease);
        finish.recv_timeout(Duration::from_secs(2)).unwrap();
        let sender = worker.join().unwrap();
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        let _next = receiver.begin(2).unwrap();
        sender.send(Ok("new owner".into())).unwrap();
        assert_eq!(receiver.recv().unwrap().unwrap(), "new owner");
    }

    #[test]
    fn unowned_and_revoked_frames_cannot_fill_a_future_request_queue() {
        let (sender, receiver) = response_channel();
        for _ in 0..100 {
            sender
                .send(Ok(r#"{"jsonrpc":"2.0","id":1,"result":null}"#.into()))
                .unwrap();
        }
        let lease = receiver.begin(2).unwrap();
        for _ in 0..100 {
            sender
                .send(Ok(r#"{"jsonrpc":"2.0","id":1,"result":null}"#.into()))
                .unwrap();
            sender.send(Ok(r#"{"jsonrpc":"2.0","method":"workdeck/cli/output","params":{"request_id":1,"stream":"stdout","bytes":[120]}}"#.into())).unwrap();
        }
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        // Unknown/future responses are still delivered for normal validation.
        let future = r#"{"jsonrpc":"2.0","id":3,"result":null}"#;
        sender.send(Ok(future.into())).unwrap();
        assert_eq!(receiver.recv().unwrap().unwrap(), future);
        drop(lease);
        drop(sender);
        assert_eq!(receiver.begin(3).unwrap_err(), LeaseError::Closed);
    }

    #[test]
    fn unowned_malformed_frames_are_terminal_not_silently_discarded() {
        let (sender, receiver) = response_channel();
        assert!(sender.send(Ok("malformed".into())).is_err());
        drop(sender);
        assert_eq!(receiver.begin(1).unwrap_err(), LeaseError::Closed);
    }

    #[test]
    fn framing_errors_are_ordered_after_valid_frames() {
        let (sender, receiver) = response_channel();
        let _lease = receiver.begin(1).unwrap();
        sender.send(Ok("frame\n".into())).unwrap();
        sender
            .send(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid frame",
            )))
            .unwrap();
        drop(sender);
        assert_eq!(receiver.recv().unwrap().unwrap(), "frame\n");
        assert_eq!(
            receiver.recv().unwrap().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(receiver.recv().is_err());
    }
}
