use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Widget};

use crate::transfer::types::{TransferDirection, TransferJob, TransferStatus};
use crate::ui::style::theme;

/// TransfersPanel shows the transfer queue with progress bars.
pub struct TransfersPanel {
    pub jobs: Vec<TransferJob>,
    pub cursor: usize,
    scroll: usize,
}

impl Default for TransfersPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl TransfersPanel {
    pub fn new() -> Self {
        TransfersPanel {
            jobs: Vec::new(),
            cursor: 0,
            scroll: 0,
        }
    }

    pub fn has_active_transfers(&self) -> bool {
        self.jobs.iter().any(|j| {
            matches!(
                j.status,
                TransferStatus::Queued | TransferStatus::InProgress { .. }
            )
        })
    }

    fn active_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| {
                matches!(
                    j.status,
                    TransferStatus::Queued | TransferStatus::InProgress { .. }
                )
            })
            .count()
    }

    /// Auto-scroll to keep the most recent active transfer visible.
    fn auto_scroll(&mut self, visible_h: usize) {
        if self.jobs.is_empty() || visible_h == 0 {
            self.scroll = 0;
            return;
        }
        // Show the latest jobs (bottom of the list)
        let total = self.jobs.len();
        if total > visible_h {
            self.scroll = total - visible_h;
        } else {
            self.scroll = 0;
        }
    }

    pub fn update_progress(&mut self, job_id: usize, percent: u8, speed: String) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id) {
            job.status = TransferStatus::InProgress { percent, speed };
        }
    }

    pub fn complete_job(&mut self, job_id: usize) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id) {
            job.status = TransferStatus::Completed;
        }
    }

    pub fn fail_job(&mut self, job_id: usize, error: String) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id) {
            job.status = TransferStatus::Failed(error);
        }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let active = self.active_count();
        let total = self.jobs.len();
        let title = if active > 0 {
            format!(" Transfers [{} active / {} total] ", active, total)
        } else if total > 0 {
            format!(" Transfers [{} done] ", total)
        } else {
            " Transfers ".to_string()
        };

        let border_color = if active > 0 {
            theme::color_primary()
        } else {
            theme::color_border()
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        let inner = block.inner(area);
        block.render(area, buf);

        if self.jobs.is_empty() {
            let style = Style::default().fg(theme::color_muted());
            buf.set_string(inner.x + 1, inner.y, "No transfers", style);
            return;
        }

        let visible_h = inner.height as usize;
        self.auto_scroll(visible_h);

        for (i, job) in self.jobs.iter().skip(self.scroll).take(visible_h).enumerate() {
            let y = inner.y + i as u16;

            let arrow = match job.direction {
                TransferDirection::Upload => "↑",
                TransferDirection::Download => "↓",
            };

            let (status_span, bar_span) = match &job.status {
                TransferStatus::Queued => (
                    Span::styled("Queued", Style::default().fg(theme::color_muted())),
                    Span::raw(""),
                ),
                TransferStatus::InProgress { percent, speed } => {
                    let bar = render_progress_bar(*percent, 20);
                    (
                        Span::styled(
                            format!("{}% {}", percent, speed),
                            Style::default().fg(theme::color_warning()),
                        ),
                        Span::styled(bar, Style::default().fg(theme::color_primary())),
                    )
                }
                TransferStatus::Completed => (
                    Span::styled(
                        "Complete",
                        Style::default()
                            .fg(theme::color_success())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(""),
                ),
                TransferStatus::Failed(err) => (
                    Span::styled(
                        format!("Error: {}", err),
                        Style::default().fg(theme::color_danger()),
                    ),
                    Span::raw(""),
                ),
            };

            let line = Line::from(vec![
                Span::styled(
                    format!(" {} {}", arrow, job.file_name),
                    Style::default().fg(theme::color_text()),
                ),
                Span::raw("  "),
                bar_span,
                Span::raw(" "),
                status_span,
            ]);
            buf.set_line(inner.x, y, &line, inner.width);
        }

        // Scroll indicator if there are more jobs than visible
        if total > visible_h {
            let indicator = format!(" {}-{}/{} ", self.scroll + 1, (self.scroll + visible_h).min(total), total);
            let ind_style = Style::default().fg(theme::color_muted());
            let x = inner.x + inner.width.saturating_sub(indicator.len() as u16 + 1);
            buf.set_string(x, area.y, &indicator, ind_style);
        }
    }
}

/// Render a text-based progress bar of given width.
fn render_progress_bar(percent: u8, width: usize) -> String {
    let filled = (percent as usize * width) / 100;
    let empty = width.saturating_sub(filled);
    format!(
        "[{}{}]",
        "━".repeat(filled),
        "░".repeat(empty)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_bar_0_percent() {
        let bar = render_progress_bar(0, 10);
        assert_eq!(bar, "[░░░░░░░░░░]");
    }

    #[test]
    fn progress_bar_50_percent() {
        let bar = render_progress_bar(50, 10);
        assert_eq!(bar, "[━━━━━░░░░░]");
    }

    #[test]
    fn progress_bar_100_percent() {
        let bar = render_progress_bar(100, 10);
        assert_eq!(bar, "[━━━━━━━━━━]");
    }

    #[test]
    fn has_active_transfers() {
        let mut panel = TransfersPanel::new();
        assert!(!panel.has_active_transfers());

        panel.jobs.push(TransferJob {
            id: 1,
            source: "/local/file.txt".to_string(),
            destination: "/remote/file.txt".to_string(),
            direction: TransferDirection::Upload,
            file_name: "file.txt".to_string(),
            file_size: 1024,
            status: TransferStatus::InProgress {
                percent: 50,
                speed: "1.0 MB/s".to_string(),
            },
        });
        assert!(panel.has_active_transfers());
    }
}
