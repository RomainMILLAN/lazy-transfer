use clap::Parser;
use std::process;

/// A TUI dual-pane file manager for remote transfers (SSH, SFTP, FTP).
#[derive(Parser)]
#[command(about)]
#[command(version = long_version())]
struct Cli {
    /// Use light theme (for light terminal backgrounds)
    #[arg(long)]
    light: bool,

    /// Protocol to use: ssh, sftp, ftp
    #[arg(long, default_value = "ssh")]
    protocol: String,

    /// Host to connect to (skips connection selection)
    #[arg(short = 'H', long)]
    host: Option<String>,

    /// User
    #[arg(short, long)]
    user: Option<String>,

    /// Port (default: 22 for SSH/SFTP, 21 for FTP)
    #[arg(short, long)]
    port: Option<u16>,

    /// Path to SSH identity file (SSH/SFTP only)
    #[arg(short, long)]
    identity: Option<String>,
}

fn long_version() -> &'static str {
    concat!(
        "version=",
        env!("CARGO_PKG_VERSION"),
        ", commit=",
        env!("LT_GIT_COMMIT"),
        ", build date=",
        env!("LT_BUILD_DATE"),
        ", os=",
        env!("LT_OS"),
        ", arch=",
        env!("LT_ARCH"),
    )
}

fn main() {
    let cli = Cli::parse();

    if let Err(e) = lazy_transfer::logger::init() {
        eprintln!("Warning: could not init logger: {e}");
    }

    let cfg = match lazy_transfer::config::resolve() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    };

    if cli.light {
        lazy_transfer::ui::style::theme::set_mode(
            lazy_transfer::ui::style::theme::ThemeMode::Light,
        );
    } else {
        let detected = lazy_transfer::ui::style::theme::detect_mode();
        lazy_transfer::ui::style::theme::set_mode(detected);
        log::info!("detected theme: {:?}", detected);
    }

    log::info!(
        "starting lazy-transfer with ssh={}, scp={}, start_dir={}",
        cfg.ssh_bin,
        cfg.scp_bin,
        cfg.start_dir
    );

    let protocol = match cli.protocol.to_lowercase().as_str() {
        "sftp" => lazy_transfer::transfer::types::Protocol::Sftp,
        "ftp" => lazy_transfer::transfer::types::Protocol::Ftp,
        _ => lazy_transfer::transfer::types::Protocol::Ssh,
    };
    let port = cli.port.unwrap_or_else(|| protocol.default_port());

    let mut app = lazy_transfer::ui::app::App::new(cfg, cli.host, cli.user, port, cli.identity, protocol);

    if let Err(e) = app.run() {
        log::error!("program error: {e}");
        eprintln!("Error: {e}");
        process::exit(1);
    }
}
