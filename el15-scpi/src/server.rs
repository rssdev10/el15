//! TCP/Telnet style SCPI server (LAN raw-socket, port 5555 by default — same
//! as Rigol's native LXI raw service). Uses `\n` and/or `\r\n` line framing.

use std::net::SocketAddr;
use std::sync::Arc;

use chrono::Local;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info};

use crate::handlers::{dispatch, CmdKind};
use crate::state::SharedState;

#[derive(Clone, Debug)]
pub struct ScpiServerConfig {
    pub bind: SocketAddr,
}

impl Default for ScpiServerConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:5555".parse().unwrap(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScpiLogEntry {
    pub timestamp: chrono::DateTime<chrono::Local>,
    pub peer: SocketAddr,
    pub direction: Direction,
    pub head: String,
    pub command: String,
    pub kind: CmdKind,
    pub reply: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub enum Direction {
    In,
    Out,
}

pub trait LogSink: Send + Sync + 'static {
    fn write(&self, entry: &ScpiLogEntry);
}

pub struct ScpiServer {
    state: SharedState,
    config: ScpiServerConfig,
    sink: Option<Arc<dyn LogSink>>,
}

impl ScpiServer {
    pub fn new(state: SharedState, config: ScpiServerConfig) -> Self {
        Self { state, config, sink: None }
    }

    pub fn with_log_sink(mut self, sink: Arc<dyn LogSink>) -> Self {
        self.sink = Some(sink);
        self
    }

    pub async fn run(self) -> std::io::Result<()> {
        let listener = TcpListener::bind(self.config.bind).await?;
        info!("SCPI server listening on {}", self.config.bind);
        loop {
            let (stream, peer) = listener.accept().await?;
            let state = self.state.clone();
            let sink = self.sink.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_session(state, sink, stream, peer).await {
                    error!("SCPI session {peer} error: {e}");
                }
            });
        }
    }
}

async fn handle_session(
    state: SharedState,
    sink: Option<Arc<dyn LogSink>>,
    stream: TcpStream,
    peer: SocketAddr,
) -> std::io::Result<()> {
    info!("SCPI client connected: {peer}");
    let (rx, mut tx) = stream.into_split();
    let mut reader = BufReader::new(rx).lines();
    while let Some(line) = reader.next_line().await? {
        let line = line.trim_end_matches('\r').to_string();
        if line.is_empty() {
            continue;
        }
        let dispatched = dispatch(&state, &line).await;
        for d in dispatched {
            if let Some(s) = &sink {
                s.write(&ScpiLogEntry {
                    timestamp: Local::now(),
                    peer,
                    direction: Direction::In,
                    head: d.head.clone(),
                    command: line.clone(),
                    kind: d.kind,
                    reply: d.reply.clone(),
                });
            }
            if let Some(reply) = d.reply {
                tx.write_all(reply.as_bytes()).await?;
                tx.write_all(b"\n").await?;
                if let Some(s) = &sink {
                    s.write(&ScpiLogEntry {
                        timestamp: Local::now(),
                        peer,
                        direction: Direction::Out,
                        head: d.head,
                        command: line.clone(),
                        kind: d.kind,
                        reply: Some(reply),
                    });
                }
            }
        }
    }
    info!("SCPI client disconnected: {peer}");
    Ok(())
}

/// Built-in stdout log sink.
pub struct StdoutLogSink;
impl LogSink for StdoutLogSink {
    fn write(&self, e: &ScpiLogEntry) {
        let dir = match e.direction {
            Direction::In => "<--",
            Direction::Out => "-->",
        };
        let kind = match e.kind {
            CmdKind::Query => "Q",
            CmdKind::Write => "W",
            CmdKind::Both  => "Q+W",
        };
        let reply = e.reply.as_deref().unwrap_or("-");
        println!(
            "[{}] {} {} {:>3} {:<24} reply={}",
            e.timestamp.format("%Y-%m-%d %H:%M:%S%.3f %:z"),
            e.peer,
            dir,
            kind,
            e.head,
            reply,
        );
    }
}
