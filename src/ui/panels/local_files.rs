use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Widget};
use std::fs;
use std::path::Path;

use crate::transfer::types::{FileEntry, SortColumn, SortOrder};
use crate::ui::style::{styles, theme};
use crate::ui::text::format_size;

/// LocalFilesPanel displays local filesystem contents.
pub struct LocalFilesPanel {
    pub current_dir: String,
    pub files: Vec<FileEntry>,
    filtered: Vec<usize>,
    pub filter: String,
    pub cursor: usize,
    pub show_hidden: bool,
    pub sort_column: SortColumn,
    pub sort_order: SortOrder,
}

impl LocalFilesPanel {
    pub fn new(start_dir: &str) -> Self {
        let mut panel = LocalFilesPanel {
            current_dir: start_dir.to_string(),
            files: Vec::new(),
            filtered: Vec::new(),
            filter: String::new(),
            cursor: 0,
            show_hidden: false,
            sort_column: SortColumn::Name,
            sort_order: SortOrder::Asc,
        };
        panel.load_dir();
        panel
    }

    /// Load the current directory contents from the local filesystem.
    pub fn load_dir(&mut self) {
        let prev_cursor = self.cursor;
        self.files.clear();
        self.filter.clear();

        let path = Path::new(&self.current_dir);

        // Add ".." unless at root
        if path.parent().is_some() {
            self.files.push(FileEntry {
                name: "..".to_string(),
                is_dir: true,
                size: 0,
                modified: String::new(),
                permissions: String::new(),
            });
        }

        if let Ok(entries) = fs::read_dir(path) {
            let mut dirs: Vec<FileEntry> = Vec::new();
            let mut files: Vec<FileEntry> = Vec::new();

            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let metadata = entry.metadata().ok();
                let is_dir = metadata.as_ref().is_some_and(|m| m.is_dir());
                let size = metadata.as_ref().map_or(0, |m| m.len());
                let modified = metadata
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .map(|t| {
                        let datetime: chrono::DateTime<chrono::Local> = t.into();
                        datetime.format("%Y-%m-%d %H:%M").to_string()
                    })
                    .unwrap_or_default();

                let entry = FileEntry {
                    name,
                    is_dir,
                    size,
                    modified,
                    permissions: String::new(),
                };

                if is_dir {
                    dirs.push(entry);
                } else {
                    files.push(entry);
                }
            }

            self.files.extend(dirs);
            self.files.extend(files);
        }

        self.rebuild_filter();
        // Restore cursor position, clamped to new list size
        let count = self.filtered.len();
        self.cursor = if count == 0 {
            0
        } else {
            prev_cursor.min(count - 1)
        };
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

    /// Cycle sort: same column toggles ASC/DESC, different column switches to it ASC.
    pub fn cycle_sort(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_order = self.sort_order.toggle();
        } else {
            self.sort_column = column;
            self.sort_order = SortOrder::Asc;
        }
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
            scored.sort_by_key(|b| std::cmp::Reverse(b.1));
            self.filtered = scored.into_iter().map(|(i, _)| i).collect();
        }

        // Apply column sort (dirs first, ".." always at top)
        let files = &self.files;
        let col = self.sort_column;
        let ord = self.sort_order;
        self.filtered.sort_by(|&a, &b| {
            let fa = &files[a];
            let fb = &files[b];
            // ".." always first
            if fa.name == ".." {
                return std::cmp::Ordering::Less;
            }
            if fb.name == ".." {
                return std::cmp::Ordering::Greater;
            }
            // Dirs before files
            if fa.is_dir != fb.is_dir {
                return if fa.is_dir {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                };
            }
            // Sort within same type
            let cmp = match col {
                SortColumn::Name => fa.name.to_lowercase().cmp(&fb.name.to_lowercase()),
                SortColumn::Size => fa.size.cmp(&fb.size),
                SortColumn::Date => fa.modified.cmp(&fb.modified),
            };
            match ord {
                SortOrder::Asc => cmp,
                SortOrder::Desc => cmp.reverse(),
            }
        });

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

    /// Enter the selected directory or go to parent.
    pub fn enter_selected(&mut self) {
        if let Some(entry) = self.selected() {
            if entry.is_dir {
                if entry.name == ".." {
                    self.go_parent();
                } else {
                    let new_dir = format!("{}/{}", self.current_dir, entry.name);
                    if let Ok(canonical) = std::fs::canonicalize(&new_dir) {
                        self.current_dir = canonical.to_string_lossy().to_string();
                        self.load_dir();
                    }
                }
            }
        }
    }

    pub fn go_parent(&mut self) {
        if let Some(parent) = Path::new(&self.current_dir).parent() {
            self.current_dir = parent.to_string_lossy().to_string();
            self.load_dir();
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, is_active: bool, filtering: bool) {
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

        let sort_text = format!(" {}{}", self.sort_column.label(), self.sort_order.arrow());
        let block = Block::default()
            .title(format!(
                " Local ({}) [{}]{}{} ",
                self.current_dir, count_text, sort_text, filter_text
            ))
            .title_style(styles::block_title_style(is_active))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color));
        let inner = block.inner(area);
        block.render(area, buf);

        if self.filtered.is_empty() {
            let style = Style::default().fg(theme::color_muted());
            buf.set_string(
                inner.x + 1,
                inner.y,
                if self.filter.is_empty() {
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
            let date_str = &entry.modified;

            // Layout: " d name          8.0 KB  2024-01-15 10:30"
            // Reserve: 3 (icon) + 10 (size) + 18 (date) + 2 (gaps) = 33
            let name_w = inner.width.saturating_sub(33) as usize;
            let name_display = if entry.name.len() > name_w {
                format!("{:.width$}", entry.name, width = name_w)
            } else {
                entry.name.clone()
            };

            let style = if is_selected {
                styles::selected_style(is_active)
            } else {
                styles::description_style()
            };

            let icon_style = if entry.is_dir && !is_selected {
                styles::directory_style()
            } else {
                style
            };

            let date_style = if is_selected {
                style
            } else {
                styles::muted_style()
            };

            let mut spans = vec![
                Span::styled(format!(" {} ", icon), icon_style),
                Span::styled(format!("{:<width$}", name_display, width = name_w), style),
                Span::styled(format!(" {:>8}", size_str), style),
                Span::styled(format!("  {:>16}", date_str), date_style),
            ];
            // `set_line` does not pad, and a selected row is painted with a
            // background: without this the highlight stops mid-row.
            if is_selected {
                let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
                let pad = (inner.width as usize).saturating_sub(used);
                if pad > 0 {
                    spans.push(Span::styled(" ".repeat(pad), style));
                }
            }
            let line = Line::from(spans);
            buf.set_line(inner.x, y, &line, inner.width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_current_dir() {
        let panel = LocalFilesPanel::new(".");
        assert!(!panel.files.is_empty());
    }

    #[test]
    fn navigation() {
        let mut panel = LocalFilesPanel::new(".");
        let initial = panel.cursor;
        panel.move_down();
        if panel.filtered.len() > 1 {
            assert_eq!(panel.cursor, initial + 1);
        }
        panel.go_to_top();
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn filter_files() {
        let mut panel = LocalFilesPanel::new(".");
        let total = panel.filtered.len();
        panel.set_filter("ZZZZNONEXISTENT");
        assert!(panel.filtered.len() < total || total == 0);
    }
}
