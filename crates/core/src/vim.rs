//! Infer the Vim (or readline vi) mode from keys alone.
//!
//! The tracker turns each key into a [`Step`]: the mode before and after,
//! whether we are confident about it, and what the key meant (a complete
//! Normal-mode command, an Insert-mode key, a finished command line).
//! Rules consume steps, never raw keys.
//!
//! Confidence: we can't see what the editor sees. Esc always brings Vim to
//! Normal mode, so Esc makes us confident; anything we can't observe (mouse
//! clicks, focus changes, the command palette) makes us unconfident until
//! the next Esc. Rules only fire while confident.

use crate::context::VimMode;
use crate::key::{Key, KeyStroke, Sym};

/// Which program's vi keys we are tracking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    /// Vim, Neovim or vscodevim.
    Vim,
    /// Bash/readline `set -o vi`: Insert by default, a reduced command mode.
    Readline,
}

/// A complete Normal/Visual-mode command such as `j`, `5j`, `dd` or `ciw`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// Leading count, e.g. 5 in `5j`.
    pub count: Option<u32>,
    /// The keys after the count and register, e.g. `[d, d]`.
    pub keys: Vec<Key>,
    /// The register named with `"`, e.g. `0` in `"0p`.
    pub register: Option<char>,
}

impl Command {
    /// Whether the keys (ignoring count) spell `s`, e.g. `cmd.is("dd")`.
    pub fn is(&self, s: &str) -> bool {
        self.keys.len() == s.chars().count()
            && self
                .keys
                .iter()
                .zip(s.chars())
                .all(|(k, c)| k.plain_char() == Some(c))
    }

    pub fn first(&self) -> Option<Key> {
        self.keys.first().copied()
    }

    /// The first key as a plain character.
    pub fn head(&self) -> Option<char> {
        self.first().and_then(|k| k.plain_char())
    }

    pub fn single(&self) -> Option<Key> {
        (self.keys.len() == 1).then(|| self.keys[0])
    }
}

/// What an Insert-mode key was, without keeping typed text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertKey {
    /// A printable character. Which one is deliberately not kept.
    Text,
    Backspace,
    /// Ctrl-w: delete word before the cursor.
    CtrlW,
    /// Ctrl-u: delete to start of line.
    CtrlU,
    /// Ctrl-r: register insert in Vim, reverse history search in bash.
    CtrlR,
    /// Ctrl-o: run one Normal-mode command (Vim).
    CtrlO,
    /// Another Ctrl key, by its letter: `Ctrl-t` indents, `Ctrl-n`
    /// completes.
    Ctrl(char),
    /// A two-key command: `Ctrl-x` then `f` (file names), `Ctrl-g` then
    /// `u`, `Ctrl-r` then `=`. The second key's letter, never text.
    Then(char, char),
    Arrow(Sym),
    Enter,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VimEvent {
    Command(Command),
    Insert(InsertKey),
    /// A finished `:` command or `/`/`?` search. `text` is `None` when it
    /// was edited with keys we can't follow (history, completion).
    Cmdline {
        kind: char,
        text: Option<String>,
    },
    /// An editing key on the command line: `Ctrl(…)` for Ctrl keys (Tab is
    /// `Ctrl('i')`), `Then('r', …)` for `Ctrl-r Ctrl-w`, `Ctrl-r Ctrl-a`,
    /// `Ctrl-r =` or a register (`"`), arrows for history.
    CmdlineKey(InsertKey),
    /// Esc (or an equivalent) was pressed.
    Escape,
    /// A key you mapped to something else: it isn't the built-in command.
    Mapped,
}

/// Your key mappings, as far as they matter for following the mode.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mappings {
    /// Two keys typed quickly in Insert mode that act as Esc (`jk`).
    pub insert_escapes: Vec<[char; 2]>,
    /// Normal-mode keys mapped to something else.
    pub normal: Vec<char>,
}

impl Mappings {
    /// From key names as editors and the config give them; anything that
    /// isn't two plain characters (or one, for Normal mode) is ignored.
    pub fn from_names(insert_escapes: &[String], normal: &[String]) -> Mappings {
        let chars = |s: &String| s.chars().collect::<Vec<_>>();
        Mappings {
            insert_escapes: insert_escapes
                .iter()
                .take(16)
                .filter_map(|s| match chars(s)[..] {
                    [a, b] if !a.is_control() && !b.is_control() => Some([a, b]),
                    _ => None,
                })
                .collect(),
            normal: normal
                .iter()
                .take(64)
                .filter_map(|s| match chars(s)[..] {
                    [c] if !c.is_control() => Some(c),
                    _ => None,
                })
                .collect(),
        }
    }
}

