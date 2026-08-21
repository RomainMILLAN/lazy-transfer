use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::ui::components::Hint;
use crate::ui::keys::{merged, KeyMap};
use crate::ui::style::{styles, theme};
use crate::ui::ActivePane;

/// StatusBar renders contextual keyboard hints at the bottom.
///
/// It holds NO hints of its own. Every other panel is handed the current state at
/// render time (`local_files.render(.., focused, filtering)`); the bar used to be
/// the exception, keeping a copy refreshed from a handful of push sites. That copy
/// is where `d download` survived: a cache someone has to remember to refresh
/// eventually says something that was true a while ago. Nothing to refresh now.
pub struct StatusBar;

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

/// Terminal columns a string occupies. `str::len()` counts bytes, and the bar
/// carried `⟳` — three bytes, one column — so byte lengths used to shift
/// everything after it two columns to the right.
fn cols(s: &str) -> u16 {
    s.chars().count() as u16
}

impl StatusBar {
    pub fn new() -> Self {
        StatusBar
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, hints: &[Hint]) {
        let bg = styles::bar_style();
        for x in area.x..area.x + area.width {
            buf.set_string(x, area.y, " ", bg);
        }

        let mut x = area.x + 1;

        // Hints. A hint that would be clipped by the right edge is dropped
        // whole rather than cut mid-word, which is what a plain `set_string`
        // does — the bar used to end in things like "q qui".
        let key_style = styles::key_style().bg(theme::color_surface());
        let right = area.x + area.width;
        for hint in hints {
            let needed = cols(&hint.key) + 1 + cols(&hint.desc);
            if x + needed > right {
                break;
            }
            buf.set_string(x, area.y, &hint.key, key_style);
            x += cols(&hint.key) + 1;
            buf.set_string(x, area.y, &hint.desc, bg);
            x += cols(&hint.desc) + 2;
        }
    }
}

/// Hints for the file browser screen, in display order.
///
/// Derived from the bindings, never written out: a hardcoded list is a second copy
/// of the truth, and this one drifted into advertising `u`/`x`/`y` — keys bound to
/// nothing — while telling users `d` downloads when `d` **deletes**, so pressing
/// the advertised download key opened the delete confirmation.
///
/// It composes bindings for a screen, so it lives with the views rather than on
/// `KeyMap`: the keymap would otherwise gain a second reason to change, and would
/// have to know that panes and tar-capable backends exist.
///
/// `pane` decides the transfer direction because `start_copy` decides it the same
/// way — the label follows the behaviour, not a parallel rule. `supports_tar` is
/// asked of the backend, never derived from the protocol: the UI does not know
/// which protocol is active and must not learn.
///
/// The bar is one row and drops overflow from the right, so the order is by
/// usefulness.
pub fn browser_hints(km: &KeyMap, pane: ActivePane, supports_tar: bool) -> Vec<Hint> {
    // "tar" rather than "upload (tar)": `c` right beside it already names the
    // direction, and the bar is one row that drops whatever runs off the right.
    let copy = if pane.is_local() {
        "upload"
    } else {
        "download"
    };

    let mut hints = vec![
        merged(&km.up, &km.down, "j/k", "navigate"),
        km.switch_pane.hint(),
        km.copy_file.hint_as(copy),
    ];
    if supports_tar {
        hints.push(km.copy_tar.hint_as("tar"));
    }
    hints.extend([
        km.delete.hint(),
        km.rename.hint(),
        km.mkdir.hint(),
        km.sort.hint(),
        km.help.hint(),
        km.quit.hint(),
    ]);
    hints
}

/// Returns the hints for the connection selection screen.
///
/// These stay a literal list on purpose: `1`-`4`, `e` and `x` are matched directly
/// in `handle_connection_key`, not through [`KeyMap`], so there is no binding to
/// ask. A decorative `KeyBinding` whose `keys` nothing matches would be exactly the
/// unread field this whole change removes. The price is that nothing checks these
/// four by construction, which `connection_hints_are_all_handled` in `app` pays.
pub fn connection_hints() -> Vec<Hint> {
    hints(&[
        ("1-4", "protocol"),
        ("j/k", "navigate"),
        ("enter", "connect"),
        ("e", "edit"),
        ("x", "remove"),
        ("/", "filter"),
        ("?", "help"),
        ("q", "quit"),
    ])
}

