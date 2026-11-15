//! Craftui CLI Remote Live Preview Server.
//! Broadcasts `.cui` template source changes to connected Android devices over TCP / ADB reverse tunnel.

use craftui_core::remote::RemotePreviewMessage;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct RemotePreviewServer {
    #[allow(dead_code)]
    pub port: u16,
    clients: Arc<Mutex<Vec<TcpStream>>>,
    latest_message: Arc<Mutex<Option<RemotePreviewMessage>>>,
    shutdown: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
}

impl RemotePreviewServer {
    /// Start remote preview server on `0.0.0.0:port` and invoke `adb reverse`.
    pub fn start(port: u16) -> Self {
        try_adb_reverse(port);

        let listener = match TcpListener::bind(("0.0.0.0", port)) {
            Ok(l) => {
                let _ = l.set_nonblocking(true);
                Some(l)
            }
            Err(e) => {
                eprintln!(
                    "\x1b[31m[craftui-remote]\x1b[0m Failed binding TCP port {}: {}",
                    port, e
                );
                None
            }
        };

        let clients: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));
        let latest_message = Arc::new(Mutex::new(None));
        let shutdown = Arc::new(AtomicBool::new(false));

        let c_clients = clients.clone();
        let c_latest = latest_message.clone();
        let c_stop = shutdown.clone();

        let thread = if let Some(l) = listener {
            std::thread::Builder::new()
                .name("craftui-remote-server".into())
                .spawn(move || {
                    run_server_accept_loop(l, c_clients, c_latest, c_stop);
                })
                .ok()
        } else {
            None
        };

        println!(
            "\x1b[32m[craftui-remote]\x1b[0m 🚀 Remote Live Preview server running on 0.0.0.0:{}",
            port
        );
        println!("\x1b[32m[craftui-remote]\x1b[0m Waiting for CraftUI mobile client to connect...");

        Self {
            port,
            clients,
            latest_message,
            shutdown,
            _thread: thread,
        }
    }

    /// Read file from disk and broadcast reload message to all connected clients.
    pub fn broadcast_file(&self, file_path: &Path, component: Option<&str>) {
        let t0 = Instant::now();
        let Ok(source) = std::fs::read_to_string(file_path) else {
            eprintln!(
                "\x1b[31m[craftui-remote]\x1b[0m Failed reading {}",
                file_path.display()
            );
            return;
        };

        let now_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let msg = RemotePreviewMessage::Reload {
            path: file_path.to_string_lossy().to_string(),
            source,
            component: component.map(ToString::to_string),
            timestamp: now_epoch,
        };

        if let Ok(mut latest) = self.latest_message.lock() {
            *latest = Some(msg.clone());
        }

        self.broadcast(&msg);
        let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
        let count = self.connected_clients_count();
        if count > 0 {
            println!(
                "\x1b[32m[craftui-remote]\x1b[0m ⚡ Broadcasted {} to {} connected device(s) ({:.2}ms)",
                file_path.display(),
                count,
                elapsed
            );
        }
    }

    /// Broadcast a single message to all active clients.
    pub fn broadcast(&self, msg: &RemotePreviewMessage) {
        let Ok(line) = msg.to_line() else {
            return;
        };
        let bytes = line.as_bytes();

        let mut clients = match self.clients.lock() {
            Ok(g) => g,
            Err(_) => return,
        };

        let mut active = Vec::with_capacity(clients.len());
        for mut stream in clients.drain(..) {
            if stream.write_all(bytes).is_ok() && stream.flush().is_ok() {
                active.push(stream);
            }
        }
        *clients = active;
    }

    /// Number of currently connected client sockets.
    pub fn connected_clients_count(&self) -> usize {
        self.clients.lock().map(|c| c.len()).unwrap_or(0)
    }
}

impl Drop for RemotePreviewServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

fn try_adb_reverse(port: u16) {
    let port_str = port.to_string();
    let port_arg = format!("tcp:{}", port_str);

    let child = std::process::Command::new("adb")
        .args(["reverse", &port_arg, &port_arg])
        .output();

    match child {
        Ok(out) if out.status.success() => {
            println!(
                "\x1b[32m[craftui-remote]\x1b[0m ✓ ADB reverse tunnel active: {} -> {}",
                port_arg, port_arg
            );
            println!(
                "\x1b[32m[craftui-remote]\x1b[0m Connected USB Android device can reach 127.0.0.1:{}",
                port
            );
        }
        Ok(out) => {
            let err_msg = String::from_utf8_lossy(&out.stderr);
            println!(
                "\x1b[33m[craftui-remote]\x1b[0m ADB reverse skipped (no USB device or error: {})",
                err_msg.trim()
            );
            println!(
                "\x1b[33m[craftui-remote]\x1b[0m Direct Wi-Fi/LAN connection enabled on port {}",
                port
            );
        }
        Err(_) => {
            println!(
                "\x1b[33m[craftui-remote]\x1b[0m 'adb' tool not found in PATH; direct Wi-Fi/LAN connection enabled on port {}",
                port
            );
        }
    }
}

