mod app;
mod buffer;
mod capture;
mod config_watch;
mod ipc_server;
mod keymap;
mod notify;
mod resolver;

use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use std::time::Duration;

use flowbuddy_core::config::{Config, State};
use flowbuddy_core::key::KeyStroke;
use flowbuddy_core::paths;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::app::App;
use crate::capture::CaptureEvent;
use crate::keymap::{Keymap, KeymapOptions, UsKeymap};
use crate::resolver::{Procfs, Resolver};

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

    let config_path = paths::config_file();
    let config = Config::load(&config_path).unwrap_or_else(|e| {
        warn!(error = %e, "invalid config, using defaults");
        Config::default()
    });
    let state_path = paths::state_file();
    let state = State::load(&state_path);

    let socket = paths::socket();
    let (listener, _socket_guard) = ipc_server::bind(&socket)?;
    info!(socket = %socket.display(), "listening");
    let (conn_tx, mut conn_rx) = mpsc::channel(256);
    let server = tokio::spawn(ipc_server::serve(listener, conn_tx));

    let (cap_tx, mut cap_rx) = mpsc::channel::<CaptureEvent>(1024);
    let capture = tokio::spawn(capture::run(cap_tx));

    let xkb_options = config
        .keyboard
        .xkb_options
        .clone()
        .unwrap_or_else(keymap::gnome_xkb_options);
    let options = KeymapOptions::from_xkb(xkb_options.iter().map(String::as_str));
    let mut keymap: Box<dyn Keymap> = Box::new(UsKeymap::new(options));
    let resolver = Resolver::new(app::app_ids(&config), Box::new(Procfs));
    let mut app =
        App::new(config, state, resolver, Box::new(notify::NotifySend)).with_state_path(state_path);
    let (cfg_tx, mut cfg_rx) = mpsc::channel::<()>(4);
    let watcher = tokio::spawn(config_watch::run(config_path.clone(), cfg_tx));
    let mut ticker = tokio::time::interval(Duration::from_secs(1));

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
                    app.on_key(stroke, now_ms());
                }
                Some(CaptureEvent::PointerClick) => app.on_pointer_click(),
                Some(CaptureEvent::Keyboards(n)) => app.set_keyboards(n),
                None => break,
            },
            Some(ev) = conn_rx.recv() => app.on_conn(ev, now_ms()),
            _ = ticker.tick() => app.tick(now_ms()),
            Some(()) = cfg_rx.recv() => match Config::load(&config_path) {
                Ok(config) => app.reload_config(config),
                Err(e) => warn!(error = %e, "config not reloaded"),
            },
            _ = &mut shutdown => {
                info!("shutting down");
                break;
            }
        }
    }
    drop(app); // clears the key buffer
    capture.abort();
    server.abort();
    watcher.abort();
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
