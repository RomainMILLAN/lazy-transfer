use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Widget};

use crate::transfer::types::{TransferDirection, TransferJob, TransferStatus};
use crate::ui::style::{styles, theme};

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
            .title_style(styles::block_title_style(active > 0))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color));
        let inner = block.inner(area);
        block.render(area, buf);

        if self.jobs.is_empty() {
            buf.set_string(inner.x + 1, inner.y, "No transfers", styles::muted_style());
            return;
        }

        let visible_h = inner.height as usize;
        self.auto_scroll(visible_h);

        for (i, job) in self
            .jobs
            .iter()
            .skip(self.scroll)
            .take(visible_h)
            .enumerate()
        {
            let y = inner.y + i as u16;

            let arrow = match job.direction {
                TransferDirection::Upload => "↑",
                TransferDirection::Download => "↓",
            };

            // Width of a rendered bar, so rows without one still line their
            // status text up with the rows that have one.
            const BAR_W: usize = 20;
            let empty_bar = || vec![Span::raw(" ".repeat(BAR_W + 2))];

            let (status_span, bar_spans) = match &job.status {
                TransferStatus::Queued => {
                    (Span::styled("Queued", styles::muted_style()), empty_bar())
                }
                TransferStatus::InProgress { percent, speed } => {
                    let (filled, empty) = progress_bar_parts(*percent, BAR_W);
                    (
                        Span::styled(format!("{}% {}", percent, speed), styles::warning_style()),
                        // The filled run is the accent, the track behind it the
                        // dimmed accent — the only non-text use of that token.
                        vec![
                            Span::styled("[", styles::muted_style()),
                            Span::styled(filled, Style::default().fg(theme::color_primary())),
                            Span::styled(empty, Style::default().fg(theme::color_accent_dim())),
                            Span::styled("]", styles::muted_style()),
                        ],
                    )
                }
                TransferStatus::Completed => (
                    Span::styled(
                        "Complete",
                        styles::success_style().add_modifier(Modifier::BOLD),
                    ),
                    empty_bar(),
                ),
                TransferStatus::Failed(err) => (
                    Span::styled(format!("Error: {}", err), styles::error_style()),
                    empty_bar(),
                ),
            };

            // Fixed-width name column: without it every row's bar and status
            // start wherever that row's filename happened to end.
            const NAME_W: usize = 34;
            let name = crate::ui::text::truncate_ellipsis(&job.file_name, NAME_W);
            let mut spans = vec![
                Span::styled(
                    format!(" {arrow} {name:<NAME_W$}"),
                    styles::description_style(),
                ),
                Span::raw("  "),
            ];
            spans.extend(bar_spans);
            spans.push(Span::raw(" "));
            spans.push(status_span);
            let line = Line::from(spans);
            buf.set_line(inner.x, y, &line, inner.width);
        }

        // Scroll indicator if there are more jobs than visible
        if total > visible_h {
            let indicator = format!(
                " {}-{}/{} ",
                self.scroll + 1,
                (self.scroll + visible_h).min(total),
                total
            );
            let ind_style = styles::muted_style();
            let x = inner.x + inner.width.saturating_sub(indicator.len() as u16 + 1);
            buf.set_string(x, area.y, &indicator, ind_style);
        }
    }
}

/// The filled run and the track of a progress bar, as separate strings so the
/// renderer can style them differently. This is the only place the bar glyphs
/// are written down.
fn progress_bar_parts(percent: u8, width: usize) -> (String, String) {
    let filled = (percent as usize * width) / 100;
    let empty = width.saturating_sub(filled);
    ("━".repeat(filled), "░".repeat(empty))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fill and the track are rendered as separate spans, so they are
    /// asserted separately; together they always span the full width.
    #[test]
    fn progress_bar_0_percent() {
        assert_eq!(progress_bar_parts(0, 10), (String::new(), "░".repeat(10)));
    }

    #[test]
    fn progress_bar_50_percent() {
        assert_eq!(progress_bar_parts(50, 10), ("━".repeat(5), "░".repeat(5)));
    }

    #[test]
    fn progress_bar_100_percent() {
        assert_eq!(progress_bar_parts(100, 10), ("━".repeat(10), String::new()));
    }

    #[test]
    fn progress_bar_always_spans_the_full_width() {
        for percent in 0..=100u8 {
            let (filled, empty) = progress_bar_parts(percent, 20);
            assert_eq!(
                filled.chars().count() + empty.chars().count(),
                20,
                "at {percent}%"
            );
        }
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
