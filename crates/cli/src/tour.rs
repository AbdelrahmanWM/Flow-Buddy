//! `flowbuddy tour`: how Flow Buddy works, in a few short parts, for
//! someone new to it. It checks the setup, shows a sample tip where tips
//! show, and asks the setup questions; the rest is reading.
//!
//! The parts are data ([`PARTS`]): this terminal tour reads them, and so
//! does the settings window's, through `flowbuddy tour --json`. Commands
//! in the text are marked with backticks.

use std::io::{BufRead, Write};

use crate::doctor::Check;
use crate::setup;

/// What a part does besides its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    None,
    /// Run the doctor's checks, between the text and the rows.
    Checks,
    /// Show the sample tip, before the text.
    Sample,
    /// Ask the setup questions (the settings window shows them as rows).
    Settings,
}

#[derive(Debug, serde::Serialize)]
pub struct Part {
    pub title: &'static str,
    pub text: &'static str,
    /// (command or label, what it is).
    pub rows: &'static [(&'static str, &'static str)],
    pub after: &'static str,
    pub action: Action,
}

/// The settings the tour asks about, as `config` keys.
pub const SETTINGS: [&str; 5] = [
    "coaching.default_mode",
    "coaching.when",
    "learn.level",
    "ai.provider",
    "text.shapes",
];

pub const PARTS: [Part; 7] = [
    Part {
        title: "What Flow Buddy does",
        text: "Flow Buddy watches how you use Vim's keys, in Vim, VS Code with vscodevim, vi-mode shells and Claude Code's Vim mode. Now and then it suggests a shorter way to do what you just did: one tip at a time, with the keys it took and the keys that would do.\n\nIt learns which commands you already use, so the tips move on as you do, and once a day it brings one new thing to try.\n\nWhat it keeps: counts and habits, like \"pressed j seven times in a row\". Never what you type, file names or window titles. Nothing goes on the network unless you choose an AI for `flowbuddy coach`.",
        rows: &[],
        after: "",
        action: Action::None,
    },
    Part {
        title: "Is everything running?",
        text: "",
        rows: &[],
        after: "`flowbuddy doctor` runs these checks any time.",
        action: Action::Checks,
    },
    Part {
        title: "A tip, and how to answer it",
        text: "Answer a tip from its menu (click it), or not at all:",
        rows: &[
            ("Got it", "you'll try it; seeing you use it is what counts"),
            (
                "Not helpful",
                "that tip comes less often, and needs a longer run to show",
            ),
            (
                "What does it do?",
                "each command in it explained, with an example",
            ),
        ],
        after: "Tips you don't answer go away by themselves. On GNOME, Got it and Not helpful have keyboard shortcuts (Settings… → Advanced).\n\n`flowbuddy tips` lists the recent ones; `flowbuddy explain 'ci\"'` explains any keys.",
        action: Action::Sample,
    },
    Part {
        title: "Where tips show",
        text: "",
        rows: &[
            (
                "In Vim",
                "a small window at the top right (:FlowBuddyGotIt, :FlowBuddyNotHelpful)",
            ),
            (
                "In VS Code",
                "the desktop card; the setting Flow Buddy: Tips can show them in VS Code",
            ),
            ("Anywhere else", "the desktop card, under the top bar"),
        ],
        after: "To have every tip on the desktop: `flowbuddy config set coaching.show_in desktop` (or editor, the default, or both).",
        action: Action::None,
    },
    Part {
        title: "The settings that matter most",
        text: "Five questions set them: how often tips come, when they show, how much Vim you know, an AI for coaching, and whether tips can name exact commands.",
        rows: &[],
        after: "Every other setting: `flowbuddy config list`, or Settings… in the top-bar menu. Ready-made sets: `flowbuddy config profile` (Gentle, Learning fast, Just watching…).",
        action: Action::Settings,
    },
    Part {
        title: "Learning, not just tips",
        text: "Once a day there's a lesson: one command you don't use yet, with an example. \"I know this\" skips it, \"Show me another\" swaps it. Something you learned and stop using comes back as a review.",
        rows: &[
            (
                "flowbuddy practice",
                "a two-minute drill that counts your keys (:FlowBuddyPractice in Vim)",
            ),
            (
                "flowbuddy learn",
                "today's lesson and how far through Vim you are",
            ),
            (
                "flowbuddy learn --map",
                "every skill as a tile: used, slipping, learning, ready",
            ),
            ("flowbuddy next", "the one habit worth working on now"),
            (
                "flowbuddy summary",
                "today against before; --week for the week",
            ),
            (
                "flowbuddy watch",
                "tips as they come, explained, in a terminal you keep open",
            ),
            (
                "flowbuddy coach '…'",
                "ask your AI, with your habits as context",
            ),
        ],
        after: "",
        action: Action::None,
    },
    Part {
        title: "Quieter, or louder",
        text: "",
        rows: &[
            (
                "flowbuddy pause 1h",
                "a break (30m, until-tomorrow); flowbuddy resume ends it",
            ),
            (
                "flowbuddy mode passive",
                "no tips during the day, a summary in the evening",
            ),
            (
                "flowbuddy mode active",
                "tips as you work; balanced is in between; off stops it",
            ),
            (
                "flowbuddy rules",
                "every rule; rules mute <id> silences one for good",
            ),
        ],
        after: "No tips at night: `flowbuddy config set coaching.quiet_hours 22:00-08:00`. Pause and the mode are in the top-bar menu too.\n\nThat's all. Try a drill now, two minutes: `flowbuddy practice`",
        action: Action::None,
    },
];

