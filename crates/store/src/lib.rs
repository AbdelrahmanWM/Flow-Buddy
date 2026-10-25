//! Stats and the mistake memory, in SQLite.
//!
//! Privacy: every column holds a count, a timestamp, an id from a fixed
//! set (rule, context, mode, stage) or a [`Shape`] — which is built only
//! from static strings and numbers. There is no column for typed text,
//! file names or titles. See docs/privacy.md and docs/memory.md.
//!
//! [`Shape`]: flowbuddy_core::rule::Shape

pub mod days;
mod export;

use std::collections::HashMap;
use std::path::Path;

pub use export::{EXPORT_VERSION, Export};
use flowbuddy_core::learning::{Stage, Window};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

pub type Result<T> = rusqlite::Result<T>;

/// Schema migrations, applied in order; `PRAGMA user_version` is the
/// number applied so far.
const MIGRATIONS: &[&str] = &[r#"
    CREATE TABLE daily_stats (
        day             TEXT    NOT NULL,  -- local date, YYYY-MM-DD
        context         TEXT    NOT NULL,
        rule_id         TEXT    NOT NULL,
        hits            INTEGER NOT NULL DEFAULT 0,
        nudges          INTEGER NOT NULL DEFAULT 0,
        dismissed       INTEGER NOT NULL DEFAULT 0,
        false_positives INTEGER NOT NULL DEFAULT 0,
        adoptions       INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (day, context, rule_id)
    );
    CREATE TABLE activity (
        day            TEXT    NOT NULL,
        context        TEXT    NOT NULL,
        active_minutes INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (day, context)
    );
    CREATE TABLE patterns (
        rule_id         TEXT    NOT NULL,
        context         TEXT    NOT NULL,
        first_seen      INTEGER NOT NULL,  -- unix seconds
        last_seen       INTEGER NOT NULL,
        occurrences     INTEGER NOT NULL DEFAULT 0,
        nudges          INTEGER NOT NULL DEFAULT 0,
        dismissed       INTEGER NOT NULL DEFAULT 0,
        false_positives INTEGER NOT NULL DEFAULT 0,
        adoptions       INTEGER NOT NULL DEFAULT 0,
        stage           TEXT    NOT NULL DEFAULT 'new',
        stage_since     INTEGER NOT NULL,
        PRIMARY KEY (rule_id, context)
    );
    CREATE TABLE episodes (
        id        INTEGER PRIMARY KEY,
        ts_bucket INTEGER NOT NULL,  -- unix seconds, truncated to the hour
        rule_id   TEXT    NOT NULL,
        context   TEXT    NOT NULL,
        mode      TEXT    NOT NULL,
        shape     TEXT    NOT NULL
    );
    CREATE INDEX episodes_ts ON episodes (ts_bucket);
"#];

/// What happened, for [`Store::record`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event<'a> {
    Hit { mode: &'a str, shape: &'a str },
    Adoption,
    Nudge,
    Dismissed,
    FalsePositive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DailyRow {
    pub day: String,
    pub context: String,
    pub rule_id: String,
    pub hits: u64,
    pub nudges: u64,
    pub dismissed: u64,
    pub false_positives: u64,
    pub adoptions: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    pub rule_id: String,
    pub context: String,
    pub first_seen: u64,
    pub last_seen: u64,
    pub occurrences: u64,
    pub nudges: u64,
    pub dismissed: u64,
    pub false_positives: u64,
    pub adoptions: u64,
    pub stage: Stage,
    pub stage_since: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Episode {
    pub ts_bucket: u64,
    pub rule_id: String,
    pub context: String,
    pub mode: String,
    pub shape: String,
}

pub struct Store {
    conn: Connection,
}

const HOUR_S: u64 = 3600;

impl Store {
    /// Open (creating if needed) with owner-only permissions: the
    /// directory 0700, the database 0600. SQLite gives its -wal and -shm
    /// files the database's permissions.
    pub fn open(path: &Path) -> Result<Store> {
        use std::os::unix::fs::PermissionsExt;
        let private = |p: &Path, mode| {
            let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode));
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
            private(dir, 0o700);
        }
        let conn = Connection::open(path)?;
        private(path, 0o600);
        Store::init(conn)
    }

    /// Open an existing database without the ability to change it.
    pub fn open_read_only(path: &Path) -> Result<Store> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Ok(Store { conn })
    }

    pub fn open_in_memory() -> Result<Store> {
        Store::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let applied: usize = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(applied) {
            self.conn.execute_batch(sql)?;
            self.conn.pragma_update(None, "user_version", i + 1)?;
        }
        Ok(())
    }

    /// Record one event for (`day`, `context`, `rule_id`) at `now_s`.
    pub fn record(
        &self,
        day: &str,
        context: &str,
        rule_id: &str,
        event: Event<'_>,
        now_s: u64,
    ) -> Result<()> {
        let (stat, pattern) = match event {
            Event::Hit { .. } => ("hits", "occurrences"),
            Event::Adoption => ("adoptions", "adoptions"),
            Event::Nudge => ("nudges", "nudges"),
            Event::Dismissed => ("dismissed", "dismissed"),
            Event::FalsePositive => ("false_positives", "false_positives"),
        };
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            &format!(
                "INSERT INTO daily_stats (day, context, rule_id, {stat}) VALUES (?1, ?2, ?3, 1)
                 ON CONFLICT (day, context, rule_id) DO UPDATE SET {stat} = {stat} + 1"
            ),
            params![day, context, rule_id],
        )?;
        let touches_last_seen = matches!(event, Event::Hit { .. });
        tx.execute(
            &format!(
                "INSERT INTO patterns (rule_id, context, first_seen, last_seen, {pattern}, stage_since)
                 VALUES (?1, ?2, ?3, ?3, 1, ?3)
                 ON CONFLICT (rule_id, context) DO UPDATE SET {pattern} = {pattern} + 1{}",
                if touches_last_seen { ", last_seen = ?3" } else { "" }
            ),
            params![rule_id, context, now_s],
        )?;
        if let Event::Hit { mode, shape } = event {
            tx.execute(
                "INSERT INTO episodes (ts_bucket, rule_id, context, mode, shape)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![now_s / HOUR_S * HOUR_S, rule_id, context, mode, shape],
            )?;
        }
        tx.commit()
    }

    pub fn add_active_minute(&self, day: &str, context: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO activity (day, context, active_minutes) VALUES (?1, ?2, 1)
             ON CONFLICT (day, context) DO UPDATE SET active_minutes = active_minutes + 1",
            params![day, context],
        )?;
        Ok(())
    }

    pub fn set_stage(&self, rule_id: &str, context: &str, stage: Stage, now_s: u64) -> Result<()> {
        self.conn.execute(
            "UPDATE patterns SET stage = ?3, stage_since = ?4
             WHERE rule_id = ?1 AND context = ?2 AND stage != ?3",
            params![rule_id, context, stage.as_str(), now_s],
        )?;
        Ok(())
    }

    /// Stages keyed by (rule, context).
    pub fn stages(&self) -> Result<HashMap<(String, String), Stage>> {
        let mut stmt = self
            .conn
            .prepare("SELECT rule_id, context, stage FROM patterns")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                (r.get::<_, String>(0)?, r.get::<_, String>(1)?),
                Stage::parse(&r.get::<_, String>(2)?),
            ))
        })?;
        rows.collect()
    }

    /// Daily rows with `from_day <= day <= to_day` (inclusive, ISO dates).
    pub fn daily(&self, from_day: &str, to_day: &str) -> Result<Vec<DailyRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT day, context, rule_id, hits, nudges, dismissed, false_positives, adoptions
             FROM daily_stats WHERE day >= ?1 AND day <= ?2 ORDER BY day, rule_id, context",
        )?;
        let rows = stmt.query_map(params![from_day, to_day], |r| {
            Ok(DailyRow {
                day: r.get(0)?,
                context: r.get(1)?,
                rule_id: r.get(2)?,
                hits: r.get(3)?,
                nudges: r.get(4)?,
                dismissed: r.get(5)?,
                false_positives: r.get(6)?,
                adoptions: r.get(7)?,
            })
        })?;
        rows.collect()
    }

    /// Active minutes per context in the day range.
    pub fn active_minutes(&self, from_day: &str, to_day: &str) -> Result<HashMap<String, u64>> {
        let mut stmt = self.conn.prepare(
            "SELECT context, SUM(active_minutes) FROM activity
             WHERE day >= ?1 AND day <= ?2 GROUP BY context",
        )?;
        let rows = stmt.query_map(params![from_day, to_day], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// Hits, adoptions and activity for one pattern over a day range.
    pub fn window(
        &self,
        rule_id: &str,
        context: &str,
        from_day: &str,
        to_day: &str,
    ) -> Result<Window> {
        let (hits, adoptions): (u64, u64) = self.conn.query_row(
            "SELECT COALESCE(SUM(hits), 0), COALESCE(SUM(adoptions), 0) FROM daily_stats
             WHERE rule_id = ?1 AND context = ?2 AND day >= ?3 AND day <= ?4",
            params![rule_id, context, from_day, to_day],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let active_minutes = self
            .active_minutes(from_day, to_day)?
            .get(context)
            .copied()
            .unwrap_or(0);
        Ok(Window {
            hits,
            adoptions,
            active_minutes,
        })
    }

    pub fn patterns(&self) -> Result<Vec<Pattern>> {
        let mut stmt = self.conn.prepare(
            "SELECT rule_id, context, first_seen, last_seen, occurrences, nudges, dismissed,
                    false_positives, adoptions, stage, stage_since
             FROM patterns ORDER BY rule_id, context",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Pattern {
                rule_id: r.get(0)?,
                context: r.get(1)?,
                first_seen: r.get(2)?,
                last_seen: r.get(3)?,
                occurrences: r.get(4)?,
                nudges: r.get(5)?,
                dismissed: r.get(6)?,
                false_positives: r.get(7)?,
                adoptions: r.get(8)?,
                stage: Stage::parse(&r.get::<_, String>(9)?),
                stage_since: r.get(10)?,
            })
        })?;
        rows.collect()
    }

    /// Most recent episodes for a pattern, newest first.
    pub fn recent_episodes(
        &self,
        rule_id: &str,
        context: &str,
        limit: usize,
    ) -> Result<Vec<Episode>> {
        let mut stmt = self.conn.prepare(
            "SELECT ts_bucket, rule_id, context, mode, shape FROM episodes
             WHERE rule_id = ?1 AND context = ?2 ORDER BY ts_bucket DESC, id DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![rule_id, context, limit as i64], |r| {
            Ok(Episode {
                ts_bucket: r.get(0)?,
                rule_id: r.get(1)?,
                context: r.get(2)?,
                mode: r.get(3)?,
                shape: r.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Delete episodes older than `before_s`. Returns how many went.
    pub fn prune_episodes(&self, before_s: u64) -> Result<usize> {
        self.conn.execute(
            "DELETE FROM episodes WHERE ts_bucket < ?1",
            params![before_s],
        )
    }

    /// Forget everything about one rule.
    pub fn forget_rule(&self, rule_id: &str) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let mut n = 0;
        for table in ["daily_stats", "patterns", "episodes"] {
            n += tx.execute(
                &format!("DELETE FROM {table} WHERE rule_id = ?1"),
                params![rule_id],
            )?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Forget everything. The file stays (the daemon may have it open) but
    /// is emptied and compacted so deleted data doesn't linger on disk.
    pub fn forget_all(&self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM daily_stats; DELETE FROM activity; DELETE FROM patterns;
             DELETE FROM episodes;",
        )?;
        self.conn.execute_batch("VACUUM")?;
        let _ = self.conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");
        Ok(())
    }

    /// The first time anything was recorded, if ever.
    pub fn first_seen(&self) -> Result<Option<u64>> {
        self.conn
            .query_row("SELECT MIN(first_seen) FROM patterns", [], |r| r.get(0))
            .optional()
            .map(Option::flatten)
    }

    /// Every text value in the database, for privacy tests.
    pub fn all_text(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for (table, cols) in [
            ("daily_stats", "day, context, rule_id"),
            ("activity", "day, context"),
            ("patterns", "rule_id, context, stage"),
            ("episodes", "rule_id, context, mode, shape"),
        ] {
            let mut stmt = self.conn.prepare(&format!("SELECT {cols} FROM {table}"))?;
            let n = stmt.column_count();
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                for i in 0..n {
                    out.push(row.get::<_, String>(i)?);
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIT: Event<'static> = Event::Hit {
        mode: "normal",
        shape: "j×7",
    };

    #[test]
    fn database_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("fb-store-{}", std::process::id()));
        let path = dir.join("sub/flowbuddy.db");
        let s = Store::open(&path).unwrap();
        s.record("2026-10-24", "bash-vi", "a", HIT, 0).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&path.with_extension("db-wal")), 0o600);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn migrations_apply_once() {
        let s = Store::open_in_memory().unwrap();
        s.migrate().unwrap();
        let v: usize = s
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, MIGRATIONS.len());
    }

    #[test]
    fn records_daily_pattern_and_episode() {
        let s = Store::open_in_memory().unwrap();
        s.record("2026-10-24", "vscode-vim", "vim.repeat-jk", HIT, 7_200)
            .unwrap();
        s.record("2026-10-24", "vscode-vim", "vim.repeat-jk", HIT, 9_000)
            .unwrap();
        s.record(
            "2026-10-24",
            "vscode-vim",
            "vim.repeat-jk",
            Event::Adoption,
            9_100,
        )
        .unwrap();
        s.record(
            "2026-10-24",
            "vscode-vim",
            "vim.repeat-jk",
            Event::Nudge,
            9_000,
        )
        .unwrap();

        let d = &s.daily("2026-10-24", "2026-10-24").unwrap()[0];
        assert_eq!((d.hits, d.adoptions, d.nudges), (2, 1, 1));

        let p = &s.patterns().unwrap()[0];
        assert_eq!(
            (p.first_seen, p.last_seen, p.occurrences),
            (7_200, 9_000, 2)
        );
        assert_eq!(p.stage, Stage::New);

        let e = s
            .recent_episodes("vim.repeat-jk", "vscode-vim", 10)
            .unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].ts_bucket, 7_200, "bucketed to the hour");
        assert_eq!(e[0].shape, "j×7");
    }

    #[test]
    fn windows_sum_a_day_range() {
        let s = Store::open_in_memory().unwrap();
        for day in ["2026-10-20", "2026-10-22", "2026-10-24"] {
            s.record(day, "bash-vi", "bash.up-history", HIT, 0).unwrap();
            s.add_active_minute(day, "bash-vi").unwrap();
        }
        s.record(
            "2026-10-24",
            "bash-vi",
            "bash.up-history",
            Event::Adoption,
            0,
        )
        .unwrap();
        let w = s
            .window("bash.up-history", "bash-vi", "2026-10-21", "2026-10-24")
            .unwrap();
        assert_eq!(
            w,
            Window {
                hits: 2,
                adoptions: 1,
                active_minutes: 2
            }
        );
    }

    #[test]
    fn stages_are_stored() {
        let s = Store::open_in_memory().unwrap();
        s.record("2026-10-24", "terminal-vim", "vim.repeat-x", HIT, 0)
            .unwrap();
        s.set_stage("vim.repeat-x", "terminal-vim", Stage::Improving, 50)
            .unwrap();
        let stages = s.stages().unwrap();
        assert_eq!(
            stages[&("vim.repeat-x".to_string(), "terminal-vim".to_string())],
            Stage::Improving
        );
        assert_eq!(s.patterns().unwrap()[0].stage_since, 50);
    }

    #[test]
    fn retention_prunes_only_old_episodes() {
        let s = Store::open_in_memory().unwrap();
        s.record(
            "2026-01-01",
            "bash-vi",
            "bash.left-streak",
            HIT,
            1_000 * HOUR_S,
        )
        .unwrap();
        s.record(
            "2026-10-24",
            "bash-vi",
            "bash.left-streak",
            HIT,
            9_000 * HOUR_S,
        )
        .unwrap();
        assert_eq!(s.prune_episodes(5_000 * HOUR_S).unwrap(), 1);
        assert_eq!(
            s.recent_episodes("bash.left-streak", "bash-vi", 9)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            s.patterns().unwrap()[0].occurrences,
            2,
            "pattern record kept"
        );
    }

    #[test]
    fn forget_rule_and_all() {
        let s = Store::open_in_memory().unwrap();
        s.record("2026-10-24", "bash-vi", "a", HIT, 0).unwrap();
        s.record("2026-10-24", "bash-vi", "b", HIT, 0).unwrap();
        s.forget_rule("a").unwrap();
        let rules: Vec<_> = s
            .patterns()
            .unwrap()
            .into_iter()
            .map(|p| p.rule_id)
            .collect();
        assert_eq!(rules, ["b"]);
        s.forget_all().unwrap();
        assert!(s.patterns().unwrap().is_empty());
        assert!(s.all_text().unwrap().is_empty());
    }
}
