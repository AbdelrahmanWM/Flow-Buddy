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
        fs::create_dir_all(dir)?;
        let name = chrono::Local::now().format("session-%Y%m%d-%H%M%S.jsonl");
        let path = dir.join(name.to_string());
        let mut r = Recorder {
            out: BufWriter::new(File::create(&path)?),
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

    pub fn stop(mut self) -> PathBuf {
        let _ = self.out.flush();
        self.path
    }
}
