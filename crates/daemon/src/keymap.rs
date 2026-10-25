//! Translate evdev keycodes into [`Key`]s.
//!
//! The built-in [`UsKeymap`] maps physical keys as on a US layout. Vim
//! commands are almost always typed on a Latin layout, so this is correct
//! even when another layout (e.g. Arabic) is configured for writing.
//! With the `xkb` feature, [`xkb::XkbKeymap`] uses libxkbcommon instead.

use evdev::KeyCode;
use flowbuddy_core::key::{Key, Mods, Sym};

pub trait Keymap {
    /// Feed one raw event (`value`: 0 release, 1 press, 2 repeat).
    /// Returns the key for presses and repeats of non-modifier keys.
    fn translate(&mut self, code: u16, value: i32) -> Option<Key>;
}

/// Options that change what physical keys mean, as in xkb options.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeymapOptions {
    /// `caps:escape`: Caps Lock acts as Escape.
    pub caps_is_escape: bool,
    /// `caps:swapescape`: Caps Lock and Escape are swapped.
    pub swap_caps_escape: bool,
    /// `ctrl:nocaps`: Caps Lock acts as Ctrl.
    pub caps_is_ctrl: bool,
}

impl KeymapOptions {
    /// Parse xkb option strings such as `caps:escape`; unknown ones are ignored.
    pub fn from_xkb<'a>(options: impl IntoIterator<Item = &'a str>) -> Self {
        let mut o = KeymapOptions::default();
        for opt in options {
            match opt {
                "caps:escape" | "caps:escape_shifted_capslock" => o.caps_is_escape = true,
                "caps:swapescape" => o.swap_caps_escape = true,
                "ctrl:nocaps" | "ctrl:swapcaps" => o.caps_is_ctrl = true,
                _ => {}
            }
        }
        o
    }
}

#[derive(Default)]
pub struct UsKeymap {
    options: KeymapOptions,
    lshift: bool,
    rshift: bool,
    lctrl: bool,
    rctrl: bool,
    capsctrl: bool,
    alt: bool,
    logo: bool,
    capslock: bool,
}

impl UsKeymap {
    pub fn new(options: KeymapOptions) -> Self {
        UsKeymap {
            options,
            ..Default::default()
        }
    }

    fn mods(&self) -> Mods {
        Mods {
            ctrl: self.lctrl || self.rctrl || self.capsctrl,
            alt: self.alt,
            shift: self.lshift || self.rshift,
            logo: self.logo,
        }
    }
}

