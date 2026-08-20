use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use crate::ui::style::theme;

/// Hint is a key/description pair.
#[derive(Debug, Clone)]
pub struct Hint {
    pub key: String,
    pub desc: String,
}

/// StatusBar renders contextual keyboard hints at the bottom.
pub struct StatusBar {
    pub width: u16,
    pub hints: Vec<Hint>,
    pub connection_info: String,
    pub loading_msg: String,
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusBar {
    pub fn new() -> Self {
        StatusBar {
            width: 0,
            hints: vec![],
            connection_info: String::new(),
            loading_msg: String::new(),
        }
    }

    pub fn set_connection_info(&mut self, info: &str) {
        self.connection_info = info.to_string();
    }

    pub fn set_loading(&mut self, msg: &str) {
        self.loading_msg = msg.to_string();
    }

    pub fn set_width(&mut self, width: u16) {
        self.width = width;
    }

    pub fn set_hints(&mut self, hints: Vec<Hint>) {
        self.hints = hints;
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let bar_bg = if theme::mode() == theme::ThemeMode::Light {
            Color::Rgb(0xE0, 0xE0, 0xE0)
        } else {
            Color::Rgb(0x1A, 0x1A, 0x1A)
        };
        let bg = Style::default().fg(theme::color_text()).bg(bar_bg);
        for x in area.x..area.x + area.width {
            buf.set_string(x, area.y, " ", bg);
        }

        let mut x = area.x + 1;

        // Loading message
        if !self.loading_msg.is_empty() {
            let msg = format!("⟳ {}", self.loading_msg);
            let style = Style::default()
                .fg(Color::Rgb(0x00, 0x00, 0x00))
                .bg(theme::color_warning());
            buf.set_string(x, area.y, &msg, style);
            x += msg.len() as u16 + 2;
        }

        // Hints
        for hint in &self.hints {
            let key_style = Style::default().fg(theme::color_primary()).bg(bar_bg);
            buf.set_string(x, area.y, &hint.key, key_style);
            x += hint.key.len() as u16 + 1;
            buf.set_string(x, area.y, &hint.desc, bg);
            x += hint.desc.len() as u16 + 2;
        }

        // Connection info on the right
        if !self.connection_info.is_empty() {
            let info_len = self.connection_info.len() as u16;
            if area.width > info_len + 2 {
                let info_x = area.x + area.width - info_len - 1;
                let info_style = Style::default().fg(theme::color_primary()).bg(bar_bg);
                buf.set_string(info_x, area.y, &self.connection_info, info_style);
            }
        }
    }
}

/// Returns the hints for the connection selection screen.
pub fn connection_hints() -> Vec<Hint> {
    vec![
        Hint {
            key: "1-4".to_string(),
            desc: "SSH/SFTP/FTP/DAV".to_string(),
        },
        Hint {
            key: "j/k".to_string(),
            desc: "navigate".to_string(),
        },
        Hint {
            key: "enter".to_string(),
            desc: "connect".to_string(),
        },
        Hint {
            key: "e".to_string(),
            desc: "edit saved".to_string(),
        },
        Hint {
            key: "x".to_string(),
            desc: "remove saved".to_string(),
        },
        Hint {
            key: "/".to_string(),
            desc: "filter".to_string(),
        },
        Hint {
            key: "?".to_string(),
            desc: "help".to_string(),
        },
        Hint {
            key: "q".to_string(),
            desc: "quit".to_string(),
        },
    ]
}

/// Returns the hints for the file browser screen.
pub fn browser_hints() -> Vec<Hint> {
    vec![
        Hint {
            key: "j/k".to_string(),
            desc: "navigate".to_string(),
        },
        Hint {
            key: "tab".to_string(),
            desc: "switch pane".to_string(),
        },
        Hint {
            key: "c".to_string(),
            desc: "copy".to_string(),
        },
        Hint {
            key: "d".to_string(),
            desc: "delete".to_string(),
        },
        Hint {
            key: "m".to_string(),
            desc: "mkdir".to_string(),
        },
        Hint {
            key: "r".to_string(),
            desc: "rename".to_string(),
        },
        Hint {
            key: "s".to_string(),
            desc: "sort".to_string(),
        },
        Hint {
            key: "R".to_string(),
            desc: "refresh".to_string(),
        },
        Hint {
            key: "?".to_string(),
            desc: "help".to_string(),
        },
        Hint {
            key: "q".to_string(),
            desc: "quit".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_hints_has_quit() {
        let hints = connection_hints();
        assert!(hints.iter().any(|h| h.key == "q"));
    }

    #[test]
    fn browser_hints_has_copy() {
        let hints = browser_hints();
        assert!(hints.iter().any(|h| h.key == "c" && h.desc == "copy"));
    }
}
