mod app;
mod buffer;
mod capture;
mod claude;
mod config_watch;
mod focus_x11;
mod ipc_server;
mod keymap;
mod memory;
mod notify;
mod recorder;
mod resolver;
mod timing;
mod typing;

use std::time::{Instant, SystemTime, UNIX_EPOCH};

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
use crate::resolver::{Procfs, Resolver, TmuxCli};

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

    let mut xkb_options = config
        .keyboard
        .xkb_options
        .clone()
        .unwrap_or_else(keymap::gnome_xkb_options);
    let mut options = KeymapOptions::from_xkb(xkb_options.iter().map(String::as_str));
    let (mut keymap, layout, note) = make_keymap(&config, &xkb_options, options, None);
    let resolver =
        Resolver::new(app::app_ids(&config), Box::new(Procfs)).with_tmux(Box::new(TmuxCli));
    let keymap_config = config.clone();
    let mut app =
        App::new(config, state, resolver, Box::new(notify::NotifySend)).with_state_path(state_path);
    app.set_keymap_layout(layout, note);
    let db = paths::database();
    match flowbuddy_store::Store::open(&db) {
        Ok(store) => app = app.with_store(store),
        Err(e) => {
            warn!(path = %db.display(), error = %e, "stats disabled: could not open database")
        }
    }
    let (cfg_tx, mut cfg_rx) = mpsc::channel::<()>(4);
    let watcher = tokio::spawn(config_watch::run(config_path.clone(), cfg_tx));
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    // Outside GNOME there's no extension to say what has focus; on X11 the
    // daemon can see it itself.
    let (x11_tx, mut x11_rx) = mpsc::unbounded_channel::<focus_x11::Focus>();
    if focus_x11::wanted() {
        info!("watching the focused window on X11");
        focus_x11::watch(x11_tx);
    }

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
                    let started = Instant::now();
                    app.on_key(stroke, now_ms());
                    app.took(started.elapsed());
                }
                Some(CaptureEvent::PointerClick) => app.on_pointer_click(now_ms()),
                Some(CaptureEvent::Keyboards(n)) => app.set_keyboards(n),
                None => break,
            },
            Some(ev) = conn_rx.recv() => {
                let started = Instant::now();
                app.on_conn(ev, now_ms());
                app.took(started.elapsed());
                // GNOME's key options changed (Caps Lock as Escape…): follow
                // them, unless the config sets its own.
                if let Some(o) = app.take_options()
                    && keymap_config.keyboard.xkb_options.is_none()
                    && o != xkb_options
                {
                    xkb_options = o;
                    options = KeymapOptions::from_xkb(xkb_options.iter().map(String::as_str));
                    let reported = app.layout();
                    let (k, layout, note) = make_keymap(
                        &keymap_config,
                        &xkb_options,
                        options,
                        reported.as_ref().map(|(l, v)| (l.as_str(), v.as_str())),
                    );
                    info!(options = ?xkb_options, "keyboard options");
                    keymap = k;
                    app.set_keymap_layout(layout, note);
                }
                // GNOME switched layouts: follow it.
                if let Some((l, v)) = app.take_layout() {
                    let (k, layout, note) =
                        make_keymap(&keymap_config, &xkb_options, options, Some((&l, &v)));
                    info!(layout = %layout, "keyboard layout");
                    keymap = k;
                    app.set_keymap_layout(layout, note);
                }
            }
            _ = ticker.tick() => app.tick(now_ms()),
            Some(f) = x11_rx.recv() => app.set_focus(f.app_id, f.pid, now_ms()),
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

/// A keymap for `layout` (from the config, else the one GNOME reports,
/// else US), the layout it really uses, and a note when that differs.
#[cfg(feature = "xkb")]
fn make_keymap(
    config: &Config,
    xkb_options: &[String],
    options: KeymapOptions,
    reported: Option<(&str, &str)>,
) -> (Box<dyn Keymap>, String, Option<String>) {
    let (layout, variant) = match (&config.keyboard.xkb_layout, reported) {
        (Some(l), _) => (l.as_str(), ""),
        (None, Some((l, v))) => (l, v),
        (None, None) => ("us", ""),
    };
    match keymap::xkb::XkbKeymap::new(layout, variant, xkb_options) {
        // Vim commands are typed on a Latin layout: for Cyrillic, Greek,
        // Arabic and the like, read keys as US.
        Ok(mut k) => {
            if keymap::is_latin(&mut k) {
                return (Box::new(k), layout.to_string(), None);
            }
            let note = format!("{layout} isn't a Latin layout; reading Vim commands as US");
            match keymap::xkb::XkbKeymap::new("us", "", xkb_options) {
                Ok(k) => (Box::new(k), "us".into(), Some(note)),
                Err(_) => (Box::new(UsKeymap::new(options)), "us".into(), Some(note)),
            }
        }
        Err(e) => {
            warn!(error = %e, "xkb keymap failed, using the built-in US keymap");
            let note = format!("could not load the {layout} layout; reading keys as US");
            (Box::new(UsKeymap::new(options)), "us".into(), Some(note))
        }
    }
}

#[cfg(not(feature = "xkb"))]
fn make_keymap(
    config: &Config,
    _: &[String],
    options: KeymapOptions,
    reported: Option<(&str, &str)>,
) -> (Box<dyn Keymap>, String, Option<String>) {
    let layout = config
        .keyboard
        .xkb_layout
        .as_deref()
        .or(reported.map(|(l, _)| l))
        .unwrap_or("us");
    let note = (layout != "us").then(|| {
        format!(
            "your layout is {layout}, but this build reads keys as US; install libxkbcommon-dev and run scripts/install.sh again"
        )
    });
    (Box::new(UsKeymap::new(options)), "us".into(), note)
}

async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}
