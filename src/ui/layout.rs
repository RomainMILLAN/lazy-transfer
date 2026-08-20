use ratatui::layout::Rect;

use super::brand;

const STATUS_BAR_HEIGHT: u16 = 1;
const CONNECTION_BAR_HEIGHT: u16 = 1;
const LEFT_RATIO: f64 = 0.50;
const TRANSFER_PANEL_MIN_H: u16 = 3;

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub width: u16,
    pub height: u16,
    pub left_width: u16,
    pub right_width: u16,
    pub connection_bar_h: u16,
    pub browser_h: u16,
    pub transfer_h: u16,
    pub status_bar_h: u16,
}

/// Calculates panel sizes from terminal dimensions.
/// `has_transfers` controls whether the transfer panel gets extra space.
pub fn compute_layout(width: u16, height: u16, has_transfers: bool) -> Layout {
    let chrome_h = STATUS_BAR_HEIGHT + CONNECTION_BAR_HEIGHT;
    let available_h = height.saturating_sub(chrome_h);

    let transfer_h = if has_transfers {
        // Give 20% of available height to transfers, minimum 5 lines
        let desired = (available_h as f64 * 0.20) as u16;
        desired.max(5).min(available_h.saturating_sub(10))
    } else {
        TRANSFER_PANEL_MIN_H.min(available_h)
    };

    let browser_h = available_h.saturating_sub(transfer_h);

    let mut left_w = (width as f64 * LEFT_RATIO) as u16;
    if left_w < 20 {
        left_w = 20.min(width);
    }
    let right_w = width.saturating_sub(left_w);

    Layout {
        width,
        height,
        left_width: left_w,
        right_width: right_w,
        connection_bar_h: CONNECTION_BAR_HEIGHT,
        browser_h,
        transfer_h,
        status_bar_h: STATUS_BAR_HEIGHT,
    }
}

/// Calculates layout for the connection selection screen (single centered panel).
pub fn compute_connection_layout(width: u16, height: u16) -> Layout {
    Layout {
        width,
        height,
        left_width: width,
        right_width: 0,
        connection_bar_h: 0,
        browser_h: height.saturating_sub(STATUS_BAR_HEIGHT),
        transfer_h: 0,
        status_bar_h: STATUS_BAR_HEIGHT,
    }
}

/// Where the pieces of the connection screen go.
///
/// This exists as a value rather than as arithmetic inlined in the renderer so
/// that the geometry has exactly one definition — a test that recomputes it
/// checks its own copy, not the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionScreenLayout {
    /// Top-left of the brand block, or `None` when it folds away.
    pub brand_at: Option<(u16, u16)>,
    pub panel: Rect,
}

/// Places the brand block and the connection panel inside `area`.
pub fn compute_connection_screen(area: Rect, status_bar_h: u16) -> ConnectionScreenLayout {
    let content_h = area.height.saturating_sub(status_bar_h);

    let panel_w = 70.min(area.width.saturating_sub(4));
    let panel_x = (area.width.saturating_sub(panel_w)) / 2;

    // Two rows of top margin, then the brand block if it fits at all. Its
    // height already includes the blank row that separates it from the panel,
    // so a folded block (height 0) leaves the panel exactly where it was.
    let brand_h = brand::block_h(content_h, area.width);
    let brand_at = if brand_h > 0 {
        // `panel_x + 2` is the panel's content column, not its border.
        Some((panel_x + 2, 2))
    } else {
        None
    };

    let panel_y = 2 + brand_h;
    let panel_h = content_h.saturating_sub(panel_y + 2);

    ConnectionScreenLayout {
        brand_at,
        panel: Rect::new(panel_x, panel_y, panel_w, panel_h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_terminal_no_transfers() {
        let l = compute_layout(120, 40, false);
        assert_eq!(l.width, 120);
        assert_eq!(l.height, 40);
        assert_eq!(l.left_width, 60);
        assert_eq!(l.right_width, 60);
        assert_eq!(l.connection_bar_h, 1);
        assert_eq!(l.status_bar_h, 1);
        assert_eq!(l.transfer_h, 3);
        assert_eq!(
            l.connection_bar_h + l.browser_h + l.transfer_h + l.status_bar_h,
            l.height
        );
    }

    #[test]
    fn normal_terminal_with_transfers() {
        let l = compute_layout(120, 40, true);
        assert!(l.transfer_h >= 5);
        assert!(l.browser_h > 0);
        assert_eq!(
            l.connection_bar_h + l.browser_h + l.transfer_h + l.status_bar_h,
            l.height
        );
    }

    #[test]
    fn narrow_terminal() {
        let l = compute_layout(40, 20, false);
        assert_eq!(l.left_width, 20);
        assert_eq!(l.right_width, 20);
    }

    #[test]
    fn connection_screen_places_the_brand_above_the_panel() {
        let l = compute_connection_screen(Rect::new(0, 0, 100, 26), 1);
        let (bx, by) = l.brand_at.expect("brand block should show at 100x26");
        assert_eq!(by, 2);
        // The mark starts on the panel's content column.
        assert_eq!(bx, l.panel.x + 2);
        // Exactly one blank row between the block and the panel.
        assert_eq!(l.panel.y, by + brand::MARK.len() as u16 + 1);
    }

    #[test]
    fn connection_screen_folds_the_brand_on_a_short_terminal() {
        let short = compute_connection_screen(Rect::new(0, 0, 100, 16), 1);
        assert_eq!(short.brand_at, None);
        // Folded means the panel sits where it always did, not pushed down.
        assert_eq!(short.panel.y, 2);
    }

    #[test]
    fn connection_screen_panel_stays_inside_the_area() {
        for (w, h) in [(100, 26), (100, 16), (60, 40), (30, 10), (12, 4)] {
            let area = Rect::new(0, 0, w, h);
            let l = compute_connection_screen(area, 1);
            assert!(
                l.panel.right() <= area.right() && l.panel.bottom() <= area.bottom(),
                "{w}x{h}: panel {:?} escapes {:?}",
                l.panel,
                area
            );
        }
    }

    #[test]
    fn connection_screen_layout() {
        let l = compute_connection_layout(120, 40);
        assert_eq!(l.left_width, 120);
        assert_eq!(l.right_width, 0);
        assert_eq!(l.browser_h, 39);
        assert_eq!(l.transfer_h, 0);
    }
}
