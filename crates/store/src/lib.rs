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

pub use export::{EXPORT_VERSION, Export, PatternExport};
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
    r#"
    -- Tips as shown, for `flowbuddy tips` and the menu's recent tips. The
    -- text is built from fixed sentences and counts, never from typing.
    CREATE TABLE tip_history (
        id       INTEGER PRIMARY KEY,
        shown_at INTEGER NOT NULL,  -- unix seconds
        rule_id  TEXT    NOT NULL,
        title    TEXT    NOT NULL,
        text     TEXT    NOT NULL,
        insert_mode INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX tip_history_shown ON tip_history (shown_at);
"#,
    r#"
    -- Uses of curriculum items per environment (terminal-vim, vscode-vim…);
    -- the ones counted before are kept as 'any', which counts everywhere.
    CREATE TABLE commands_used_ctx (
        day     TEXT    NOT NULL,
        context TEXT    NOT NULL,
        item_id TEXT    NOT NULL,
        count   INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (day, context, item_id)
    );
    INSERT INTO commands_used_ctx (day, context, item_id, count)
        SELECT day, 'any', item_id, count FROM commands_used;
    DROP TABLE commands_used;
    ALTER TABLE commands_used_ctx RENAME TO commands_used;
"#,
    r#"
    -- What you said you know (1) or don't (0), over what use shows.
    CREATE TABLE known_overrides (
        item_id TEXT    PRIMARY KEY,
        known   INTEGER NOT NULL
    );
"#,
    r#"
    -- Challenges after lessons ("use it the next time it fits") and how
    -- they ended: done, missed (the old habit instead) or expired.
    CREATE TABLE challenges (
        id      INTEGER PRIMARY KEY,
        item_id TEXT    NOT NULL,
        set_at  INTEGER NOT NULL,  -- unix seconds
        day     TEXT    NOT NULL,  -- local date it ended
        outcome TEXT    NOT NULL
    );
"#,
    r#"
    -- Tips held back at random ([learn] holdout), to measure what tips do.
    ALTER TABLE tip_outcomes ADD COLUMN held INTEGER NOT NULL DEFAULT 0;
"#,
];

/// One tip as it was shown.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HistoryTip {
    pub shown_at: u64,
    pub rule_id: String,
    pub title: String,
    pub text: String,
    /// Shown for Insert mode: its commands mean their Insert-mode things.
    pub insert: bool,
}

/// How much a curriculum item was used: see [`Store::item_uses`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ItemUse {
    pub total: u64,
    pub recent: u64,
    pub days_idle: u64,
}

/// Progress on one curriculum item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LearnRow {
    pub item_id: String,
    pub first_shown: u64,
    pub last_shown: u64,
    pub times_shown: u32,
    pub learned_at: Option<u64>,
}

