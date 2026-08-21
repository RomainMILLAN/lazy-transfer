use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::ui::components::Hint;
use crate::ui::style::{styles, theme};

/// StatusBar renders contextual keyboard hints at the bottom.
pub struct StatusBar {
    pub width: u16,
    pub hints: Vec<Hint>,
    pub loading_msg: String,
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

/// Terminal columns a string occupies. `str::len()` counts bytes, and the bar
/// carries `⟳` — three bytes, one column — so byte lengths used to shift
/// everything after the loading message two columns to the right.
fn cols(s: &str) -> u16 {
    s.chars().count() as u16
}

impl StatusBar {
    pub fn new() -> Self {
        StatusBar {
            width: 0,
            hints: vec![],
            loading_msg: String::new(),
        }
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
        let bg = styles::bar_style();
        for x in area.x..area.x + area.width {
            buf.set_string(x, area.y, " ", bg);
        }

        let mut x = area.x + 1;

        // Loading message
        if !self.loading_msg.is_empty() {
            let msg = format!("⟳ {}", self.loading_msg);
            let style = Style::default()
                .fg(theme::color_background())
                .bg(theme::color_warning());
            buf.set_string(x, area.y, &msg, style);
            x += cols(&msg) + 2;
        }

        // Hints. A hint that would be clipped by the right edge is dropped
        // whole rather than cut mid-word, which is what a plain `set_string`
        // does — the bar used to end in things like "q qui".
        let key_style = styles::key_style().bg(theme::color_surface());
        let right = area.x + area.width;
        for hint in &self.hints {
            let needed = cols(&hint.key) + 1 + cols(&hint.desc);
            if x + needed > right {
                break;
            }
            buf.set_string(x, area.y, &hint.key, key_style);
            x += cols(&hint.key) + 1;
            buf.set_string(x, area.y, &hint.desc, bg);
            x += cols(&hint.desc) + 2;
        }
    }
}

/// Returns the hints for the connection selection screen.
///
/// These stay a literal list on purpose: `1`-`4`, `e` and `x` are matched directly
/// in `handle_connection_key`, not through [`crate::ui::keys::KeyMap`], so there is
/// no binding to ask. The browser hints, which DO have bindings, come from the
/// keymap instead — see `KeyMap::browser_hints`.
pub fn connection_hints() -> Vec<Hint> {
    hints(&[
        ("1-4", "protocol"),
        ("j/k", "navigate"),
        ("enter", "connect"),
        ("e", "edit"),
        ("x", "remove"),
        ("/", "filter"),
        ("?", "help"),
        ("q", "quit"),
    ])
}

fn hints(pairs: &[(&str, &str)]) -> Vec<Hint> {
    pairs
        .iter()
        .map(|(key, desc)| Hint {
            key: key.to_string(),
            desc: desc.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_hints_has_quit() {
        let hints = connection_hints();
        assert!(hints.iter().any(|h| h.key == "q"));
    }

    /// The bar is one row: whatever does not fit must be dropped whole.
    #[test]
    fn hints_are_never_cut_mid_word() {
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;

        let area = Rect::new(0, 0, 24, 1);
        let mut buf = Buffer::empty(area);
        let mut bar = StatusBar::new();
        bar.set_hints(connection_hints());
        bar.render(area, &mut buf);

        let row: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();
        // Every description present must be present in full.
        for hint in connection_hints() {
            if let Some(pos) = row.find(&hint.desc) {
                assert_eq!(
                    &row[pos..pos + hint.desc.len()],
                    hint.desc,
                    "truncated hint in {row:?}"
                );
            }
        }
        assert!(row.contains("protocol"), "first hint missing from {row:?}");
        assert!(
            !row.contains("quit"),
            "row should not have fitted quit: {row:?}"
        );
    }

    #[test]
    fn cols_counts_columns_not_bytes() {
        assert_eq!(cols("⟳ Connecting"), 12);
    }
}
