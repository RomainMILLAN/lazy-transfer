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

pub fn color_primary() -> Color {
    Color::Rgb(0x0D, 0x94, 0x88)
}
pub fn color_secondary() -> Color {
    if is_light() {
        Color::Rgb(0xE8, 0xEB, 0xF0)
    } else {
        Color::Rgb(0x23, 0x2F, 0x3E)
    }
}
pub fn color_success() -> Color {
    if is_light() {
        Color::Rgb(0x00, 0x80, 0x00)
    } else {
        Color::Rgb(0x00, 0xCC, 0x00)
    }
}
pub fn color_warning() -> Color {
    if is_light() {
        Color::Rgb(0x99, 0x80, 0x00)
    } else {
        Color::Rgb(0xCC, 0xCC, 0x00)
    }
}
pub fn color_danger() -> Color {
    Color::Rgb(0xCC, 0x00, 0x00)
}
pub fn color_info() -> Color {
    if is_light() {
        Color::Rgb(0x00, 0x80, 0x99)
    } else {
        Color::Rgb(0x00, 0xCC, 0xCC)
    }
}
pub fn color_muted() -> Color {
    Color::Rgb(0x80, 0x80, 0x80)
}
pub fn color_text() -> Color {
    if is_light() {
        Color::Rgb(0x1A, 0x1A, 0x1A)
    } else {
        Color::Rgb(0xCC, 0xCC, 0xCC)
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
        Color::Rgb(0x00, 0x00, 0x00)
    }
}
pub fn color_border() -> Color {
    if is_light() {
        Color::Rgb(0xAA, 0xAA, 0xAA)
    } else {
        Color::Rgb(0x80, 0x80, 0x80)
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

    #[test]
    fn toggle_mode_switches() {
        set_mode(ThemeMode::Dark);
        toggle_mode();
        assert_eq!(mode(), ThemeMode::Light);
        toggle_mode();
        assert_eq!(mode(), ThemeMode::Dark);
    }
}
