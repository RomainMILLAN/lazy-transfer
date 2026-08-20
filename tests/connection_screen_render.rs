//! Renders the connection screen into an off-screen buffer and asserts the
//! brand block is actually drawn — and, on a short terminal, that it is not.
//!
//! Run with `cargo test --test connection_screen_render -- --nocapture` to look
//! at the screen as text.

use lazy_transfer::transfer::types::SshHost;
use lazy_transfer::ui::brand;
use lazy_transfer::ui::layout::compute_connection_screen;
use lazy_transfer::ui::panels::connection::ConnectionPanel;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn hosts() -> Vec<SshHost> {
    vec![
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
    ]
}

/// Draws through the same geometry the application uses — never a local copy
/// of the arithmetic, which would only ever assert against itself.
fn draw(width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);

    let l = compute_connection_screen(area, 1);
    if let Some((x, y)) = l.brand_at {
        brand::render(x, y, &mut buf);
    }
    ConnectionPanel::new(hosts(), vec![]).render(l.panel, &mut buf, false, false);
    buf
}

fn as_text(buf: &Buffer) -> String {
    let area = *buf.area();
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_roomy_terminal_shows_the_brand_block() {
    let buf = draw(100, 26);
    let text = as_text(&buf);
    println!("\n--- 100x26 ---\n{text}\n");

    assert!(text.contains(brand::MARK[1]), "mark missing:\n{text}");
    assert!(text.contains(brand::TAGLINE), "tagline missing:\n{text}");
    // The panel title carries the name too.
    assert!(
        text.contains(&format!("{} · Connections", brand::NAME)),
        "panel title missing:\n{text}"
    );
    assert!(text.contains("myserver"), "entries missing:\n{text}");
    assert!(text.contains("[1:SSH]"), "tab strip missing:\n{text}");
    // Rounded frame.
    assert!(text.contains('╭') && text.contains('╰'), "\n{text}");
}

#[test]
fn a_short_terminal_folds_the_brand_block_away() {
    let buf = draw(100, 16);
    let text = as_text(&buf);
    println!("\n--- 100x16 ---\n{text}\n");

    assert!(!text.contains(brand::MARK[1]), "mark should fold:\n{text}");
    // The panel itself is still fully drawn.
    assert!(text.contains("[1:SSH]"), "tab strip missing:\n{text}");
    assert!(text.contains("myserver"), "entries missing:\n{text}");
    assert!(
        text.contains("Manual connection"),
        "manual entry missing:\n{text}"
    );
}

#[test]
fn a_narrow_terminal_folds_the_brand_block_away() {
    let buf = draw(brand::block_w() - 1, 40);
    let text = as_text(&buf);
    println!("\n--- narrow ---\n{text}\n");

    assert!(!text.contains(brand::MARK[1]), "mark should fold:\n{text}");
    assert!(text.contains("myserver"), "entries missing:\n{text}");
}