/// `tour --json`: the parts, for the settings window.
pub fn json() -> serde_json::Value {
    serde_json::json!({ "version": 1, "parts": PARTS, "settings": SETTINGS })
}

/// What the tour needs from outside, so it can be tested.
pub struct Env<'a> {
    pub checks: &'a dyn Fn() -> Vec<Check>,
    /// Show the sample tip: where it went ("card", "notification"), or why
    /// it couldn't be shown.
    pub sample: &'a dyn Fn() -> Result<String, String>,
    /// The config file's text, and how to save it.
    pub config: String,
    pub save: &'a dyn Fn(&str) -> std::io::Result<()>,
    pub color: bool,
}

struct Tour<'a, 'b> {
    input: &'a mut dyn BufRead,
    out: &'a mut dyn Write,
    env: &'a Env<'b>,
}

/// Wrapped at this width, like the rest of the CLI's prose.
pub(crate) const WIDTH: usize = 79;

impl Tour<'_, '_> {
    fn heading(&mut self, n: usize, title: &str) -> std::io::Result<()> {
        let text = format!(" {n} of {} · {title} ", PARTS.len());
        if self.env.color {
            writeln!(self.out, "\n\x1b[1;37;44m{text}\x1b[0m\n")
        } else {
            writeln!(self.out, "\n──{text}──\n")
        }
    }

    /// A command, picked out when there's colour.
    fn cmd(&self, c: &str) -> String {
        if self.env.color {
            format!("\x1b[1;36m{c}\x1b[0m")
        } else {
            c.to_string()
        }
    }

    /// Text with `commands` marked, wrapped, paragraph by paragraph.
    fn prose(&mut self, text: &str) -> std::io::Result<()> {
        for (i, para) in text.split("\n\n").enumerate() {
            if i > 0 {
                writeln!(self.out)?;
            }
            for line in wrap(para, WIDTH) {
                let marked = marks(&line, |c| self.cmd(c));
                writeln!(self.out, "{marked}")?;
            }
        }
        Ok(())
    }

    /// Rows of (command, what it does), lined up.
    fn table(&mut self, rows: &[(&str, &str)]) -> std::io::Result<()> {
        let width = rows
            .iter()
            .map(|(c, _)| c.chars().count())
            .max()
            .unwrap_or(0);
        // The second column wraps within itself, lined up under its start.
        let indent = " ".repeat(2 + width + 2);
        for (c, what) in rows {
            let pad = " ".repeat(width - c.chars().count());
            let c = self.cmd(c);
            let lines = wrap(what, WIDTH.saturating_sub(indent.len()).max(20));
            for (i, line) in lines.iter().enumerate() {
                if i == 0 {
                    writeln!(self.out, "  {c}{pad}  {line}")?;
                } else {
                    writeln!(self.out, "{indent}{line}")?;
                }
            }
        }
        Ok(())
    }

    /// Wait for Enter; q stops. The input ending stops too.
    fn next(&mut self) -> std::io::Result<bool> {
        write!(self.out, "\n[Enter] next · q to stop: ")?;
        self.out.flush()?;
        let mut line = String::new();
        Ok(self.input.read_line(&mut line)? != 0 && !line.trim().eq_ignore_ascii_case("q"))
    }

