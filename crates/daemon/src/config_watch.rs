//! Reload the config when the file changes.

use std::path::PathBuf;

use futures_util::StreamExt;
use inotify::{Inotify, WatchMask};
use tokio::sync::mpsc;

/// Watch the config file's directory (editors often replace the file
/// rather than writing it) and send `()` whenever the file changes.
pub async fn run(path: PathBuf, tx: mpsc::Sender<()>) -> anyhow::Result<()> {
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
