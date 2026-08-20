use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::style::styles;

/// Render a labeled field with automatic word-wrapping.
pub fn wrap_field<'a>(
    label: &'a str,
    value: &'a str,
    value_style: Style,
    width: u16,
) -> Vec<Line<'a>> {
    let label_width = label.len() + 2;
    let available = (width as usize).saturating_sub(label_width);

    if available == 0 || value.len() <= available {
        return vec![Line::from(vec![
            Span::styled(label, styles::key_style()),
            Span::raw("  "),
            Span::styled(value, value_style),
        ])];
    }

    let mut lines = Vec::new();
    let mut remaining = value;
    let mut first = true;

    while !remaining.is_empty() {
        let chunk_end = if remaining.len() <= available {
            remaining.len()
        } else {
            remaining[..available]
                .rfind(' ')
                .map_or(available, |pos| pos)
        };

        let chunk = &remaining[..chunk_end];
        remaining = if chunk_end < remaining.len() {
            remaining[chunk_end..].trim_start()
        } else {
            ""
        };

        if first {
            lines.push(Line::from(vec![
                Span::styled(label, styles::key_style()),
                Span::raw("  "),
                Span::styled(chunk, value_style),
            ]));
            first = false;
        } else {
            lines.push(Line::from(vec![
                Span::raw(" ".repeat(label_width)),
                Span::styled(chunk, value_style),
            ]));
        }
    }

    lines
}

/// Format a byte count into a human-readable size string.
/// Shortens `text` to `max` display columns, marking the cut with an ellipsis.
/// Char-based, so it never splits a multi-byte character mid-way.
pub fn truncate_ellipsis(text: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if text.chars().count() <= max {
        return text.to_string();
    }
    if max == 1 {
        return "…".to_string();
    }
    let kept: String = text.chars().take(max - 1).collect();
    format!("{kept}…")
}

pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_leaves_short_text_alone() {
        assert_eq!(truncate_ellipsis("short", 10), "short");
        assert_eq!(truncate_ellipsis("exactly10!", 10), "exactly10!");
    }

    #[test]
    fn truncate_marks_the_cut() {
        assert_eq!(truncate_ellipsis("abcdefghij", 5), "abcd…");
        assert_eq!(truncate_ellipsis("abc", 1), "…");
        assert_eq!(truncate_ellipsis("abc", 0), "");
    }

    #[test]
    fn truncate_never_splits_a_multibyte_char() {
        // Would panic on a byte-based slice.
        let out = truncate_ellipsis("café-très-long", 6);
        assert_eq!(out.chars().count(), 6);
        assert!(out.starts_with("café"), "{out}");
    }

    #[test]
    fn short_value_no_wrap() {
        let lines = wrap_field("Label:", "short", Style::default(), 80);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn long_value_wraps() {
        let lines = wrap_field(
            "Desc:",
            "This is a very long description that should wrap onto multiple lines",
            Style::default(),
            30,
        );
        assert!(lines.len() > 1);
    }

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
    }

    #[test]
    fn format_size_kb() {
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
    }

    #[test]
    fn format_size_mb() {
        assert_eq!(format_size(1048576), "1.0 MB");
    }

    #[test]
    fn format_size_gb() {
        assert_eq!(format_size(1073741824), "1.0 GB");
    }
}