/// Unshifted and shifted characters for printable keys on a US layout.
fn us_chars(code: KeyCode) -> Option<(char, char)> {
    const LETTERS: [(KeyCode, char); 26] = [
        (KeyCode::KEY_A, 'a'),
        (KeyCode::KEY_B, 'b'),
        (KeyCode::KEY_C, 'c'),
        (KeyCode::KEY_D, 'd'),
        (KeyCode::KEY_E, 'e'),
        (KeyCode::KEY_F, 'f'),
        (KeyCode::KEY_G, 'g'),
        (KeyCode::KEY_H, 'h'),
        (KeyCode::KEY_I, 'i'),
        (KeyCode::KEY_J, 'j'),
        (KeyCode::KEY_K, 'k'),
        (KeyCode::KEY_L, 'l'),
        (KeyCode::KEY_M, 'm'),
        (KeyCode::KEY_N, 'n'),
        (KeyCode::KEY_O, 'o'),
        (KeyCode::KEY_P, 'p'),
        (KeyCode::KEY_Q, 'q'),
        (KeyCode::KEY_R, 'r'),
        (KeyCode::KEY_S, 's'),
        (KeyCode::KEY_T, 't'),
        (KeyCode::KEY_U, 'u'),
        (KeyCode::KEY_V, 'v'),
        (KeyCode::KEY_W, 'w'),
        (KeyCode::KEY_X, 'x'),
        (KeyCode::KEY_Y, 'y'),
        (KeyCode::KEY_Z, 'z'),
    ];
    const OTHERS: [(KeyCode, char, char); 22] = [
        (KeyCode::KEY_1, '1', '!'),
        (KeyCode::KEY_2, '2', '@'),
        (KeyCode::KEY_3, '3', '#'),
        (KeyCode::KEY_4, '4', '$'),
        (KeyCode::KEY_5, '5', '%'),
        (KeyCode::KEY_6, '6', '^'),
        (KeyCode::KEY_7, '7', '&'),
        (KeyCode::KEY_8, '8', '*'),
        (KeyCode::KEY_9, '9', '('),
        (KeyCode::KEY_0, '0', ')'),
        (KeyCode::KEY_MINUS, '-', '_'),
        (KeyCode::KEY_EQUAL, '=', '+'),
        (KeyCode::KEY_LEFTBRACE, '[', '{'),
        (KeyCode::KEY_RIGHTBRACE, ']', '}'),
        (KeyCode::KEY_SEMICOLON, ';', ':'),
        (KeyCode::KEY_APOSTROPHE, '\'', '"'),
        (KeyCode::KEY_GRAVE, '`', '~'),
        (KeyCode::KEY_BACKSLASH, '\\', '|'),
        (KeyCode::KEY_COMMA, ',', '<'),
        (KeyCode::KEY_DOT, '.', '>'),
        (KeyCode::KEY_SLASH, '/', '?'),
        (KeyCode::KEY_SPACE, ' ', ' '),
    ];
    if let Some(&(_, c)) = LETTERS.iter().find(|(k, _)| *k == code) {
        return Some((c, c.to_ascii_uppercase()));
    }
    OTHERS
        .iter()
        .find(|(k, _, _)| *k == code)
        .map(|&(_, a, b)| (a, b))
}

fn named_sym(code: KeyCode) -> Option<Sym> {
    Some(match code {
        KeyCode::KEY_ESC => Sym::Esc,
        KeyCode::KEY_ENTER | KeyCode::KEY_KPENTER => Sym::Enter,
        KeyCode::KEY_TAB => Sym::Tab,
        KeyCode::KEY_BACKSPACE => Sym::Backspace,
        KeyCode::KEY_DELETE => Sym::Delete,
        KeyCode::KEY_INSERT => Sym::Insert,
        KeyCode::KEY_UP => Sym::Up,
        KeyCode::KEY_DOWN => Sym::Down,
        KeyCode::KEY_LEFT => Sym::Left,
        KeyCode::KEY_RIGHT => Sym::Right,
        KeyCode::KEY_HOME => Sym::Home,
        KeyCode::KEY_END => Sym::End,
        KeyCode::KEY_PAGEUP => Sym::PageUp,
        KeyCode::KEY_PAGEDOWN => Sym::PageDown,
        KeyCode::KEY_F1 => Sym::F(1),
        KeyCode::KEY_F2 => Sym::F(2),
        KeyCode::KEY_F3 => Sym::F(3),
        KeyCode::KEY_F4 => Sym::F(4),
        KeyCode::KEY_F5 => Sym::F(5),
        KeyCode::KEY_F6 => Sym::F(6),
        KeyCode::KEY_F7 => Sym::F(7),
        KeyCode::KEY_F8 => Sym::F(8),
        KeyCode::KEY_F9 => Sym::F(9),
        KeyCode::KEY_F10 => Sym::F(10),
        KeyCode::KEY_F11 => Sym::F(11),
        KeyCode::KEY_F12 => Sym::F(12),
        _ => return None,
    })
}

