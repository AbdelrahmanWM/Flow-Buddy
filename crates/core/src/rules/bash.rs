//! Bash vi-mode rule pack.

use crate::context::{ContextKind, VimMode};
use crate::key::{Key, Sym};
use crate::rule::Rule;
use crate::rules::repeat::{RepeatRule, RepeatSpec};
use crate::vim::{Command, InsertKey, Step, VimEvent};

const BASH: &[ContextKind] = &[ContextKind::BashVi];

fn insert_key(step: &Step) -> Option<InsertKey> {
    match step.event {
        Some(VimEvent::Insert(k)) if step.before == VimMode::Insert => Some(k),
        _ => None,
    }
}

fn command(step: &Step) -> Option<&Command> {
    match &step.event {
        Some(VimEvent::Command(c))
            if matches!(step.before, VimMode::Normal | VimMode::OpPending) =>
        {
            Some(c)
        }
        _ => None,
    }
}

/// An arrow (in either mode) or its vi key (in command mode).
fn moves(step: &Step, arrow: Sym, vi: char) -> bool {
    insert_key(step) == Some(InsertKey::Arrow(arrow))
        || command(step).is_some_and(|c| {
            c.count.is_none() && (c.single() == Some(Key::sym(arrow)) || c.is(&vi.to_string()))
        })
}

pub fn up_history() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "bash.up-history",
        description: "Pressing Up many times to find an old command",
        title: "Search your history",
        contexts: BASH,
        threshold: 4,
        unit: |s| moves(s, Sym::Up, 'k').then_some("up"),
        adopted: |s| {
            insert_key(s) == Some(InsertKey::CtrlR)
                || command(s).is_some_and(|c| matches!(c.head(), Some('/' | '?')))
        },
        message: |_, n| {
            format!(
                "{n} presses of Up to find a command. Try `Ctrl-r` and type part of it (or `Esc` then `/`)."
            )
        },
    })
}

pub fn backspace_clear() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "bash.backspace-clear",
        description: "Holding Backspace to clear the command line",
        title: "Clear the line at once",
        contexts: BASH,
        threshold: 10,
        unit: |s| (insert_key(s) == Some(InsertKey::Backspace)).then_some("BS"),
        adopted: |s| {
            matches!(insert_key(s), Some(InsertKey::CtrlU | InsertKey::CtrlW))
                || command(s).is_some_and(|c| {
                    ["cc", "S", "dd", "d0", "c0", "db", "cb"]
                        .iter()
                        .any(|x| c.is(x))
                })
        },
        message: |_, n| {
            format!(
                "{n} Backspaces. `Ctrl-u` clears to the start of the line, `Ctrl-w` deletes a word."
            )
        },
    })
}

pub fn left_streak() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "bash.left-streak",
        description: "Pressing Left many times to reach an earlier word",
        title: "Jump by word",
        contexts: BASH,
        threshold: 6,
        unit: |s| moves(s, Sym::Left, 'h').then_some("left"),
        adopted: |s| {
            command(s).is_some_and(|c| matches!(c.head(), Some('b' | 'B' | 'F' | 'T' | '0' | '^')))
        },
        message: |_, n| {
            format!(
                "{n} presses of Left. Press `Esc`, then `b` to jump back a word or `F<char>` to a character."
            )
        },
    })
}

pub fn all() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(up_history()),
        Box::new(backspace_clear()),
        Box::new(left_streak()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::RuleEvent;
    use crate::rules::testing::{adoptions, findings, run};
    use crate::vim::Flavor;

    fn bash(rule: impl Rule + 'static, keys: &str) -> Vec<RuleEvent> {
        run(Box::new(rule), Flavor::Readline, VimMode::Insert, keys)
    }

    #[test]
    fn up_arrow_history_hunting() {
        assert_eq!(
            findings(&bash(up_history(), "<Up><Up><Up><Up><Up><CR>")).len(),
            1
        );
        assert_eq!(findings(&bash(up_history(), "<Esc>kkkk<CR>")).len(), 1);
        assert!(findings(&bash(up_history(), "<Up><Up><CR>")).is_empty());
        assert_eq!(adoptions(&bash(up_history(), "<C-r>git<CR>")), 1);
    }

    #[test]
    fn backspace_to_clear() {
        let bs = "<BS>".repeat(12);
        assert_eq!(
            findings(&bash(backspace_clear(), &format!("{bs}<CR>"))).len(),
            1
        );
        assert!(findings(&bash(backspace_clear(), "<BS><BS><BS><CR>")).is_empty());
        assert_eq!(adoptions(&bash(backspace_clear(), "<C-u><Esc>cc")), 2);
    }

    #[test]
    fn left_arrow_streak() {
        let left = "<Left>".repeat(7);
        assert_eq!(
            findings(&bash(left_streak(), &format!("{left}<CR>"))).len(),
            1
        );
        assert!(findings(&bash(left_streak(), "<Left><Left><CR>")).is_empty());
        assert_eq!(adoptions(&bash(left_streak(), "<Esc>bFx")), 2);
    }
}
