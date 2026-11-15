//! Remote live preview client for CraftUI Android runtime.
//! Connects over local ADB reverse TCP tunnel (127.0.0.1:9876) or direct LAN Wi-Fi
//! to receive sub-5ms in-memory AST hot-reloads without APK reinstall.

use craftui_core::UiDispatcher;
use craftui_core::remote::RemotePreviewMessage;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Payload received from host live preview server.
#[derive(Debug, Clone)]
pub struct PendingRemoteReload {
    pub path: String,
    pub source: String,
    pub component: Option<String>,
    pub timestamp: u64,
}

/// Shared thread-safe mailbox coordinating background socket I/O with UI thread frame steps.
pub struct RemotePreviewMailbox {
    pending_reload: Mutex<Option<PendingRemoteReload>>,
    outgoing: Mutex<VecDeque<RemotePreviewMessage>>,
    connected: AtomicBool,
    dispatcher: UiDispatcher,
}

impl RemotePreviewMailbox {
    pub fn new(dispatcher: UiDispatcher) -> Self {
        Self {
            pending_reload: Mutex::new(None),
            outgoing: Mutex::new(VecDeque::new()),
            connected: AtomicBool::new(false),
            dispatcher,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn take_pending_reload(&self) -> Option<PendingRemoteReload> {
        let mut guard = self.pending_reload.lock().ok()?;
        guard.take()
    }

    pub fn send_ack(&self, path: String, duration_ms: f64) {
        if let Ok(mut out) = self.outgoing.lock() {
            out.push_back(RemotePreviewMessage::Ack {
                path,
                status: "ok".into(),
                duration_ms,
            });
        }
    }

    pub fn send_error(&self, path: String, message: String) {
        if let Ok(mut out) = self.outgoing.lock() {
            out.push_back(RemotePreviewMessage::Error { path, message });
        }
    }
}

/// Handle managing the background live preview socket client.
pub struct RemotePreviewClient {
    pub mailbox: Arc<RemotePreviewMailbox>,
    shutdown: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
}

impl RemotePreviewClient {
    /// Connect in a background thread to the host server at `host:port`.
    pub fn start(host: &str, port: u16, dispatcher: UiDispatcher) -> Self {
        let mailbox = Arc::new(RemotePreviewMailbox::new(dispatcher));
        let shutdown = Arc::new(AtomicBool::new(false));

        let mb = mailbox.clone();
        let stop = shutdown.clone();
        let host_string = host.to_string();

        let thread = std::thread::Builder::new()
            .name("craftui-remote-preview".into())
            .spawn(move || {
                run_client_socket_loop(&host_string, port, mb, stop);
            })
            .ok();

        Self {
            mailbox,
            shutdown,
            _thread: thread,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.mailbox.is_connected()
    }

    pub fn take_pending_reload(&self) -> Option<PendingRemoteReload> {
        self.mailbox.take_pending_reload()
    }

    pub fn send_ack(&self, path: String, duration_ms: f64) {
        self.mailbox.send_ack(path, duration_ms);
    }

    pub fn send_error(&self, path: String, message: String) {
        self.mailbox.send_error(path, message);
    }
}

impl Drop for RemotePreviewClient {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

fn run_client_socket_loop(
    host: &str,
    port: u16,
    mailbox: Arc<RemotePreviewMailbox>,
    shutdown: Arc<AtomicBool>,
) {
    let addr_str = format!("{}:{}", host, port);

    while !shutdown.load(Ordering::Relaxed) {
        let addrs = match addr_str.to_socket_addrs() {
            Ok(iter) => iter.collect::<Vec<SocketAddr>>(),
            Err(_) => {
                std::thread::sleep(Duration::from_millis(1500));
                continue;
            }
        };

        let mut stream = None;
        for addr in addrs {
            if let Ok(s) = TcpStream::connect_timeout(&addr, Duration::from_millis(1000)) {
                stream = Some(s);
                break;
            }
        }

        let Some(mut sock) = stream else {
            // Server not up yet or ADB reverse tunnel not initialized — wait quietly
            std::thread::sleep(Duration::from_millis(2000));
            continue;
        };

        let _ = sock.set_nodelay(true);
        let _ = sock.set_read_timeout(Some(Duration::from_millis(250)));
        let _ = sock.set_write_timeout(Some(Duration::from_millis(1000)));

        mailbox.connected.store(true, Ordering::SeqCst);
        tracing::info!(
            "[craftui-remote] Connected to live preview host at {}",
            addr_str
        );

        // Send handshake
        let handshake = RemotePreviewMessage::Connected {
            device: "CraftUI Android Native".into(),
            os: "Android".into(),
            screen_width: None,
            screen_height: None,
            scale_factor: None,
        };
        if let Ok(line) = handshake.to_line() {
            let _ = sock.write_all(line.as_bytes());
            let _ = sock.flush();
        }

        let reader_stream = match sock.try_clone() {
            Ok(s) => s,
            Err(_) => {
                mailbox.connected.store(false, Ordering::SeqCst);
                continue;
            }
        };
        let mut reader = BufReader::new(reader_stream);

        while !shutdown.load(Ordering::Relaxed) {
            // 1. Flush any outgoing replies (ack/error/pong)
            {
                if let Ok(mut q) = mailbox.outgoing.lock() {
                    while let Some(msg) = q.pop_front() {
                        if let Ok(line) = msg.to_line()
                            && (sock.write_all(line.as_bytes()).is_err() || sock.flush().is_err())
                        {
                            break;
                        }
                    }
                }
            }

            // 2. Read incoming messages
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    // Socket closed by remote host
                    break;
                }
                Ok(_) => {
                    if let Ok(msg) = RemotePreviewMessage::from_line(&line) {
                        match msg {
                            RemotePreviewMessage::Reload {
                                path,
                                source,
                                component,
                                timestamp,
                            } => {
                                if let Ok(mut p) = mailbox.pending_reload.lock() {
                                    *p = Some(PendingRemoteReload {
                                        path,
                                        source,
                                        component,
                                        timestamp,
                                    });
                                }
                                // Wake Android event loop immediately
                                mailbox.dispatcher.post(|| {});
                            }
                            RemotePreviewMessage::Ping => {
                                if let Ok(mut q) = mailbox.outgoing.lock() {
                                    q.push_back(RemotePreviewMessage::Pong);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    // Non-blocking read timeout: loop to flush outgoing and check shutdown
                    continue;
                }
                Err(_) => {
                    // Stream broken
                    break;
                }
            }
        }

        mailbox.connected.store(false, Ordering::SeqCst);
        tracing::info!("[craftui-remote] Disconnected from preview host, will retry...");
        std::thread::sleep(Duration::from_millis(1500));
    }
}
