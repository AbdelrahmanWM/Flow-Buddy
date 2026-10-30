//! `flowbuddy tour`: how Flow Buddy works, in a few short parts, for
//! someone new to it. It checks the setup, shows a sample tip where tips
//! show, and asks the setup questions; the rest is reading.

use std::io::{BufRead, Write};

use crate::doctor::Check;
use crate::setup;

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

const PARTS: usize = 7;

/// What the reader chose at the end of a part.
#[derive(PartialEq)]
enum Go {
    On,
    Stop,
}

struct Tour<'a, 'b> {
    input: &'a mut dyn BufRead,
    out: &'a mut dyn Write,
    env: &'a Env<'b>,
}

impl Tour<'_, '_> {
    fn heading(&mut self, n: usize, title: &str) -> std::io::Result<()> {
        let text = format!(" {n} of {PARTS} · {title} ");
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

    fn line(&mut self, text: &str) -> std::io::Result<()> {
        writeln!(self.out, "{text}")
    }

    /// Rows of (command, what it does), lined up.
    fn table(&mut self, rows: &[(&str, &str)]) -> std::io::Result<()> {
        let width = rows
            .iter()
            .map(|(c, _)| c.chars().count())
            .max()
            .unwrap_or(0);
        for (c, what) in rows {
            let pad = " ".repeat(width - c.chars().count());
            let c = self.cmd(c);
            writeln!(self.out, "  {c}{pad}  {what}")?;
        }
        Ok(())
    }

    /// Wait for Enter; q stops. The input ending stops too.
    fn next(&mut self) -> std::io::Result<Go> {
        write!(self.out, "\n[Enter] next · q to stop: ")?;
        self.out.flush()?;
        let mut line = String::new();
        if self.input.read_line(&mut line)? == 0 || line.trim().eq_ignore_ascii_case("q") {
            return Ok(Go::Stop);
        }
        Ok(Go::On)
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

    fn what_it_does(&mut self) -> std::io::Result<()> {
        self.heading(1, "What Flow Buddy does")?;
        self.line("Flow Buddy watches how you use Vim's keys, in Vim, VS Code with vscodevim,\nvi-mode shells and Claude Code's Vim mode. Now and then it suggests a shorter\nway to do what you just did: one tip at a time, with the keys it took and the\nkeys that would do.")?;
        self.line("\nIt learns which commands you already use, so the tips move on as you do, and\nonce a day it brings one new thing to try.")?;
        self.line("\nWhat it keeps: counts and habits, like \"pressed j seven times in a row\".\nNever what you type, file names or window titles. Nothing goes on the network\nunless you choose an AI for `flowbuddy coach`.")
    }

    fn running(&mut self) -> std::io::Result<()> {
        self.heading(2, "Is everything running?")?;
        let checks = (self.env.checks)();
        let bad: Vec<&Check> = checks.iter().filter(|c| !c.ok).collect();
        if bad.is_empty() {
            self.line(&format!(
                "Yes: all {} checks pass. Flow Buddy is watching.",
                checks.len()
            ))?;
        } else {
            self.line(&format!(
                "{} of {} checks pass. To fix:",
                checks.len() - bad.len(),
                checks.len()
            ))?;
            for c in bad {
                let fix = c.fix.as_deref().unwrap_or("");
                writeln!(self.out, "  ✗ {}\n    {fix}", c.what)?;
            }
        }
        let doctor = self.cmd("flowbuddy doctor");
        self.line(&format!("\n{doctor} runs these checks any time."))
    }

    fn a_tip(&mut self) -> std::io::Result<()> {
        self.heading(3, "A tip, and how to answer it")?;
        match (self.env.sample)() {
            Ok(w) if w == "card" => self.line(
                "A made-up tip is on your screen now, under the top bar. Real ones look the\nsame: what you did, and the shorter way.",
            )?,
            Ok(_) => self.line(
                "A made-up tip is in your notifications now. Real ones look the same: what\nyou did, and the shorter way.",
            )?,
            Err(why) => {
                self.line(&format!("(The sample can't be shown: {why}. It would read:)"))?;
                self.line("  A sample tip: That took 7 keys. } gets there in 1, or 7j in 2.")?;
            }
        }
        self.line("\nAnswer a tip from its menu (click it), or not at all:")?;
        self.table(&[
            ("Got it", "you'll try it; seeing you use it is what counts"),
            (
                "Not helpful",
                "that tip comes less often, and needs a longer run to show",
            ),
            (
                "What does it do?",
                "each command in it explained, with an example",
            ),
        ])?;
        self.line("Tips you don't answer go away by themselves. On GNOME, Got it and Not helpful\nhave keyboard shortcuts (Settings… → Advanced).")?;
        let tips = self.cmd("flowbuddy tips");
        let explain = self.cmd("flowbuddy explain 'ci\"'");
        self.line(&format!(
            "\n{tips} lists the recent ones; {explain} explains any keys."
        ))
    }

    fn where_tips_show(&mut self) -> std::io::Result<()> {
        self.heading(4, "Where tips show")?;
        self.table(&[
            (
                "In Vim",
                "a small window at the top right (:FlowBuddyGotIt, :FlowBuddyNotHelpful)",
            ),
            (
                "In VS Code",
                "the desktop card; the setting Flow Buddy: Tips can show them in VS Code",
            ),
            ("Anywhere else", "the desktop card, under the top bar"),
        ])?;
        let set = self.cmd("flowbuddy config set coaching.show_in desktop");
        self.line(&format!(
            "\nTo have every tip on the desktop: {set}\n(or editor, the default, or both)."
        ))
    }

    fn settings(&mut self) -> std::io::Result<Go> {
        self.heading(5, "The settings that matter most")?;
        self.line("Five questions set them: how often tips come, when they show, how much Vim\nyou know, an AI for coaching, and whether tips can name exact commands.")?;
        let setup = self.cmd("flowbuddy setup");
        match self.yes("\nAnswer them now?")? {
            None => return Ok(Go::Stop),
            Some(false) => {
                self.line(&format!("\nThey're in {setup} whenever you like."))?;
            }
            Some(true) => {
                writeln!(self.out, "Enter keeps the current choice (*).")?;
                match setup::ask(&self.env.config, self.input, self.out)? {
                    None => return Ok(Go::Stop),
                    Some(answers) => {
                        let text = setup::apply(&self.env.config, &answers);
                        (self.env.save)(&text)?;
                        self.line(&format!(
                            "\nSaved; the daemon picks it up by itself. {setup} asks again."
                        ))?;
                    }
                }
            }
        }
        let list = self.cmd("flowbuddy config list");
        let profile = self.cmd("flowbuddy config profile");
        self.line(&format!(
            "Every other setting: {list}, or Settings… in the top-bar menu.\nReady-made sets: {profile} (Gentle, Learning fast, Just watching…)."
        ))?;
        Ok(Go::On)
    }

    fn learning(&mut self) -> std::io::Result<()> {
        self.heading(6, "Learning, not just tips")?;
        self.line("Once a day there's a lesson: one command you don't use yet, with an example.\n\"I know this\" skips it, \"Show me another\" swaps it. Something you learned and\nstop using comes back as a review.\n")?;
        self.table(&[
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
        ])
    }

    fn quieter(&mut self) -> std::io::Result<()> {
        self.heading(7, "Quieter, or louder")?;
        self.table(&[
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
        ])?;
        let quiet = self.cmd("flowbuddy config set coaching.quiet_hours 22:00-08:00");
        self.line(&format!(
            "No tips at night: {quiet}\nPause and the mode are in the top-bar menu too."
        ))
    }
}

/// Run the tour. Returns whether it got to the end.
pub fn run(env: &Env, input: &mut dyn BufRead, out: &mut dyn Write) -> std::io::Result<bool> {
    let mut t = Tour { input, out, env };
    t.line(&format!(
        "Flow Buddy in {PARTS} short parts. Enter goes on; q stops (start again with flowbuddy tour)."
    ))?;
    t.what_it_does()?;
    if t.next()? == Go::Stop {
        return Ok(false);
    }
    t.running()?;
    if t.next()? == Go::Stop {
        return Ok(false);
    }
    t.a_tip()?;
    if t.next()? == Go::Stop {
        return Ok(false);
    }
    t.where_tips_show()?;
    if t.next()? == Go::Stop {
        return Ok(false);
    }
    if t.settings()? == Go::Stop || t.next()? == Go::Stop {
        return Ok(false);
    }
    t.learning()?;
    if t.next()? == Go::Stop {
        return Ok(false);
    }
    t.quieter()?;
    let practice = t.cmd("flowbuddy practice");
    t.line(&format!(
        "\nThat's all. Try a drill now, two minutes: {practice}"
    ))?;
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
}
