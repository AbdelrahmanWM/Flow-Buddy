//! Recorded sessions: what the rules were fed, one JSON line per event,
//! so they can be checked against real use (`flowbuddy record`).
//!
//! Recordings hold the tracker's steps, not raw keys, so typed text never
//! gets in: Insert-mode keys are only their kind ("text", "backspace"),
//! the character after `f`, `t` or `r` becomes `x` (or `1` for a digit),
//! searches lose their pattern, and a `:` command keeps its name and
//! punctuation with every letter or digit after the name replaced
//! (`:%s/x/x/x`). Motion and operator reports are numbers and flags
//! already.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::context::{ContextKind, VimMode};
use crate::editor::{Motion, OperatorReport};
use crate::engine::{Engine, RuleSettings};
use crate::key::{Key, Mods, Sym};
use crate::motion::{MotionEvent, OperatorEvent};
use crate::rule::{Finding, RuleEvent};
use crate::vim::{Command, InsertKey, Step, VimEvent};

pub const VERSION: u32 = 1;

/// One line of a recording. `t` is milliseconds since it started.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "e", rename_all = "snake_case")]
pub enum Entry {
    Start {
        version: u32,
    },
    /// The context changed; `editor`: an editor plugin reports motions.
    Context {
        t: u64,
        context: ContextKind,
        editor: bool,
    },
    Step {
        t: u64,
        before: VimMode,
        after: VimMode,
        confident: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        repeat: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event: Option<Event>,
    },
    Motion {
        t: u64,
        motion: Motion,
        keys_used: u32,
    },
    Operator {
        t: u64,
        report: OperatorReport,
        keys_used: u32,
        #[serde(default)]
        op_keys: u32,
    },
    /// In-progress streaks were dropped (a click, a VS Code shortcut).
    Reset {
        t: u64,
    },
    /// A tip was shown, or what you did with it: `shown`, `dismiss`,
    /// `mute_rule` or `false_positive` (not helpful).
    Feedback {
        t: u64,
        rule: String,
        what: String,
    },
}

/// A step's event, scrubbed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Command {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<u32>,
        keys: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        register: Option<char>,
    },
    Insert {
        key: String,
    },
    Cmdline {
        cmd: char,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    Escape,
    Mapped,
}

/// Commands whose next key is a character typed, not a command.
fn takes_char(c: char) -> bool {
    matches!(c, 'f' | 't' | 'F' | 'T' | 'r')
}

fn scrub_char(c: char) -> char {
    if c.is_ascii_digit() { '1' } else { 'x' }
}

/// Keep a `:` command's name and punctuation; letters and digits after
/// the name become `x`.
fn scrub_cmdline(text: &str) -> String {
    let lead = text.len() - text.trim_start().len();
    let rest = &text[lead..];
    let name_len = rest
        .char_indices()
        .find(|(i, c)| !(c.is_ascii_alphabetic() || (*i == 0 && *c == '%')))
        .map_or(rest.len(), |(i, _)| i);
    let (name, args) = rest.split_at(name_len);
    let args: String = args
        .chars()
        .map(|c| if c.is_alphanumeric() { 'x' } else { c })
        .collect();
    format!("{}{name}{args}", &text[..lead])
}

fn insert_name(k: InsertKey) -> &'static str {
    match k {
        InsertKey::Text => "text",
        InsertKey::Backspace => "backspace",
        InsertKey::CtrlW => "ctrl-w",
        InsertKey::CtrlU => "ctrl-u",
        InsertKey::CtrlR => "ctrl-r",
        InsertKey::CtrlO => "ctrl-o",
        InsertKey::Arrow(Sym::Up) => "up",
        InsertKey::Arrow(Sym::Down) => "down",
        InsertKey::Arrow(Sym::Left) => "left",
        InsertKey::Arrow(Sym::Right) => "right",
        InsertKey::Arrow(_) => "arrow",
        InsertKey::Enter => "enter",
        InsertKey::Other => "other",
    }
}

fn insert_key(name: &str) -> InsertKey {
    match name {
        "text" => InsertKey::Text,
        "backspace" => InsertKey::Backspace,
        "ctrl-w" => InsertKey::CtrlW,
        "ctrl-u" => InsertKey::CtrlU,
        "ctrl-r" => InsertKey::CtrlR,
        "ctrl-o" => InsertKey::CtrlO,
        "up" => InsertKey::Arrow(Sym::Up),
        "down" => InsertKey::Arrow(Sym::Down),
        "left" => InsertKey::Arrow(Sym::Left),
        "right" => InsertKey::Arrow(Sym::Right),
        "arrow" => InsertKey::Arrow(Sym::Other),
        "enter" => InsertKey::Enter,
        _ => InsertKey::Other,
    }
}

