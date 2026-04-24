use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A keyboard binding with multiple possible keys and help text.
pub struct KeyBinding {
    pub keys: Vec<KeyEvent>,
    pub help_key: String,
    pub help_desc: String,
}

impl KeyBinding {
    pub fn matches(&self, key: &KeyEvent) -> bool {
        self.keys
            .iter()
            .any(|k| k.code == key.code && k.modifiers == key.modifiers)
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
            help_desc: "copy file".to_string(),
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
