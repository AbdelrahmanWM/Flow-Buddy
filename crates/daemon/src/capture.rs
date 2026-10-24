//! Keyboard capture via evdev.
//!
//! Devices are only *read*, never grabbed: Flow Buddy coaches, it never
//! blocks input. Hotplug is handled by watching `/dev/input` with inotify,
//! which avoids a dependency on libudev.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use evdev::{Device, EventType, KeyCode};
use futures_util::StreamExt;
use inotify::{Inotify, WatchMask};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

const INPUT_DIR: &str = "/dev/input";

/// A raw key event, before translation. Never leaves the daemon.
#[derive(Clone, Copy)]
pub struct RawKey {
    pub code: u16,
    /// 0 = release, 1 = press, 2 = auto-repeat.
    pub value: i32,
    pub at_ms: u64,
}

/// What the capture layer reports to the rest of the daemon.
pub enum CaptureEvent {
    Key(RawKey),
    /// A mouse button was pressed. Only the fact is reported (no position);
    /// it tells mode inference that the editor may have changed mode.
    PointerClick,
    /// Number of keyboards currently being read.
    Keyboards(usize),
}

/// Keys every real keyboard has. Power buttons, media remotes and
/// "Consumer Control" devices also report EV_KEY but lack these.
const REQUIRED_KEYS: [KeyCode; 6] = [
    KeyCode::KEY_A,
    KeyCode::KEY_Z,
    KeyCode::KEY_J,
    KeyCode::KEY_SPACE,
    KeyCode::KEY_ENTER,
    KeyCode::KEY_ESC,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Pointer,
}

/// Classify a device from the key codes it supports.
pub fn classify(keys: &[KeyCode]) -> Option<DeviceKind> {
    if REQUIRED_KEYS.iter().all(|k| keys.contains(k)) {
        Some(DeviceKind::Keyboard)
    } else if keys.contains(&KeyCode::BTN_LEFT) {
        Some(DeviceKind::Pointer)
    } else {
        None
    }
}

fn device_kind(dev: &Device) -> Option<DeviceKind> {
    if !dev.supported_events().contains(EventType::KEY) {
        return None;
    }
    let keys: Vec<KeyCode> = dev.supported_keys()?.iter().collect();
    classify(&keys)
}

fn is_event_node(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("event"))
}

/// Result of scanning `/dev/input` once.
#[derive(Default)]
pub struct Scan {
    pub opened: Vec<(PathBuf, Device, DeviceKind)>,
    /// Event nodes we were not allowed to open.
    pub denied: usize,
}

fn open_node(path: &Path) -> io::Result<Option<(Device, DeviceKind)>> {
    let dev = Device::open(path)?;
    Ok(device_kind(&dev).map(|kind| (dev, kind)))
}

pub fn scan(skip: &HashSet<&PathBuf>) -> io::Result<Scan> {
    let mut scan = Scan::default();
    for entry in std::fs::read_dir(INPUT_DIR)? {
        let path = entry?.path();
        if !is_event_node(&path) || skip.contains(&path) {
            continue;
        }
        match open_node(&path) {
            Ok(Some((dev, kind))) => scan.opened.push((path, dev, kind)),
            Ok(None) => {}
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => scan.denied += 1,
            Err(e) => debug!(path = %path.display(), error = %e, "skipping input node"),
        }
    }
    Ok(scan)
}

/// Run capture until `tx` is closed. Spawns one reader task per device.
pub async fn run(tx: mpsc::Sender<CaptureEvent>) -> anyhow::Result<()> {
    let (gone_tx, mut gone_rx) = mpsc::unbounded_channel::<PathBuf>();
    let mut active: HashMap<PathBuf, DeviceKind> = HashMap::new();

    let inotify = Inotify::init()?;
    inotify
        .watches()
        .add(INPUT_DIR, WatchMask::CREATE | WatchMask::ATTRIB)?;
    let mut changes = inotify.into_event_stream([0u8; 1024])?;

    let initial = scan(&active.keys().collect())?;
    if initial
        .opened
        .iter()
        .all(|(_, _, k)| *k != DeviceKind::Keyboard)
    {
        warn!(
            denied = initial.denied,
            "no readable keyboard found; see docs/privacy.md#keyboard-access"
        );
    }
    spawn_readers(initial, &mut active, &tx, &gone_tx).await;

    loop {
        tokio::select! {
            change = changes.next() => {
                if change.is_none() {
                    break;
                }
                // udev sets permissions shortly after the node appears.
                tokio::time::sleep(Duration::from_millis(200)).await;
                if let Ok(found) = scan(&active.keys().collect()) {
                    spawn_readers(found, &mut active, &tx, &gone_tx).await;
                }
            }
            Some(path) = gone_rx.recv() => {
                info!(path = %path.display(), "input device removed");
                active.remove(&path);
                let _ = tx.send(CaptureEvent::Keyboards(keyboards(&active))).await;
            }
            _ = tx.closed() => break,
        }
    }
    Ok(())
}