/// Two keys of an Insert-mode mapping must come within this long (Vim's
/// default 'timeoutlen').
const MAPPING_TIMEOUT_MS: u64 = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub before: VimMode,
    pub after: VimMode,
    /// Whether `before` is known to be right.
    pub confident: bool,
    pub event: Option<VimEvent>,
    pub repeat: bool,
    pub at_ms: u64,
}

enum Parse {
    Incomplete { op_pending: bool },
    Complete { enters: Option<VimMode> },
}

const MAX_PENDING: usize = 12;
const MAX_CMDLINE: usize = 64;

pub struct Tracker {
    flavor: Flavor,
    mode: VimMode,
    confident: bool,
    pending: Vec<Key>,
    cmdline: String,
    cmdline_kind: char,
    cmdline_known: bool,
    cmdline_return: VimMode,
    /// Insert-mode Ctrl-o: run one Normal command, then back to Insert.
    insert_once: bool,
    /// Ctrl-r in Insert mode: the next key is a register name (Vim) or
    /// search text follows until Enter/Esc (readline).
    after_ctrl_r: bool,
    /// After Insert-mode `Ctrl-x`/`Ctrl-g` (the next key says what), or
    /// `Ctrl-v`/`Ctrl-k` (the next keys are text, not kept): the prefix and
    /// how many keys it takes.
    insert_prefix: Option<(char, u8)>,
    /// After `Ctrl-r` (or `Ctrl-v`/`Ctrl-k`, `'v'`/`'k'`) on the command line.
    cmdline_prefix: Option<(char, u8)>,
    mappings: Mappings,
    /// The last character typed in Insert mode, for two-key mappings.
    last_text: Option<(char, u64)>,
    /// The time of the key being handled.
    now_ms: u64,
}

impl Tracker {
    pub fn new(flavor: Flavor) -> Self {
        Tracker {
            flavor,
            mode: match flavor {
                Flavor::Vim => VimMode::Normal,
                Flavor::Readline => VimMode::Insert,
            },
            confident: false,
            pending: Vec::new(),
            cmdline: String::new(),
            cmdline_kind: ':',
            cmdline_known: true,
            cmdline_return: VimMode::Normal,
            insert_once: false,
            after_ctrl_r: false,
            insert_prefix: None,
            cmdline_prefix: None,
            mappings: Mappings::default(),
            last_text: None,
            now_ms: 0,
        }
    }

    pub fn set_mappings(&mut self, mappings: Mappings) {
        self.mappings = mappings;
    }

    pub fn mode(&self) -> VimMode {
        self.mode
    }

    pub fn confident(&self) -> bool {
        self.confident
    }

    /// We know the mode for sure, e.g. vim just started (Normal) or the
    /// bash prompt was just drawn (Insert).
    pub fn reset_known(&mut self, mode: VimMode) {
        self.clear_pending();
        self.mode = mode;
        self.confident = true;
    }

    /// Something happened that we couldn't see; distrust the mode until Esc.
    pub fn invalidate(&mut self) {
        self.clear_pending();
        self.confident = false;
    }

    fn clear_pending(&mut self) {
        self.pending.clear();
        self.cmdline.clear();
        self.insert_once = false;
        self.after_ctrl_r = false;
        self.insert_prefix = None;
        self.cmdline_prefix = None;
    }

    pub fn feed(&mut self, stroke: KeyStroke) -> Step {
        self.now_ms = stroke.at_ms;
        let before = self.mode;
        let confident = self.confident;
        let event = self.advance(stroke.key);
        Step {
            before,
            after: self.mode,
            confident,
            event,
            repeat: stroke.repeat,
            at_ms: stroke.at_ms,
        }
    }

    fn advance(&mut self, key: Key) -> Option<VimEvent> {
        if key.is_escape() {
            return self.escape(key);
        }
        match self.mode {
            VimMode::Insert => self.insert_key(key),
            VimMode::Cmdline => self.cmdline_key(key),
            VimMode::Normal | VimMode::OpPending => self.normal_key(key),
            VimMode::Visual | VimMode::VisualLine | VimMode::VisualBlock => self.visual_key(key),
        }
    }

    fn escape(&mut self, key: Key) -> Option<VimEvent> {
        // In bash, Ctrl-c cancels the line rather than leaving Insert mode.
        if self.flavor == Flavor::Readline && key.is_ctrl('c') {
            self.reset_known(VimMode::Insert);
            return None;
        }
        self.clear_pending();
        self.mode = VimMode::Normal;
        self.confident = true;
        Some(VimEvent::Escape)
    }