impl Event {
    pub fn scrubbed(ev: &VimEvent) -> Event {
        match ev {
            VimEvent::Command(c) => {
                let mut keys: Vec<String> = Vec::with_capacity(c.keys.len());
                let mut arg = false;
                for k in &c.keys {
                    let plain = k.plain_char();
                    if arg {
                        keys.push(plain.map_or('x', scrub_char).to_string());
                        arg = false;
                        continue;
                    }
                    arg = plain.is_some_and(takes_char);
                    keys.push(k.to_string());
                }
                Event::Command {
                    count: c.count,
                    keys,
                    register: c.register,
                }
            }
            VimEvent::Insert(k) => Event::Insert {
                key: insert_name(*k).into(),
            },
            VimEvent::Cmdline { kind, text } => Event::Cmdline {
                cmd: *kind,
                text: text.as_deref().map(|t| {
                    if *kind == ':' {
                        scrub_cmdline(t)
                    } else {
                        "x".into()
                    }
                }),
            },
            VimEvent::Escape => Event::Escape,
            VimEvent::Mapped => Event::Mapped,
        }
    }

    fn event(&self) -> Option<VimEvent> {
        Some(match self {
            Event::Command {
                count,
                keys,
                register,
            } => VimEvent::Command(Command {
                count: *count,
                keys: keys.iter().map(|k| parse_key(k)).collect::<Option<_>>()?,
                register: *register,
            }),
            Event::Insert { key } => VimEvent::Insert(insert_key(key)),
            Event::Cmdline { cmd, text } => VimEvent::Cmdline {
                kind: *cmd,
                text: text.clone(),
            },
            Event::Escape => VimEvent::Escape,
            Event::Mapped => VimEvent::Mapped,
        })
    }
}

/// One key in the notation [`Key`]'s `Display` writes: `j`, `<`, `<C-d>`,
/// `<Esc>`, `<C-M-Space>`.
pub fn parse_key(s: &str) -> Option<Key> {
    let mut chars = s.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Some(Key::ch(c));
    }
    let mut name = s.strip_prefix('<')?.strip_suffix('>')?;
    let mut mods = Mods::NONE;
    loop {
        let (flag, rest) = match name.split_at_checked(2) {
            Some(("C-", r)) if !r.is_empty() => (&mut mods.ctrl, r),
            Some(("M-", r)) if !r.is_empty() => (&mut mods.alt, r),
            Some(("S-", r)) if !r.is_empty() => (&mut mods.shift, r),
            Some(("D-", r)) if !r.is_empty() => (&mut mods.logo, r),
            _ => break,
        };
        *flag = true;
        name = rest;
    }
    let sym = match name {
        "Space" => Sym::Char(' '),
        "Esc" => Sym::Esc,
        "CR" => Sym::Enter,
        "Tab" => Sym::Tab,
        "BS" => Sym::Backspace,
        "Del" => Sym::Delete,
        "Insert" => Sym::Insert,
        "Up" => Sym::Up,
        "Down" => Sym::Down,
        "Left" => Sym::Left,
        "Right" => Sym::Right,
        "Home" => Sym::Home,
        "End" => Sym::End,
        "PageUp" => Sym::PageUp,
        "PageDown" => Sym::PageDown,
        "?" => Sym::Other,
        f if f.starts_with('F') && f.len() > 1 => Sym::F(f[1..].parse().ok()?),
        c => {
            let mut it = c.chars();
            match (it.next(), it.next()) {
                (Some(ch), None) => Sym::Char(ch),
                _ => return None,
            }
        }
    };
    Some(Key { sym, mods })
}

/// The entry for a step, if it's worth keeping: anything that happened,
/// and unconfident steps, which drop streaks.
pub fn step_entry(t: u64, step: &Step) -> Option<Entry> {
    if step.event.is_none() && step.confident {
        return None;
    }
    Some(Entry::Step {
        t,
        before: step.before,
        after: step.after,
        confident: step.confident,
        repeat: step.repeat,
        event: step.event.as_ref().map(Event::scrubbed),
    })
}

/// Parse a recording, one entry per line; bad lines are errors.
pub fn parse(text: &str) -> Result<Vec<Entry>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("line {}: {e}", i + 1)))
        .collect()
}

/// Run a recording through the rules as the daemon would, ticking once a
/// second, and return what they found.
pub fn replay(entries: &[Entry]) -> Vec<Finding> {
    replay_with(entries, &RuleSettings::default())
}

