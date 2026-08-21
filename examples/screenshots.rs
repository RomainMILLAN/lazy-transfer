//! Renders the real screens into a ratatui buffer and writes each one out as an
//! HTML page, one `<span>` per run of same-styled cells.
//!
//! This is not a mockup: the colors and glyphs come from the same widgets and
//! the same `theme::color_*` functions the application draws with, so a palette
//! change shows up here without anyone redrawing anything by hand.
//!
//! One known cosmetic limit: the webfont's `│` does not quite fill its line box,
//! so long vertical rules show hairline seams in the PNGs. That is this HTML
//! approximation of a terminal grid, not the application — a real terminal
//! abuts cells exactly.
//!
//! ```text
//! cargo run --example screenshots
//! bash docs/assets/src/render.sh
//! ```

use std::fmt::Write as _;
use std::fs;

use lazy_transfer::transfer::types::{
    FileEntry, SshHost, TransferDirection, TransferJob, TransferStatus,
};
use lazy_transfer::ui::brand;
use lazy_transfer::ui::components::connectionbar;
use lazy_transfer::ui::components::statusbar::{connection_hints, StatusBar};
use lazy_transfer::ui::keys::default_key_map;
use lazy_transfer::ui::layout::{compute_connection_screen, compute_layout};
use lazy_transfer::ui::panels::connection::ConnectionPanel;
use lazy_transfer::ui::panels::local_files::LocalFilesPanel;
use lazy_transfer::ui::panels::remote_files::RemoteFilesPanel;
use lazy_transfer::ui::panels::transfers::TransfersPanel;
use lazy_transfer::ui::style::theme::{self, ThemeMode};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

const FONT_PX: f32 = 15.0;
const CELL_W: f32 = 9.0;
// JetBrains Mono draws its box-drawing glyphs across the full 1.32em line box —
// 19.8px at 15px. Rows shorter than that make consecutive vertical rules
// overlap; rows taller leave a hairline gap in every border.
const CELL_H: f32 = 18.4;

fn dir(name: &str, modified: &str) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        is_dir: true,
        size: 4096,
        modified: modified.to_string(),
        permissions: "drwxr-xr-x".to_string(),
    }
}

fn file(name: &str, size: u64, modified: &str) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        is_dir: false,
        size,
        modified: modified.to_string(),
        permissions: "-rw-r--r--".to_string(),
    }
}

fn job(id: usize, name: &str, direction: TransferDirection, status: TransferStatus) -> TransferJob {
    TransferJob {
        id,
        source: format!("/src/{name}"),
        destination: format!("/dst/{name}"),
        direction,
        file_name: name.to_string(),
        file_size: 1_048_576,
        status,
    }
}

fn connection_screen(width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);

    let l = compute_connection_screen(area, 1);
    if let Some((x, y)) = l.brand_at {
        brand::render(x, y, &mut buf);
    }
    let hosts = vec![
        SshHost {
            alias: "myserver".to_string(),
            hostname: "10.0.0.1".to_string(),
            user: "admin".to_string(),
            port: 22,
            identity_file: String::new(),
        },
        SshHost {
            alias: "production".to_string(),
            hostname: "prod.example.com".to_string(),
            user: "deploy".to_string(),
            port: 22,
            identity_file: String::new(),
        },
        SshHost {
            alias: "staging".to_string(),
            hostname: "staging.example.com".to_string(),
            user: "deploy".to_string(),
            port: 2222,
            identity_file: String::new(),
        },
    ];
    ConnectionPanel::new(hosts, Vec::new()).render(l.panel, &mut buf, false, false);

    let mut bar = StatusBar::new();
    bar.set_hints(connection_hints());
    bar.render(Rect::new(0, height - 1, width, 1), &mut buf);
    buf
}

