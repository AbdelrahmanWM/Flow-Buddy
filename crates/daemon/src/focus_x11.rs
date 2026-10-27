//! The focused window on X11 desktops (KDE, Xfce, i3, GNOME on Xorg), where
//! there's no GNOME extension to ask: read `_NET_ACTIVE_WINDOW` from the X
//! server and the window's class and pid. Only those are read, never
//! titles. The X server is a Unix socket, which the daemon's sandbox allows.

use tokio::sync::mpsc::UnboundedSender;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ConnectionExt, EventMask, Window,
};

/// The focused window: its class (the app id) and pid, when known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Focus {
    pub app_id: Option<String>,
    pub pid: Option<u32>,
}

/// The class from a `WM_CLASS` value: "instance\0Class\0" → "Class".
pub fn class(wm_class: &[u8]) -> Option<String> {
    let mut parts = wm_class.split(|b| *b == 0).filter(|p| !p.is_empty());
    let instance = parts.next()?;
    let class = parts.next().unwrap_or(instance);
    String::from_utf8(class.to_vec()).ok()
}

/// Whether to watch X11 for focus: an X11 session.
pub fn wanted() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "x11")
        && std::env::var_os("DISPLAY").is_some()
}

/// Send the focused window, and again each time the X server says it
/// changed. Runs on its own thread; gives up quietly if X can't be reached.
pub fn watch(tx: UnboundedSender<Focus>) {
    std::thread::spawn(move || {
        if let Err(e) = run(&tx) {
            tracing::warn!(error = %e, "can't watch the focused window on X11");
        }
    });
}

fn run(tx: &UnboundedSender<Focus>) -> Result<(), Box<dyn std::error::Error>> {
    let (conn, screen) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen].root;
    let atom = |name: &[u8]| -> Result<u32, Box<dyn std::error::Error>> {
        Ok(conn.intern_atom(false, name)?.reply()?.atom)
    };
    let active = atom(b"_NET_ACTIVE_WINDOW")?;
    let pid_atom = atom(b"_NET_WM_PID")?;
    // The window manager sets _NET_ACTIVE_WINDOW on the root window: hear
    // about each change instead of asking.
    conn.change_window_attributes(
        root,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?
    .check()?;
    let mut last: Option<Focus> = None;
    loop {
        let window: Option<Window> = conn
            .get_property(false, root, active, AtomEnum::WINDOW, 0, 1)?
            .reply()?
            .value32()
            .and_then(|mut v| v.next())
            .filter(|w| *w != 0);
        let focus = match window {
            None => Focus {
                app_id: None,
                pid: None,
            },
            Some(w) => Focus {
                app_id: conn
                    .get_property(false, w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)?
                    .reply()
                    .ok()
                    .and_then(|r| class(&r.value)),
                pid: conn
                    .get_property(false, w, pid_atom, AtomEnum::CARDINAL, 0, 1)?
                    .reply()
                    .ok()
                    .and_then(|r| r.value32().and_then(|mut v| v.next())),
            },
        };
        if last.as_ref() != Some(&focus) {
            if tx.send(focus.clone()).is_err() {
                return Ok(());
            }
            last = Some(focus);
        }
        // Until the active window changes (other root properties change
        // too; those are skipped).
        loop {
            if let Event::PropertyNotify(e) = conn.wait_for_event()?
                && e.atom == active
            {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn class_from_wm_class() {
        assert_eq!(
            class(b"gnome-terminal-server\0Gnome-terminal\0").as_deref(),
            Some("Gnome-terminal")
        );
        assert_eq!(class(b"code\0Code\0").as_deref(), Some("Code"));
        assert_eq!(class(b"xterm\0").as_deref(), Some("xterm"));
        assert_eq!(class(b""), None);
    }

    /// Against a real X server: `cargo test -p flowbuddy-daemon x11_smoke -- --ignored`.
    #[test]
    #[ignore]
    fn x11_smoke() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        watch(tx);
        std::thread::sleep(Duration::from_millis(800));
        let first = rx.try_recv();
        eprintln!("x11 focus: {first:?}");
        assert!(first.is_ok(), "no answer from the X server");
    }
}
