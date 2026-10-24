//! The daemon's state and its reaction to every event.
//!
//! `App` is synchronous and takes the current time as input, so the whole
//! pipeline can be tested without sockets or devices.

use std::collections::HashMap;

use flowbuddy_core::ipc::{ClientKind, ClientMsg, Command, ServerMsg, Status};
use flowbuddy_core::policy::CoachingMode;
use serde_json::json;
use tokio::sync::mpsc;

use crate::ipc_server::{ConnEvent, ConnId};

struct Client {
    kind: Option<ClientKind>,
    tx: mpsc::UnboundedSender<ServerMsg>,
}

pub struct App {
    clients: HashMap<ConnId, Client>,
    mode: CoachingMode,
    /// Unix seconds.
    paused_until: Option<u64>,
    keyboards: usize,
}

impl App {
    pub fn new(mode: CoachingMode) -> Self {
        App {
            clients: HashMap::new(),
            mode,
            paused_until: None,
            keyboards: 0,
        }
    }

    pub fn set_keyboards(&mut self, n: usize) {
        self.keyboards = n;
    }

    pub fn on_conn(&mut self, ev: ConnEvent, now_ms: u64) {
        match ev {
            ConnEvent::Opened { id, tx } => {
                self.clients.insert(id, Client { kind: None, tx });
            }
            ConnEvent::Closed { id } => {
                self.clients.remove(&id);
            }
            ConnEvent::Msg { id, msg } => self.on_msg(id, msg, now_ms),
        }
    }

    fn reply(&self, id: ConnId, msg: ServerMsg) {
        if let Some(c) = self.clients.get(&id) {
            let _ = c.tx.send(msg);
        }
    }

    /// Send to every client that stays connected (the extension).
    fn broadcast(&self, msg: ServerMsg) {
        for c in self.clients.values() {
            if c.kind == Some(ClientKind::GnomeExtension) {
                let _ = c.tx.send(msg.clone());
            }
        }
    }

    fn on_msg(&mut self, conn: ConnId, msg: ClientMsg, now_ms: u64) {
        match msg {
            ClientMsg::Hello { client, .. } => {
                if let Some(c) = self.clients.get_mut(&conn) {
                    c.kind = Some(client);
                }
                self.reply(
                    conn,
                    ServerMsg::Welcome {
                        daemon_version: flowbuddy_core::VERSION.into(),
                        mode: self.mode,
                    },
                );
            }
            ClientMsg::Focus { .. } | ClientMsg::Ctx { .. } | ClientMsg::NudgeResponse { .. } => {}
            ClientMsg::Command { id, command } => {
                let reply = self.on_command(id, command, now_ms);
                self.reply(conn, reply);
            }
        }
    }

    fn on_command(&mut self, id: Option<u64>, command: Command, now_ms: u64) -> ServerMsg {
        let now_s = now_ms / 1000;
        match command {
            Command::Status => ServerMsg::Result {
                id,
                data: json!(self.status(now_s)),
            },
            Command::SetMode { mode } => {
                self.mode = mode;
                self.mode_changed();
                ServerMsg::Ack { id }
            }
            Command::Pause { seconds } => {
                self.paused_until = Some(now_s + seconds);
                self.mode_changed();
                ServerMsg::Ack { id }
            }
            Command::Resume => {
                self.paused_until = None;
                self.mode_changed();
                ServerMsg::Ack { id }
            }
            Command::Rules | Command::Rule { .. } | Command::ReportFalsePositive => {
                ServerMsg::Result {
                    id,
                    data: json!([]),
                }
            }
        }
    }

    fn mode_changed(&self) {
        self.broadcast(ServerMsg::ModeChanged {
            mode: self.mode,
            paused_until: self.paused_until,
        });
    }

    pub fn status(&self, now_s: u64) -> Status {
        let mut clients: Vec<ClientKind> = self.clients.values().filter_map(|c| c.kind).collect();
        clients.sort_by_key(|k| *k as u8);
        clients.dedup();
        Status {
            running: true,
            daemon_version: flowbuddy_core::VERSION.into(),
            mode: self.mode,
            paused_until: self.paused_until.filter(|&t| t > now_s),
            context: "unsupported".into(),
            confident: false,
            keyboards: self.keyboards,
            clients,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tokio::sync::mpsc::UnboundedReceiver;

    pub(crate) fn connect(app: &mut App, id: ConnId) -> UnboundedReceiver<ServerMsg> {
        let (tx, rx) = mpsc::unbounded_channel();
        app.on_conn(ConnEvent::Opened { id, tx }, 0);
        rx
    }

    pub(crate) fn send(app: &mut App, id: ConnId, msg: ClientMsg, now_ms: u64) {
        app.on_conn(ConnEvent::Msg { id, msg }, now_ms);
    }

    fn hello(kind: ClientKind) -> ClientMsg {
        ClientMsg::Hello {
            client: kind,
            client_version: "test".into(),
        }
    }

    #[test]
    fn hello_gets_welcome_with_mode() {
        let mut app = App::new(CoachingMode::Passive);
        let mut rx = connect(&mut app, 1);
        send(&mut app, 1, hello(ClientKind::Cli), 0);
        assert_eq!(
            rx.try_recv().unwrap(),
            ServerMsg::Welcome {
                daemon_version: flowbuddy_core::VERSION.into(),
                mode: CoachingMode::Passive
            }
        );
    }

    #[test]
    fn mode_change_is_broadcast_to_extension_only() {
        let mut app = App::new(CoachingMode::Balanced);
        let mut ext = connect(&mut app, 1);
        let mut cli = connect(&mut app, 2);
        send(&mut app, 1, hello(ClientKind::GnomeExtension), 0);
        send(&mut app, 2, hello(ClientKind::Cli), 0);
        ext.try_recv().unwrap();
        cli.try_recv().unwrap();

        let cmd = ClientMsg::Command {
            id: Some(5),
            command: Command::SetMode {
                mode: CoachingMode::Active,
            },
        };
        send(&mut app, 2, cmd, 0);
        assert_eq!(cli.try_recv().unwrap(), ServerMsg::Ack { id: Some(5) });
        assert!(cli.try_recv().is_err());
        assert_eq!(
            ext.try_recv().unwrap(),
            ServerMsg::ModeChanged {
                mode: CoachingMode::Active,
                paused_until: None
            }
        );
    }

    #[test]
    fn pause_shows_in_status_until_it_expires() {
        let mut app = App::new(CoachingMode::Balanced);
        let _rx = connect(&mut app, 1);
        let pause = ClientMsg::Command {
            id: None,
            command: Command::Pause { seconds: 60 },
        };
        send(&mut app, 1, pause, 1_000_000);
        assert_eq!(app.status(1_000).paused_until, Some(1_060));
        assert_eq!(app.status(1_061).paused_until, None);
    }
}