fn browser_screen(width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    let l = compute_layout(width, height, true);

    // The local pane reads a real directory, so this lists the repository it
    // was built from. Only the displayed path is substituted, so the screenshot
    // does not carry whoever ran it home directory.
    let mut local = LocalFilesPanel::new(".");
    local.current_dir = "~/projects/lazy-transfer".to_string();

    let mut remote = RemoteFilesPanel::new();
    remote.set_dir("/volume1/backups");
    remote.set_files(vec![
        dir("..", ""),
        dir("2026-08", "Aug 19 02:00"),
        dir("2026-07", "Jul 31 02:00"),
        file("db.sql.gz", 190_840_832, "Aug 19 02:04"),
        file("photos.tar", 1_503_238_553, "Aug 12 21:18"),
        file("notes.md", 3_482, "Aug 18 08:02"),
        file("checksums.txt", 812, "Aug 19 02:05"),
    ]);

    let mut transfers = TransfersPanel::new();
    transfers.jobs = vec![
        job(
            1,
            "target/release/lazy-transfer",
            TransferDirection::Upload,
            TransferStatus::InProgress {
                percent: 64,
                speed: "3.1 MB/s".to_string(),
            },
        ),
        job(
            2,
            "2026-08/db.sql.gz",
            TransferDirection::Download,
            TransferStatus::Completed,
        ),
        job(
            3,
            "docs/assets/banner-dark.png",
            TransferDirection::Upload,
            TransferStatus::InProgress {
                percent: 12,
                speed: "820 KB/s".to_string(),
            },
        ),
    ];

    let mut y = 0;
    connectionbar::render(
        Rect::new(0, y, width, l.connection_bar_h),
        &mut buf,
        Some(("nas-backup", "SFTP")),
    );
    y += l.connection_bar_h;

    local.render(
        Rect::new(0, y, l.left_width, l.browser_h),
        &mut buf,
        true,
        false,
    );
    remote.render(
        Rect::new(l.left_width, y, l.right_width, l.browser_h),
        &mut buf,
        false,
        false,
        false,
    );
    y += l.browser_h;

    transfers.render(Rect::new(0, y, width, l.transfer_h), &mut buf);

    let mut bar = StatusBar::new();
    bar.set_hints(default_key_map().browser_hints());
    bar.render(Rect::new(0, height - 1, width, 1), &mut buf);
    buf
}

fn hex(c: Color, fallback: &str) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("#{r:02X}{g:02X}{b:02X}"),
        _ => fallback.to_string(),
    }
}

/// One `<span>` per run of cells sharing a style, so the output stays small.
fn to_html(buf: &Buffer, title: &str) -> String {
    let area = *buf.area();
    let page_bg = hex(theme::color_background(), "#000000");
    let page_fg = hex(theme::color_text(), "#FFFFFF");

    let mut body = String::new();
    for y in 0..area.height {
        let mut runs: Vec<(String, String)> = Vec::new();
        for x in 0..area.width {
            let cell = &buf[(x, y)];
            let reversed = cell.modifier.contains(Modifier::REVERSED);
            let (fg, bg) = if reversed {
                (hex(cell.bg, &page_bg), hex(cell.fg, &page_fg))
            } else {
                (hex(cell.fg, &page_fg), hex(cell.bg, &page_bg))
            };
            let weight = if cell.modifier.contains(Modifier::BOLD) {
                "700"
            } else {
                "400"
            };
            let key = format!("color:{fg};background:{bg};font-weight:{weight}");
            let symbol = match cell.symbol() {
                "" => " ".to_string(),
                s => s
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;"),
            };
            match runs.last_mut() {
                Some((k, text)) if *k == key => text.push_str(&symbol),
                _ => runs.push((key, symbol)),
            }
        }
        let _ = write!(body, "<div class=\"row\">");
        for (style, text) in runs {
            let _ = write!(body, "<span style=\"{style}\">{text}</span>");
        }
        let _ = writeln!(body, "</div>");
    }

    let w = (area.width as f32 * CELL_W).round() as u32;
    let h = (area.height as f32 * CELL_H).round() as u32;

    format!(
        r#"<!doctype html>
<meta charset="utf-8">
<title>{title}</title>
<!-- Generated by `cargo run --example screenshots`. Do not edit by hand. -->
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;700&display=swap">
<style>
  *, *::before, *::after {{ box-sizing: border-box; }}
  html, body {{ margin: 0; padding: 0; }}
  body {{
    width: {w}px; height: {h}px; overflow: hidden;
    background: {page_bg}; color: {page_fg};
    font-family: "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: {FONT_PX}px; line-height: {CELL_H}px;
    -webkit-font-smoothing: antialiased;
  }}
  .row {{ white-space: pre; height: {CELL_H}px; }}
</style>
{body}"#
    )
}

fn main() -> std::io::Result<()> {
    fs::create_dir_all("docs/assets/src")?;

    for (mode, suffix) in [(ThemeMode::Dark, "dark"), (ThemeMode::Light, "light")] {
        theme::set_mode(mode);

        let name = format!("screenshot-connection-{suffix}");
        fs::write(
            format!("docs/assets/src/{name}.html"),
            to_html(&connection_screen(100, 24), &name),
        )?;
        println!("wrote docs/assets/src/{name}.html");

        let name = format!("screenshot-browser-{suffix}");
        fs::write(
            format!("docs/assets/src/{name}.html"),
            to_html(&browser_screen(120, 28), &name),
        )?;
        println!("wrote docs/assets/src/{name}.html");
    }
    Ok(())
}