    /// A yes/no question; Enter is yes.
    fn yes(&mut self, ask: &str) -> std::io::Result<Option<bool>> {
        write!(self.out, "{ask} [Y/n]: ")?;
        self.out.flush()?;
        let mut line = String::new();
        if self.input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        Ok(Some(!line.trim().to_lowercase().starts_with('n')))
    }

    fn checks(&mut self) -> std::io::Result<()> {
        let checks = (self.env.checks)();
        let bad: Vec<&Check> = checks.iter().filter(|c| !c.ok).collect();
        if bad.is_empty() {
            writeln!(
                self.out,
                "Yes: all {} checks pass. Flow Buddy is watching.",
                checks.len()
            )?;
        } else {
            writeln!(
                self.out,
                "{} of {} checks pass. To fix:",
                checks.len() - bad.len(),
                checks.len()
            )?;
            for c in bad {
                let fix = c.fix.as_deref().unwrap_or("");
                writeln!(self.out, "  ✗ {}\n    {fix}", c.what)?;
            }
        }
        writeln!(self.out)
    }

    fn sample(&mut self) -> std::io::Result<()> {
        let said = match (self.env.sample)() {
            Ok(w) if w == "card" => "A made-up tip is on your screen now, under the top bar. Real ones look the same: what you did, and the shorter way.".to_string(),
            Ok(_) => "A made-up tip is in your notifications now. Real ones look the same: what you did, and the shorter way.".to_string(),
            Err(why) => format!("(The sample can't be shown: {why}. It would read:)\n  A sample tip: That took 7 keys. }} gets there in 1, or 7j in 2."),
        };
        for line in said.lines() {
            if line.starts_with("  ") {
                writeln!(self.out, "{line}")?;
            } else {
                for l in wrap(line, WIDTH) {
                    writeln!(self.out, "{l}")?;
                }
            }
        }
        writeln!(self.out)
    }

    /// The setup questions; false if the input ended.
    fn settings(&mut self) -> std::io::Result<bool> {
        match self.yes("\nAnswer them now?")? {
            None => return Ok(false),
            Some(false) => {
                let setup = self.cmd("flowbuddy setup");
                writeln!(self.out, "\nThey're in {setup} whenever you like.")?;
            }
            Some(true) => {
                writeln!(self.out, "Enter keeps the current choice (*).")?;
                let Some(answers) = setup::ask(&self.env.config, self.input, self.out)? else {
                    return Ok(false);
                };
                (self.env.save)(&setup::apply(&self.env.config, &answers))?;
                let setup = self.cmd("flowbuddy setup");
                writeln!(
                    self.out,
                    "\nSaved; the daemon picks it up by itself. {setup} asks again."
                )?;
            }
        }
        Ok(true)
    }

    fn part(&mut self, n: usize, part: &Part) -> std::io::Result<bool> {
        self.heading(n, part.title)?;
        if part.action == Action::Sample {
            self.sample()?;
        }
        if !part.text.is_empty() {
            self.prose(part.text)?;
        }
        if part.action == Action::Checks {
            self.checks()?;
        }
        if part.action == Action::Settings && !self.settings()? {
            return Ok(false);
        }
        if !part.rows.is_empty() {
            if !part.text.is_empty() && part.action != Action::Sample {
                writeln!(self.out)?;
            }
            self.table(part.rows)?;
        }
        if !part.after.is_empty() {
            if !part.rows.is_empty() || part.action == Action::Settings {
                writeln!(self.out)?;
            }
            self.prose(part.after)?;
        }
        Ok(true)
    }
}

/// Words joined into lines no wider than `width` (marks don't count).
pub(crate) fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let len = |s: &str| s.chars().filter(|c| *c != '`').count();
        if !line.is_empty() && len(&line) + 1 + len(word) > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// `cmd` applied to each `marked` part, without the backticks.
fn marks(text: &str, cmd: impl Fn(&str) -> String) -> String {
    let mut out = String::new();
    for (i, piece) in text.split('`').enumerate() {
        if i % 2 == 1 {
            out.push_str(&cmd(piece));
        } else {
            out.push_str(piece);
        }
    }
    out
}