    fn insert_key(&mut self, key: Key) -> Option<VimEvent> {
        if let Some((prefix, left)) = self.insert_prefix.take() {
            if left > 1 {
                self.insert_prefix = Some((prefix, left - 1));
            }
            return Some(VimEvent::Insert(match (prefix, key.sym) {
                // Literal keys and digraphs are text: not kept.
                ('v' | 'k', _) => InsertKey::Other,
                // `Ctrl-\ Ctrl-n`: to Normal mode (out of a terminal, too).
                ('\\', Sym::Char('n' | 'N')) => {
                    self.mode = VimMode::Normal;
                    InsertKey::Then('\\', 'n')
                }
                ('g', Sym::Down) => InsertKey::Then('g', 'j'),
                ('g', Sym::Up) => InsertKey::Then('g', 'k'),
                // Only the keys that mean something after the prefix: any
                // other is text, and isn't kept.
                (_, Sym::Char(c)) => {
                    let c = c.to_ascii_lowercase();
                    let known = match prefix {
                        'x' => "defiklnoprstuvyz]",
                        'g' => "ujk",
                        _ => "",
                    };
                    if known.contains(c) {
                        InsertKey::Then(prefix, c)
                    } else {
                        InsertKey::Other
                    }
                }
                _ => InsertKey::Other,
            }));
        }
        if self.after_ctrl_r {
            match self.flavor {
                Flavor::Vim => {
                    self.after_ctrl_r = false;
                    // `Ctrl-r =`: work something out and insert it.
                    if key.sym == Sym::Char('=') {
                        return Some(VimEvent::Insert(InsertKey::Then('r', '=')));
                    }
                }
                Flavor::Readline => {
                    if key == Key::ENTER {
                        self.after_ctrl_r = false;
                    }
                    return None;
                }
            }
            return Some(VimEvent::Insert(InsertKey::Other));
        }
        let ik = if key.is_ctrl('w') {
            InsertKey::CtrlW
        } else if key.is_ctrl('u') {
            InsertKey::CtrlU
        } else if key.is_ctrl('r') {
            self.after_ctrl_r = true;
            InsertKey::CtrlR
        } else if key.is_ctrl('o') && self.flavor == Flavor::Vim {
            self.mode = VimMode::Normal;
            self.insert_once = true;
            InsertKey::CtrlO
        } else if key.is_arrow() {
            InsertKey::Arrow(key.sym)
        } else if key.mods.ctrl
            && self.flavor == Flavor::Vim
            && let Sym::Char(c) = key.sym
            && (c.is_ascii_alphabetic() || matches!(c, '@' | ']' | '\\'))
        {
            let c = c.to_ascii_lowercase();
            self.insert_prefix = match c {
                'x' | 'g' | 'v' | 'q' | '\\' => Some((if c == 'q' { 'v' } else { c }, 1)),
                'k' => Some(('k', 2)),
                _ => None,
            };
            InsertKey::Ctrl(c)
        } else {
            match key.sym {
                Sym::Backspace if key.mods.is_empty() => InsertKey::Backspace,
                Sym::Enter => InsertKey::Enter,
                Sym::Char(c) if !key.mods.ctrl && !key.mods.alt && !key.mods.logo => {
                    // A two-key mapping to Esc, such as `jk`.
                    let previous = self.last_text.take();
                    if let Some((prev, at)) = previous
                        && self.now_ms.saturating_sub(at) <= MAPPING_TIMEOUT_MS
                        && self.mappings.insert_escapes.contains(&[prev, c])
                    {
                        self.clear_pending();
                        self.mode = VimMode::Normal;
                        self.confident = true;
                        return Some(VimEvent::Escape);
                    }
                    self.last_text = Some((c, self.now_ms));
                    InsertKey::Text
                }
                _ => InsertKey::Other,
            }
        };
        if ik != InsertKey::Text {
            self.last_text = None;
        }
        Some(VimEvent::Insert(ik))
    }

