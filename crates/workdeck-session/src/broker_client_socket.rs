//! Native blocking WebSocket adapter for producer-side broker connections.

use std::io;
use std::net::TcpStream;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak, mpsc};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use tungstenite::error::Error as WebSocketError;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::{CloseFrame, Message};
use tungstenite::stream::MaybeTlsStream;

use crate::{
    SessionBrokerConnectionError, SessionBrokerSocketCloseEvent, SessionBrokerSocketCloseHandler,
    SessionBrokerSocketErrorHandler, SessionBrokerSocketLike, SessionBrokerSocketMessageEvent,
    SessionBrokerSocketMessageHandler, SessionBrokerSocketOpenHandler,
};

const CONNECTING: u16 = 0;
const OPEN: u16 = 1;
const CLOSING: u16 = 2;
const CLOSED: u16 = 3;
const SOCKET_POLL_INTERVAL: Duration = Duration::from_millis(20);

enum SocketCommand {
    Text(String),
    Close(Option<u16>, Option<String>),
}

#[derive(Default)]
struct SocketHandlers {
    open: Option<SessionBrokerSocketOpenHandler>,
    message: Option<SessionBrokerSocketMessageHandler>,
    close: Option<SessionBrokerSocketCloseHandler>,
    error: Option<SessionBrokerSocketErrorHandler>,
}

impl SocketHandlers {
    fn installed(&self) -> bool {
        self.open.is_some()
            && self.message.is_some()
            && self.close.is_some()
            && self.error.is_some()
    }
}

/// One owner-thread native socket exposing the browser-shaped callback contract used by the core.
pub struct NativeSessionBrokerClientSocket {
    url: String,
    state: AtomicU16,
    sender: mpsc::Sender<SocketCommand>,
    handlers: (Mutex<SocketHandlers>, Condvar),
}

impl std::fmt::Debug for NativeSessionBrokerClientSocket {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeSessionBrokerClientSocket")
            .field("url", &self.url)
            .field("ready_state", &self.ready_state())
            .finish_non_exhaustive()
    }
}

impl NativeSessionBrokerClientSocket {
    pub fn connect(
        url: impl Into<String>,
    ) -> Result<Arc<dyn SessionBrokerSocketLike>, SessionBrokerConnectionError> {
        let (sender, receiver) = mpsc::channel();
        let socket = Arc::new(Self {
            url: url.into(),
            state: AtomicU16::new(CONNECTING),
            sender,
            handlers: (Mutex::new(SocketHandlers::default()), Condvar::new()),
        });
        let weak = Arc::downgrade(&socket);
        thread::Builder::new()
            .name("workdeck-session-producer-socket".into())
            .spawn(move || run_socket(weak, receiver))
            .map_err(|error| SessionBrokerConnectionError::Socket(error.to_string()))?;
        Ok(socket)
    }

    fn notify_open(&self) {
        let callback = self
            .handlers
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .open
            .clone();
        if let Some(callback) = callback {
            callback();
        }
    }

    fn notify_message(&self, data: Value) {
        let callback = self
            .handlers
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .message
            .clone();
        if let Some(callback) = callback {
            callback(SessionBrokerSocketMessageEvent { data });
        }
    }

    fn notify_error(&self) {
        let callback = self
            .handlers
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .error
            .clone();
        if let Some(callback) = callback {
            callback();
        }
    }

    fn notify_close(&self, code: u16, reason: String) {
        if self.state.swap(CLOSED, Ordering::AcqRel) == CLOSED {
            return;
        }
        let callback = self
            .handlers
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .close
            .clone();
        if let Some(callback) = callback {
            callback(SessionBrokerSocketCloseEvent {
                code,
                reason,
                authenticated: None,
            });
        }
    }

    fn wait_until_installed(&self) -> bool {
        let (lock, changed) = &self.handlers;
        let handlers = lock.lock().unwrap_or_else(|error| error.into_inner());
        let handlers = changed
            .wait_while(handlers, |handlers| {
                !handlers.installed() && self.state.load(Ordering::Acquire) == CONNECTING
            })
            .unwrap_or_else(|error| error.into_inner());
        handlers.installed() && self.state.load(Ordering::Acquire) == CONNECTING
    }

    fn update_handler(&self, update: impl FnOnce(&mut SocketHandlers)) {
        let mut handlers = self
            .handlers
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        update(&mut handlers);
        self.handlers.1.notify_all();
    }
}

impl SessionBrokerSocketLike for NativeSessionBrokerClientSocket {
    fn ready_state(&self) -> u16 {
        self.state.load(Ordering::Acquire)
    }

