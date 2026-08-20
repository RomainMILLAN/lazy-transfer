//! The product name, as it appears inside the application.
//!
//! Until this module existed, `lazy-transfer` never named itself anywhere in
//! the TUI — the first screen a user saw was an unlabelled `Connections` box.

use ratatui::buffer::Buffer;
use ratatui::style::{Modifier, Style};

use crate::ui::style::theme;

/// The mark in box-drawing cells — the same two panes and crossing arrow as the
/// SVG logo, at 9x3. The arrow crosses the broken divider at `┼`.
pub const MARK: [&str; 3] = ["╭───┬───╮", "│ ──┼──▶│", "╰───┴───╯"];

/// Canonical spelling: lowercase, hyphenated. It is also the GitHub repository
/// name; `lazy_transfer` is the Rust module path, and nothing else is valid.
pub const NAME: &str = "lazy-transfer";

pub const TAGLINE: &str = "remote file transfers · two panes · four protocols";

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Columns between the mark and the wordmark.
const GAP: u16 = 3;

/// Rows the block occupies, the blank spacer below it included.
pub const BLOCK_H: u16 = MARK.len() as u16 + 1;

/// Content rows below which the block folds away entirely. A short terminal
/// needs every row for the connection list, and four rows of branding is
/// exactly the wrong thing to spend them on.
const MIN_CONTENT_H: u16 = 20;

fn cols(s: &str) -> u16 {
    s.chars().count() as u16
}

fn mark_w() -> u16 {
    MARK.iter().map(|r| cols(r)).max().unwrap_or(0)
}

/// Columns the full block needs.
pub fn block_w() -> u16 {
    mark_w() + GAP + cols(NAME).max(cols(TAGLINE))
}

/// Rows the brand block will actually take in a viewport of this size — `0`
/// when it folds away. Callers offset their own layout by the result, so a
/// folded block costs nothing rather than pushing the panel off-screen.
pub fn block_h(content_h: u16, width: u16) -> u16 {
    if content_h >= MIN_CONTENT_H && width >= block_w() {
        BLOCK_H
    } else {
        0
    }
}

/// Draws the mark, wordmark and tagline with their top-left at `(x, y)`.
/// Callers gate on [`block_h`] first; this does no fitting of its own.
pub fn render(x: u16, y: u16, buf: &mut Buffer) {
    let accent = Style::default()
        .fg(theme::color_primary())
        .add_modifier(Modifier::BOLD);
    for (i, row) in MARK.iter().enumerate() {
        buf.set_string(x, y + i as u16, row, accent);
    }

    let text_x = x + mark_w() + GAP;
    buf.set_string(
        text_x,
        y + 1,
        NAME,
        Style::default()
            .fg(theme::color_bright())
            .add_modifier(Modifier::BOLD),
    );
    buf.set_string(
        text_x,
        y + 2,
        TAGLINE,
        Style::default().fg(theme::color_muted()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_rows_are_all_the_same_width() {
        let widths: Vec<u16> = MARK.iter().map(|r| cols(r)).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "the mark must be a rectangle, got widths {widths:?}"
        );
    }

    #[test]
    fn the_block_fits_inside_the_connection_panel() {
        // `render_connection_screen` caps the panel at 70 columns; the brand
        // block sits above it and must not be wider.
        assert!(block_w() <= 70, "block is {} columns", block_w());
    }

    #[test]
    fn block_folds_away_on_a_short_terminal() {
        assert_eq!(block_h(MIN_CONTENT_H - 1, 120), 0);
        assert_eq!(block_h(MIN_CONTENT_H, 120), BLOCK_H);
    }

    #[test]
    fn block_folds_away_on_a_narrow_terminal() {
        assert_eq!(block_h(40, block_w() - 1), 0);
        assert_eq!(block_h(40, block_w()), BLOCK_H);
    }
}
