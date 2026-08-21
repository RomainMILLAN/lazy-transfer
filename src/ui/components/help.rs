use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Widget};

use super::hint::Hint;
use crate::ui::keys::{default_key_map, merged, KeyMap};
use crate::ui::style::{styles, theme};

/// HelpSection groups related keybindings.
pub struct HelpSection {
    pub title: String,
    pub bindings: Vec<Hint>,
}

/// HelpPopup displays a modal listing all keyboard shortcuts.
pub struct HelpPopup {
    visible: bool,
    scroll: usize,
}

impl Default for HelpPopup {
    fn default() -> Self {
        Self::new()
    }
}

impl HelpPopup {
    pub fn new() -> Self {
        HelpPopup {
            visible: false,
            scroll: 0,
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.scroll = 0;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if !self.visible {
            return;
        }

        match key.code {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => {
                self.visible = false;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll += 1;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll = self.scroll.saturating_sub(1);
            }
            _ => {}
        }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if !self.visible {
            return;
        }

        Clear.render(area, buf);

        let block = Block::default()
            .title(" Help [j/k: scroll] ")
            .title_style(styles::block_title_style(true))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(
                Style::default()
                    .fg(theme::color_primary())
                    .add_modifier(Modifier::BOLD),
            );
        let inner = block.inner(area);
        block.render(area, buf);

        let key_style = Style::default()
            .fg(theme::color_primary())
            .add_modifier(Modifier::BOLD);
        let desc_style = Style::default().fg(theme::color_bright());
        let section_style = Style::default()
            .fg(theme::color_info())
            .add_modifier(Modifier::BOLD);
        let title_style = Style::default()
            .fg(theme::color_bright())
            .add_modifier(Modifier::BOLD);
        let footer_style = Style::default().fg(theme::color_muted());

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(Span::styled("Keyboard Shortcuts", title_style)));
        lines.push(Line::raw(""));

        let sections = help_sections(&default_key_map());
        for (i, section) in sections.iter().enumerate() {
            if i > 0 {
                lines.push(Line::raw(""));
            }
            lines.push(Line::from(Span::styled(&section.title, section_style)));
            for binding in &section.bindings {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(format!("{:<12}", binding.key), key_style),
                    Span::styled(&binding.desc, desc_style),
                ]));
            }
        }

        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "Press ? or esc to close | j/k to scroll",
            footer_style,
        )));

        let visible_h = inner.height as usize;
        let max_scroll = lines.len().saturating_sub(visible_h);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }

        for (i, line) in lines.iter().skip(self.scroll).enumerate() {
            if i >= visible_h {
                break;
            }
            buf.set_line(
                inner.x + 1,
                inner.y + i as u16,
                line,
                inner.width.saturating_sub(2),
            );
        }

        if max_scroll > 0 {
            let indicator = format!(" {}/{} ", self.scroll + 1, lines.len());
            let ind_style = Style::default().fg(theme::color_muted());
            let x = inner.x + inner.width.saturating_sub(indicator.len() as u16 + 1);
            buf.set_string(x, area.y, &indicator, ind_style);
        }
    }

    pub fn view(&self) -> String {
        if !self.visible {
            return String::new();
        }

        let sections = help_sections(&default_key_map());
        let mut b = String::new();

        b.push_str("Keyboard Shortcuts\n\n");

        for (i, section) in sections.iter().enumerate() {
            if i > 0 {
                b.push('\n');
            }
            b.push_str(&section.title);
            b.push('\n');
            for binding in &section.bindings {
                b.push_str(&format!("  {:12} {}\n", binding.key, binding.desc));
            }
        }

        b.push_str("\nPress ? or esc to close");
        b
    }
}

