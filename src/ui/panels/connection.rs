use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Widget};

use crate::transfer::types::SshHost;
use crate::ui::style::theme;

/// ConnectionPanel shows available SSH hosts from ~/.ssh/config.
pub struct ConnectionPanel {
    pub hosts: Vec<SshHost>,
    filtered: Vec<usize>,
    pub filter: String,
    pub cursor: usize,
}

impl ConnectionPanel {
    pub fn new(hosts: Vec<SshHost>) -> Self {
        let filtered: Vec<usize> = (0..hosts.len()).collect();
        ConnectionPanel {
            hosts,
            filtered,
            filter: String::new(),
            cursor: 0,
        }
    }

    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_string();
        self.rebuild_filter();
    }

    pub fn clear_filter(&mut self) {
        self.set_filter("");
    }

    pub fn filter_push(&mut self, c: char) {
        self.filter.push(c);
        self.rebuild_filter();
    }

    pub fn filter_pop(&mut self) {
        self.filter.pop();
        self.rebuild_filter();
    }

    fn rebuild_filter(&mut self) {
        if self.filter.is_empty() {
            self.filtered = (0..self.hosts.len()).collect();
        } else {
            let matcher = SkimMatcherV2::default();
            let mut scored: Vec<(usize, i64)> = self
                .hosts
                .iter()
                .enumerate()
                .filter_map(|(i, h)| {
                    let haystack = format!("{} {} {}", h.alias, h.hostname, h.user);
                    matcher
                        .fuzzy_match(&haystack, &self.filter)
                        .map(|score| (i, score))
                })
                .collect();
            scored.sort_by(|a, b| b.1.cmp(&a.1));
            self.filtered = scored.into_iter().map(|(i, _)| i).collect();
        }

        // +1 for the "Manual connection" entry
        let count = self.total_entries();
        if self.cursor >= count && count > 0 {
            self.cursor = count - 1;
        }
    }

    /// Total visible entries (filtered hosts + "Manual connection").
    fn total_entries(&self) -> usize {
        self.filtered.len() + 1
    }

    /// Returns true if the cursor is on the "Manual connection" entry.
    pub fn is_manual_selected(&self) -> bool {
        self.cursor == self.filtered.len()
    }

    /// Returns the selected SshHost, or None if "Manual connection" is selected.
    pub fn selected_host(&self) -> Option<&SshHost> {
        if self.is_manual_selected() {
            None
        } else {
            self.filtered
                .get(self.cursor)
                .and_then(|&i| self.hosts.get(i))
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let count = self.total_entries();
        if count > 0 && self.cursor < count - 1 {
            self.cursor += 1;
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, loading: bool) {
        let border_color = theme::color_border_focus();

        let filter_text = if self.filter.is_empty() {
            String::new()
        } else {
            format!(" /{}", self.filter)
        };
        let count_text = format!("{}", self.hosts.len());

        let block = Block::default()
            .title(format!(" SSH Connections [{}]{} ", count_text, filter_text))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        let inner = block.inner(area);
        block.render(area, buf);

        if loading {
            let style = Style::default().fg(theme::color_primary());
            buf.set_string(inner.x + 1, inner.y, "Loading...", style);
            return;
        }

        if self.hosts.is_empty() && self.filter.is_empty() {
            let style = Style::default().fg(theme::color_muted());
            buf.set_string(
                inner.x + 1,
                inner.y,
                "No SSH hosts found in ~/.ssh/config",
                style,
            );
        }

        let visible_h = inner.height as usize;
        let total = self.total_entries();
        let offset = if self.cursor >= visible_h {
            self.cursor - visible_h + 1
        } else {
            0
        };

        let mut y = 0;

        // Render filtered hosts
        for (display_idx, &host_idx) in self.filtered.iter().enumerate() {
            if display_idx < offset {
                continue;
            }
            if y >= visible_h {
                break;
            }

            let host = &self.hosts[host_idx];
            let is_selected = display_idx == self.cursor;

            let info = if host.user.is_empty() {
                format!("{}:{}", host.hostname, host.port)
            } else {
                format!("{}@{}:{}", host.user, host.hostname, host.port)
            };

            let style = if is_selected {
                Style::default()
                    .fg(theme::color_bright())
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(theme::color_text())
            };

            let alias_style = if is_selected {
                style
            } else {
                Style::default()
                    .fg(theme::color_primary())
                    .add_modifier(Modifier::BOLD)
            };

            let line = Line::from(vec![
                Span::styled(format!(" {:<20}", host.alias), alias_style),
                Span::styled(format!(" {}", info), style),
            ]);
            buf.set_line(inner.x, inner.y + y as u16, &line, inner.width);
            y += 1;
        }

        // Separator
        if y < visible_h && !self.filtered.is_empty() {
            let sep_style = Style::default().fg(theme::color_muted());
            let sep = "─".repeat(inner.width.saturating_sub(2) as usize);
            buf.set_string(inner.x + 1, inner.y + y as u16, &sep, sep_style);
            y += 1;
        }

        // "Manual connection" entry
        if y < visible_h && total > offset + y {
            let is_selected = self.is_manual_selected();
            let style = if is_selected {
                Style::default()
                    .fg(theme::color_bright())
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(theme::color_info())
            };

            let line = Line::from(Span::styled(" [+] Manual connection...", style));
            buf.set_line(inner.x, inner.y + y as u16, &line, inner.width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_hosts() -> Vec<SshHost> {
        vec![
            SshHost {
                alias: "myserver".to_string(),
                hostname: "10.0.0.1".to_string(),
                user: "admin".to_string(),
                port: 22,
                identity_file: String::new(),
            },
            SshHost {
                alias: "production".to_string(),
                hostname: "prod.example.com".to_string(),
                user: "deploy".to_string(),
                port: 22,
                identity_file: String::new(),
            },
        ]
    }

    #[test]
    fn initial_state() {
        let panel = ConnectionPanel::new(sample_hosts());
        assert_eq!(panel.cursor, 0);
        assert!(!panel.is_manual_selected());
        assert!(panel.selected_host().is_some());
    }

    #[test]
    fn navigate_to_manual() {
        let mut panel = ConnectionPanel::new(sample_hosts());
        panel.move_down(); // host 1
        panel.move_down(); // manual
        assert!(panel.is_manual_selected());
        assert!(panel.selected_host().is_none());
    }

    #[test]
    fn filter_hosts() {
        let mut panel = ConnectionPanel::new(sample_hosts());
        panel.set_filter("prod");
        assert_eq!(panel.filtered.len(), 1);
    }

    #[test]
    fn empty_hosts() {
        let panel = ConnectionPanel::new(vec![]);
        assert!(panel.is_manual_selected());
    }
}
