use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Widget};

use super::statusbar::Hint;
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

        let sections = help_sections();
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

        let sections = help_sections();
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

fn help_sections() -> Vec<HelpSection> {
    vec![
        HelpSection {
            title: "Navigation".to_string(),
            bindings: vec![
                Hint {
                    key: "j/k / arrows".to_string(),
                    desc: "navigate up/down".to_string(),
                },
                Hint {
                    key: "tab".to_string(),
                    desc: "switch pane (local/remote)".to_string(),
                },
                Hint {
                    key: "enter".to_string(),
                    desc: "open directory".to_string(),
                },
                Hint {
                    key: "backspace/h".to_string(),
                    desc: "parent directory".to_string(),
                },
                Hint {
                    key: "g / G".to_string(),
                    desc: "go to top / bottom".to_string(),
                },
                Hint {
                    key: "/".to_string(),
                    desc: "search / filter".to_string(),
                },
                Hint {
                    key: ".".to_string(),
                    desc: "toggle hidden files".to_string(),
                },
                Hint {
                    key: "s".to_string(),
                    desc: "sort (name/size/date)".to_string(),
                },
            ],
        },
        HelpSection {
            title: "File Operations".to_string(),
            bindings: vec![
                Hint {
                    key: "c".to_string(),
                    desc: "copy file (upload or download)".to_string(),
                },
                Hint {
                    key: "C".to_string(),
                    desc: "copy file via tar (SSH only, compresses)".to_string(),
                },
                Hint {
                    key: "d".to_string(),
                    desc: "delete file/directory".to_string(),
                },
                Hint {
                    key: "r".to_string(),
                    desc: "rename file/directory".to_string(),
                },
                Hint {
                    key: "m".to_string(),
                    desc: "create directory (mkdir)".to_string(),
                },
            ],
        },
        HelpSection {
            title: "General".to_string(),
            bindings: vec![
                Hint {
                    key: "R".to_string(),
                    desc: "refresh current panel".to_string(),
                },
                Hint {
                    key: "Ctrl+L".to_string(),
                    desc: "toggle light/dark theme".to_string(),
                },
                Hint {
                    key: "?".to_string(),
                    desc: "show this help".to_string(),
                },
                Hint {
                    key: "q".to_string(),
                    desc: "quit".to_string(),
                },
            ],
        },
        HelpSection {
            title: "Mouse".to_string(),
            bindings: vec![
                Hint {
                    key: "click".to_string(),
                    desc: "focus panel".to_string(),
                },
                Hint {
                    key: "scroll".to_string(),
                    desc: "navigate up/down in lists".to_string(),
                },
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
        assert_eq!(help_sections().len(), 4);
    }
}