/// [`replay`] with rule settings (thresholds) other than the defaults.
pub fn replay_with(entries: &[Entry], settings: &RuleSettings) -> Vec<Finding> {
    let mut engine = Engine::standard();
    engine.apply(settings);
    let mut context = ContextKind::Unsupported;
    let mut found = Vec::new();
    let mut last_tick = 0;
    let mut keep = |events: Vec<RuleEvent>| {
        found.extend(events.into_iter().filter_map(|e| match e {
            RuleEvent::Finding(f) => Some(f),
            RuleEvent::Adoption { .. } => None,
        }))
    };
    for entry in entries {
        let t = match entry {
            Entry::Start { .. } => continue,
            Entry::Context { t, .. }
            | Entry::Step { t, .. }
            | Entry::Motion { t, .. }
            | Entry::Operator { t, .. }
            | Entry::Reset { t }
            | Entry::Feedback { t, .. } => *t,
        };
        while last_tick + 1000 <= t {
            last_tick += 1000;
            keep(engine.tick(context, last_tick));
        }
        match entry {
            Entry::Start { .. } => {}
            Entry::Context {
                context: c, editor, ..
            } => {
                keep(engine.context_changed(context, t));
                context = *c;
                engine.set_editor_motions(*editor);
            }
            Entry::Step {
                before,
                after,
                confident,
                repeat,
                event,
                ..
            } => {
                let step = Step {
                    before: *before,
                    after: *after,
                    confident: *confident,
                    event: event.as_ref().and_then(Event::event),
                    repeat: *repeat,
                    at_ms: t,
                };
                keep(engine.on_step(context, &step));
            }
            Entry::Motion {
                motion, keys_used, ..
            } => keep(engine.on_motion(
                context,
                &MotionEvent {
                    motion: motion.clone(),
                    keys_used: *keys_used,
                },
            )),
            Entry::Operator {
                report,
                keys_used,
                op_keys,
                ..
            } => keep(engine.on_operator(
                context,
                &OperatorEvent {
                    report: report.clone(),
                    keys_used: *keys_used,
                    op_keys: *op_keys,
                },
            )),
            Entry::Reset { .. } => engine.reset(),
            Entry::Feedback { .. } => {}
        }
    }
    keep(engine.tick(context, last_tick + 1_000_000));
    found
}

/// Findings per rule.
pub fn counts(found: &[Finding]) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    for f in found {
        *out.entry(f.rule_id.to_string()).or_default() += 1;
    }
    out
}

/// Times (ms into the recording) a rule's tips were marked dismissed or not
/// helpful.
pub fn unwanted(entries: &[Entry], rule: &str) -> Vec<u64> {
    entries
        .iter()
        .filter_map(|e| match e {
            Entry::Feedback { t, rule: r, what }
                if r == rule
                    && matches!(what.as_str(), "dismiss" | "false_positive" | "mute_rule") =>
            {
                Some(*t)
            }
            _ => None,
        })
        .collect()
}

/// One row of `flowbuddy record tune`: at this threshold, how many tips
/// the rule gives over the recordings, and how many of them were ones you
/// marked unwanted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TuneRow {
    pub threshold: u32,
    pub tips: u32,
    pub unwanted: u32,
}

/// A tip is the one you reacted to if your reaction came within this long
/// after it (time to read it and click).
const REACTION_MS: u64 = 120_000;

/// Each threshold-carrying rule, with what lower and higher thresholds
/// would have given over `sessions`.
pub fn tune(sessions: &[Vec<Entry>]) -> Vec<(String, u32, Vec<TuneRow>)> {
    let engine = Engine::standard();
    let mut out = Vec::new();
    for rule in engine.rules() {
        let id = rule.id();
        let Some(default) = engine.default_threshold(id) else {
            continue;
        };
        let mut values = vec![
            default.saturating_sub(1).max(1),
            default,
            default + 1,
            default * 2,
        ];
        values.dedup();
        let rows: Vec<TuneRow> = values
            .into_iter()
            .map(|threshold| {
                let settings = RuleSettings {
                    thresholds: [(id.to_string(), threshold)].into(),
                    ..Default::default()
                };
                let (mut tips, mut unwanted_n) = (0, 0);
                for entries in sessions {
                    let marks = unwanted(entries, id);
                    for f in replay_with(entries, &settings)
                        .into_iter()
                        .filter(|f| f.rule_id == id)
                    {
                        tips += 1;
                        if marks
                            .iter()
                            .any(|m| *m >= f.at_ms && m - f.at_ms <= REACTION_MS)
                        {
                            unwanted_n += 1;
                        }
                    }
                }
                TuneRow {
                    threshold,
                    tips,
                    unwanted: unwanted_n,
                }
            })
            .collect();
        if rows.iter().any(|r| r.tips > 0) {
            out.push((id.to_string(), default, rows));
        }
    }
    out
}