/// A day whose adoptions were counted per key (see
/// [`Store::inflated_adoptions`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InflatedDay {
    pub day: String,
    pub context: String,
    pub rule_id: String,
    pub adoptions: u64,
    /// What it would be capped at.
    pub capped: u64,
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
    /// Held back at random, not shown: the comparison that tells what
    /// showing tips does (`[learn] holdout`).
    pub held: bool,
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
    /// Held back, not shown: see [`TipShown::held`].
    pub held: bool,
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

    /// Days before `before_day` whose adoptions are implausibly high next to
    /// their hits (over 10 × hits + 20: counted per key before the fix in
    /// #48); they'd be capped at 3 × hits + 3.
    pub fn inflated_adoptions(&self, before_day: &str) -> Result<Vec<InflatedDay>> {
        let mut stmt = self.conn.prepare(
            "SELECT day, context, rule_id, adoptions, hits * 3 + 3 FROM daily_stats
             WHERE day < ?1 AND adoptions > hits * 10 + 20 ORDER BY day, rule_id",
        )?;
        let rows = stmt.query_map(params![before_day], |r| {
            Ok(InflatedDay {
                day: r.get(0)?,
                context: r.get(1)?,
                rule_id: r.get(2)?,
                adoptions: r.get(3)?,
                capped: r.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Cap those days' adoptions, and recompute each pattern's total.
    pub fn fix_adoptions(&self, before_day: &str) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE daily_stats SET adoptions = hits * 3 + 3
             WHERE day < ?1 AND adoptions > hits * 10 + 20",
            params![before_day],
        )?;
        tx.execute(
            "UPDATE patterns SET adoptions = (
                 SELECT COALESCE(SUM(d.adoptions), 0) FROM daily_stats d
                 WHERE d.rule_id = patterns.rule_id AND d.context = patterns.context)",
            [],
        )?;
        tx.commit()?;
        Ok(changed)
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
             DELETE FROM tip_outcomes; DELETE FROM commands_used; DELETE FROM learn_items;
             DELETE FROM tip_history; DELETE FROM known_overrides;
             DELETE FROM challenges;",
        )?;
        self.conn.execute_batch("VACUUM")?;
        let _ = self.conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");
        Ok(())
    }

    pub fn add_command_use(
        &self,
        day: &str,
        context: &str,
        item_id: &str,
        count: u64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO commands_used (day, context, item_id, count) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (day, context, item_id) DO UPDATE SET count = count + excluded.count",
            params![day, context, item_id, count],
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

    /// For each curriculum item: uses ever, uses since `recent_from`, and
    /// days from its last use to `today` (ISO dates); in one environment
    /// (and uses from before they were told apart), or `None` for all.
    pub fn item_uses(
        &self,
        today: &str,
        recent_from: &str,
        context: Option<&str>,
    ) -> Result<HashMap<String, ItemUse>> {
        let mut stmt = self.conn.prepare(
            "SELECT item_id, SUM(count),
                    SUM(CASE WHEN day >= ?2 THEN count ELSE 0 END),
                    CAST(julianday(?1) - julianday(MAX(day)) AS INTEGER)
             FROM commands_used WHERE ?3 IS NULL OR context = ?3 OR context = 'any'
             GROUP BY item_id",
        )?;
        let rows = stmt.query_map(params![today, recent_from, context], |r| {
            Ok((
                r.get(0)?,
                ItemUse {
                    total: r.get(1)?,
                    recent: r.get(2)?,
                    days_idle: r.get::<_, i64>(3)?.max(0) as u64,
                },
            ))
        })?;
        rows.collect()
    }

    /// Each skill's mastery and the half-life model, from every day's uses
    /// and the habits that speak against them: see
    /// [`flowbuddy_core::mastery`]. In one environment (and uses from before
    /// they were told apart), or `None` for all.
    pub fn skills(
        &self,
        today: &str,
        context: Option<&str>,
    ) -> Result<(
        HashMap<String, flowbuddy_core::mastery::Skill>,
        flowbuddy_core::mastery::Model,
    )> {
        let rows = |sql: &str| -> Result<Vec<(u64, String, u64)>> {
            let mut stmt = self.conn.prepare(sql)?;
            let rows = stmt.query_map(params![today, context], |r| {
                Ok((r.get::<_, i64>(0)?.max(0) as u64, r.get(1)?, r.get(2)?))
            })?;
            rows.collect()
        };
        let uses = rows(
            "SELECT CAST(julianday(?1) - julianday(day) AS INTEGER), item_id, SUM(count)
             FROM commands_used WHERE ?2 IS NULL OR context = ?2 OR context = 'any'
             GROUP BY day, item_id",
        )?;
        let hits = rows(
            "SELECT CAST(julianday(?1) - julianday(day) AS INTEGER), rule_id, SUM(hits)
             FROM daily_stats WHERE ?2 IS NULL OR context = ?2
             GROUP BY day, rule_id",
        )?;
        let mut histories = flowbuddy_core::mastery::histories(
            uses.iter().map(|(a, i, n)| (*a, i.as_str(), *n)),
            hits.iter().map(|(a, r, n)| (*a, r.as_str(), *n)),
            |rule| {
                flowbuddy_core::curriculum::rule_skills(rule)
                    .unwrap_or_default()
                    .to_vec()
            },
        );
        // A missed challenge is about that one skill: a whole miss.
        let mut stmt = self.conn.prepare(
            "SELECT CAST(julianday(?1) - julianday(day) AS INTEGER), item_id
             FROM challenges WHERE outcome = 'missed'",
        )?;
        let missed = stmt
            .query_map(params![today], |r| {
                Ok((r.get::<_, i64>(0)?.max(0) as u64, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>>>()?;
        for (ago, item) in missed {
            flowbuddy_core::mastery::missed(histories.entry(item).or_default(), ago);
        }
        Ok(flowbuddy_core::mastery::assess(&histories))
    }

    /// How a challenge ended: `done`, `missed` or `expired`.
    pub fn challenge_ended(
        &self,
        item_id: &str,
        set_at: u64,
        day: &str,
        outcome: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO challenges (item_id, set_at, day, outcome) VALUES (?1, ?2, ?3, ?4)",
            params![item_id, set_at, day, outcome],
        )?;
        Ok(())
    }

    /// Challenges per item: (done, missed, expired).
    pub fn challenge_outcomes(&self) -> Result<HashMap<String, (u64, u64, u64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT item_id, SUM(outcome = 'done'), SUM(outcome = 'missed'),
                    SUM(outcome = 'expired')
             FROM challenges GROUP BY item_id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))?;
        rows.collect()
    }

    /// Say an item is known (`Some(true)`), not known, or back to what use
    /// shows (`None`).
    pub fn set_known_override(&self, item_id: &str, known: Option<bool>) -> Result<()> {
        match known {
            Some(k) => self.conn.execute(
                "INSERT INTO known_overrides (item_id, known) VALUES (?1, ?2)
                 ON CONFLICT (item_id) DO UPDATE SET known = ?2",
                params![item_id, k],
            )?,
            None => self.conn.execute(
                "DELETE FROM known_overrides WHERE item_id = ?1",
                params![item_id],
            )?,
        };
        Ok(())
    }

    pub fn known_overrides(&self) -> Result<HashMap<String, bool>> {
        let mut stmt = self
            .conn
            .prepare("SELECT item_id, known FROM known_overrides")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// The day each curriculum item was first used.
    pub fn item_first_used(&self) -> Result<HashMap<String, String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT item_id, MIN(day) FROM commands_used GROUP BY item_id")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// Per environment (uses from before they were told apart as 'any'),
    /// the day each curriculum item was first used: the orders
    /// [`flowbuddy_core::links`] learns from.
    pub fn first_uses_by_context(&self) -> Result<HashMap<String, HashMap<String, String>>> {
        let mut stmt = self.conn.prepare(
            "SELECT context, item_id, MIN(day) FROM commands_used GROUP BY context, item_id",
        )?;
        let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?)))?;
        for row in rows {
            let (ctx, item, day) = row?;
            out.entry(ctx).or_default().insert(item, day);
        }
        Ok(out)
    }

    /// The prerequisite links with the evidence from your order of
    /// learning.
    pub fn links(&self) -> Result<Vec<flowbuddy_core::links::Link>> {
        Ok(flowbuddy_core::links::learn(
            self.first_uses_by_context()?.values(),
        ))
    }

    /// The environments uses were counted in (not 'any').
    pub fn use_contexts(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT context FROM commands_used WHERE context != 'any' ORDER BY context",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
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
            "INSERT INTO tip_outcomes (shown_at, rule_id, context, stage, hour, mode, prior_today,
                                       held)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                tip.shown_at,
                tip.rule_id,
                tip.context,
                tip.stage,
                tip.hour,
                tip.mode,
                tip.prior_today,
                tip.held
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
            "SELECT shown_at, rule_id, context, stage, hour, mode, prior_today, outcome, held
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
                held: r.get(8)?,
            })
        })?;
        rows.collect()
    }

    /// Drop tip rows older than `before_s`.
    pub fn prune_tips(&self, before_s: u64) -> Result<usize> {
        self.conn.execute(
            "DELETE FROM tip_history WHERE shown_at < ?1",
            params![before_s],
        )?;
        self.conn.execute(
            "DELETE FROM tip_outcomes WHERE shown_at < ?1",
            params![before_s],
        )
    }

    pub fn add_history(&self, tip: &HistoryTip) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tip_history (shown_at, rule_id, title, text, insert_mode)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![tip.shown_at, tip.rule_id, tip.title, tip.text, tip.insert],
        )?;
        Ok(())
    }

    /// The latest `limit` tips whose title, text or rule contains every
    /// word of `search` (any case), newest first.
    pub fn history(&self, search: &str, limit: usize) -> Result<Vec<HistoryTip>> {
        let mut stmt = self.conn.prepare(
            "SELECT shown_at, rule_id, title, text, insert_mode FROM tip_history
             ORDER BY shown_at DESC, id DESC",
        )?;
        let words: Vec<String> = search.split_whitespace().map(str::to_lowercase).collect();
        let rows = stmt.query_map([], |r| {
            Ok(HistoryTip {
                shown_at: r.get(0)?,
                rule_id: r.get(1)?,
                title: r.get(2)?,
                text: r.get(3)?,
                insert: r.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for tip in rows {
            let tip = tip?;
            let hay = format!("{} {} {}", tip.rule_id, tip.title, tip.text).to_lowercase();
            if words.iter().all(|w| hay.contains(w.as_str())) {
                out.push(tip);
                if out.len() == limit {
                    break;
                }
            }
        }
        Ok(out)
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
            ("commands_used", "day, context, item_id"),
            ("learn_items", "item_id"),
            ("tip_history", "rule_id, title, text"),
            ("known_overrides", "item_id"),
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
    // What got better leads: the habit done the better way most more
    // often than last week.
    let gain = now
        .iter()
        .map(|(id, (_, a))| (id, *a, then.get(id).map_or(0, |v| v.1)))
        .filter(|(_, a, was)| *a >= 3 && a > was)
        .max_by_key(|(id, a, was)| (a - was, std::cmp::Reverse((*id).clone())));
    let mut parts = Vec::new();
    if let Some((id, a, was)) = gain {
        parts.push(format!(
            "Better this week: {} the better way {a} times, up from {was}.",
            describe(id)
        ));
    }
    if !mastered.is_empty() {
        parts.push(format!("Mastered: {}.", mastered.join("; ")));
    }
    if !improved.is_empty() {
        parts.push(format!(
            "Less often than last week: {}.",
            improved.join("; ")
        ));
    }
    let times = |n: u64| {
        if n == 1 {
            "once".to_string()
        } else {
            format!("{n} times")
        }
    };
    parts.push(format!(
        "The better way {}, {hits} habit{} spotted.",
        times(adoptions),
        if hits == 1 { "" } else { "s" }
    ));
    if let Some(focus) = focus {
        parts.push(format!("Work on next: {}.", describe(focus)));
    }
    let text = parts.join(" ");
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
    fn the_week_leads_with_what_got_better() {
        let s = Store::open_in_memory().unwrap();
        let now = 1_800_000_000_000u64;
        let today = days::day(now);
        let last_week = days::day(now - 8 * 86_400_000);
        for _ in 0..3 {
            s.record(&last_week, "terminal-vim", "vim.repeat-jk", HIT, 0)
                .unwrap();
        }
        s.record(
            &last_week,
            "terminal-vim",
            "vim.repeat-jk",
            Event::Adoption,
            0,
        )
        .unwrap();
        for _ in 0..5 {
            s.record(&today, "terminal-vim", "vim.repeat-jk", Event::Adoption, 0)
                .unwrap();
        }
        s.record(&today, "terminal-vim", "vim.repeat-x", HIT, 0)
            .unwrap();
        let text = week_summary(&s, now, &|id| format!("<{id}>"), Some("vim.repeat-x")).unwrap();
        assert_eq!(
            text,
            "Better this week: <vim.repeat-jk> the better way 5 times, up from 1. \
             Less often than last week: <vim.repeat-jk>. The better way 5 times, 1 habit spotted. \
             Work on next: <vim.repeat-x>."
        );
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
    fn recent_and_idle_uses() {
        let s = Store::open_in_memory().unwrap();
        s.add_command_use("2026-06-01", "any", "marks", 9).unwrap();
        s.add_command_use("2026-10-20", "terminal-vim", "ciw", 2)
            .unwrap();
        s.add_command_use("2026-10-26", "vscode-vim", "ciw", 1)
            .unwrap();
        let u = s.item_uses("2026-10-27", "2026-09-27", None).unwrap();
        assert_eq!(
            u["ciw"],
            ItemUse {
                total: 3,
                recent: 3,
                days_idle: 1
            }
        );
        assert_eq!(u["marks"].recent, 0);
        assert!(u["marks"].days_idle > 140);
        // Per environment: its own uses, and the old ones.
        let vs = s
            .item_uses("2026-10-27", "2026-09-27", Some("vscode-vim"))
            .unwrap();
        assert_eq!(vs["ciw"].total, 1);
        assert_eq!(vs["marks"].total, 9);
    }

    #[test]
    fn tip_history_is_searchable() {
        let s = Store::open_in_memory().unwrap();
        let tip = |at, rule: &str, text: &str| HistoryTip {
            shown_at: at,
            rule_id: rule.into(),
            title: "Try a count".into(),
            text: text.into(),
            insert: false,
        };
        s.add_history(&tip(1, "vim.repeat-jk", "Try `7j`."))
            .unwrap();
        s.add_history(&tip(2, "vim.undo-run", "`7u` undoes that many."))
            .unwrap();
        s.add_history(&tip(3, "vim.repeat-jk", "Try `9j` or `}`."))
            .unwrap();
        let all = s.history("", 10).unwrap();
        assert_eq!(
            all.iter().map(|t| t.shown_at).collect::<Vec<_>>(),
            [3, 2, 1]
        );
        assert_eq!(s.history("", 2).unwrap().len(), 2);
        assert_eq!(s.history("UNDOES", 10).unwrap()[0].shown_at, 2);
        assert_eq!(s.history("repeat-jk 7j", 10).unwrap()[0].shown_at, 1);
        s.prune_tips(3).unwrap();
        assert_eq!(s.history("", 10).unwrap().len(), 1);
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
            held: false,
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
        s.add_command_use("2026-10-24", "terminal-vim", "ciw", 2)
            .unwrap();
        s.add_command_use("2026-10-25", "terminal-vim", "ciw", 1)
            .unwrap();
        s.add_command_use("2026-10-25", "terminal-vim", "star", 4)
            .unwrap();
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

    #[test]
    fn inflated_adoptions_are_capped() {
        let s = Store::open_in_memory().unwrap();
        s.record("2026-10-24", "vscode-vim", "vim.arrows-normal", HIT, 0)
            .unwrap();
        for _ in 0..600 {
            s.record(
                "2026-10-24",
                "vscode-vim",
                "vim.arrows-normal",
                Event::Adoption,
                0,
            )
            .unwrap();
        }
        // After the fix: left alone.
        for _ in 0..50 {
            s.record(
                "2026-10-27",
                "vscode-vim",
                "vim.arrows-normal",
                Event::Adoption,
                0,
            )
            .unwrap();
        }
        let found = s.inflated_adoptions("2026-10-26").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].adoptions, found[0].capped), (600, 6));
        assert_eq!(s.fix_adoptions("2026-10-26").unwrap(), 1);
        let p = &s.patterns().unwrap()[0];
        assert_eq!(p.adoptions, 56);
        assert!(s.inflated_adoptions("2026-10-26").unwrap().is_empty());
    }

    #[test]
    fn skills_come_from_uses_and_habits() {
        let s = Store::open_in_memory().unwrap();
        for day in ["2026-10-20", "2026-10-21", "2026-10-22"] {
            s.add_command_use(day, "vscode-vim", "find", 1).unwrap();
        }
        s.add_command_use("2026-10-22", "terminal-vim", "word", 1)
            .unwrap();
        // Back on hhhhh three days running: against word and find.
        for day in ["2026-10-24", "2026-10-25", "2026-10-26"] {
            s.record(day, "terminal-vim", "vim.repeat-hl", HIT, 0)
                .unwrap();
        }
        let (all, model) = s.skills("2026-10-27", None).unwrap();
        assert!(all["find"].mastery > all["word"].mastery, "{all:?}");
        assert_eq!(all["find"].idle, Some(5));
        assert!(all["find"].recall(&model) > 0.0);
        let (here, _) = s.skills("2026-10-27", Some("vscode-vim")).unwrap();
        assert!(!here.contains_key("word"), "{here:?}");
    }

    #[test]
    fn links_learn_from_each_places_order() {
        let s = Store::open_in_memory().unwrap();
        for ctx in ["vscode-vim", "terminal-vim", "bash-vi"] {
            s.add_command_use("2026-10-01", ctx, "till", 1).unwrap();
        }
        s.add_command_use("2026-10-03", "vscode-vim", "find", 1)
            .unwrap();
        let firsts = s.first_uses_by_context().unwrap();
        assert_eq!(firsts["vscode-vim"]["find"], "2026-10-03");
        let links = s.links().unwrap();
        let till = links.iter().find(|l| l.item == "till").unwrap();
        assert_eq!((till.for_, till.against), (0, 3));
        assert!(!till.holds());
    }
}
