//! Style presets.
//!
//! Prefer these over hand-rolled `Style::default().fg(theme::color_x())` at the
//! call site: a preset is where a decision about *meaning* lives (a badge, a
//! selected row, a pane frame), so changing that decision is one edit here
//! rather than a sweep across the panels.

use ratatui::style::{Modifier, Style};

use super::theme;

pub fn title_style() -> Style {
    Style::default()
        .fg(theme::color_bright())
        .add_modifier(Modifier::BOLD)
}

pub fn description_style() -> Style {
    Style::default().fg(theme::color_text())
}

pub fn key_style() -> Style {
    Style::default()
        .fg(theme::color_primary())
        .add_modifier(Modifier::BOLD)
}

pub fn muted_style() -> Style {
    Style::default().fg(theme::color_muted())
}

pub fn success_style() -> Style {
    Style::default().fg(theme::color_success())
}

pub fn warning_style() -> Style {
    Style::default().fg(theme::color_warning())
}

pub fn error_style() -> Style {
    Style::default()
        .fg(theme::color_danger())
        .add_modifier(Modifier::BOLD)
}

pub fn section_header_style() -> Style {
    Style::default()
        .fg(theme::color_muted())
        .add_modifier(Modifier::BOLD)
}

/// A named thing worth reading first: a host alias, a saved connection, the
/// active protocol tab.
pub fn accent_style() -> Style {
    Style::default()
        .fg(theme::color_primary())
        .add_modifier(Modifier::BOLD)
}

/// A pane frame. Focus is the accent border and nothing else — no second
/// signal, so this is the only place that decision is made.
pub fn border_style(focused: bool) -> Style {
    let color = if focused {
        theme::color_border_focus()
    } else {
        theme::color_border()
    };
    Style::default().fg(color)
}

/// The block title of a focused/unfocused pane. Titles used to inherit the
/// border color, which made an inactive pane's title as dim as its frame.
pub fn block_title_style(focused: bool) -> Style {
    if focused {
        title_style()
    } else {
        muted_style()
    }
}

/// A trailing `[SFTP]`-style tag. Deliberately muted, not accent-dim: the dim
/// accent is a decorative tone and fails contrast as text.
pub fn badge_style() -> Style {
    Style::default().fg(theme::color_muted())
}

/// The cursor row. Painted with a background rather than `REVERSED`: reversing
/// bright-on-nothing produced a full-width block of pure white, the loudest
/// thing on the screen, drowning the accent everywhere else.
///
/// `focused` distinguishes the cursor in the active pane from the one the other
/// pane remembers.
pub fn selected_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(theme::color_background())
            .bg(theme::color_primary())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme::color_text())
            .bg(theme::color_border())
    }
}

/// A directory, in the file-manager convention.
pub fn directory_style() -> Style {
    Style::default()
        .fg(theme::color_info())
        .add_modifier(Modifier::BOLD)
}

/// Text on the raised ground of the status and connection bars.
pub fn bar_style() -> Style {
    Style::default()
        .fg(theme::color_text())
        .bg(theme::color_surface())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_style_distinguishes_focus() {
        assert_ne!(border_style(true), border_style(false));
    }

    #[test]
    fn block_title_style_distinguishes_focus() {
        assert_ne!(block_title_style(true), block_title_style(false));
    }

    #[test]
    fn selected_style_distinguishes_focus() {
        assert_ne!(selected_style(true), selected_style(false));
    }
}