/// The popup's contents, built from the bindings.
///
/// Nothing here writes a key out. The popup happened to be CORRECT while the
/// status bar lied, but for the same reason the bar was wrong: someone copied by
/// hand and got lucky. A second copy of the truth is a bug waiting for its turn.
fn help_sections(km: &KeyMap) -> Vec<HelpSection> {
    vec![
        HelpSection {
            title: "Connection screen".to_string(),
            bindings: vec![
                // `1`-`4`, `e` and `x` are matched directly in
                // `handle_connection_key`, not through the keymap, so they are the
                // one place a literal is still correct — see `connection_hints`.
                Hint::new("1-4", "protocol tab (SSH/SFTP/FTP/WebDAV)"),
                km.enter.hint_as("connect to the selected entry"),
                Hint::new("e", "edit a saved connection"),
                Hint::new("x", "remove a saved connection"),
                km.search.hint_as("filter the list"),
            ],
        },
        HelpSection {
            title: "Navigation".to_string(),
            bindings: vec![
                merged(&km.up, &km.down, "j/k/arrows", "navigate up/down"),
                km.switch_pane.hint_as("switch pane (local/remote)"),
                km.enter.hint_as("open directory"),
                km.back.hint_as("parent directory"),
                merged(&km.top, &km.bottom, "g/G", "go to top / bottom"),
                km.search.hint_as("search / filter"),
                km.escape.hint_as("clear the filter / cancel a dialog"),
                km.toggle_hidden.hint_as("toggle hidden files"),
                km.sort.hint_as("sort (name/size/date)"),
            ],
        },
        HelpSection {
            title: "File Operations".to_string(),
            bindings: vec![
                // One key, both directions: the focused pane decides which way it
                // goes, exactly as `start_copy` does.
                km.copy_file
                    .hint_as("copy — upload from the local pane, download from the remote one"),
                km.copy_tar
                    .hint_as("copy via tar (backends with shell execution only)"),
                km.delete.hint_as("delete file/directory"),
                km.rename.hint_as("rename file/directory"),
                km.mkdir.hint_as("create directory"),
            ],
        },
        HelpSection {
            title: "General".to_string(),
            bindings: vec![
                km.refresh.hint_as("refresh current panel"),
                km.toggle_theme.hint_as("toggle light/dark theme"),
                km.help.hint_as("show this help"),
                km.quit.hint_as("quit"),
            ],
        },
        HelpSection {
            title: "Mouse".to_string(),
            bindings: vec![
                Hint::new("click", "focus a panel (file browser only)"),
                Hint::new("scroll", "navigate up/down in lists"),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn show_hide() {
        let mut h = HelpPopup::new();
        assert!(!h.is_visible());
        h.show();
        assert!(h.is_visible());
        h.hide();
        assert!(!h.is_visible());
    }

    #[test]
    fn close_with_esc() {
        let mut h = HelpPopup::new();
        h.show();
        h.handle_key(key(KeyCode::Esc));
        assert!(!h.is_visible());
    }

    #[test]
    fn view_visible_has_sections() {
        let mut h = HelpPopup::new();
        h.show();
        let view = h.view();
        assert!(view.contains("Keyboard Shortcuts"));
        assert!(view.contains("Navigation"));
        assert!(view.contains("File Operations"));
        assert!(view.contains("General"));
    }

    #[test]
    fn help_sections_count() {
        assert_eq!(help_sections(&default_key_map()).len(), 5);
    }

    /// Same invariant as the status bar: nothing in the popup may name a key that
    /// is bound to nothing.
    ///
    /// The exemptions are named, never a wildcard. `click`/`scroll` are not keys;
    /// `1-4`, `e` and `x` are matched directly in `handle_connection_key` rather
    /// than through the keymap, so this test cannot reach them — which is the whole
    /// reason `connection_hints_are_all_handled` exists in `app`.
    #[test]
    fn every_help_key_is_bound() {
        let km = default_key_map();
        const NOT_KEYS: [&str; 5] = ["click", "scroll", "1-4", "e", "x"];
        for section in help_sections(&km) {
            for binding in section.bindings {
                if NOT_KEYS.contains(&binding.key.as_str()) {
                    continue;
                }
                assert!(
                    km.names_only_bound_keys(&binding.key),
                    "help advertises {:?} ({}) but nothing answers to it",
                    binding.key,
                    section.title
                );
            }
        }
    }

    /// The vim key for "parent directory" must stay visible. It is documented
    /// nowhere else, and the label it is derived from used to omit it.
    #[test]
    fn parent_directory_still_shows_the_vim_key() {
        let km = default_key_map();
        let nav = help_sections(&km)
            .into_iter()
            .find(|s| s.title == "Navigation")
            .expect("no Navigation section");
        let back = nav
            .bindings
            .iter()
            .find(|b| b.desc == "parent directory")
            .expect("no parent directory row");
        assert!(back.key.contains('h'), "h vanished from {:?}", back.key);
    }

    /// The connection screen has its own keys and the popup used to omit them.
    #[test]
    fn connection_screen_is_documented() {
        let sections = help_sections(&default_key_map());
        assert!(sections.iter().any(|s| s.title == "Connection screen"));
    }
}
