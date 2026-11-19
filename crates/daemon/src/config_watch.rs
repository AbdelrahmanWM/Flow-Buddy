//! Reload the config when the file changes.

use std::path::PathBuf;

use tokio::sync::mpsc;

/// Watch the config file's directory (editors often replace the file
/// rather than writing it) and send `()` whenever the file changes.
#[cfg(target_os = "linux")]
pub async fn run(path: PathBuf, tx: mpsc::Sender<()>) -> anyhow::Result<()> {
    use futures_util::StreamExt;
    use inotify::{Inotify, WatchMask};

    let dir = path.parent().map(PathBuf::from).unwrap_or_default();
    std::fs::create_dir_all(&dir)?;
    let inotify = Inotify::init()?;
    inotify.watches().add(
        &dir,
        WatchMask::CLOSE_WRITE | WatchMask::MOVED_TO | WatchMask::CREATE | WatchMask::DELETE,
    )?;
    let name = path.file_name().map(|n| n.to_os_string());
    let mut events = inotify.into_event_stream([0u8; 1024])?;
    while let Some(ev) = events.next().await {
        let ev = ev?;
        if ev.name.map(|n| n.to_os_string()) == name && tx.send(()).await.is_err() {
            break;
        }
    }
    Ok(())
}

/// Elsewhere (macOS), no inotify: look at the file every two seconds and
/// send `()` when its size or time changed, or it came or went.
#[cfg(not(target_os = "linux"))]
pub async fn run(path: PathBuf, tx: mpsc::Sender<()>) -> anyhow::Result<()> {
    let stamp = |p: &PathBuf| {
        std::fs::metadata(p)
            .ok()
            .map(|m| (m.len(), m.modified().ok()))
    };
    let mut last = stamp(&path);
    let mut every = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        every.tick().await;
        let now = stamp(&path);
        if now != last {
            last = now;
            if tx.send(()).await.is_err() {
                break;
            }
        }
    }
    Ok(())
}
