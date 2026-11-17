//! `flowbuddy record`: write what the rules see to a local file, without
//! anything typed (see `flowbuddy_core::session`), for checking rules
//! against real use. Off unless you start it; nothing is ever sent.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use flowbuddy_core::session::{Entry, VERSION};

pub struct Recorder {
    out: BufWriter<File>,
    path: PathBuf,
    started_ms: u64,
}

impl Recorder {
    /// Start a new recording in `dir`, named after the time.
    pub fn start(dir: &Path, now_ms: u64) -> std::io::Result<Recorder> {
        Recorder::start_as(dir, "session", now_ms)
    }

    /// Start a recording named `<prefix>-<time>.jsonl`.
    pub fn start_as(dir: &Path, prefix: &str, now_ms: u64) -> std::io::Result<Recorder> {
        flowbuddy_core::paths::private_dir(dir)?;
        let name = format!(
            "{prefix}-{}",
            flowbuddy_core::time::format(&flowbuddy_core::time::now(), "%Y%m%d-%H%M%S.jsonl")
        );
        let path = dir.join(name);
        let mut r = Recorder {
            out: BufWriter::new({
                use std::os::unix::fs::OpenOptionsExt;
                fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(&path)?
            }),
            path,
            started_ms: now_ms,
        };
        r.write(&Entry::Start { version: VERSION });
        Ok(r)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Milliseconds since the start, for an entry's `t`.
    pub fn t(&self, at_ms: u64) -> u64 {
        at_ms.saturating_sub(self.started_ms)
    }

    pub fn write(&mut self, entry: &Entry) {
        let line = serde_json::to_string(entry).unwrap_or_default();
        if let Err(e) = writeln!(self.out, "{line}") {
            tracing::warn!(error = %e, "could not write the recording");
        }
    }

    pub fn flush(&mut self) {
        let _ = self.out.flush();
    }

    /// Delete rolling recordings (`auto-*`) older than `days`.
    pub fn prune(dir: &Path, days: u32) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let limit = std::time::Duration::from_secs(u64::from(days) * 86_400);
        for e in entries.flatten() {
            let name = e.file_name();
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > limit);
            if name.to_string_lossy().starts_with("auto-") && old {
                let _ = fs::remove_file(e.path());
            }
        }
    }

    pub fn stop(mut self) -> PathBuf {
        let _ = self.out.flush();
        self.path
    }
}