impl Keymap for UsKeymap {
    fn translate(&mut self, code: u16, value: i32) -> Option<Key> {
        let pressed = value != 0;
        let mut code = KeyCode(code);
        if self.options.swap_caps_escape {
            code = match code {
                KeyCode::KEY_CAPSLOCK => KeyCode::KEY_ESC,
                KeyCode::KEY_ESC => KeyCode::KEY_CAPSLOCK,
                c => c,
            };
        } else if self.options.caps_is_escape && code == KeyCode::KEY_CAPSLOCK {
            code = KeyCode::KEY_ESC;
        }

        match code {
            KeyCode::KEY_LEFTSHIFT => self.lshift = pressed,
            KeyCode::KEY_RIGHTSHIFT => self.rshift = pressed,
            KeyCode::KEY_LEFTCTRL => self.lctrl = pressed,
            KeyCode::KEY_RIGHTCTRL => self.rctrl = pressed,
            KeyCode::KEY_LEFTALT | KeyCode::KEY_RIGHTALT => self.alt = pressed,
            KeyCode::KEY_LEFTMETA | KeyCode::KEY_RIGHTMETA => self.logo = pressed,
            KeyCode::KEY_CAPSLOCK if self.options.caps_is_ctrl => self.capsctrl = pressed,
            KeyCode::KEY_CAPSLOCK => {
                if value == 1 {
                    self.capslock = !self.capslock;
                }
            }
            _ if !pressed => {}
            _ => {
                let mods = self.mods();
                if let Some((lower, upper)) = us_chars(code) {
                    let shifted = mods.shift ^ (self.capslock && lower.is_ascii_alphabetic());
                    let c = if shifted { upper } else { lower };
                    return Some(Key {
                        sym: Sym::Char(c),
                        mods: Mods {
                            shift: false,
                            ..mods
                        },
                    });
                }
                return Some(Key {
                    sym: named_sym(code).unwrap_or(Sym::Other),
                    mods,
                });
            }
        }
        None
    }
}

#[cfg(feature = "xkb")]
pub mod xkb {
    //! libxkbcommon-backed keymap. Requires `libxkbcommon-dev` to build.

    use super::Keymap;
    use evdev::KeyCode;
    use flowbuddy_core::key::{Key, Mods, Sym};
    use xkbcommon::xkb;

    pub struct XkbKeymap {
        state: xkb::State,
    }

    impl XkbKeymap {
        pub fn new(layout: &str, variant: &str, options: &[String]) -> anyhow::Result<Self> {
            let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
            let opts = (!options.is_empty()).then(|| options.join(","));
            let keymap = xkb::Keymap::new_from_names(
                &ctx,
                "",
                "",
                layout,
                variant,
                opts,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )
            .ok_or_else(|| anyhow::anyhow!("could not compile xkb keymap for layout {layout}"))?;
            Ok(XkbKeymap {
                state: xkb::State::new(&keymap),
            })
        }

        fn active(&self, name: &str) -> bool {
            self.state
                .mod_name_is_active(name, xkb::STATE_MODS_EFFECTIVE)
        }
    }