fn hints(pairs: &[(&str, &str)]) -> Vec<Hint> {
    pairs
        .iter()
        .map(|(key, desc)| Hint::new(key, desc))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::keys::default_key_map;

    #[test]
    fn connection_hints_has_quit() {
        let hints = connection_hints();
        assert!(hints.iter().any(|h| h.key == "q"));
    }

    /// The test the old one should have been. It used to enumerate the expected
    /// keys by hand, which is how it came to certify `u`/`d`/`n`/`x`/`y`.
    ///
    /// `all`, not `any`: a label is only honest if EVERY key it names is bound.
    /// Accepting a label because one of its keys is reachable would wave through
    /// `"u/c"`, with `u` dead beside a working `c` — the partial drift that is the
    /// exact shape of the original bug.
    #[test]
    fn every_browser_hint_is_bound() {
        let km = default_key_map();
        for pane in [ActivePane::Local, ActivePane::Remote] {
            for tar in [false, true] {
                for hint in browser_hints(&km, pane, tar) {
                    assert!(
                        km.names_only_bound_keys(&hint.key),
                        "hint {hint:?} ({pane:?}, tar={tar}) names a key nothing answers to"
                    );
                }
            }
        }
    }

    /// The regression that was reported: `c` transfers, and which way it goes is
    /// which pane has focus.
    #[test]
    fn copy_key_names_the_direction_of_the_focused_pane() {
        let km = default_key_map();

        let local = browser_hints(&km, ActivePane::Local, false);
        let up = local.iter().find(|h| h.key == "c").expect("no c hint");
        assert_eq!(up.desc, "upload");

        let remote = browser_hints(&km, ActivePane::Remote, false);
        let down = remote.iter().find(|h| h.key == "c").expect("no c hint");
        assert_eq!(down.desc, "download");
    }

    /// `d` is delete on every screen and in every direction. The bar said
    /// otherwise for a while, and following it deleted the file.
    #[test]
    fn d_is_never_advertised_as_download() {
        let km = default_key_map();
        for pane in [ActivePane::Local, ActivePane::Remote] {
            for tar in [false, true] {
                for hint in browser_hints(&km, pane, tar) {
                    if hint.key == "d" {
                        assert_eq!(hint.desc, "delete", "d must always read delete");
                    }
                    assert_ne!(
                        (hint.key.as_str(), hint.desc.as_str()),
                        ("d", "download"),
                        "the reported bug is back"
                    );
                }
            }
        }
    }

    /// `C` only works where the backend can tar, so it is only offered there.
    #[test]
    fn tar_hint_follows_the_backend_capability() {
        let km = default_key_map();
        assert!(
            !browser_hints(&km, ActivePane::Local, false)
                .iter()
                .any(|h| h.key == "C"),
            "C offered on a backend that cannot tar"
        );
        assert!(
            browser_hints(&km, ActivePane::Local, true)
                .iter()
                .any(|h| h.key == "C" && h.desc == "tar"),
            "C missing on a tar-capable backend"
        );
    }

    /// The bar drops overflow from the right, so total length is a feature
    /// decision, and `hints_are_never_cut_mid_word` does not cover it: that one
    /// proves nothing is cut in half, not that nothing was lost.
    ///
    /// 80 columns has never been enough — the old bar needed 105 — so the bar is
    /// pinned to the width the app itself falls back to (120 in `compute_layout`),
    /// where the whole set must survive INCLUDING the tar hint. This is what keeps
    /// `copy_tar` from silently pushing `q quit` off the end.
    #[test]
    fn the_longest_bar_fits_the_default_width() {
        let km = default_key_map();
        let hints = browser_hints(&km, ActivePane::Local, true);
        let last = hints.last().expect("no hints").clone();
        assert_eq!(last.desc, "quit", "quit must stay the last hint");

        let area = Rect::new(0, 0, 120, 1);
        let mut buf = Buffer::empty(area);
        StatusBar::new().render(area, &mut buf, &hints);

        let row: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(
            row.contains(&last.desc),
            "the tar hint pushed {:?} off a 120-column bar: {row:?}",
            last.desc
        );
    }

    /// What must survive when the terminal is genuinely narrow. The order is by
    /// usefulness precisely so the transfer key outlives `? help`.
    #[test]
    fn the_transfer_key_survives_a_narrow_terminal() {
        let km = default_key_map();
        let hints = browser_hints(&km, ActivePane::Remote, true);

        let area = Rect::new(0, 0, 48, 1);
        let mut buf = Buffer::empty(area);
        StatusBar::new().render(area, &mut buf, &hints);

        let row: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(row.contains("download"), "transfer key lost first: {row:?}");
    }

    /// The bar is one row: whatever does not fit must be dropped whole.
    #[test]
    fn hints_are_never_cut_mid_word() {
        let area = Rect::new(0, 0, 24, 1);
        let mut buf = Buffer::empty(area);
        StatusBar::new().render(area, &mut buf, &connection_hints());

        let row: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();
        // Every description present must be present in full.
        for hint in connection_hints() {
            if let Some(pos) = row.find(&hint.desc) {
                assert_eq!(
                    &row[pos..pos + hint.desc.len()],
                    hint.desc,
                    "truncated hint in {row:?}"
                );
            }
        }
        assert!(row.contains("protocol"), "first hint missing from {row:?}");
        assert!(
            !row.contains("quit"),
            "row should not have fitted quit: {row:?}"
        );
    }

    #[test]
    fn cols_counts_columns_not_bytes() {
        assert_eq!(cols("⟳ Connecting"), 12);
    }
}
