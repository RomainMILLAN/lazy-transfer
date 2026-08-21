use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::components::Hint;

/// A keyboard binding: the keys it answers to, and how it presents itself.
///
/// `help_key`/`help_desc` are PRIVATE. Public, every surface recomposed its own
/// label from the pieces and the duplication merely moved down a level; that is
/// how the status bar came to advertise `d` for "download" while `d` deleted. Ask
/// the binding to describe itself with [`KeyBinding::hint`] instead.
pub struct KeyBinding {
    pub keys: Vec<KeyEvent>,
    help_key: String,
    help_desc: String,
}

impl KeyBinding {
    pub fn matches(&self, key: &KeyEvent) -> bool {
        self.keys
            .iter()
            .any(|k| k.code == key.code && k.modifiers == key.modifiers)
    }

    /// The binding presents itself. Nobody else has to glue its pieces together.
    pub fn hint(&self) -> Hint {
        Hint::new(&self.help_key, &self.help_desc)
    }

    /// The same, with a context-specific description. The binding still owns the
    /// key — only the wording changes, e.g. `copy_file` reads "upload" on the
    /// local pane and "download" on the remote one.
    pub fn hint_as(&self, desc: &str) -> Hint {
        Hint::new(&self.help_key, desc)
    }

    /// Whether `label` names only keys this binding answers to.
    ///
    /// A label is a human spelling of a key set (`"backspace/h"`), so it is the one
    /// place a typo cannot be caught by construction. Segments are split on `/`,
    /// and a segment naming a non-character key (`backspace`, `tab`, `enter`) is
    /// matched by name.
    pub fn spells(&self, label: &str) -> bool {
        // A one-character label IS the key, even when that character is the `/`
        // used as the separator — `search` advertises exactly "/".
        if label.chars().count() == 1 {
            return self.answers_to(label);
        }
        label.split('/').all(|seg| self.answers_to(seg.trim()))
    }

    fn answers_to(&self, seg: &str) -> bool {
        // Modifier notation: "Ctrl+L", "Shift+Tab". The last `+`-separated part
        // names the key, everything before it names modifiers.
        let mut mods = KeyModifiers::NONE;
        let mut name = seg;
        while let Some((prefix, rest)) = name.split_once('+') {
            match prefix.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods |= KeyModifiers::CONTROL,
                "shift" => mods |= KeyModifiers::SHIFT,
                "alt" => mods |= KeyModifiers::ALT,
                _ => return false,
            }
            name = rest;
        }
        let name = name.trim();

        let code = match name {
            "backspace" => KeyCode::Backspace,
            "tab" => KeyCode::Tab,
            "enter" => KeyCode::Enter,
            "esc" => KeyCode::Esc,
            "\u{2191}" => KeyCode::Up,
            "\u{2193}" => KeyCode::Down,
            "arrows" => {
                return self
                    .keys
                    .iter()
                    .any(|k| matches!(k.code, KeyCode::Up | KeyCode::Down))
            }
            _ => {
                let mut chars = name.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => KeyCode::Char(c),
                    _ => return false,
                }
            }
        };

        self.keys.iter().any(|k| {
            let code_matches = match (k.code, code) {
                // A label spells a letter in whichever case reads best: the theme
                // toggle is bound to Ctrl+`l` and advertised as "Ctrl+L".
                (KeyCode::Char(a), KeyCode::Char(b)) => a.eq_ignore_ascii_case(&b),
                (a, b) => a == b,
            };
            code_matches && k.modifiers.contains(mods)
        })
    }
}

/// One hint from two bindings, for the rows that have no single label:
/// `up` + `down` display as "j/k", `top` + `bottom` as "g / G".
///
/// `label` is free text, which is exactly why [`spells_merged_label`] checks it
/// against both bindings — a hand-written label is the last place drift can hide.
pub fn merged(a: &KeyBinding, b: &KeyBinding, label: &str, desc: &str) -> Hint {
    debug_assert!(
        spells_merged_label(a, b, label),
        "merged label {label:?} names a key neither binding answers to"
    );
    Hint::new(label, desc)
}

/// Whether every key named in `label` is answered by `a` or by `b`.
pub fn spells_merged_label(a: &KeyBinding, b: &KeyBinding, label: &str) -> bool {
    label
        .split('/')
        .all(|seg| a.spells(seg.trim()) || b.spells(seg.trim()))
}

/// All keybindings for lazy-transfer.
pub struct KeyMap {
    pub quit: KeyBinding,
    pub help: KeyBinding,
    pub up: KeyBinding,
    pub down: KeyBinding,
    pub enter: KeyBinding,
    pub back: KeyBinding,
    pub switch_pane: KeyBinding,
    pub copy_file: KeyBinding,
    pub copy_tar: KeyBinding,
    pub delete: KeyBinding,
    pub rename: KeyBinding,
    pub mkdir: KeyBinding,
    pub search: KeyBinding,
    pub escape: KeyBinding,
    pub refresh: KeyBinding,
    pub top: KeyBinding,
    pub bottom: KeyBinding,
    pub toggle_theme: KeyBinding,
    pub toggle_hidden: KeyBinding,
    pub sort: KeyBinding,
}

