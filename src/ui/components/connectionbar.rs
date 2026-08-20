//! The one-row bar above the panes: who you are connected to on the left, the
//! product signature on the right.
//!
//! It lives here rather than inside `App` so that anything drawing the browser
//! screen — the app, or the screenshot example — draws the same bar.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::ui::brand;
use crate::ui::style::{styles, theme};

/// Draws the bar. `label` is the connection identity, e.g. `nas-backup`, and
/// `protocol` its display name; `None` leaves the left side empty.
pub fn render(area: Rect, buf: &mut Buffer, label: Option<(&str, &str)>) {
    let bar_bg = theme::color_surface();
    let bg = styles::bar_style();

    for x in area.x..area.x + area.width {
        buf.set_string(x, area.y, " ", bg);
    }

    // Right side first: the signature is fixed-width, the label is not, so the
    // label is the one that gets truncated by the buffer edge if they collide.
    let signature = format!(" {} v{} ", brand::NAME, brand::VERSION);
    let sig_len = signature.chars().count() as u16;
    if area.width > sig_len {
        buf.set_string(
            area.x + area.width - sig_len,
            area.y,
            &signature,
            styles::muted_style().bg(bar_bg),
        );
    }

    if let Some((label, protocol)) = label {
        let text = format!(" Connection: {label} via {protocol} ");
        let room = area.width.saturating_sub(sig_len) as usize;
        buf.set_stringn(
            area.x,
            area.y,
            &text,
            room,
            styles::accent_style().bg(bar_bg),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(buf: &Buffer, width: u16) -> String {
        (0..width).map(|x| buf[(x, 0)].symbol()).collect()
    }

    #[test]
    fn shows_the_connection_and_the_signature() {
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        render(area, &mut buf, Some(("nas-backup", "SFTP")));

        let text = row(&buf, 80);
        assert!(text.contains("nas-backup"), "{text:?}");
        assert!(text.contains("SFTP"), "{text:?}");
        assert!(text.contains(brand::NAME), "{text:?}");
        assert!(text.contains(brand::VERSION), "{text:?}");
    }

    /// A long connection label must not run over the signature.
    #[test]
    fn the_label_never_overruns_the_signature() {
        let area = Rect::new(0, 0, 46, 1);
        let mut buf = Buffer::empty(area);
        render(
            area,
            &mut buf,
            Some(("a-very-long-saved-connection-name", "WebDAV")),
        );

        let text = row(&buf, 46);
        let sig = format!("{} v{}", brand::NAME, brand::VERSION);
        assert!(text.ends_with(&format!("{sig} ")), "{text:?}");
    }
}