/// How a replay compares with what was expected, per rule.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Score {
    /// Expected and found.
    pub hits: u32,
    /// Found but not expected: false positives.
    pub extra: u32,
    /// Expected but not found.
    pub missed: u32,
}

pub fn score(
    expected: &BTreeMap<String, u32>,
    found: &BTreeMap<String, u32>,
) -> BTreeMap<String, Score> {
    let mut out: BTreeMap<String, Score> = BTreeMap::new();
    for rule in expected.keys().chain(found.keys()) {
        let e = expected.get(rule).copied().unwrap_or(0);
        let f = found.get(rule).copied().unwrap_or(0);
        out.insert(
            rule.clone(),
            Score {
                hits: e.min(f),
                extra: f.saturating_sub(e),
                missed: e.saturating_sub(f),
            },
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vim::notation::strokes;
    use crate::vim::{Flavor, Tracker};

    /// Record `keys` typed in terminal Vim, as the daemon would.
    pub(crate) fn record(keys: &str) -> Vec<Entry> {
        let mut t = Tracker::new(Flavor::Vim);
        t.reset_known(VimMode::Normal);
        let mut out = vec![
            Entry::Start { version: VERSION },
            Entry::Context {
                t: 0,
                context: ContextKind::TerminalVim,
                editor: false,
            },
        ];
        for s in strokes(keys, 10) {
            let step = t.feed(s);
            out.extend(step_entry(s.at_ms, &step));
        }
        out
    }

    #[test]
    fn nothing_typed_is_kept() {
        let entries = record("ihello world<Esc>fqrZ/secret<CR>:s/foo/bar/g<CR>");
        let text: String = entries
            .iter()
            .map(|e| serde_json::to_string(e).unwrap() + "\n")
            .collect();
        for word in ["hello", "world", "secret", "foo", "bar", "\"q\"", "\"Z\""] {
            assert!(!text.contains(word), "{word} leaked:\n{text}");
        }
        assert!(text.contains(r#""text":"s/xxx/xxx/x""#), "{text}");
        assert_eq!(parse(&text).unwrap(), entries);
    }

    #[test]
    fn keys_round_trip() {
        for k in [
            Key::ch('j'),
            Key::ch('<'),
            Key::ctrl('d'),
            Key::ESC,
            Key::ENTER,
            Key::UP,
            Key::ctrl(' '),
            Key {
                sym: Sym::F(5),
                mods: Mods::NONE,
            },
        ] {
            assert_eq!(parse_key(&k.to_string()), Some(k), "{k}");
        }
        assert_eq!(parse_key("<Nope>"), None);
    }

    #[test]
    fn replays_like_the_daemon() {
        let found = replay(&record("jjjjjjj<Esc>xxxxxxxihi<Esc>"));
        let ids: Vec<_> = found.iter().map(|f| f.rule_id).collect();
        assert_eq!(ids, ["vim.repeat-jk", "vim.delete-then-insert"]);
        let s = score(
            &BTreeMap::from([("vim.repeat-jk".into(), 1), ("vim.macro".into(), 1)]),
            &counts(&found),
        );
        assert_eq!(s["vim.repeat-jk"].hits, 1);
        assert_eq!(s["vim.delete-then-insert"].extra, 1);
        assert_eq!(s["vim.macro"].missed, 1);
    }

    #[test]
    fn tuning_shows_what_thresholds_would_give() {
        let mut entries = record("jjjjjj<Esc>kkkkkkkkkk<Esc>");
        // The first streak's tip was marked not helpful.
        let first = replay(&entries)[0].at_ms;
        entries.push(Entry::Feedback {
            t: first + 1000,
            rule: "vim.repeat-jk".into(),
            what: "false_positive".into(),
        });
        let rows = tune(&[entries]);
        let (_, default, jk) = rows
            .iter()
            .find(|(id, _, _)| id == "vim.repeat-jk")
            .unwrap();
        assert_eq!(*default, 5);
        let at = |t: u32| jk.iter().find(|r| r.threshold == t).unwrap().clone();
        assert_eq!(
            at(5),
            TuneRow {
                threshold: 5,
                tips: 2,
                unwanted: 1
            }
        );
        // At 10, the 6-press streak is left alone: the unwanted tip goes.
        assert_eq!(
            at(10),
            TuneRow {
                threshold: 10,
                tips: 1,
                unwanted: 0
            }
        );
        assert_eq!(
            jk.iter().map(|r| r.threshold).collect::<Vec<_>>(),
            [4, 5, 6, 10]
        );
    }
}