/// Run the tour. Returns whether it got to the end.
pub fn run(env: &Env, input: &mut dyn BufRead, out: &mut dyn Write) -> std::io::Result<bool> {
    let mut t = Tour { input, out, env };
    writeln!(
        t.out,
        "Flow Buddy in {} short parts. Enter goes on; q stops (flowbuddy tour again).",
        PARTS.len()
    )?;
    for (i, part) in PARTS.iter().enumerate() {
        if !t.part(i + 1, part)? {
            return Ok(false);
        }
        if i + 1 < PARTS.len() && !t.next()? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn tour(
        input: &[u8],
        sample: Result<String, String>,
        checks: Vec<Check>,
    ) -> (String, bool, Option<String>) {
        let saved = RefCell::new(None);
        let env = Env {
            checks: &|| checks.clone(),
            sample: &|| sample.clone(),
            config: String::new(),
            save: &|t| {
                *saved.borrow_mut() = Some(t.to_string());
                Ok(())
            },
            color: false,
        };
        let mut input = input;
        let mut out = Vec::new();
        let done = run(&env, &mut input, &mut out).unwrap();
        (String::from_utf8(out).unwrap(), done, saved.into_inner())
    }

    fn check(ok: bool, what: &str) -> Check {
        Check {
            ok,
            what: what.into(),
            fix: (!ok).then(|| "do this".into()),
        }
    }

    #[test]
    fn all_the_way_through_without_the_questions() {
        let (out, done, saved) = tour(
            b"\n\n\n\nn\n\n\n",
            Ok("card".into()),
            vec![check(true, "daemon"), check(true, "keyboard")],
        );
        assert!(done, "{out}");
        assert!(saved.is_none());
        assert!(out.contains("── 1 of 7 · What Flow Buddy does ──"));
        assert!(out.contains("Yes: all 2 checks pass."));
        assert!(out.contains("on your screen now, under the top bar"));
        assert!(out.contains("  Got it            you'll try it"), "{out}");
        assert!(out.contains("They're in flowbuddy setup whenever you like."));
        assert!(out.contains("── 7 of 7 · Quieter, or louder ──"));
        assert!(out.ends_with("Try a drill now, two minutes: flowbuddy practice\n"));
        // No line is too wide, tables included, and no marks are left.
        for line in out.lines() {
            assert!(line.chars().count() <= WIDTH, "too wide: {line}");
        }
        assert!(!out.contains("`flowbuddy"), "{out}");
    }

    #[test]
    fn the_questions_are_saved() {
        // Through part 4, yes, five answers (the second: active), then on.
        let (out, done, saved) = tour(
            b"\n\n\n\ny\n2\n\n\n\n\n\n\n",
            Ok("notification".into()),
            vec![check(false, "the GNOME extension isn't running")],
        );
        assert!(done, "{out}");
        assert!(out.contains(
            "0 of 1 checks pass. To fix:\n  ✗ the GNOME extension isn't running\n    do this"
        ));
        assert!(out.contains("in your notifications now"));
        let saved = saved.expect("saved");
        assert!(saved.contains("default_mode = \"active\""), "{saved}");
    }

    #[test]
    fn q_stops_and_no_daemon_still_shows_the_tip() {
        let (out, done, _) = tour(b"\n\n\nq\n", Err("the daemon isn't running".into()), vec![]);
        assert!(!done);
        assert!(
            out.contains("(The sample can't be shown: the daemon isn't running. It would read:)")
        );
        assert!(out.contains("4 of 7 · Where tips show"));
        assert!(!out.contains("5 of 7"));
        // Ctrl-D stops too.
        let (_, done, _) = tour(b"", Ok("card".into()), vec![]);
        assert!(!done);
    }

    #[test]
    fn the_json_has_every_part_and_real_settings() {
        let j = json();
        assert_eq!(j["parts"].as_array().unwrap().len(), PARTS.len());
        assert_eq!(j["parts"][1]["action"], "checks");
        assert_eq!(j["parts"][2]["rows"][0][0], "Got it");
        // The settings are the setup questions', and real keys.
        for (key, q) in SETTINGS.iter().zip(setup::QUESTIONS) {
            assert_eq!(*key, format!("{}.{}", q.section, q.key));
            assert!(flowbuddy_core::settings::find(key).is_some(), "{key}");
        }
    }
}