    fn cmdline_key(&mut self, key: Key) -> Option<VimEvent> {
        if let Some((prefix, left)) = self.cmdline_prefix.take() {
            if left > 1 {
                self.cmdline_prefix = Some((prefix, left - 1));
            }
            self.cmdline_known = false;
            return match (prefix, key.sym) {
                ('r', Sym::Char(c))
                    if key.mods.ctrl && matches!(c, 'w' | 'a' | 'l' | 'f' | 'p') =>
                {
                    Some(VimEvent::CmdlineKey(InsertKey::Then('r', c)))
                }
                ('r', Sym::Char('=')) => Some(VimEvent::CmdlineKey(InsertKey::Then('r', '='))),
                // A register's contents: which register isn't kept.
                ('r', _) => Some(VimEvent::CmdlineKey(InsertKey::Then('r', '"'))),
                // What follows Ctrl-v or Ctrl-k is text.
                _ => None,
            };
        }
        let edit = match key.sym {
            Sym::Char(c) if key.mods.ctrl && (c.is_ascii_alphabetic() || c == ']') => {
                let c = c.to_ascii_lowercase();
                self.cmdline_prefix = match c {
                    'r' => Some(('r', 1)),
                    'v' | 'q' => Some(('v', 1)),
                    'k' => Some(('k', 2)),
                    _ => None,
                };
                Some(InsertKey::Ctrl(c))
            }
            Sym::Tab => Some(InsertKey::Ctrl('i')),
            Sym::Up | Sym::Down | Sym::PageUp | Sym::PageDown => {
                Some(InsertKey::Arrow(match key.sym {
                    Sym::PageUp => Sym::Up,
                    Sym::PageDown => Sym::Down,
                    s => s,
                }))
            }
            _ => None,
        };
        if let Some(k) = edit {
            // Ctrl-w and Ctrl-u edit text we can still follow? Not
            // exactly: whatever the key, the text is no longer known.
            self.cmdline_known = false;
            return Some(VimEvent::CmdlineKey(k));
        }
        match key.sym {
            Sym::Enter => {
                let text = self
                    .cmdline_known
                    .then(|| std::mem::take(&mut self.cmdline));
                self.cmdline.clear();
                self.mode = self.cmdline_return;
                Some(VimEvent::Cmdline {
                    kind: self.cmdline_kind,
                    text,
                })
            }
            Sym::Backspace => {
                if self.cmdline.pop().is_none() {
                    self.mode = self.cmdline_return.min_normal();
                }
                None
            }
            Sym::Char(c) if key.mods.is_empty() => {
                if self.cmdline.len() < MAX_CMDLINE {
                    self.cmdline.push(c);
                } else {
                    self.cmdline_known = false;
                }
                None
            }
            _ => {
                // History, completion, Ctrl-r etc.: text no longer known.
                self.cmdline_known = false;
                None
            }
        }
    }

    fn enter_cmdline(&mut self, kind: char, ret: VimMode) {
        self.mode = VimMode::Cmdline;
        self.cmdline.clear();
        self.cmdline_kind = kind;
        self.cmdline_known = true;
        self.cmdline_return = ret;
    }

    fn take_command(&mut self) -> Command {
        let keys = std::mem::take(&mut self.pending);
        let (count, rest) = split_count(&keys);
        let prefix = &keys[..keys.len() - rest.len()];
        let register = prefix
            .windows(2)
            .find(|w| w[0].plain_char() == Some('"'))
            .and_then(|w| w[1].plain_char());
        Command {
            count,
            keys: rest.to_vec(),
            register,
        }
    }

    fn normal_key(&mut self, key: Key) -> Option<VimEvent> {
        // A key you mapped: not the built-in command it looks like.
        if self.pending.is_empty()
            && key
                .plain_char()
                .is_some_and(|c| self.mappings.normal.contains(&c))
        {
            self.mode = VimMode::Normal;
            return Some(VimEvent::Mapped);
        }
        self.pending.push(key);
        match parse_normal(&self.pending) {
            Parse::Incomplete { op_pending } => {
                self.mode = if op_pending {
                    VimMode::OpPending
                } else {
                    VimMode::Normal
                };
                if self.pending.len() > MAX_PENDING {
                    self.invalidate();
                    self.mode = VimMode::Normal;
                }
                None
            }
            Parse::Complete { enters } => {
                let cmd = self.take_command();
                let once = std::mem::take(&mut self.insert_once);
                let mut next = enters.unwrap_or(VimMode::Normal);
                if self.flavor == Flavor::Readline && next.is_visual() {
                    // `v` opens $EDITOR from bash; we can't follow that.
                    self.invalidate();
                    next = VimMode::Normal;
                }
                if once && enters.is_none() {
                    next = VimMode::Insert;
                }
                if next == VimMode::Cmdline {
                    let kind = cmd.keys.last().and_then(|k| k.plain_char()).unwrap_or(':');
                    let ret = if once || cmd.head() == Some('c') {
                        VimMode::Insert
                    } else {
                        VimMode::Normal
                    };
                    self.enter_cmdline(kind, ret);
                } else {
                    self.mode = next;
                }
                Some(VimEvent::Command(cmd))
            }
        }
    }

    fn visual_key(&mut self, key: Key) -> Option<VimEvent> {
        self.pending.push(key);
        match parse_visual(&self.pending, self.mode) {
            None => {
                if self.pending.len() > MAX_PENDING {
                    self.invalidate();
                }
                None
            }
            Some(next) => {
                let cmd = self.take_command();
                if next == VimMode::Cmdline {
                    let kind = cmd.keys.last().and_then(|k| k.plain_char()).unwrap_or(':');
                    let ret = if kind == ':' {
                        VimMode::Normal
                    } else {
                        self.mode
                    };
                    self.enter_cmdline(kind, ret);
                } else {
                    self.mode = next;
                }
                Some(VimEvent::Command(cmd))
            }
        }
    }
}

