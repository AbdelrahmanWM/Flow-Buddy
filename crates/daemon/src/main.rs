mod app;
#[allow(dead_code)] // wired up by the context resolver (#13)
mod buffer;
mod capture;
mod ipc_server;
mod keymap;

use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use flowbuddy_core::context::ContextKind;
use flowbuddy_core::key::KeyStroke;
use flowbuddy_core::policy::CoachingMode;
use tokio::sync::mpsc;
use tracing::info;

use crate::app::App;
use crate::buffer::{ClearReason, KeyBuffer};
use crate::capture::CaptureEvent;
use crate::keymap::{Keymap, KeymapOptions, UsKeymap};

/// Flow Buddy daemon: watches how you work and coaches you locally.
#[derive(Parser)]
#[command(name = "flowbuddyd", version = flowbuddy_core::VERSION)]
struct Args {
    /// Print translated keys to stderr. Development builds only.
    #[cfg(debug_assertions)]
    #[arg(long)]
    debug_keys: bool,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    info!(version = flowbuddy_core::VERSION, "starting");

    let socket = flowbuddy_core::paths::socket();
    let (listener, _socket_guard) = ipc_server::bind(&socket)?;
    info!(socket = %socket.display(), "listening");
    let (conn_tx, mut conn_rx) = mpsc::channel(256);
    let server = tokio::spawn(ipc_server::serve(listener, conn_tx));

    let (cap_tx, mut cap_rx) = mpsc::channel::<CaptureEvent>(1024);
    let capture = tokio::spawn(capture::run(cap_tx));

    let xkb_options = keymap::gnome_xkb_options();
    let options = KeymapOptions::from_xkb(xkb_options.iter().map(String::as_str));
    let mut keymap: Box<dyn Keymap> = Box::new(UsKeymap::new(options));
    let mut buffer = KeyBuffer::default();
    // Until the context resolver lands (#13), every key is dropped.
    let context = ContextKind::Unsupported;
    let mut app = App::new(CoachingMode::default());

    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            ev = cap_rx.recv() => match ev {
                Some(CaptureEvent::Key(raw)) => {
                    let Some(key) = keymap.translate(raw.code, raw.value) else { continue };
                    #[cfg(debug_assertions)]
                    if args.debug_keys {
                        eprintln!("key {key}");
                    }
                    let stroke = KeyStroke { key, repeat: raw.value == 2, at_ms: raw.at_ms };
                    buffer.push(stroke, context);
                }
                Some(CaptureEvent::PointerClick) => {}
                Some(CaptureEvent::Keyboards(n)) => app.set_keyboards(n),
                None => break,
            },
            Some(ev) = conn_rx.recv() => app.on_conn(ev, now_ms()),
            _ = &mut shutdown => {
                info!("shutting down");
                break;
            }
        }
    }
    buffer.clear(ClearReason::Shutdown);
    capture.abort();
    server.abort();
    Ok(())
}

async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}