impl KeyMap {
    /// Every binding with its field name, in declaration order.
    ///
    /// One list, so a test cannot silently cover fewer bindings than exist: the
    /// `len` assertion in `no_two_bindings_claim_the_same_key` trips the moment a
    /// binding is added to the struct without being added here.
    pub fn all_named(&self) -> Vec<(&'static str, &KeyBinding)> {
        vec![
            ("quit", &self.quit),
            ("help", &self.help),
            ("up", &self.up),
            ("down", &self.down),
            ("enter", &self.enter),
            ("back", &self.back),
            ("switch_pane", &self.switch_pane),
            ("copy_file", &self.copy_file),
            ("copy_tar", &self.copy_tar),
            ("delete", &self.delete),
            ("rename", &self.rename),
            ("mkdir", &self.mkdir),
            ("search", &self.search),
            ("escape", &self.escape),
            ("refresh", &self.refresh),
            ("top", &self.top),
            ("bottom", &self.bottom),
            ("toggle_theme", &self.toggle_theme),
            ("toggle_hidden", &self.toggle_hidden),
            ("sort", &self.sort),
        ]
    }

    /// Whether every key named in `label` is answered by SOME binding.
    ///
    /// The check behind "no surface advertises a key bound to nothing". It spans
    /// the whole map because a merged label legitimately draws on two bindings:
    /// `"j/k"` is `down` and `up`, and no single binding spells it.
    ///
    /// Every segment must be reachable, not merely one of them — accepting a label
    /// because part of it works would wave through `"u/c"`, with a dead `u` beside
    /// a working `c`. That partial drift is the exact shape of the original bug.
    pub fn names_only_bound_keys(&self, label: &str) -> bool {
        let named = self.all_named();
        let reachable = |seg: &str| named.iter().any(|(_, b)| b.spells(seg));
        if label.chars().count() == 1 {
            return reachable(label);
        }
        label.split('/').all(|seg| reachable(seg.trim()))
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

fn key_shift(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

pub fn default_key_map() -> KeyMap {
    KeyMap {
        quit: KeyBinding {
            keys: vec![key(KeyCode::Char('q'))],
            help_key: "q".to_string(),
            help_desc: "quit".to_string(),
        },
        help: KeyBinding {
            keys: vec![key(KeyCode::Char('?'))],
            help_key: "?".to_string(),
            help_desc: "help".to_string(),
        },
        up: KeyBinding {
            keys: vec![key(KeyCode::Char('k')), key(KeyCode::Up)],
            help_key: "k/↑".to_string(),
            help_desc: "up".to_string(),
        },
        down: KeyBinding {
            keys: vec![key(KeyCode::Char('j')), key(KeyCode::Down)],
            help_key: "j/↓".to_string(),
            help_desc: "down".to_string(),
        },
        enter: KeyBinding {
            keys: vec![key(KeyCode::Enter)],
            help_key: "enter".to_string(),
            help_desc: "open".to_string(),
        },
        back: KeyBinding {
            keys: vec![key(KeyCode::Backspace), key(KeyCode::Char('h'))],
            // Must name BOTH keys: the help popup derives this label, and a
            // `"backspace"` here silently drops `h` from the only place it is
            // documented.
            help_key: "backspace/h".to_string(),
            help_desc: "parent dir".to_string(),
        },
        switch_pane: KeyBinding {
            keys: vec![key(KeyCode::Tab)],
            help_key: "tab".to_string(),
            help_desc: "switch pane".to_string(),
        },
        copy_file: KeyBinding {
            keys: vec![key(KeyCode::Char('c'))],
            help_key: "c".to_string(),
            // Not "copy file": the one thing a user needs from this label is that
            // it is how you download, and the status bar is one row wide.
            help_desc: "upload/download".to_string(),
        },
        copy_tar: KeyBinding {
            keys: vec![key_shift(KeyCode::Char('C'))],
            help_key: "C".to_string(),
            help_desc: "copy file (tar)".to_string(),
        },
        delete: KeyBinding {
            keys: vec![key(KeyCode::Char('d'))],
            help_key: "d".to_string(),
            help_desc: "delete".to_string(),
        },
        rename: KeyBinding {
            keys: vec![key(KeyCode::Char('r'))],
            help_key: "r".to_string(),
            help_desc: "rename".to_string(),
        },
        mkdir: KeyBinding {
            keys: vec![key(KeyCode::Char('m'))],
            help_key: "m".to_string(),
            help_desc: "mkdir".to_string(),
        },
        search: KeyBinding {
            keys: vec![key(KeyCode::Char('/'))],
            help_key: "/".to_string(),
            help_desc: "filter".to_string(),
        },
        escape: KeyBinding {
            keys: vec![key(KeyCode::Esc)],
            help_key: "esc".to_string(),
            help_desc: "cancel".to_string(),
        },
        refresh: KeyBinding {
            keys: vec![key_shift(KeyCode::Char('R'))],
            help_key: "R".to_string(),
            help_desc: "refresh".to_string(),
        },
        top: KeyBinding {
            keys: vec![key(KeyCode::Char('g'))],
            help_key: "g".to_string(),
            help_desc: "top".to_string(),
        },
        bottom: KeyBinding {
            keys: vec![key_shift(KeyCode::Char('G'))],
            help_key: "G".to_string(),
            help_desc: "bottom".to_string(),
        },
        toggle_theme: KeyBinding {
            keys: vec![key_ctrl(KeyCode::Char('l'))],
            help_key: "Ctrl+L".to_string(),
            help_desc: "toggle theme".to_string(),
        },
        toggle_hidden: KeyBinding {
            keys: vec![key(KeyCode::Char('.'))],
            help_key: ".".to_string(),
            help_desc: "toggle hidden files".to_string(),
        },
        sort: KeyBinding {
            keys: vec![key(KeyCode::Char('s'))],
            help_key: "s".to_string(),
            help_desc: "sort".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The invariant the constructor does NOT give for free.
    ///
    /// Deriving the hints from the bindings makes "every advertised key is bound"
    /// true by construction. This is the real gap: nothing stops someone binding
    /// `d` to download while `delete` still answers `d`, which is a near miss from
    /// the bug that made the status bar lie. First match wins in
    /// `handle_browser_key`, so the loser would simply go dead.
    #[test]
    fn no_two_bindings_claim_the_same_key() {
        let km = default_key_map();
        let named = km.all_named();
        assert_eq!(
            named.len(),
            20,
            "a binding was added to KeyMap without being listed in all_named()"
        );
        for (i, (name, a)) in named.iter().enumerate() {
            for (other, b) in &named[i + 1..] {
                for k in &a.keys {
                    assert!(!b.matches(k), "{name} and {other} both answer to {k:?}");
                }
            }
        }
    }

    /// Every binding's own label must name only keys it answers to.
    ///
    /// `back` is why this exists: its label read `"backspace"` while the binding
    /// also answered `h`, so deriving the help popup from it would have dropped
    /// the one key CLAUDE.md documents for "parent directory".
    #[test]
    fn every_label_spells_its_own_keys() {
        let km = default_key_map();
        for (name, b) in km.all_named() {
            let hint = b.hint();
            assert!(
                b.spells(&hint.key),
                "{name} advertises {:?} but does not answer to all of it",
                hint.key
            );
        }
    }

    /// The merged rows the help popup and the bar use.
    #[test]
    fn merged_labels_name_only_bound_keys() {
        let km = default_key_map();
        assert!(spells_merged_label(&km.up, &km.down, "j/k"));
        assert!(spells_merged_label(&km.up, &km.down, "j/k/arrows"));
        assert!(spells_merged_label(&km.top, &km.bottom, "g/G"));
        // A key neither binding answers to must be rejected.
        assert!(!spells_merged_label(&km.up, &km.down, "j/u"));
    }

    #[test]
    fn back_label_keeps_the_vim_key() {
        let km = default_key_map();
        assert_eq!(km.back.hint().key, "backspace/h");
        assert!(km.back.spells("backspace/h"));
    }

    #[test]
    fn hint_as_keeps_the_key_and_swaps_the_wording() {
        let km = default_key_map();
        let h = km.copy_file.hint_as("upload");
        assert_eq!(h.key, "c");
        assert_eq!(h.desc, "upload");
    }

    #[test]
    fn quit_matches_q() {
        let km = default_key_map();
        assert!(km.quit.matches(&key(KeyCode::Char('q'))));
        assert!(!km.quit.matches(&key(KeyCode::Char('x'))));
    }

    #[test]
    fn up_matches_k_and_arrow() {
        let km = default_key_map();
        assert!(km.up.matches(&key(KeyCode::Char('k'))));
        assert!(km.up.matches(&key(KeyCode::Up)));
    }

    #[test]
    fn toggle_theme_requires_ctrl() {
        let km = default_key_map();
        assert!(km.toggle_theme.matches(&key_ctrl(KeyCode::Char('l'))));
        assert!(!km.toggle_theme.matches(&key(KeyCode::Char('l'))));
    }

    #[test]
    fn copy_tar_matches_shift_c() {
        let km = default_key_map();
        assert!(km.copy_tar.matches(&key_shift(KeyCode::Char('C'))));
        assert!(!km.copy_tar.matches(&key(KeyCode::Char('c'))));
    }
}