fn run_server_accept_loop(
    listener: TcpListener,
    clients: Arc<Mutex<Vec<TcpStream>>>,
    latest_msg: Arc<Mutex<Option<RemotePreviewMessage>>>,
    shutdown: Arc<AtomicBool>,
) {
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer_addr)) => {
                let _ = stream.set_nodelay(true);
                let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
                let _ = stream.set_write_timeout(Some(Duration::from_millis(1000)));

                println!(
                    "\x1b[32m[craftui-remote]\x1b[0m 📱 Connected device client from {}",
                    peer_addr
                );

                // Immediately send current live UI source if available
                if let Ok(guard) = latest_msg.lock()
                    && let Some(msg) = guard.as_ref()
                    && let Ok(line) = msg.to_line()
                {
                    let mut s = match stream.try_clone() {
                        Ok(cloned) => cloned,
                        Err(_) => continue,
                    };
                    let _ = s.write_all(line.as_bytes());
                    let _ = s.flush();
                }

                let reader_stream = match stream.try_clone() {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                // Add to client list
                if let Ok(mut guard) = clients.lock() {
                    guard.push(stream);
                }

                // Spawn reader thread for client notifications (Connected, Ack, Error)
                std::thread::Builder::new()
                    .name("craftui-client-reader".into())
                    .spawn(move || {
                        let mut reader = BufReader::new(reader_stream);
                        let mut line = String::new();
                        loop {
                            line.clear();
                            match reader.read_line(&mut line) {
                                Ok(0) => break, // EOF
                                Ok(_) => {
                                    if let Ok(msg) = RemotePreviewMessage::from_line(&line) {
                                        match msg {
                                            RemotePreviewMessage::Connected {
                                                device,
                                                os,
                                                ..
                                            } => {
                                                println!(
                                                    "\x1b[32m[craftui-remote]\x1b[0m Handshake confirmed: {} ({})",
                                                    device, os
                                                );
                                            }
                                            RemotePreviewMessage::Ack {
                                                path,
                                                duration_ms,
                                                ..
                                            } => {
                                                println!(
                                                    "\x1b[32m[craftui-remote]\x1b[0m ✓ Remote device reloaded {} in {:.2}ms",
                                                    path, duration_ms
                                                );
                                            }
                                            RemotePreviewMessage::Error { path, message } => {
                                                eprintln!(
                                                    "\x1b[31m[craftui-remote]\x1b[0m ✗ Device reported reload error in {}: {}",
                                                    path, message
                                                );
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                Err(ref e)
                                    if e.kind() == std::io::ErrorKind::WouldBlock
                                        || e.kind() == std::io::ErrorKind::TimedOut =>
                                {
                                    std::thread::sleep(Duration::from_millis(50));
                                    continue;
                                }
                                Err(_) => break,
                            }
                        }
                    })
                    .ok();
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    #[test]
    fn test_remote_server_broadcast_and_client_ack_roundtrip() {
        let port = 19876;
        let server = RemotePreviewServer::start(port);

        // Connect a mock device client
        let mut client =
            TcpStream::connect(("127.0.0.1", port)).expect("must connect to remote preview server");
        client.set_nodelay(true).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();

        // Device sends Connected handshake
        let handshake = RemotePreviewMessage::Connected {
            device: "Test Android Device".into(),
            os: "Android 15".into(),
            screen_width: Some(1080.0),
            screen_height: Some(2400.0),
            scale_factor: Some(2.625),
        };
        client
            .write_all(handshake.to_line().unwrap().as_bytes())
            .unwrap();
        client.flush().unwrap();

        // The server accepts connections on a polling thread, so wait for the
        // client to be registered instead of assuming a fixed delay is enough.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while server.connected_clients_count() == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            server.connected_clients_count() >= 1,
            "the server did not register the client within 10 seconds"
        );

        // Host broadcasts a reload message
        let reload = RemotePreviewMessage::Reload {
            path: "ui/scanapp.cui".into(),
            source: "component App { Text { text: \"Hello Remote Live Preview\"; } }".into(),
            component: Some("App".into()),
            timestamp: 12345678,
        };
        server.broadcast(&reload);

        // Client reads reload message
        let mut reader = BufReader::new(client.try_clone().unwrap());
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("must read broadcasted line");
        let received = RemotePreviewMessage::from_line(&line).expect("must parse reload message");
        assert_eq!(received, reload);

        // Client sends Ack back
        let ack = RemotePreviewMessage::Ack {
            path: "ui/scanapp.cui".into(),
            status: "ok".into(),
            duration_ms: 3.2,
        };
        client.write_all(ack.to_line().unwrap().as_bytes()).unwrap();
        client.flush().unwrap();

        std::thread::sleep(Duration::from_millis(100));
    }
}
