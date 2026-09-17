//! One owned, mutable read session; pending work is bounded and coalesced.
//!
//! The closure owns the projection store and its immutable SQLite handles.
//! Neither a connection nor a mutable draft ever crosses the worker boundary.
use std::{
    collections::BTreeMap,
    io,
    sync::mpsc,
    thread::{self, JoinHandle},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ReadLane {
    Refresh,
    Query,
    Page,
    Board,
    Locate,
    Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReadTicket {
    pub epoch: u64,
    pub sequence: u64,
    pub lane: ReadLane,
}

#[derive(Debug)]
struct Job<I> {
    ticket: ReadTicket,
    input: I,
}

#[derive(Debug)]
pub(super) struct ReadCompletion<O> {
    pub ticket: ReadTicket,
    pub result: Result<O, String>,
    terminal: bool,
}

#[derive(Debug)]
pub(super) struct ProjectionWorker<I, O> {
    sender: Option<mpsc::Sender<Job<I>>>,
    receiver: mpsc::Receiver<ReadCompletion<O>>,
    worker: Option<JoinHandle<()>>,
    pending: BTreeMap<ReadLane, Job<I>>,
    latest: BTreeMap<ReadLane, ReadTicket>,
    active: Option<ReadTicket>,
    epoch: u64,
    sequence: u64,
    stopped: bool,
    failure: Option<String>,
}

impl<I: Send + 'static, O: Send + 'static> ProjectionWorker<I, O> {
    pub fn new(mut read: impl FnMut(I) -> Result<O, String> + Send + 'static) -> io::Result<Self> {
        let (sender, jobs) = mpsc::channel::<Job<I>>();
        let (completed, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("workdeck-projection-reader".into())
            .spawn(move || {
                while let Ok(job) = jobs.recv() {
                    let result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read(job.input)));
                    let panicked = result.is_err();
                    let result = result.unwrap_or_else(|_| {
                        Err("Projection reader panicked; retained data is stale".into())
                    });
                    if completed
                        .send(ReadCompletion {
                            ticket: job.ticket,
                            result,
                            terminal: panicked,
                        })
                        .is_err()
                        || panicked
                    {
                        // Do not reuse an opaque mutable read session after unwind.
                        break;
                    }
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            receiver,
            worker: Some(worker),
            pending: BTreeMap::new(),
            latest: BTreeMap::new(),
            active: None,
            epoch: 0,
            sequence: 0,
            stopped: false,
            failure: None,
        })
    }
}

impl<I, O> ProjectionWorker<I, O> {
    /// The adapter increments the epoch when adopting another source or query
    /// generation. A retained, explicitly opened citation is independent state.
    pub fn invalidate(&mut self) -> io::Result<()> {
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or_else(|| io::Error::other("Projection epoch exhausted"))?;
        self.pending.clear();
        self.latest.clear();
        Ok(())
    }

    pub fn request(&mut self, lane: ReadLane, input: I) -> io::Result<ReadTicket> {
        if self.stopped {
            return Err(io::Error::other("Projection reader is stopped"));
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| io::Error::other("Projection request sequence exhausted"))?;
        let ticket = ReadTicket {
            epoch: self.epoch,
            sequence: self.sequence,
            lane,
        };
        self.latest.insert(lane, ticket);
        self.pending.insert(lane, Job { ticket, input });
        self.dispatch();
        Ok(ticket)
    }

    #[cfg(test)]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
    pub fn is_idle(&self) -> bool {
        self.active.is_none() && self.pending.is_empty()
    }
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// Only the latest request in the current source/query epoch can publish.
    /// Errors are delivered to the adapter, which retains its last good data.
    pub fn poll(&mut self) -> Option<ReadCompletion<O>> {
        let completion = match self.receiver.try_recv() {
            Ok(completion) => {
                self.active = None;
                if completion.terminal {
                    self.fail("Projection reader panicked; retained data is stale");
                }
                Some(completion)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                if !self.stopped {
                    self.fail("Projection reader disconnected; retained data is stale");
                }
                self.active.take().map(|ticket| ReadCompletion {
                    ticket,
                    result: Err("Projection reader disconnected; retained data is stale".into()),
                    terminal: true,
                })
            }
        };
        let completion = completion.filter(|value| {
            value.ticket.epoch == self.epoch
                && self.latest.get(&value.ticket.lane) == Some(&value.ticket)
        });
        self.dispatch();
        completion
    }

    fn dispatch(&mut self) {
        if self.active.is_some() || self.stopped {
            return;
        }
        // Oldest pending lane first prevents refresh polling from starving a
        // requested detail. Each lane still contains only its newest request.
        let key = self
            .pending
            .iter()
            .min_by_key(|(_, job)| job.ticket.sequence)
            .map(|(key, _)| *key);
        let Some(job) = key.and_then(|key| self.pending.remove(&key)) else {
            return;
        };
        let ticket = job.ticket;
        if self
            .sender
            .as_ref()
            .is_some_and(|sender| sender.send(job).is_ok())
        {
            self.active = Some(ticket);
        } else {
            self.fail("Projection reader disconnected; retained data is stale");
        }
    }

    fn fail(&mut self, message: &str) {
        self.failure = Some(message.into());
        self.stopped = true;
        self.pending.clear();
        self.sender.take();
    }

    /// Close input immediately; an already running bounded read stays owned.
    /// Dropping the owner joins the reader; the host does so outside its shell lock.
    pub fn begin_shutdown(&mut self) {
        self.stopped = true;
        self.pending.clear();
        self.latest.clear();
        self.sender.take();
    }

    #[cfg(test)]
    pub fn finish_shutdown(&mut self) -> bool {
        if !self.stopped {
            return false;
        }
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return false;
        }
        self.join();
        true
    }

    fn join(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.active = None;
    }
}

impl<I, O> Drop for ProjectionWorker<I, O> {
    fn drop(&mut self) {
        self.begin_shutdown();
        self.join();
    }
}