    fn send(&self, data: &str) -> Result<(), String> {
        if self.ready_state() != OPEN {
            return Err("Session broker socket is not open.".into());
        }
        self.sender
            .send(SocketCommand::Text(data.into()))
            .map_err(|_| "Session broker socket is closed.".into())
    }

    fn close(&self, code: Option<u16>, reason: Option<&str>) {
        let previous = self.state.swap(CLOSING, Ordering::AcqRel);
        if matches!(previous, CLOSING | CLOSED) {
            return;
        }
        let _ = self
            .sender
            .send(SocketCommand::Close(code, reason.map(str::to_owned)));
        self.handlers.1.notify_all();
    }

    fn set_on_open(&self, handler: Option<SessionBrokerSocketOpenHandler>) {
        self.update_handler(|handlers| handlers.open = handler);
    }

    fn set_on_message(&self, handler: Option<SessionBrokerSocketMessageHandler>) {
        self.update_handler(|handlers| handlers.message = handler);
    }

    fn set_on_close(&self, handler: Option<SessionBrokerSocketCloseHandler>) {
        self.update_handler(|handlers| handlers.close = handler);
    }

    fn set_on_error(&self, handler: Option<SessionBrokerSocketErrorHandler>) {
        self.update_handler(|handlers| handlers.error = handler);
    }
}

fn run_socket(
    socket: Weak<NativeSessionBrokerClientSocket>,
    receiver: mpsc::Receiver<SocketCommand>,
) {
    let Some(socket) = socket.upgrade() else {
        return;
    };
    if !socket.wait_until_installed() {
        socket.notify_close(1000, String::new());
        return;
    }
    let connected = tungstenite::connect(socket.url.as_str());
    let (mut websocket, _) = match connected {
        Ok(connected) => connected,
        Err(_) => {
            socket.notify_error();
            socket.notify_close(1006, String::new());
            return;
        }
    };
    set_poll_timeout(websocket.get_mut());
    if socket
        .state
        .compare_exchange(CONNECTING, OPEN, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        let _ = websocket.close(None);
        socket.notify_close(1000, String::new());
        return;
    }
    socket.notify_open();

    let mut close_event = None;
    'transport: loop {
        loop {
            match receiver.try_recv() {
                Ok(SocketCommand::Text(text)) => {
                    if websocket.send(Message::Text(text.into())).is_err() {
                        socket.notify_error();
                        close_event = Some((1006, String::new()));
                        break 'transport;
                    }
                }
                Ok(SocketCommand::Close(code, reason)) => {
                    let frame = code.map(|code| CloseFrame {
                        code: CloseCode::from(code),
                        reason: reason.clone().unwrap_or_default().into(),
                    });
                    let _ = websocket.close(frame);
                    close_event = Some((code.unwrap_or(1000), reason.unwrap_or_default()));
                    break 'transport;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    close_event = Some((1000, String::new()));
                    break 'transport;
                }
            }
        }

        match websocket.read() {
            Ok(Message::Text(text)) => socket.notify_message(Value::String(text.to_string())),
            Ok(Message::Binary(_)) => socket.notify_message(Value::Null),
            Ok(Message::Close(frame)) => {
                close_event = Some(frame.map_or((1005, String::new()), |frame| {
                    (u16::from(frame.code), frame.reason.to_string())
                }));
                break;
            }
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
            Err(WebSocketError::Io(error)) if is_poll_timeout(&error) => {}
            Err(WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed) => {
                close_event.get_or_insert((1006, String::new()));
                break;
            }
            Err(_) => {
                socket.notify_error();
                close_event = Some((1006, String::new()));
                break;
            }
        }
    }
    let (code, reason) = close_event.unwrap_or((1006, String::new()));
    socket.notify_close(code, reason);
}

fn set_poll_timeout(stream: &mut MaybeTlsStream<TcpStream>) {
    if let MaybeTlsStream::Plain(stream) = stream {
        let _ = stream.set_read_timeout(Some(SOCKET_POLL_INTERVAL));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    }
}

