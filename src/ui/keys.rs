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
    /// Hints for the file browser screen, in display order.
    ///
    /// Derived from the bindings, never written out: a hardcoded list is a second
    /// copy of the truth, and this one drifted into advertising `u`/`x`/`y` — keys
    /// bound to nothing — while telling users `d` downloads when `d` deletes.
    ///
    /// The bar is one row and drops overflow from the right, so the order is by
    /// usefulness. `copy_tar` is deliberately absent: it only works on SSH
    /// (`RemoteBackend::download_tar` defaults to an error), and advertising a
    /// capability the active backend lacks is the very fault this method fixes.
    pub fn browser_hints(&self) -> Vec<Hint> {
        vec![
            // Two bindings, one hint: `up`/`down` have no single label.
            Hint::new("j/k", "navigate"),
            self.switch_pane.hint(),
            self.copy_file.hint(),
            self.delete.hint(),
            self.rename.hint(),
            self.mkdir.hint(),
            self.sort.hint(),
            self.help.hint(),
            self.quit.hint(),
        ]
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
            help_key: "backspace".to_string(),
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
    /// true by construction — testing that would just re-check the constructor.
    /// This is the real gap: nothing stops someone binding `d` to download while
    /// `delete` still answers `d`, which is a near miss from the bug that made the
    /// status bar lie in the first place. First match wins in `handle_browser_key`,
    /// so the loser would simply go dead.
    #[test]
    fn no_two_bindings_claim_the_same_key() {
        let km = default_key_map();
        let named: Vec<(&str, &KeyBinding)> = vec![
            ("quit", &km.quit),
            ("help", &km.help),
            ("up", &km.up),
            ("down", &km.down),
            ("enter", &km.enter),
            ("back", &km.back),
            ("switch_pane", &km.switch_pane),
            ("copy_file", &km.copy_file),
            ("copy_tar", &km.copy_tar),
            ("delete", &km.delete),
            ("rename", &km.rename),
            ("mkdir", &km.mkdir),
            ("search", &km.search),
            ("escape", &km.escape),
            ("refresh", &km.refresh),
            ("top", &km.top),
            ("bottom", &km.bottom),
            ("toggle_theme", &km.toggle_theme),
            ("toggle_hidden", &km.toggle_hidden),
            ("sort", &km.sort),
        ];
        assert_eq!(
            named.len(),
            20,
            "a binding was added to KeyMap without being listed here"
        );
        for (i, (name, a)) in named.iter().enumerate() {
            for (other, b) in &named[i + 1..] {
                for k in &a.keys {
                    assert!(!b.matches(k), "{name} and {other} both answer to {k:?}");
                }
            }
        }
    }

    /// Every advertised hint must name a key some binding answers to. Vacuous for
    /// the derived ones; it catches the literals (`j/k`) drifting off the keymap.
    #[test]
    fn every_browser_hint_is_reachable() {
        let km = default_key_map();
        for hint in km.browser_hints() {
            let reachable = hint.key.chars().any(|c| {
                let ev = key(KeyCode::Char(c));
                km.up.matches(&ev)
                    || km.down.matches(&ev)
                    || km.copy_file.matches(&ev)
                    || km.delete.matches(&ev)
                    || km.rename.matches(&ev)
                    || km.mkdir.matches(&ev)
                    || km.sort.matches(&ev)
                    || km.help.matches(&ev)
                    || km.quit.matches(&ev)
            }) || hint.key == "tab";
            assert!(reachable, "hint {hint:?} is bound to nothing");
        }
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
