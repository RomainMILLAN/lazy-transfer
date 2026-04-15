use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Widget};

use crate::transfer::types::FileEntry;
use crate::ui::style::theme;
use crate::ui::text::format_size;

/// RemoteFilesPanel displays remote filesystem contents.
pub struct RemoteFilesPanel {
    pub current_dir: String,
    pub files: Vec<FileEntry>,
    filtered: Vec<usize>,
    pub filter: String,
    pub cursor: usize,
    pub show_hidden: bool,
}

impl RemoteFilesPanel {
    pub fn new() -> Self {
        RemoteFilesPanel {
            current_dir: String::new(),
            files: Vec::new(),
            filtered: Vec::new(),
            filter: String::new(),
            cursor: 0,
            show_hidden: false,
        }
    }

    /// Set remote files from a background load result.
    pub fn set_files(&mut self, files: Vec<FileEntry>) {
        let prev_cursor = self.cursor;
        self.files = files;
        self.rebuild_filter();
        // Restore cursor position, clamped to new list size
        let count = self.filtered.len();
        self.cursor = if count == 0 { 0 } else { prev_cursor.min(count - 1) };
    }

    pub fn set_dir(&mut self, dir: &str) {
        self.current_dir = dir.to_string();
    }

    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_string();
        self.rebuild_filter();
    }

    pub fn clear_filter(&mut self) {
        self.set_filter("");
    }

    pub fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.rebuild_filter();
    }

    /// Append a character to the filter (inline fzf mode).
    pub fn filter_push(&mut self, c: char) {
        self.filter.push(c);
        self.rebuild_filter();
    }

    /// Remove last character from the filter (inline fzf mode).
    pub fn filter_pop(&mut self) {
        self.filter.pop();
        self.rebuild_filter();
    }

    fn rebuild_filter(&mut self) {
        let hide_dot = !self.show_hidden;
        if self.filter.is_empty() {
            self.filtered = self
                .files
                .iter()
                .enumerate()
                .filter(|(_, f)| !hide_dot || f.name == ".." || !f.name.starts_with('.'))
                .map(|(i, _)| i)
                .collect();
        } else {
            let matcher = SkimMatcherV2::default();
            let mut scored: Vec<(usize, i64)> = self
                .files
                .iter()
                .enumerate()
                .filter(|(_, f)| !hide_dot || f.name == ".." || !f.name.starts_with('.'))
                .filter_map(|(i, f)| {
                    matcher
                        .fuzzy_match(&f.name, &self.filter)
                        .map(|score| (i, score))
                })
                .collect();
            scored.sort_by(|a, b| b.1.cmp(&a.1));
            self.filtered = scored.into_iter().map(|(i, _)| i).collect();
        }

        let count = self.filtered.len();
        if self.cursor >= count && count > 0 {
            self.cursor = count - 1;
        } else if count == 0 {
            self.cursor = 0;
        }
    }

    pub fn selected(&self) -> Option<&FileEntry> {
        self.filtered
            .get(self.cursor)
            .and_then(|&i| self.files.get(i))
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

    pub fn go_to_top(&mut self) {
        self.cursor = 0;
    }

    pub fn go_to_bottom(&mut self) {
        let count = self.filtered.len();
        if count > 0 {
            self.cursor = count - 1;
        }
    }

    /// Compute the path to navigate into for the selected entry.
    /// Returns None if no directory is selected.
    pub fn enter_path(&self) -> Option<String> {
        if let Some(entry) = self.selected() {
            if entry.is_dir {
                if entry.name == ".." {
                    return self.parent_path();
                } else {
                    let new_dir = if self.current_dir.ends_with('/') {
                        format!("{}{}", self.current_dir, entry.name)
                    } else {
                        format!("{}/{}", self.current_dir, entry.name)
                    };
                    return Some(new_dir);
                }
            }
        }
        None
    }

    pub fn parent_path(&self) -> Option<String> {
        if self.current_dir == "/" {
            return None;
        }
        let trimmed = self.current_dir.trim_end_matches('/');
        if let Some(pos) = trimmed.rfind('/') {
            if pos == 0 {
                Some("/".to_string())
            } else {
                Some(trimmed[..pos].to_string())
            }
        } else {
            None
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, is_active: bool, loading: bool, filtering: bool) {
        let border_color = if filtering && is_active {
            theme::color_warning()
        } else if is_active {
            theme::color_border_focus()
        } else {
            theme::color_border()
        };

        let filter_text = if self.filter.is_empty() {
            if filtering && is_active {
                " /".to_string()
            } else {
                String::new()
            }
        } else if filtering && is_active {
            format!(" /{}█", self.filter)
        } else {
            format!(" /{}", self.filter)
        };
        let count_text = if self.filter.is_empty() {
            format!("{}", self.filtered.len())
        } else {
            format!("{}/{}", self.filtered.len(), self.files.len())
        };

        let title = if self.current_dir.is_empty() {
            format!(" Remote [{}]{} ", count_text, filter_text)
        } else {
            format!(
                " Remote ({}) [{}]{} ",
                self.current_dir, count_text, filter_text
            )
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        let inner = block.inner(area);
        block.render(area, buf);

        if loading {
            let style = Style::default().fg(theme::color_primary());
            buf.set_string(inner.x + 1, inner.y, "Loading...", style);
            return;
        }

        if self.files.is_empty() {
            let style = Style::default().fg(theme::color_muted());
            buf.set_string(
                inner.x + 1,
                inner.y,
                if self.current_dir.is_empty() {
                    "Not connected"
                } else if self.filter.is_empty() {
                    "Empty directory"
                } else {
                    "No match"
                },
                style,
            );
            return;
        }

        let visible_h = inner.height as usize;
        let offset = if self.cursor >= visible_h {
            self.cursor - visible_h + 1
        } else {
            0
        };

        let items: Vec<&FileEntry> = self
            .filtered
            .iter()
            .filter_map(|&i| self.files.get(i))
            .collect();

        for (i, entry) in items.iter().skip(offset).enumerate() {
            if i >= visible_h {
                break;
            }

            let y = inner.y + i as u16;
            let is_selected = (i + offset) == self.cursor;

            let icon = if entry.is_dir { "d" } else { "-" };
            let size_str = if entry.is_dir {
                String::new()
            } else {
                format_size(entry.size)
            };

            let name_w = inner.width.saturating_sub(22) as usize;
            let name_display = if entry.name.len() > name_w {
                format!("{:.width$}", entry.name, width = name_w)
            } else {
                entry.name.clone()
            };

            let style = if is_selected && is_active {
                Style::default()
                    .fg(theme::color_bright())
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else if is_selected {
                Style::default()
                    .fg(theme::color_text())
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(theme::color_text())
            };

            let icon_style = if entry.is_dir && !is_selected {
                Style::default()
                    .fg(theme::color_info())
                    .add_modifier(Modifier::BOLD)
            } else {
                style
            };

            let line = Line::from(vec![
                Span::styled(format!(" {} ", icon), icon_style),
                Span::styled(format!("{:<width$}", name_display, width = name_w), style),
                Span::styled(format!(" {:>8}", size_str), style),
            ]);
            buf.set_line(inner.x, y, &line, inner.width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let panel = RemoteFilesPanel::new();
        assert!(panel.current_dir.is_empty());
        assert!(panel.files.is_empty());
    }

    #[test]
    fn set_files_resets_cursor() {
        let mut panel = RemoteFilesPanel::new();
        panel.cursor = 5;
        panel.set_files(vec![FileEntry {
            name: "test".to_string(),
            is_dir: false,
            size: 100,
            modified: String::new(),
            permissions: String::new(),
        }]);
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn parent_path_from_nested() {
        let mut panel = RemoteFilesPanel::new();
        panel.set_dir("/home/user/documents");
        assert_eq!(panel.parent_path(), Some("/home/user".to_string()));
    }

    #[test]
    fn parent_path_from_root() {
        let mut panel = RemoteFilesPanel::new();
        panel.set_dir("/");
        assert_eq!(panel.parent_path(), None);
    }

    #[test]
    fn parent_path_to_root() {
        let mut panel = RemoteFilesPanel::new();
        panel.set_dir("/home");
        assert_eq!(panel.parent_path(), Some("/".to_string()));
    }
}