impl VimMode {
    /// Where Backspace on an empty command line goes: never stay in Cmdline.
    fn min_normal(self) -> VimMode {
        if self == VimMode::Cmdline {
            VimMode::Normal
        } else {
            self
        }
    }
}

fn digit(key: &Key, first: bool) -> Option<u32> {
    match key.plain_char()? {
        '0' if first => None,
        c => c.to_digit(10),
    }
}

/// Split a leading count off, e.g. `5j` → (Some(5), [j]).
fn split_count(keys: &[Key]) -> (Option<u32>, &[Key]) {
    let mut count: Option<u32> = None;
    let mut i = 0;
    while i < keys.len() {
        // Register prefix `"x` can appear before or after the count.
        if keys[i].plain_char() == Some('"') && i + 1 < keys.len() {
            i += 2;
            continue;
        }
        match digit(&keys[i], count.is_none()) {
            Some(d) => {
                count = Some(count.unwrap_or(0).saturating_mul(10).saturating_add(d));
                i += 1;
            }
            None => break,
        }
    }
    (count, &keys[i..])
}

fn needs_char(c: char) -> bool {
    matches!(
        c,
        'f' | 't' | 'F' | 'T' | 'r' | 'm' | '\'' | '`' | 'q' | '@'
    )
}

fn parse_normal(keys: &[Key]) -> Parse {
    let (_, rest) = split_count(keys);
    let Some((&k, rest)) = rest.split_first() else {
        return Parse::Incomplete { op_pending: false };
    };
    let complete = |enters| Parse::Complete { enters };
    let need_one = |rest: &[Key], enters| {
        if rest.is_empty() {
            Parse::Incomplete { op_pending: false }
        } else {
            Parse::Complete { enters }
        }
    };
    if k.is_ctrl('v') {
        return complete(Some(VimMode::VisualBlock));
    }
    // Window commands: Ctrl-w and one more key, or two after `g`
    // (`Ctrl-w gf`).
    if k.is_ctrl('w') {
        if rest.first().and_then(|k| k.plain_char()) == Some('g') {
            return need_one(&rest[1..], None);
        }
        return need_one(rest, None);
    }
    let Some(c) = k.plain_char() else {
        return complete(None);
    };
    match c {
        '"' => Parse::Incomplete { op_pending: false },
        c if needs_char(c) => need_one(rest, None),
        'g' => match rest.first().map(|k| k.plain_char()) {
            None => Parse::Incomplete { op_pending: false },
            Some(Some('~' | 'u' | 'U' | '?' | 'q' | 'w')) => operator('g', &rest[1..]),
            // vim-commentary: `gcc` a line, `gc` + a motion.
            Some(Some('c')) if rest.get(1).and_then(|k| k.plain_char()) == Some('c') => {
                complete(None)
            }
            Some(Some('c')) => operator('g', &rest[1..]),
            Some(Some('i' | 'I')) => complete(Some(VimMode::Insert)),
            Some(Some('v')) => complete(Some(VimMode::Visual)),
            Some(_) => complete(None),
        },
        'z' | 'Z' | '[' | ']' => need_one(rest, None),
        // vim-surround: none of these is a built-in command, so it's safe
        // to read them as surround even without the plugin.
        'y' if rest.first().and_then(|k| k.plain_char()) == Some('s') => surround_add(&rest[1..]),
        'c' if rest.first().and_then(|k| k.plain_char()) == Some('s') => need_n(rest, 3),
        'd' if rest.first().and_then(|k| k.plain_char()) == Some('s') => need_n(rest, 2),
        'd' | 'c' | 'y' | '<' | '>' | '=' | '!' => operator(c, rest),
        'i' | 'a' | 'I' | 'A' | 'o' | 'O' | 's' | 'S' | 'C' | 'R' => {
            complete(Some(VimMode::Insert))
        }
        'v' => complete(Some(VimMode::Visual)),
        'V' => complete(Some(VimMode::VisualLine)),
        ':' | '/' | '?' => complete(Some(VimMode::Cmdline)),
        _ => complete(None),
    }
}

/// `n` keys after the command's first (`cs` plus two characters).
fn need_n(rest: &[Key], n: usize) -> Parse {
    if rest.len() >= n {
        Parse::Complete { enters: None }
    } else {
        Parse::Incomplete { op_pending: true }
    }
}

/// vim-surround's `ys`: a motion or text object, then the character to
/// surround it with (`ysiw)`); `yss` surrounds the line.
fn surround_add(rest: &[Key]) -> Parse {
    if rest.first().and_then(|k| k.plain_char()) == Some('s') {
        return need_n(rest, 2);
    }
    for k in 1..rest.len() {
        if matches!(operator('y', &rest[..k]), Parse::Complete { .. }) {
            return need_n(&rest[k..], 1);
        }
    }
    Parse::Incomplete { op_pending: true }
}

