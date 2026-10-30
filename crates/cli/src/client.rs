//! Blocking IPC client: connect, say hello, send one request, read the reply.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use anyhow::{Context, bail};
use flowbuddy_core::ipc::{self, ClientKind, ClientMsg, Command, ServerMsg};

pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
}

impl Client {
    pub fn connect(kind: ClientKind) -> anyhow::Result<Client> {
        let path = flowbuddy_core::paths::socket();
        let stream = UnixStream::connect(&path).with_context(|| {
            format!(
                "flowbuddyd is not running (no socket at {})",
                path.display()
            )
        })?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut client = Client {
            reader: BufReader::new(stream.try_clone()?),
            writer: stream,
            next_id: 1,
        };
        client.send(&ClientMsg::Hello {
            client: kind,
            client_version: flowbuddy_core::VERSION.into(),
            editor: None,
            pid: None,
            shows_tips: false,
        })?;
        match client.read()? {
            ServerMsg::Welcome { .. } => Ok(client),
            other => bail!("unexpected reply to hello: {other:?}"),
        }
    }

    pub fn send(&mut self, msg: &ClientMsg) -> anyhow::Result<()> {
        let mut line = ipc::encode(msg);
        line.push('\n');
        self.writer.write_all(line.as_bytes())?;
        Ok(())
    }

    fn read(&mut self) -> anyhow::Result<ServerMsg> {
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            bail!("daemon closed the connection");
        }
        Ok(ipc::decode_server(line.trim_end())?)
    }

    /// Send a command and wait for its reply, skipping unrelated pushes.
    pub fn command(&mut self, command: Command) -> anyhow::Result<ServerMsg> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&ClientMsg::Command {
            id: Some(id),
            command,
        })?;
        loop {
            let msg = self.read()?;
            match &msg {
                ServerMsg::Ack { id: Some(got) }
                | ServerMsg::Result { id: Some(got), .. }
                | ServerMsg::Error { id: Some(got), .. }
                    if *got == id =>
                {
                    return match msg {
                        ServerMsg::Error { message, .. } => bail!("{message}"),
                        ok => Ok(ok),
                    };
                }
                ServerMsg::Error {
                    id: None, message, ..
                } => bail!("{message}"),
                _ => continue,
            }
        }
    }

    /// Wait as long as it takes for the daemon's next message.
    pub fn next(&mut self) -> anyhow::Result<ServerMsg> {
        self.writer.set_read_timeout(None)?;
        self.read()
    }

    /// Like [`Client::command`], returning the `data` of a `result`.
    pub fn query(&mut self, command: Command) -> anyhow::Result<serde_json::Value> {
        match self.command(command)? {
            ServerMsg::Result { data, .. } => Ok(data),
            other => bail!("expected a result, got {other:?}"),
        }
    }
}