fn keyboards(active: &HashMap<PathBuf, DeviceKind>) -> usize {
    active
        .values()
        .filter(|k| **k == DeviceKind::Keyboard)
        .count()
}

async fn spawn_readers(
    scan: Scan,
    active: &mut HashMap<PathBuf, DeviceKind>,
    tx: &mpsc::Sender<CaptureEvent>,
    gone: &mpsc::UnboundedSender<PathBuf>,
) {
    if scan.opened.is_empty() {
        return;
    }
    for (path, dev, kind) in scan.opened {
        info!(
            path = %path.display(),
            name = dev.name().unwrap_or("?"),
            ?kind,
            "reading input device"
        );
        active.insert(path.clone(), kind);
        let (tx, gone) = (tx.clone(), gone.clone());
        tokio::spawn(async move {
            if let Err(e) = read_device(dev, kind, &tx).await {
                debug!(path = %path.display(), error = %e, "device reader stopped");
            }
            let _ = gone.send(path);
        });
    }
    let _ = tx.send(CaptureEvent::Keyboards(keyboards(active))).await;
}

async fn read_device(
    dev: Device,
    kind: DeviceKind,
    tx: &mpsc::Sender<CaptureEvent>,
) -> io::Result<()> {
    let mut events = dev.into_event_stream()?;
    loop {
        let ev = events.next_event().await?;
        if ev.event_type() != EventType::KEY {
            continue;
        }
        let out = match kind {
            DeviceKind::Keyboard => CaptureEvent::Key(RawKey {
                code: ev.code(),
                value: ev.value(),
                at_ms: ev
                    .timestamp()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64),
            }),
            DeviceKind::Pointer if ev.value() == 1 => CaptureEvent::PointerClick,
            DeviceKind::Pointer => continue,
        };
        if tx.send(out).await.is_err() {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_keyboard_is_keyboard() {
        let keys = [
            KeyCode::KEY_ESC,
            KeyCode::KEY_A,
            KeyCode::KEY_J,
            KeyCode::KEY_Z,
            KeyCode::KEY_SPACE,
            KeyCode::KEY_ENTER,
            KeyCode::KEY_LEFTSHIFT,
        ];
        assert_eq!(classify(&keys), Some(DeviceKind::Keyboard));
    }

    #[test]
    fn power_button_and_media_keys_are_ignored() {
        assert_eq!(classify(&[KeyCode::KEY_POWER]), None);
        assert_eq!(
            classify(&[
                KeyCode::KEY_VOLUMEUP,
                KeyCode::KEY_VOLUMEDOWN,
                KeyCode::KEY_MUTE
            ]),
            None
        );
    }

    #[test]
    fn mouse_is_pointer() {
        assert_eq!(
            classify(&[KeyCode::BTN_LEFT, KeyCode::BTN_RIGHT]),
            Some(DeviceKind::Pointer)
        );
    }

    /// Coach, don't block: the daemon must never take exclusive access.
    #[test]
    fn devices_are_never_grabbed() {
        let grab_call = [".gr", "ab("].concat();
        for src in [include_str!("capture.rs"), include_str!("main.rs")] {
            assert!(!src.contains(&grab_call));
        }
    }

    #[test]
    fn only_event_nodes_are_considered() {
        assert!(is_event_node(Path::new("/dev/input/event3")));
        assert!(!is_event_node(Path::new("/dev/input/mice")));
        assert!(!is_event_node(Path::new("/dev/input/by-id")));
    }
}
