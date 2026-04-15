use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Widget};

use crate::transfer::connections::SavedConnection;
use crate::transfer::types::{Protocol, SshHost};
use crate::ui::style::theme;

/// An entry in the connection list (either SSH host, saved connection, or manual).
#[derive(Debug, Clone)]
pub enum ConnectionEntry {
    SshHost(usize), // index into ssh_hosts
    Saved(usize),   // index into saved_connections
    Manual,
}

/// ConnectionPanel shows available connections with protocol tabs.
pub struct ConnectionPanel {
    pub ssh_hosts: Vec<SshHost>,
    pub saved_connections: Vec<SavedConnection>,
    pub selected_protocol: Protocol,
    entries: Vec<ConnectionEntry>,
    filtered: Vec<usize>,
    pub filter: String,
    pub cursor: usize,
}

impl ConnectionPanel {
    pub fn new(ssh_hosts: Vec<SshHost>, saved_connections: Vec<SavedConnection>) -> Self {
        let mut panel = ConnectionPanel {
            ssh_hosts,
            saved_connections,
            selected_protocol: Protocol::Ssh,
            entries: Vec::new(),
            filtered: Vec::new(),
            filter: String::new(),
            cursor: 0,
        };
        panel.rebuild_entries();
        panel
    }

    pub fn select_protocol(&mut self, protocol: Protocol) {
        if self.selected_protocol != protocol {
            self.selected_protocol = protocol;
            self.filter.clear();
            self.cursor = 0;
            self.rebuild_entries();
        }
    }