fn is_poll_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    fn install_handlers(
        socket: &Arc<dyn SessionBrokerSocketLike>,
        opened: Arc<AtomicBool>,
        messages: Arc<Mutex<Vec<Value>>>,
        closed: Arc<Mutex<Vec<SessionBrokerSocketCloseEvent>>>,
        errored: Arc<AtomicBool>,
    ) {
        socket.set_on_open(Some(Arc::new(move || {
            opened.store(true, Ordering::Release);
        })));
        socket.set_on_message(Some(Arc::new(move |event| {
            messages
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(event.data);
        })));
        socket.set_on_close(Some(Arc::new(move |event| {
            closed
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(event);
        })));
        socket.set_on_error(Some(Arc::new(move || {
            errored.store(true, Ordering::Release);
        })));
    }

    fn wait_until(predicate: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if predicate() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("socket fixture timed out");
    }

    #[test]
    fn native_socket_round_trips_text_and_reports_server_close() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut websocket = tungstenite::accept(stream).unwrap();
            let message = websocket.read().unwrap();
            assert_eq!(message.into_text().unwrap(), "producer hello");
            websocket
                .send(Message::Text("daemon hello".into()))
                .unwrap();
            websocket
                .close(Some(CloseFrame {
                    code: CloseCode::Policy,
                    reason: "fixture close".into(),
                }))
                .unwrap();
        });
        let socket =
            NativeSessionBrokerClientSocket::connect(format!("ws://127.0.0.1:{port}")).unwrap();
        let opened = Arc::new(AtomicBool::new(false));
        let messages = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(Mutex::new(Vec::new()));
        let errored = Arc::new(AtomicBool::new(false));
        install_handlers(
            &socket,
            Arc::clone(&opened),
            Arc::clone(&messages),
            Arc::clone(&closed),
            Arc::clone(&errored),
        );
        wait_until(|| opened.load(Ordering::Acquire));
        socket.send("producer hello").unwrap();
        wait_until(|| !closed.lock().unwrap().is_empty());
        assert_eq!(
            messages.lock().unwrap().as_slice(),
            &[Value::String("daemon hello".into())]
        );
        assert_eq!(closed.lock().unwrap()[0].code, 1008);
        assert_eq!(closed.lock().unwrap()[0].reason, "fixture close");
        assert!(!errored.load(Ordering::Acquire));
        server.join().unwrap();
    }

    #[test]
    fn queued_command_results_reach_the_peer_before_local_close() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut websocket = tungstenite::accept(stream).unwrap();
            for index in 0..4 {
                let message = websocket.read().unwrap();
                let value: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                assert_eq!(
                    value,
                    serde_json::json!({"type": "command-result",
                    "requestId": format!("request-{index}"), "ok": true, "result": {}})
                );
            }
            let Message::Close(Some(frame)) = websocket.read().unwrap() else {
                panic!("all queued replies must precede the close frame");
            };
            assert_eq!(frame.code, CloseCode::Normal);
            assert_eq!(frame.reason, "review quit");
        });
        let socket =
            NativeSessionBrokerClientSocket::connect(format!("ws://127.0.0.1:{port}")).unwrap();
        let closed = Arc::new(Mutex::new(Vec::new()));
        let errored = Arc::new(AtomicBool::new(false));
        // on_open runs before transport queue draining: enqueue the entire batch
        // and close here so the ordering assertion does not depend on scheduling.
        let weak = Arc::downgrade(&socket);
        socket.set_on_open(Some(Arc::new(move || {
            let socket = weak.upgrade().unwrap();
            for index in 0..4 {
                socket
                    .send(
                        &serde_json::json!({"type": "command-result",
                    "requestId": format!("request-{index}"), "ok": true, "result": {}})
                        .to_string(),
                    )
                    .unwrap();
            }
            socket.close(Some(1000), Some("review quit"));
            assert!(socket.send("late reply").is_err());
        })));
        socket.set_on_message(Some(Arc::new(|_| {})));
        let observed_closed = Arc::clone(&closed);
        socket.set_on_close(Some(Arc::new(move |event| {
            observed_closed.lock().unwrap().push(event)
        })));
        let observed_error = Arc::clone(&errored);
        socket.set_on_error(Some(Arc::new(move || {
            observed_error.store(true, Ordering::Release)
        })));
        wait_until(|| !closed.lock().unwrap().is_empty());
        server.join().unwrap();
        assert_eq!(closed.lock().unwrap().len(), 1);
        assert_eq!(closed.lock().unwrap()[0].code, 1000);
        assert!(!errored.load(Ordering::Acquire));
    }

    #[test]
    fn native_socket_reports_connection_failure_once() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let socket =
            NativeSessionBrokerClientSocket::connect(format!("ws://127.0.0.1:{port}")).unwrap();
        let opened = Arc::new(AtomicBool::new(false));
        let messages = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(Mutex::new(Vec::new()));
        let errored = Arc::new(AtomicBool::new(false));
        install_handlers(
            &socket,
            Arc::clone(&opened),
            messages,
            Arc::clone(&closed),
            Arc::clone(&errored),
        );
        wait_until(|| !closed.lock().unwrap().is_empty());
        assert!(!opened.load(Ordering::Acquire));
        assert!(errored.load(Ordering::Acquire));
        assert_eq!(closed.lock().unwrap()[0].code, 1006);
        assert_eq!(socket.ready_state(), CLOSED);
    }
}
