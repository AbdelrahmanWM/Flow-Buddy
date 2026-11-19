//! `flowbuddy setup`: a few questions, written to config.toml. Lines you
//! wrote yourself, and comments, are kept; only the keys asked about change.

use std::io::{BufRead, Write};

pub use flowbuddy_core::settings::set_key;

/// One question: what it is, the choices as (value, what it means), and
/// the current value's index.
pub struct Question {
    pub section: &'static str,
    pub key: &'static str,
    pub ask: &'static str,
    /// More about it, under the question.
    pub about: Option<&'static str>,
    pub choices: &'static [(&'static str, &'static str)],
}

pub const QUESTIONS: &[Question] = &[
    Question {
        section: "coaching",
        key: "default_mode",
        ask: "How often should tips come?",
        about: None,
        choices: &[
            (
                "\"balanced\"",
                "a couple of tips every half hour (recommended to start)",
            ),
            ("\"active\"", "as you work, with a cooldown per habit"),
            (
                "\"passive\"",
                "none during the day; a summary in the evening",
            ),
        ],
    },
    Question {
        section: "coaching",
        key: "when",
        ask: "When should a live tip show?",
        about: None,
        choices: &[
            ("\"now\"", "right away"),
            ("\"pause\"", "when you stop typing for a moment"),
        ],
    },
    Question {
        section: "learn",
        key: "level",
        ask: "How much Vim do you know?",
        about: None,
        choices: &[
            (
                "\"beginner\"",
                "start from the basics; Flow Buddy learns what you use",
            ),
            (
                "\"intermediate\"",
                "I use counts, w/b/e, f/t, dd, cw, . already",
            ),
            (
                "\"advanced\"",
                "text objects, paragraphs, search, marks too",
            ),
        ],
    },
    Question {
        section: "ai",
        key: "provider",
        ask: "Use an AI for `coach`, the weekly note and reviews?",
        about: None,
        choices: &[
            (
                "\"none\"",
                "no AI (the learned tip filter still works, without one)",
            ),
            (
                "\"claude-code\"",
                "Claude Code: your Claude subscription, no key",
            ),
            (
                "\"command\"",
                "a program on this machine, e.g. ollama (set [ai] command)",
            ),
            ("\"anthropic\"", "the Anthropic API, with your key"),
            ("\"openai\"", "an OpenAI-compatible API, with your key"),
        ],
    },
    Question {
        section: "text",
        key: "shapes",
        ask: "Exact tips: let the editor plugins look at the text around your edits?",
        about: Some(
            "With it, a tip can name the exact command for what you just did (d} or de, not \"use a motion\"). The Vim plugin and the VS Code companion read the text inside the editor and send only counts; the text never leaves the editor, and files that look secret (.env, keys) are never read.",
        ),
        choices: &[
            ("false", "no, tips from keys only"),
            ("true", "yes, exact tips"),
        ],
    },
];

/// The answers so far: the value chosen for each question, by index.
pub fn apply(text: &str, answers: &[(usize, usize)]) -> String {
    answers.iter().fold(text.to_string(), |t, &(q, c)| {
        let q = &QUESTIONS[q];
        set_key(&t, q.section, q.key, q.choices[c].0)
    })
}

/// The current choice for `q` in `text`, if it's one of the choices.
fn current(text: &str, q: &Question) -> Option<usize> {
    let table: toml::Table = text.parse().ok()?;
    let value = table.get(q.section)?.get(q.key)?;
    let value = toml::Value::to_string(value);
    q.choices.iter().position(|(v, _)| *v == value)
}

/// Ask each question on the terminal and return the answers; `None` when
/// the input ends first (Ctrl-D), so nothing is saved.
pub fn ask(
    text: &str,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> std::io::Result<Option<Vec<(usize, usize)>>> {
    let mut answers = Vec::new();
    for (qi, q) in QUESTIONS.iter().enumerate() {
        let now = current(text, q).unwrap_or(0);
        writeln!(out, "\n{}", q.ask)?;
        if let Some(about) = q.about {
            for line in crate::tour::wrap(about, crate::tour::WIDTH) {
                writeln!(out, "{line}")?;
            }
        }
        for (i, (_, what)) in q.choices.iter().enumerate() {
            let mark = if i == now { "*" } else { " " };
            writeln!(out, " {mark}{}. {what}", i + 1)?;
        }
        let pick = loop {
            write!(out, "Choice [{}]: ", now + 1)?;
            out.flush()?;
            let mut line = String::new();
            if input.read_line(&mut line)? == 0 {
                return Ok(None);
            }
            let line = line.trim();
            if line.is_empty() {
                break now;
            }
            match line.parse::<usize>() {
                Ok(n) if (1..=q.choices.len()).contains(&n) => break n - 1,
                _ => writeln!(out, "Type a number from 1 to {}.", q.choices.len())?,
            }
        };
        answers.push((qi, pick));
    }
    Ok(Some(answers))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_set_and_comments_kept() {
        let text = "# mine\n[coaching]\ndefault_mode = \"passive\"  # quiet at first\nquiet_hours = \"22:00-08:00\"\n\n[ai]\nfilter = true\n";
        let t = set_key(text, "coaching", "default_mode", "\"active\"");
        assert!(
            t.contains("default_mode = \"active\"  # quiet at first"),
            "{t}"
        );
        assert!(t.starts_with("# mine\n"));
        let t = set_key(&t, "ai", "provider", "\"claude-code\"");
        assert!(
            t.contains("[ai]\nfilter = true\nprovider = \"claude-code\"\n"),
            "{t}"
        );
        let t = set_key(&t, "learn", "level", "\"advanced\"");
        assert!(t.ends_with("\n\n[learn]\nlevel = \"advanced\"\n"), "{t}");
        // A key with a longer name isn't mistaken for it.
        let t = set_key("[ai]\nproviders = 1\n", "ai", "provider", "\"none\"");
        assert!(t.contains("providers = 1\nprovider = \"none\""), "{t}");
        assert!(t.parse::<toml::Table>().is_ok());
    }

    #[test]
    fn questions_and_answers() {
        let text = "[coaching]\ndefault_mode = \"active\"\n";
        // Enter keeps the current choice; a number picks another.
        let mut input: &[u8] = b"\n2\nzz\n3\n2\n2\n";
        let mut out = Vec::new();
        let answers = ask(text, &mut input, &mut out).unwrap().unwrap();
        let written = apply(text, &answers);
        let table: toml::Table = written.parse().unwrap();
        assert_eq!(table["coaching"]["default_mode"].as_str(), Some("active"));
        assert_eq!(table["coaching"]["when"].as_str(), Some("pause"));
        assert_eq!(table["learn"]["level"].as_str(), Some("advanced"));
        assert_eq!(table["ai"]["provider"].as_str(), Some("claude-code"));
        assert_eq!(table["text"]["shapes"].as_bool(), Some(true));
        // And it's a config the daemon reads.
        assert!(toml::from_str::<flowbuddy_core::config::Config>(&written).is_ok());
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains(" *2. as you work"));
        assert!(out.contains("Type a number from 1 to"));
        assert!(out.lines().all(|l| l.chars().count() <= 79), "{out}");
        assert!(out.contains("the text never leaves the editor"));
        assert!(
            out.contains(" *1. no, tips from keys only"),
            "off unless you say"
        );
    }

    #[test]
    fn ctrl_d_saves_nothing() {
        let mut input: &[u8] = b"2\n";
        assert_eq!(ask("", &mut input, &mut Vec::new()).unwrap(), None);
    }
}