/// After an operator such as `d`: wait for a motion or text object.
fn operator(op: char, rest: &[Key]) -> Parse {
    let pending = Parse::Incomplete { op_pending: true };
    let (_, rest) = split_count(rest);
    // Forced motions (dv, dV) just modify the motion that follows.
    let rest = match rest.first().and_then(|k| k.plain_char()) {
        Some('v' | 'V') => &rest[1..],
        _ => rest,
    };
    let Some((&m, after)) = rest.split_first() else {
        return pending;
    };
    let done = Parse::Complete {
        enters: (op == 'c').then_some(VimMode::Insert),
    };
    match m.plain_char() {
        Some('i' | 'a' | 'f' | 't' | 'F' | 'T' | '\'' | '`' | 'g') if after.is_empty() => pending,
        Some('/' | '?') => Parse::Complete {
            enters: Some(VimMode::Cmdline),
        },
        _ => done,
    }
}

/// Parse a Visual-mode command. `None` while incomplete, otherwise the
/// mode it leads to.
fn parse_visual(keys: &[Key], current: VimMode) -> Option<VimMode> {
    let (_, rest) = split_count(keys);
    let (&k, rest) = rest.split_first()?;
    if k.is_ctrl('v') {
        return Some(toggle(current, VimMode::VisualBlock));
    }
    let Some(c) = k.plain_char() else {
        return Some(current);
    };
    let need_one = |then: VimMode| (!rest.is_empty()).then_some(then);
    match c {
        '"' => None,
        'v' => Some(toggle(current, VimMode::Visual)),
        'V' => Some(toggle(current, VimMode::VisualLine)),
        'd' | 'x' | 'X' | 'y' | 'Y' | 'D' | 'J' | 'u' | 'U' | '~' | '>' | '<' | '=' | '!' | 'p'
        | 'P' => Some(VimMode::Normal),
        'c' | 's' | 'C' | 'S' | 'R' | 'I' | 'A' => Some(VimMode::Insert),
        'r' => need_one(VimMode::Normal),
        'i' | 'a' | 'f' | 't' | 'F' | 'T' | '\'' | '`' => need_one(current),
        'g' => match rest.first().map(|k| k.plain_char()) {
            None => None,
            Some(Some('u' | 'U' | '~' | '?' | 'J' | 'q' | 'w')) => Some(VimMode::Normal),
            Some(_) => Some(current),
        },
        ':' | '/' | '?' => Some(VimMode::Cmdline),
        _ => Some(current),
    }
}

fn toggle(current: VimMode, target: VimMode) -> VimMode {
    if current == target {
        VimMode::Normal
    } else {
        target
    }
}

/// Helpers for tests across the crate: build key strokes from Vim notation.
pub mod notation {
    use crate::key::{Key, KeyStroke, Sym};

