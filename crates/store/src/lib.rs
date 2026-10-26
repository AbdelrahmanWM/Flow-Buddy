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
const MIGRATIONS: &[&str] = &[
    r#"
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
"#,
    r#"
    -- Opt-in typing accuracy ([typing] accuracy): counts only.
    CREATE TABLE typing (
        day         TEXT    NOT NULL,
        context     TEXT    NOT NULL,
        typed       INTEGER NOT NULL DEFAULT 0,  -- characters typed
        corrections INTEGER NOT NULL DEFAULT 0,  -- of them deleted again
        PRIMARY KEY (day, context)
    );
    CREATE TABLE typo_pairs (
        day   TEXT    NOT NULL,
        pair  TEXT    NOT NULL,  -- two characters: the one before, the one deleted
        count INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (day, pair)
    );
"#,
    r#"
    -- What happened after each tip, for the learned tip filter.
    CREATE TABLE tip_outcomes (
        id          INTEGER PRIMARY KEY,
        shown_at    INTEGER NOT NULL,  -- unix seconds
        rule_id     TEXT    NOT NULL,
        context     TEXT    NOT NULL,
        stage       TEXT    NOT NULL,
        hour        INTEGER NOT NULL,  -- local hour of day
        mode        TEXT    NOT NULL,  -- coaching mode
        prior_today INTEGER NOT NULL,  -- tips shown earlier that day
        outcome     TEXT               -- adopted, dismissed, not_helpful, ignored; NULL while open
    );
    CREATE INDEX tip_outcomes_shown ON tip_outcomes (shown_at);
"#,
    r#"
    -- Which curriculum items you use, and the daily "something new" tips.
    CREATE TABLE commands_used (
        day     TEXT    NOT NULL,
        item_id TEXT    NOT NULL,  -- a curriculum id such as ci-quote
        count   INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (day, item_id)
    );
    CREATE TABLE learn_items (
        item_id     TEXT    PRIMARY KEY,
        first_shown INTEGER NOT NULL,  -- unix seconds
        last_shown  INTEGER NOT NULL,
        times_shown INTEGER NOT NULL DEFAULT 1,
        learned_at  INTEGER            -- when you started using it
    );
"#,
];

/// Progress on one curriculum item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LearnRow {
    pub item_id: String,
    pub first_shown: u64,
    pub last_shown: u64,
    pub times_shown: u32,
    pub learned_at: Option<u64>,
}

/// A tip as shown, for the learned filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TipShown<'a> {
    pub shown_at: u64,
    pub rule_id: &'a str,
    pub context: &'a str,
    pub stage: &'a str,
    pub hour: u32,
    pub mode: &'a str,
    pub prior_today: u32,
}

/// What happened after a tip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Adopted,
    Dismissed,
    NotHelpful,
    Ignored,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Adopted => "adopted",
            Outcome::Dismissed => "dismissed",
            Outcome::NotHelpful => "not_helpful",
            Outcome::Ignored => "ignored",
        }
    }

    pub fn parse(s: &str) -> Option<Outcome> {
        [
            Outcome::Adopted,
            Outcome::Dismissed,
            Outcome::NotHelpful,
            Outcome::Ignored,
        ]
        .into_iter()
        .find(|o| o.as_str() == s)
    }
}

