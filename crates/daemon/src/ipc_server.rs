//! Unix socket server for IPC protocol v0.
//!
//! Each connection gets a reader task (decode lines, forward to the app)
//! and a writer task (encode replies and pushes). All state lives in the
//! app; this module only moves messages.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use flowbuddy_core::ipc::{self, ClientMsg, ServerMsg};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tracing::{debug, warn};

pub type ConnId = u64;

/// Lines longer than this are rejected; protocol messages are tiny.
const MAX_LINE: usize = 16 * 1024;

pub enum ConnEvent {
    Opened {
        id: ConnId,
        tx: mpsc::UnboundedSender<ServerMsg>,
    },
    Msg {
        id: ConnId,
        msg: Box<ClientMsg>,
    },
    Closed {
        id: ConnId,
    },
}

/// Removes the socket file when the server is dropped.
pub struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Bind the socket with mode 0600, replacing a stale socket file.
/// Fails if another daemon is already listening.
pub fn bind(path: &Path) -> io::Result<(UnixListener, SocketGuard)> {
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("another flowbuddyd is listening on {}", path.display()),
            ));
        }
        std::fs::remove_file(path)?;
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok((listener, SocketGuard(path.to_path_buf())))
}

pub async fn serve(listener: UnixListener, events: mpsc::Sender<ConnEvent>) {
    let mut next_id: ConnId = 1;
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(e) => {
                warn!(error = %e, "accept failed");
                continue;
            }
        };
        let id = next_id;
        next_id += 1;
        tokio::spawn(handle_conn(id, stream, events.clone()));
    }
}

async fn handle_conn(id: ConnId, stream: UnixStream, events: mpsc::Sender<ConnEvent>) {
    let (read, mut write) = stream.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();
    if events
        .send(ConnEvent::Opened { id, tx: tx.clone() })
        .await
        .is_err()
    {
        return;
    }

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let mut line = ipc::encode(&msg);
            line.push('\n');
            if write.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });

    let mut lines = BufReader::new(read).lines();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(e) => {
                debug!(conn = id, error = %e, "read failed");
                break;
            }
        };
        if line.len() > MAX_LINE {
            let _ = tx.send(ServerMsg::Error {
                id: None,
                code: ipc::ErrorCode::BadRequest,
                message: "line too long".into(),
            });
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        match ipc::decode_client(&line) {
            Ok(msg) => {
                if events
                    .send(ConnEvent::Msg {
                        id,
                        msg: Box::new(msg),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Err(e) => {
                let _ = tx.send(e.to_server_msg());
                if e.is_fatal() {
                    break;
                }
            }
        }
    }

    let _ = events.send(ConnEvent::Closed { id }).await;
    drop(tx);
    // Let queued replies (e.g. an error before closing) go out.
    let _ = writer.await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    fn temp_socket(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flowbuddy-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[tokio::test]
    async fn socket_is_private_and_replaced_when_stale() {
        let path = temp_socket("perm.sock");
        std::fs::write(&path, b"").unwrap(); // stale leftover
        let (_listener, _guard) = bind(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let second = bind(&path).map(|_| ()).unwrap_err();
        assert_eq!(
            second.kind(),
            io::ErrorKind::AddrInUse,
            "second daemon must refuse to start"
        );
    }

    #[tokio::test]
    async fn messages_are_forwarded_and_errors_answered() {
        let path = temp_socket("conn.sock");
        let (listener, _guard) = bind(&path).unwrap();
        let (ev_tx, mut ev_rx) = mpsc::channel(16);
        tokio::spawn(serve(listener, ev_tx));

        let stream = UnixStream::connect(&path).await.unwrap();
        let (r, mut w) = stream.into_split();
        let mut lines = BufReader::new(r).lines();

        let Some(ConnEvent::Opened { id, tx }) = ev_rx.recv().await else {
            panic!("expected Opened")
        };

        w.write_all(b"{\"v\":0,\"type\":\"focus\",\"app_id\":\"code\",\"pid\":1}\n")
            .await
            .unwrap();
        match ev_rx.recv().await {
            Some(ConnEvent::Msg { id: got, msg }) => {
                assert_eq!(got, id);
                let ClientMsg::Focus { app_id, .. } = *msg else {
                    panic!("expected focus")
                };
                assert_eq!(app_id.as_deref(), Some("code"));
            }
            _ => panic!("expected focus"),
        }

        tx.send(ServerMsg::Ack { id: Some(1) }).unwrap();
        let line = lines.next_line().await.unwrap().unwrap();
        assert_eq!(line, r#"{"v":0,"type":"ack","id":1}"#);

        w.write_all(b"{\"v\":0,\"type\":\"nope\"}\n").await.unwrap();
        let line = lines.next_line().await.unwrap().unwrap();
        assert!(line.contains("unknown_type"), "{line}");

        w.write_all(b"{\"v\":7,\"type\":\"hello\"}\n")
            .await
            .unwrap();
        let line = lines.next_line().await.unwrap().unwrap();
        assert!(line.contains("bad_version"), "{line}");
        drop(tx);
        assert!(matches!(ev_rx.recv().await, Some(ConnEvent::Closed { .. })));
    }
}