    fn rebuild_entries(&mut self) {
        self.entries.clear();

        match self.selected_protocol {
            Protocol::Ssh => {
                // SSH config hosts
                for i in 0..self.ssh_hosts.len() {
                    self.entries.push(ConnectionEntry::SshHost(i));
                }
                // Saved SSH connections
                for (i, saved) in self.saved_connections.iter().enumerate() {
                    if saved.matches_protocol(&Protocol::Ssh) {
                        self.entries.push(ConnectionEntry::Saved(i));
                    }
                }
            }
            Protocol::Sftp => {
                for (i, saved) in self.saved_connections.iter().enumerate() {
                    if saved.matches_protocol(&Protocol::Sftp) {
                        self.entries.push(ConnectionEntry::Saved(i));
                    }
                }
            }
            Protocol::Ftp => {
                for (i, saved) in self.saved_connections.iter().enumerate() {
                    if saved.matches_protocol(&Protocol::Ftp) {
                        self.entries.push(ConnectionEntry::Saved(i));
                    }
                }
            }
        }

        // Always add Manual at the end
        self.entries.push(ConnectionEntry::Manual);
        self.rebuild_filter();
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
            self.filtered = (0..self.entries.len()).collect();
        } else {
            let matcher = SkimMatcherV2::default();
            let mut scored: Vec<(usize, i64)> = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(i, entry)| {
                    let haystack = match entry {
                        ConnectionEntry::SshHost(idx) => {
                            let h = &self.ssh_hosts[*idx];
                            format!("{} {} {}", h.alias, h.hostname, h.user)
                        }
                        ConnectionEntry::Saved(idx) => {
                            let s = &self.saved_connections[*idx];
                            format!("{} {} {}", s.name, s.host, s.user)
                        }
                        ConnectionEntry::Manual => return None, // Always show manual
                    };
                    matcher
                        .fuzzy_match(&haystack, &self.filter)
                        .map(|score| (i, score))
                })
                .collect();
            scored.sort_by(|a, b| b.1.cmp(&a.1));
            self.filtered = scored.into_iter().map(|(i, _)| i).collect();
            // Always include Manual at the end
            let manual_idx = self.entries.len() - 1;
            if !self.filtered.contains(&manual_idx) {
                self.filtered.push(manual_idx);
            }
        }

        let count = self.filtered.len();
        if self.cursor >= count && count > 0 {
            self.cursor = count - 1;
        }
    }

    pub fn is_manual_selected(&self) -> bool {
        self.filtered
            .get(self.cursor)
            .map(|&i| matches!(self.entries.get(i), Some(ConnectionEntry::Manual)))
            .unwrap_or(false)
    }

    /// Returns the selected SSH host, if applicable.
    pub fn selected_ssh_host(&self) -> Option<&SshHost> {
        let &entry_idx = self.filtered.get(self.cursor)?;
        match &self.entries[entry_idx] {
            ConnectionEntry::SshHost(idx) => self.ssh_hosts.get(*idx),
            _ => None,
        }
    }

    /// Returns the selected saved connection, if applicable.
    pub fn selected_saved(&self) -> Option<&SavedConnection> {
        let &entry_idx = self.filtered.get(self.cursor)?;
        match &self.entries[entry_idx] {
            ConnectionEntry::Saved(idx) => self.saved_connections.get(*idx),
            _ => None,
        }
    }

    /// Returns the index of the selected saved connection (for deletion).
    pub fn selected_saved_index(&self) -> Option<usize> {
        let &entry_idx = self.filtered.get(self.cursor)?;
        match &self.entries[entry_idx] {
            ConnectionEntry::Saved(idx) => Some(*idx),
            _ => None,
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let count = self.filtered.len();
        if count > 0 && self.cursor < count - 1 {
            self.cursor += 1;
        }
    }

    /// Reload saved connections from disk.
    pub fn reload_saved(&mut self) {
        self.saved_connections = crate::transfer::connections::load().entries;
        self.rebuild_entries();
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, loading: bool, filtering: bool) {
        let border_color = theme::color_border_focus();

        let block = Block::default()
            .title(" Connections ".to_string())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height < 2 {
            return;
        }

        // Render tab strip on first line
        let tab_y = inner.y;
        self.render_tabs(inner.x, tab_y, inner.width, buf);

        let content_y = tab_y + 1;
        let content_h = inner.height.saturating_sub(1) as usize;

        // Filter indicator
        if filtering || !self.filter.is_empty() {
            let filter_text = if self.filter.is_empty() {
                " /".to_string()
            } else if filtering {
                format!(" /{}█", self.filter)
            } else {
                format!(" /{}", self.filter)
            };
            let filter_style = Style::default().fg(theme::color_warning());
            let fx = inner.x + inner.width.saturating_sub(filter_text.len() as u16 + 1);
            buf.set_string(fx, tab_y, &filter_text, filter_style);
        }

        if loading {
            let style = Style::default().fg(theme::color_primary());
            buf.set_string(inner.x + 1, content_y, "Connecting...", style);
            return;
        }

        if self.filtered.is_empty() {
            let style = Style::default().fg(theme::color_muted());
            buf.set_string(inner.x + 1, content_y, "No connections found", style);
            return;
        }

        let offset = if self.cursor >= content_h {
            self.cursor - content_h + 1
        } else {
            0
        };

        let mut y = 0usize;
        for (display_idx, &entry_idx) in self.filtered.iter().enumerate() {
            if display_idx < offset {
                continue;
            }
            if y >= content_h {
                break;
            }

            let is_selected = display_idx == self.cursor;
            let entry = &self.entries[entry_idx];

            match entry {
                ConnectionEntry::SshHost(idx) => {
                    let host = &self.ssh_hosts[*idx];
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
                    buf.set_line(inner.x, content_y + y as u16, &line, inner.width);
                }
                ConnectionEntry::Saved(idx) => {
                    let saved = &self.saved_connections[*idx];
                    let info = format!("{}@{}:{}", saved.user, saved.host, saved.port);
                    let badge = format!("[{}]", saved.protocol.to_uppercase());

                    let style = if is_selected {
                        Style::default()
                            .fg(theme::color_bright())
                            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
                    } else {
                        Style::default().fg(theme::color_text())
                    };
                    let name_style = if is_selected {
                        style
                    } else {
                        Style::default()
                            .fg(theme::color_success())
                            .add_modifier(Modifier::BOLD)
                    };

                    let line = Line::from(vec![
                        Span::styled(format!(" {:<20}", saved.name), name_style),
                        Span::styled(format!(" {} ", info), style),
                        Span::styled(badge, Style::default().fg(theme::color_muted())),
                    ]);
                    buf.set_line(inner.x, content_y + y as u16, &line, inner.width);
                }
                ConnectionEntry::Manual => {
                    let style = if is_selected {
                        Style::default()
                            .fg(theme::color_bright())
                            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
                    } else {
                        Style::default().fg(theme::color_info())
                    };
                    let line = Line::from(Span::styled(" [+] Manual connection...", style));
                    buf.set_line(inner.x, content_y + y as u16, &line, inner.width);
                }
            }

            y += 1;
        }
    }

    fn render_tabs(&self, x: u16, y: u16, width: u16, buf: &mut Buffer) {
        let tabs = [
            ("1:SSH", Protocol::Ssh),
            ("2:SFTP", Protocol::Sftp),
            ("3:FTP", Protocol::Ftp),
        ];

        let mut tx = x + 1;
        for (label, proto) in &tabs {
            let is_active = *proto == self.selected_protocol;
            let style = if is_active {
                Style::default()
                    .fg(theme::color_primary())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::color_muted())
            };

            let text = if is_active {
                format!("[{}]", label)
            } else {
                format!(" {} ", label)
            };

            if tx + text.len() as u16 > x + width {
                break;
            }
            buf.set_string(tx, y, &text, style);
            tx += text.len() as u16 + 1;
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
        let panel = ConnectionPanel::new(sample_hosts(), vec![]);
        assert_eq!(panel.cursor, 0);
        assert!(!panel.is_manual_selected());
        assert!(panel.selected_ssh_host().is_some());
    }

    #[test]
    fn navigate_to_manual() {
        let mut panel = ConnectionPanel::new(sample_hosts(), vec![]);
        panel.move_down(); // host 1
        panel.move_down(); // manual
        assert!(panel.is_manual_selected());
        assert!(panel.selected_ssh_host().is_none());
    }

    #[test]
    fn filter_hosts() {
        let mut panel = ConnectionPanel::new(sample_hosts(), vec![]);
        panel.set_filter("prod");
        // Should have 1 filtered host + Manual
        assert_eq!(panel.filtered.len(), 2);
    }

    #[test]
    fn empty_hosts() {
        let panel = ConnectionPanel::new(vec![], vec![]);
        assert!(panel.is_manual_selected());
    }

    #[test]
    fn switch_protocol() {
        let mut panel = ConnectionPanel::new(sample_hosts(), vec![]);
        assert_eq!(panel.selected_protocol, Protocol::Ssh);

        panel.select_protocol(Protocol::Ftp);
        assert_eq!(panel.selected_protocol, Protocol::Ftp);
        // FTP tab with no saved connections: only Manual entry
        assert_eq!(panel.entries.len(), 1);
        assert!(panel.is_manual_selected());
    }

    #[test]
    fn saved_connections_shown() {
        let saved = vec![SavedConnection {
            name: "My FTP".to_string(),
            protocol: "ftp".to_string(),
            host: "ftp.example.com".to_string(),
            user: "admin".to_string(),
            port: 21,
            auth_method: "password".to_string(),
            identity_file: None,
            password: None,
        }];
        let mut panel = ConnectionPanel::new(vec![], saved);
        panel.select_protocol(Protocol::Ftp);
        // 1 saved FTP + Manual
        assert_eq!(panel.entries.len(), 2);
        assert!(panel.selected_saved().is_some());
    }
}