/// A tip and what came of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TipRow {
    pub shown_at: u64,
    pub rule_id: String,
    pub context: String,
    pub stage: String,
    pub hour: u32,
    pub mode: String,
    pub prior_today: u32,
    /// `None` while still open, or if the daemon stopped before it closed.
    pub outcome: Option<Outcome>,
}

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

    /// Whether the database has every table this version knows. A database
    /// opened read-only by the CLI is only migrated by the daemon.
    pub fn is_current(&self) -> bool {
        self.conn
            .pragma_query_value(None, "user_version", |r| r.get::<_, usize>(0))
            .is_ok_and(|v| v >= MIGRATIONS.len())
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

    /// Nudges shown for one pattern from `from_day` on.
    pub fn nudges_since(&self, rule_id: &str, context: &str, from_day: &str) -> Result<u64> {
        self.conn.query_row(
            "SELECT COALESCE(SUM(nudges), 0) FROM daily_stats
             WHERE rule_id = ?1 AND context = ?2 AND day >= ?3",
            params![rule_id, context, from_day],
            |r| r.get(0),
        )
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
             DELETE FROM episodes; DELETE FROM typing; DELETE FROM typo_pairs;
             DELETE FROM tip_outcomes; DELETE FROM commands_used; DELETE FROM learn_items;",
        )?;
        self.conn.execute_batch("VACUUM")?;
        let _ = self.conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");
        Ok(())
    }

    pub fn add_command_use(&self, day: &str, item_id: &str, count: u64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO commands_used (day, item_id, count) VALUES (?1, ?2, ?3)
             ON CONFLICT (day, item_id) DO UPDATE SET count = count + excluded.count",
            params![day, item_id, count],
        )?;
        Ok(())
    }

    /// Uses of each curriculum item from `from_day` on.
    pub fn command_uses(&self, from_day: &str) -> Result<HashMap<String, u64>> {
        let mut stmt = self.conn.prepare(
            "SELECT item_id, SUM(count) FROM commands_used WHERE day >= ?1 GROUP BY item_id",
        )?;
        let rows = stmt.query_map(params![from_day], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// A curriculum item was shown as the day's "something new".
    pub fn learn_shown(&self, item_id: &str, now_s: u64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO learn_items (item_id, first_shown, last_shown) VALUES (?1, ?2, ?2)
             ON CONFLICT (item_id) DO UPDATE SET last_shown = ?2, times_shown = times_shown + 1",
            params![item_id, now_s],
        )?;
        Ok(())
    }

    pub fn learn_learned(&self, item_id: &str, now_s: u64) -> Result<()> {
        self.conn.execute(
            "UPDATE learn_items SET learned_at = ?2 WHERE item_id = ?1 AND learned_at IS NULL",
            params![item_id, now_s],
        )?;
        Ok(())
    }

    pub fn learn_items(&self) -> Result<Vec<LearnRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT item_id, first_shown, last_shown, times_shown, learned_at FROM learn_items
             ORDER BY first_shown",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(LearnRow {
                item_id: r.get(0)?,
                first_shown: r.get(1)?,
                last_shown: r.get(2)?,
                times_shown: r.get(3)?,
                learned_at: r.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// A tip was shown; returns its row, for [`Store::close_tip`].
    pub fn tip_shown(&self, tip: &TipShown<'_>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO tip_outcomes (shown_at, rule_id, context, stage, hour, mode, prior_today)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                tip.shown_at,
                tip.rule_id,
                tip.context,
                tip.stage,
                tip.hour,
                tip.mode,
                tip.prior_today
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// What came of a tip; only the first outcome counts.
    pub fn close_tip(&self, id: i64, outcome: Outcome) -> Result<()> {
        self.conn.execute(
            "UPDATE tip_outcomes SET outcome = ?2 WHERE id = ?1 AND outcome IS NULL",
            params![id, outcome.as_str()],
        )?;
        Ok(())
    }

    /// Tips shown since `since_s`, oldest first.
    pub fn tips(&self, since_s: u64) -> Result<Vec<TipRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT shown_at, rule_id, context, stage, hour, mode, prior_today, outcome
             FROM tip_outcomes WHERE shown_at >= ?1 ORDER BY shown_at, id",
        )?;
        let rows = stmt.query_map(params![since_s], |r| {
            Ok(TipRow {
                shown_at: r.get(0)?,
                rule_id: r.get(1)?,
                context: r.get(2)?,
                stage: r.get(3)?,
                hour: r.get(4)?,
                mode: r.get(5)?,
                prior_today: r.get(6)?,
                outcome: r
                    .get::<_, Option<String>>(7)?
                    .as_deref()
                    .and_then(Outcome::parse),
            })
        })?;
        rows.collect()
    }

    /// Drop tip rows older than `before_s`.
    pub fn prune_tips(&self, before_s: u64) -> Result<usize> {
        self.conn.execute(
            "DELETE FROM tip_outcomes WHERE shown_at < ?1",
            params![before_s],
        )
    }

    /// Add typing counts for `day` in `context` (opt-in).
    pub fn add_typing(&self, day: &str, context: &str, typed: u64, corrections: u64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO typing (day, context, typed, corrections) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (day, context) DO UPDATE SET
                 typed = typed + excluded.typed, corrections = corrections + excluded.corrections",
            params![day, context, typed, corrections],
        )?;
        Ok(())
    }

    pub fn add_typo_pair(&self, day: &str, pair: &str, count: u64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO typo_pairs (day, pair, count) VALUES (?1, ?2, ?3)
             ON CONFLICT (day, pair) DO UPDATE SET count = count + excluded.count",
            params![day, pair, count],
        )?;
        Ok(())
    }

    /// (typed, corrections) over a day range, all contexts.
    pub fn typing(&self, from_day: &str, to_day: &str) -> Result<(u64, u64)> {
        self.conn.query_row(
            "SELECT COALESCE(SUM(typed), 0), COALESCE(SUM(corrections), 0) FROM typing
             WHERE day >= ?1 AND day <= ?2",
            params![from_day, to_day],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// The most often corrected pairs over a day range, most first.
    pub fn typo_pairs(
        &self,
        from_day: &str,
        to_day: &str,
        limit: usize,
    ) -> Result<Vec<(String, u64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT pair, SUM(count) AS n FROM typo_pairs WHERE day >= ?1 AND day <= ?2
             GROUP BY pair ORDER BY n DESC, pair LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![from_day, to_day, limit], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
        rows.collect()
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
            ("typing", "day, context"),
            ("typo_pairs", "day, pair"),
            ("tip_outcomes", "rule_id, context, stage, mode"),
            ("commands_used", "day, item_id"),
            ("learn_items", "item_id"),
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

/// A few lines about the week for the weekly note: what improved against
/// the week before, what was mastered, and `focus`, the habit to work on.
pub fn week_summary(
    store: &Store,
    now_ms: u64,
    describe: &dyn Fn(&str) -> String,
    focus: Option<&str>,
) -> Option<String> {
    let this = days::range(now_ms, 7, 0);
    let before = days::range(now_ms, 7, 7);
    let totals = |range: &(String, String)| -> HashMap<String, (u64, u64)> {
        let mut out: HashMap<String, (u64, u64)> = HashMap::new();
        for r in store.daily(&range.0, &range.1).unwrap_or_default() {
            let e = out.entry(r.rule_id).or_default();
            e.0 += r.hits;
            e.1 += r.adoptions;
        }
        out
    };
    let (now, then) = (totals(&this), totals(&before));
    let hits: u64 = now.values().map(|v| v.0).sum();
    let adoptions: u64 = now.values().map(|v| v.1).sum();
    if hits + adoptions == 0 {
        return None;
    }
    let mut improved: Vec<String> = then
        .iter()
        .filter(|(id, (was, _))| {
            let is = now.get(*id).map_or(0, |v| v.0);
            *was >= 3 && is * 10 <= was * 7
        })
        .map(|(id, _)| describe(id))
        .collect();
    improved.sort();
    let week_ago_s = (now_ms / 1000).saturating_sub(7 * 86_400);
    let mut mastered: Vec<String> = store
        .patterns()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.stage == Stage::Mastered && p.stage_since >= week_ago_s)
        .map(|p| describe(&p.rule_id))
        .collect();
    mastered.sort();
    mastered.dedup();
    let mut text = format!("{adoptions} times the better way, {hits} habits spotted.");
    if !improved.is_empty() {
        text.push_str(&format!(
            " Less often than last week: {}.",
            improved.join("; ")
        ));
    }
    if !mastered.is_empty() {
        text.push_str(&format!(" Mastered: {}.", mastered.join("; ")));
    }
    if let Some(focus) = focus {
        text.push_str(&format!(" Work on next: {}.", describe(focus)));
    }
    Some(text)
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

    #[test]
    fn typing_counts() {
        let s = Store::open_in_memory().unwrap();
        s.add_typing("2026-10-24", "vscode-vim", 100, 4).unwrap();
        s.add_typing("2026-10-24", "vscode-vim", 50, 1).unwrap();
        s.add_typing("2026-10-25", "bash-vi", 10, 0).unwrap();
        assert_eq!(s.typing("2026-10-24", "2026-10-25").unwrap(), (160, 5));
        s.add_typo_pair("2026-10-24", "er", 3).unwrap();
        s.add_typo_pair("2026-10-25", "er", 1).unwrap();
        s.add_typo_pair("2026-10-25", "th", 2).unwrap();
        assert_eq!(
            s.typo_pairs("2026-10-24", "2026-10-25", 5).unwrap(),
            [("er".to_string(), 4), ("th".to_string(), 2)]
        );
        s.forget_all().unwrap();
        assert_eq!(s.typing("2026-10-24", "2026-10-25").unwrap(), (0, 0));
    }

    #[test]
    fn tip_outcomes() {
        let s = Store::open_in_memory().unwrap();
        let tip = |at| TipShown {
            shown_at: at,
            rule_id: "vim.repeat-jk",
            context: "vscode-vim",
            stage: "nudged",
            hour: 14,
            mode: "active",
            prior_today: 2,
        };
        let a = s.tip_shown(&tip(100)).unwrap();
        s.tip_shown(&tip(200)).unwrap();
        s.close_tip(a, Outcome::Adopted).unwrap();
        s.close_tip(a, Outcome::Dismissed).unwrap();
        let rows = s.tips(0).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].outcome,
            Some(Outcome::Adopted),
            "the first outcome sticks"
        );
        assert_eq!(rows[1].outcome, None);
        assert_eq!(s.prune_tips(150).unwrap(), 1);
        assert_eq!(s.tips(0).unwrap().len(), 1);
    }

    #[test]
    fn command_uses_and_learning() {
        let s = Store::open_in_memory().unwrap();
        s.add_command_use("2026-10-24", "ciw", 2).unwrap();
        s.add_command_use("2026-10-25", "ciw", 1).unwrap();
        s.add_command_use("2026-10-25", "star", 4).unwrap();
        let uses = s.command_uses("2026-10-25").unwrap();
        assert_eq!((uses["ciw"], uses["star"]), (1, 4));
        s.learn_shown("ciw", 100).unwrap();
        s.learn_shown("ciw", 200).unwrap();
        s.learn_learned("ciw", 300).unwrap();
        s.learn_learned("ciw", 400).unwrap();
        let rows = s.learn_items().unwrap();
        assert_eq!(
            (rows[0].times_shown, rows[0].first_shown, rows[0].last_shown),
            (2, 100, 200)
        );
        assert_eq!(rows[0].learned_at, Some(300));
    }
}