    /// Parse `"5j<Esc>dd<C-d><Up>"` into keys.
    pub fn keys(s: &str) -> Vec<Key> {
        let mut out = Vec::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '<' || chars.peek().is_none() {
                out.push(Key::ch(c));
                continue;
            }
            let name: String = chars.by_ref().take_while(|&c| c != '>').collect();
            out.push(match name.as_str() {
                "Esc" => Key::ESC,
                "CR" => Key::ENTER,
                "BS" => Key::BACKSPACE,
                "Up" => Key::UP,
                "Down" => Key::DOWN,
                "Left" => Key::LEFT,
                "Right" => Key::RIGHT,
                "Tab" => Key::sym(Sym::Tab),
                "lt" => Key::ch('<'),
                n if n.starts_with("C-") => Key::ctrl(n.chars().nth(2).unwrap()),
                n => panic!("unknown key <{n}>"),
            });
        }
        out
    }

    /// Strokes 100 ms apart, starting at `t0`.
    pub fn strokes(s: &str, t0: u64) -> Vec<KeyStroke> {
        keys(s)
            .into_iter()
            .enumerate()
            .map(|(i, key)| KeyStroke {
                key,
                repeat: false,
                at_ms: t0 + 100 * i as u64,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::notation::strokes;
    use super::*;

    fn run(t: &mut Tracker, s: &str) -> Vec<Step> {
        strokes(s, 0).into_iter().map(|k| t.feed(k)).collect()
    }

    fn vim() -> Tracker {
        let mut t = Tracker::new(Flavor::Vim);
        t.reset_known(VimMode::Normal);
        t
    }

    fn commands(steps: &[Step]) -> Vec<String> {
        steps
            .iter()
            .filter_map(|s| match &s.event {
                Some(VimEvent::Command(c)) => Some(format!(
                    "{}{}",
                    c.count.map(|n| n.to_string()).unwrap_or_default(),
                    c.keys.iter().map(|k| k.to_string()).collect::<String>()
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn starts_unconfident_until_escape() {
        let mut t = Tracker::new(Flavor::Vim);
        let steps = run(&mut t, "j<Esc>j");
        assert!(!steps[0].confident);
        assert!(steps[2].confident);
    }

    #[test]
    fn simple_motions_and_counts() {
        let mut t = vim();
        let steps = run(&mut t, "jj5j}<C-d>");
        assert_eq!(commands(&steps), ["j", "j", "5j", "}", "<C-d>"]);
        assert_eq!(t.mode(), VimMode::Normal);
    }

    #[test]
    fn operators_wait_for_motion() {
        let mut t = vim();
        let steps = run(&mut t, "dd3ddd2wdiwdfx");
        assert_eq!(commands(&steps), ["dd", "3dd", "d2w", "diw", "dfx"]);
        assert_eq!(steps[6].before, VimMode::OpPending, "after `d` in d2w");
    }

    #[test]
    fn text_object_i_is_not_insert() {
        let mut t = vim();
        run(&mut t, "di(");
        assert_eq!(t.mode(), VimMode::Normal);
        run(&mut t, "ci(");
        assert_eq!(t.mode(), VimMode::Insert);
    }

    #[test]
    fn argument_keys_are_not_commands() {
        let mut t = vim();
        let steps = run(&mut t, "fxrjmaj");
        assert_eq!(commands(&steps), ["fx", "rj", "ma", "j"]);
    }

    #[test]
    fn insert_mode_entry_and_exit() {
        for entry in [
            "i", "a", "I", "A", "o", "O", "s", "S", "C", "cw", "cc", "gi",
        ] {
            let mut t = vim();
            run(&mut t, entry);
            assert_eq!(t.mode(), VimMode::Insert, "{entry}");
            let steps = run(&mut t, "hello<BS><C-w><Esc>");
            assert_eq!(steps[0].event, Some(VimEvent::Insert(InsertKey::Text)));
            assert_eq!(steps[5].event, Some(VimEvent::Insert(InsertKey::Backspace)));
            assert_eq!(steps[6].event, Some(VimEvent::Insert(InsertKey::CtrlW)));
            assert_eq!(t.mode(), VimMode::Normal);
        }
    }

    #[test]
    fn insert_ctrl_o_runs_one_command() {
        let mut t = vim();
        run(&mut t, "i<C-o>");
        assert_eq!(t.mode(), VimMode::Normal);
        run(&mut t, "dd");
        assert_eq!(t.mode(), VimMode::Insert);
    }

    #[test]
    fn cmdline_reports_text_on_enter() {
        let mut t = vim();
        let steps = run(&mut t, ":w<CR>");
        assert_eq!(steps[1].before, VimMode::Cmdline);
        assert_eq!(
            steps[2].event,
            Some(VimEvent::Cmdline {
                kind: ':',
                text: Some("w".into())
            })
        );
        assert_eq!(t.mode(), VimMode::Normal);

        let steps = run(&mut t, ":<Up><CR>");
        assert_eq!(
            steps[2].event,
            Some(VimEvent::Cmdline {
                kind: ':',
                text: None
            })
        );
        run(&mut t, ":<BS>");
        assert_eq!(t.mode(), VimMode::Normal, "BS on empty cmdline leaves it");
    }

    #[test]
    fn visual_modes() {
        let mut t = vim();
        run(&mut t, "v");
        assert_eq!(t.mode(), VimMode::Visual);
        run(&mut t, "iw");
        assert_eq!(t.mode(), VimMode::Visual, "iw is a text object");
        run(&mut t, "V");
        assert_eq!(t.mode(), VimMode::VisualLine);
        run(&mut t, "d");
        assert_eq!(t.mode(), VimMode::Normal);
        run(&mut t, "<C-v>jjI");
        assert_eq!(t.mode(), VimMode::Insert);
        run(&mut t, "<Esc>vv");
        assert_eq!(t.mode(), VimMode::Normal);
        run(&mut t, "vjc");
        assert_eq!(t.mode(), VimMode::Insert);
    }

    #[test]
    fn invalidate_until_escape() {
        let mut t = vim();
        t.invalidate();
        let steps = run(&mut t, "j<Esc>j");
        assert!(!steps[0].confident);
        assert!(steps[2].confident);
    }

    #[test]
    fn readline_starts_in_insert_and_c_cancels_line() {
        let mut t = Tracker::new(Flavor::Readline);
        t.reset_known(VimMode::Insert);
        run(&mut t, "ls<Esc>");
        assert_eq!(t.mode(), VimMode::Normal);
        run(&mut t, "cw");
        assert_eq!(t.mode(), VimMode::Insert);
        run(&mut t, "<C-c>");
        assert_eq!(t.mode(), VimMode::Insert);
        assert!(t.confident());
    }

    #[test]
    fn readline_v_opens_editor_and_loses_track() {
        let mut t = Tracker::new(Flavor::Readline);
        t.reset_known(VimMode::Insert);
        run(&mut t, "<Esc>v");
        assert!(!t.confident());
    }

    #[test]
    fn insert_ctrl_keys_are_told_apart_and_literal_text_is_not_kept() {
        let mut t = Tracker::new(Flavor::Vim);
        t.reset_known(VimMode::Insert);
        let ev = |t: &mut Tracker, keys| {
            run(t, keys)
                .into_iter()
                .filter_map(|s| s.event)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ev(&mut t, "<C-t><C-x><C-f>"),
            [
                VimEvent::Insert(InsertKey::Ctrl('t')),
                VimEvent::Insert(InsertKey::Ctrl('x')),
                VimEvent::Insert(InsertKey::Then('x', 'f')),
            ]
        );
        // What follows Ctrl-v, Ctrl-k, or a prefix it doesn't mean
        // anything after, is text: only "other" is kept.
        for keys in ["<C-v>a", "<C-k>e'", "<C-g>q"] {
            let events = ev(&mut t, keys);
            assert!(
                events[1..]
                    .iter()
                    .all(|e| *e == VimEvent::Insert(InsertKey::Other)),
                "{keys}: {events:?}"
            );
        }
        assert_eq!(t.mode(), VimMode::Insert);
    }

    #[test]
    fn readline_ctrl_r_search_text_is_not_insert_text() {
        let mut t = Tracker::new(Flavor::Readline);
        t.reset_known(VimMode::Insert);
        let steps = run(&mut t, "<C-r>git<CR>");
        assert_eq!(steps[0].event, Some(VimEvent::Insert(InsertKey::CtrlR)));
        assert!(steps[1..].iter().all(|s| s.event.is_none()));
    }

    #[test]
    fn runaway_pending_is_bounded() {
        let mut t = vim();
        run(&mut t, "\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"");
        assert!(t.pending.len() <= MAX_PENDING);
    }

    #[test]
    fn insert_mappings_to_escape() {
        let mut t = Tracker::new(Flavor::Vim);
        t.reset_known(VimMode::Normal);
        t.set_mappings(Mappings {
            insert_escapes: vec![['j', 'k']],
            ..Default::default()
        });
        let steps: Vec<_> = strokes("ihijkx", 0)
            .into_iter()
            .map(|k| t.feed(k))
            .collect();
        assert_eq!(steps[4].event, Some(VimEvent::Escape));
        assert_eq!(steps[4].after, VimMode::Normal);
        // Then x is a Normal-mode delete.
        assert!(matches!(&steps[5].event, Some(VimEvent::Command(c)) if c.is("x")));
        // Too slow, or other keys: just text.
        let mut t = Tracker::new(Flavor::Vim);
        t.reset_known(VimMode::Insert);
        t.set_mappings(Mappings {
            insert_escapes: vec![['j', 'k']],
            ..Default::default()
        });
        let j = t.feed(KeyStroke {
            key: Key::ch('j'),
            repeat: false,
            at_ms: 0,
        });
        let k = t.feed(KeyStroke {
            key: Key::ch('k'),
            repeat: false,
            at_ms: 5000,
        });
        assert_eq!((j.after, k.after), (VimMode::Insert, VimMode::Insert));
    }

    #[test]
    fn mapped_normal_keys_are_opaque() {
        let mut t = Tracker::new(Flavor::Vim);
        t.reset_known(VimMode::Normal);
        t.set_mappings(Mappings {
            normal: vec!['H'],
            ..Default::default()
        });
        let steps: Vec<_> = strokes("Hj", 0).into_iter().map(|k| t.feed(k)).collect();
        assert_eq!(steps[0].event, Some(VimEvent::Mapped));
        assert!(matches!(&steps[1].event, Some(VimEvent::Command(c)) if c.is("j")));
    }

    #[test]
    fn surround_and_commentary_stay_in_step() {
        for (keys, mode_after) in [
            ("ysiw)", VimMode::Normal),
            ("yss]", VimMode::Normal),
            ("cs\"'", VimMode::Normal),
            ("ds(", VimMode::Normal),
            ("gcc", VimMode::Normal),
            ("gcip", VimMode::Normal),
        ] {
            let mut t = Tracker::new(Flavor::Vim);
            t.reset_known(VimMode::Normal);
            let steps: Vec<_> = strokes(keys, 0).into_iter().map(|k| t.feed(k)).collect();
            let last = steps.last().unwrap();
            assert!(
                matches!(last.event, Some(VimEvent::Command(_))),
                "{keys}: {:?}",
                last.event
            );
            assert_eq!(last.after, mode_after, "{keys}");
            assert!(
                steps[..steps.len() - 1].iter().all(|s| s.event.is_none()),
                "{keys} split up"
            );
        }
    }
}