    impl Keymap for XkbKeymap {
        fn translate(&mut self, code: u16, value: i32) -> Option<Key> {
            let kc = xkb::Keycode::new(u32::from(code) + 8);
            let sym = self.state.key_get_one_sym(kc);
            let result = (value != 0).then(|| {
                let mods = Mods {
                    ctrl: self.active(xkb::MOD_NAME_CTRL),
                    alt: self.active(xkb::MOD_NAME_ALT),
                    shift: self.active(xkb::MOD_NAME_SHIFT),
                    logo: self.active(xkb::MOD_NAME_LOGO),
                };
                (sym, mods)
            });
            if value != 2 {
                let dir = if value == 1 {
                    xkb::KeyDirection::Down
                } else {
                    xkb::KeyDirection::Up
                };
                self.state.update_key(kc, dir);
            }
            let (sym, mods) = result?;
            // Shift_L..Hyper_R and ISO_Level*_Shift/Lock are modifiers only.
            if (0xffe1..=0xffee).contains(&sym.raw()) || (0xfe01..=0xfe13).contains(&sym.raw()) {
                return None;
            }
            let ch = xkb::keysym_to_utf32(sym);
            match char::from_u32(ch).filter(|c| !c.is_control()) {
                Some(c) => Some(Key {
                    sym: Sym::Char(c),
                    mods: Mods {
                        shift: false,
                        ..mods
                    },
                }),
                None => Some(Key {
                    sym: super::named_sym(KeyCode(code)).unwrap_or(Sym::Other),
                    mods,
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(km: &mut UsKeymap, code: KeyCode) -> Option<Key> {
        let k = km.translate(code.code(), 1);
        km.translate(code.code(), 0);
        k
    }

    #[test]
    fn letters_and_shifted_punctuation() {
        let mut km = UsKeymap::default();
        assert_eq!(press(&mut km, KeyCode::KEY_J), Some(Key::ch('j')));
        km.translate(KeyCode::KEY_LEFTSHIFT.code(), 1);
        assert_eq!(press(&mut km, KeyCode::KEY_RIGHTBRACE), Some(Key::ch('}')));
        assert_eq!(press(&mut km, KeyCode::KEY_P), Some(Key::ch('P')));
        km.translate(KeyCode::KEY_LEFTSHIFT.code(), 0);
        assert_eq!(press(&mut km, KeyCode::KEY_P), Some(Key::ch('p')));
    }

    #[test]
    fn ctrl_chords() {
        let mut km = UsKeymap::default();
        km.translate(KeyCode::KEY_LEFTCTRL.code(), 1);
        assert_eq!(press(&mut km, KeyCode::KEY_D), Some(Key::ctrl('d')));
        km.translate(KeyCode::KEY_LEFTCTRL.code(), 0);
        assert_eq!(press(&mut km, KeyCode::KEY_D), Some(Key::ch('d')));
    }

    #[test]
    fn modifiers_alone_produce_nothing() {
        let mut km = UsKeymap::default();
        assert_eq!(km.translate(KeyCode::KEY_LEFTSHIFT.code(), 1), None);
        assert_eq!(km.translate(KeyCode::KEY_LEFTCTRL.code(), 1), None);
    }

    #[test]
    fn repeat_counts_as_a_key_release_does_not() {
        let mut km = UsKeymap::default();
        assert_eq!(km.translate(KeyCode::KEY_J.code(), 1), Some(Key::ch('j')));
        assert_eq!(km.translate(KeyCode::KEY_J.code(), 2), Some(Key::ch('j')));
        assert_eq!(km.translate(KeyCode::KEY_J.code(), 0), None);
    }

    #[test]
    fn caps_lock_affects_letters_only() {
        let mut km = UsKeymap::default();
        press(&mut km, KeyCode::KEY_CAPSLOCK);
        assert_eq!(press(&mut km, KeyCode::KEY_A), Some(Key::ch('A')));
        assert_eq!(press(&mut km, KeyCode::KEY_1), Some(Key::ch('1')));
    }

    #[test]
    fn caps_as_escape_option() {
        let mut km = UsKeymap::new(KeymapOptions::from_xkb(["caps:escape"]));
        assert_eq!(press(&mut km, KeyCode::KEY_CAPSLOCK), Some(Key::ESC));
        let mut km = UsKeymap::new(KeymapOptions::from_xkb(["ctrl:nocaps"]));
        km.translate(KeyCode::KEY_CAPSLOCK.code(), 1);
        assert_eq!(press(&mut km, KeyCode::KEY_W), Some(Key::ctrl('w')));
    }

    #[test]
    fn named_keys() {
        let mut km = UsKeymap::default();
        assert_eq!(press(&mut km, KeyCode::KEY_ESC), Some(Key::ESC));
        assert_eq!(press(&mut km, KeyCode::KEY_UP), Some(Key::UP));
        assert_eq!(press(&mut km, KeyCode::KEY_BACKSPACE), Some(Key::BACKSPACE));
    }
}

/// Read xkb options from GNOME settings, e.g. `['caps:escape']`.
/// Returns an empty list if gsettings is unavailable.
pub fn gnome_xkb_options() -> Vec<String> {
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.input-sources", "xkb-options"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_gvariant_strings(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// Parse a GVariant string array such as `['a', 'b']` or `@as []`.
fn parse_gvariant_strings(s: &str) -> Vec<String> {
    s.split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod gsettings_tests {
    use super::parse_gvariant_strings;

    #[test]
    fn parses_gvariant_arrays() {
        assert_eq!(
            parse_gvariant_strings("['grp_led:scroll', 'caps:escape']\n"),
            ["grp_led:scroll", "caps:escape"]
        );
        assert!(parse_gvariant_strings("@as []\n").is_empty());
    }
}
