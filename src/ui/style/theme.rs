use ratatui::style::Color;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThemeMode {
    Dark,
    Light,
}

static THEME_MODE: Mutex<ThemeMode> = Mutex::new(ThemeMode::Dark);

pub fn set_mode(mode: ThemeMode) {
    *THEME_MODE.lock().unwrap() = mode;
}

pub fn toggle_mode() {
    let mut m = THEME_MODE.lock().unwrap();
    *m = match *m {
        ThemeMode::Dark => ThemeMode::Light,
        ThemeMode::Light => ThemeMode::Dark,
    };
}

pub fn mode() -> ThemeMode {
    *THEME_MODE.lock().unwrap()
}

fn is_light() -> bool {
    mode() == ThemeMode::Light
}

// --- Dynamic colors based on theme ---
//
// One accent (teal), two grounds. Every text color below clears 4.5:1 against
// both `color_background` and `color_surface` in its own mode; `color_border`
// and `color_accent_dim` are decorative and deliberately do not.
//
// The accent used to be a single `#0D9488` shared by both modes, which scored
// only 3.74:1 on white — it failed AA in light mode. It is now split per mode.

pub fn color_primary() -> Color {
    if is_light() {
        Color::Rgb(0x0F, 0x76, 0x6E)
    } else {
        Color::Rgb(0x2D, 0xD4, 0xBF)
    }
}

/// A dimmed accent for surfaces the eye should not read as text: progress
/// tracks, rules, brand-tinted separators. Never put text in this color.
pub fn color_accent_dim() -> Color {
    if is_light() {
        Color::Rgb(0x99, 0xF6, 0xE4)
    } else {
        Color::Rgb(0x13, 0x4E, 0x4A)
    }
}

/// The one raised ground: status bar, connection bar, modal fills. Before this
/// existed the three bars each hardcoded their own slightly different value.
pub fn color_surface() -> Color {
    if is_light() {
        Color::Rgb(0xF1, 0xF4, 0xF5)
    } else {
        Color::Rgb(0x15, 0x1B, 0x1E)
    }
}

pub fn color_secondary() -> Color {
    color_surface()
}
pub fn color_success() -> Color {
    if is_light() {
        Color::Rgb(0x15, 0x80, 0x3D)
    } else {
        Color::Rgb(0x4A, 0xDE, 0x80)
    }
}
pub fn color_warning() -> Color {
    if is_light() {
        Color::Rgb(0xB4, 0x53, 0x09)
    } else {
        Color::Rgb(0xFB, 0xBF, 0x24)
    }
}
pub fn color_danger() -> Color {
    if is_light() {
        Color::Rgb(0xB9, 0x1C, 0x1C)
    } else {
        Color::Rgb(0xF8, 0x71, 0x71)
    }
}
/// Directories, and only directories. The blue is a file-manager convention
/// rather than a second brand color.
pub fn color_info() -> Color {
    if is_light() {
        Color::Rgb(0x03, 0x69, 0xA1)
    } else {
        Color::Rgb(0x7D, 0xD3, 0xFC)
    }
}
pub fn color_muted() -> Color {
    if is_light() {
        Color::Rgb(0x5E, 0x6E, 0x73)
    } else {
        Color::Rgb(0x7C, 0x8B, 0x90)
    }
}
pub fn color_text() -> Color {
    if is_light() {
        Color::Rgb(0x1A, 0x22, 0x24)
    } else {
        Color::Rgb(0xC9, 0xD4, 0xD6)
    }
}
pub fn color_bright() -> Color {
    if is_light() {
        Color::Rgb(0x00, 0x00, 0x00)
    } else {
        Color::Rgb(0xFF, 0xFF, 0xFF)
    }
}
pub fn color_background() -> Color {
    if is_light() {
        Color::Rgb(0xFF, 0xFF, 0xFF)
    } else {
        Color::Rgb(0x0B, 0x0F, 0x11)
    }
}
pub fn color_border() -> Color {
    if is_light() {
        Color::Rgb(0xD3, 0xDB, 0xDC)
    } else {
        Color::Rgb(0x33, 0x44, 0x4A)
    }
}
pub fn color_border_focus() -> Color {
    color_primary()
}

/// Auto-detect terminal background color.
/// Uses COLORFGBG env var if available, defaults to Dark otherwise.
pub fn detect_mode() -> ThemeMode {
    if let Ok(val) = std::env::var("COLORFGBG") {
        if let Some(bg) = val.rsplit(';').next() {
            if let Ok(n) = bg.parse::<u32>() {
                return if n >= 7 && n != 8 {
                    ThemeMode::Light
                } else {
                    ThemeMode::Dark
                };
            }
        }
    }

    ThemeMode::Dark
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `THEME_MODE` is process-global, so every test that sets it has to take
    /// this lock or a sibling test flips the mode mid-assertion.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn toggle_mode_switches() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_mode(ThemeMode::Dark);
        toggle_mode();
        assert_eq!(mode(), ThemeMode::Light);
        toggle_mode();
        assert_eq!(mode(), ThemeMode::Dark);
    }

    fn rgb(c: Color) -> (f64, f64, f64) {
        match c {
            Color::Rgb(r, g, b) => (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0),
            other => panic!("expected an Rgb color, got {other:?}"),
        }
    }

    /// WCAG relative luminance.
    fn luminance(c: Color) -> f64 {
        let lin = |v: f64| {
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = rgb(c);
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    fn contrast(a: Color, b: Color) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// The accent was once a single value shared by both modes and scored
    /// 3.74:1 on white. Anything that reads as text has to clear AA against
    /// both grounds, in both modes, or the light theme quietly regresses again.
    #[test]
    fn text_colors_clear_wcag_aa_against_both_grounds() {
        let _guard = TEST_LOCK.lock().unwrap();
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            set_mode(mode);
            let grounds = [
                ("background", color_background()),
                ("surface", color_surface()),
            ];
            let text_colors = [
                ("primary", color_primary()),
                ("text", color_text()),
                ("bright", color_bright()),
                ("muted", color_muted()),
                ("success", color_success()),
                ("warning", color_warning()),
                ("danger", color_danger()),
                ("info", color_info()),
            ];
            for (fg_name, fg) in text_colors {
                for (bg_name, bg) in grounds {
                    let ratio = contrast(fg, bg);
                    assert!(
                        ratio >= 4.5,
                        "{mode:?}: color_{fg_name} on color_{bg_name} is {ratio:.2}:1, \
                         below the 4.5:1 AA floor"
                    );
                }
            }
        }
        set_mode(ThemeMode::Dark);
    }

    /// `color_muted` and `color_border` were both `#808080`, which made a
    /// muted hint and an inactive pane frame the same color.
    #[test]
    fn muted_and_border_are_distinguishable() {
        let _guard = TEST_LOCK.lock().unwrap();
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            set_mode(mode);
            assert_ne!(color_muted(), color_border(), "{mode:?}");
        }
        set_mode(ThemeMode::Dark);
    }
}
