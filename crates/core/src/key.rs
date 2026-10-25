//! Normalized keys, independent of evdev keycodes and keyboard layout.

use std::fmt;

/// What a key produces, after layout and Shift have been applied.
///
/// `Char('}')` is what you get from Shift+]; `Char('P')` from Shift+p.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sym {
    Char(char),
    Esc,
    Enter,
    Tab,
    Backspace,
    Delete,
    Insert,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
    Other,
}

/// Modifiers held while the key was pressed.
///
/// `shift` is only set for non-character keys (e.g. Shift-Tab); for
/// characters it is already folded into the [`Sym::Char`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        ctrl: false,
        alt: false,
        shift: false,
        logo: false,
    };
    pub const CTRL: Mods = Mods {
        ctrl: true,
        ..Mods::NONE
    };

    pub fn is_empty(self) -> bool {
        self == Mods::NONE
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub sym: Sym,
    pub mods: Mods,
}

impl Key {
    pub const ESC: Key = Key::sym(Sym::Esc);
    pub const ENTER: Key = Key::sym(Sym::Enter);
    pub const BACKSPACE: Key = Key::sym(Sym::Backspace);
    pub const UP: Key = Key::sym(Sym::Up);
    pub const DOWN: Key = Key::sym(Sym::Down);
    pub const LEFT: Key = Key::sym(Sym::Left);
    pub const RIGHT: Key = Key::sym(Sym::Right);

    pub const fn sym(sym: Sym) -> Key {
        Key {
            sym,
            mods: Mods::NONE,
        }
    }

    /// A plain character key, e.g. `Key::ch('j')`.
    pub const fn ch(c: char) -> Key {
        Key::sym(Sym::Char(c))
    }

    /// A Ctrl-chord, e.g. `Key::ctrl('d')`.
    pub const fn ctrl(c: char) -> Key {
        Key {
            sym: Sym::Char(c),
            mods: Mods::CTRL,
        }
    }

    /// The character, if this is an unmodified character key.
    pub fn plain_char(&self) -> Option<char> {
        match self.sym {
            Sym::Char(c) if self.mods.is_empty() => Some(c),
            _ => None,
        }
    }

    pub fn is_ctrl(&self, c: char) -> bool {
        self.sym == Sym::Char(c) && self.mods == Mods::CTRL
    }

    pub fn is_arrow(&self) -> bool {
        matches!(self.sym, Sym::Up | Sym::Down | Sym::Left | Sym::Right) && self.mods.is_empty()
    }

    /// Esc, or one of its Vim equivalents (Ctrl-[ and Ctrl-c).
    pub fn is_escape(&self) -> bool {
        *self == Key::ESC || self.is_ctrl('[') || self.is_ctrl('c')
    }
}

/// Vim-style notation: `j`, `<Esc>`, `<C-d>`, `<Up>`.
impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.sym {
            Sym::Char(c) if self.mods.is_empty() => return write!(f, "{c}"),
            Sym::Char(' ') => "Space".to_string(),
            Sym::Char(c) => c.to_string(),
            Sym::Esc => "Esc".into(),
            Sym::Enter => "CR".into(),
            Sym::Tab => "Tab".into(),
            Sym::Backspace => "BS".into(),
            Sym::Delete => "Del".into(),
            Sym::Insert => "Insert".into(),
            Sym::Up => "Up".into(),
            Sym::Down => "Down".into(),
            Sym::Left => "Left".into(),
            Sym::Right => "Right".into(),
            Sym::Home => "Home".into(),
            Sym::End => "End".into(),
            Sym::PageUp => "PageUp".into(),
            Sym::PageDown => "PageDown".into(),
            Sym::F(n) => format!("F{n}"),
            Sym::Other => "?".into(),
        };
        let m = self.mods;
        let prefix = format!(
            "{}{}{}{}",
            if m.ctrl { "C-" } else { "" },
            if m.alt { "M-" } else { "" },
            if m.shift { "S-" } else { "" },
            if m.logo { "D-" } else { "" },
        );
        write!(f, "<{prefix}{name}>")
    }
}

/// One key press (or auto-repeat) with a timestamp in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyStroke {
    pub key: Key,
    pub repeat: bool,
    pub at_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_uses_vim_notation() {
        assert_eq!(Key::ch('j').to_string(), "j");
        assert_eq!(Key::ctrl('d').to_string(), "<C-d>");
        assert_eq!(Key::ESC.to_string(), "<Esc>");
        assert_eq!(Key::UP.to_string(), "<Up>");
    }

    #[test]
    fn escape_equivalents() {
        assert!(Key::ESC.is_escape());
        assert!(Key::ctrl('[').is_escape());
        assert!(Key::ctrl('c').is_escape());
        assert!(!Key::ch('c').is_escape());
    }
}
