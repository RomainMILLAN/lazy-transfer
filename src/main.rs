use clap::Parser;
use std::process;
use std::sync::LazyLock;

/// A TUI dual-pane file manager for remote transfers (SSH, SFTP, FTP, WebDAV).
#[derive(Parser)]
#[command(about)]
#[command(version = long_version())]
struct Cli {
    /// Use light theme (for light terminal backgrounds)
    #[arg(long)]
    light: bool,

    /// Protocol to use: ssh, sftp, ftp, webdav
    #[arg(long, default_value = "ssh")]
    protocol: String,

    /// Host to connect to (skips connection selection)
    #[arg(short = 'H', long)]
    host: Option<String>,

    /// User
    #[arg(short, long)]
    user: Option<String>,

    /// Port (default: 22 for SSH/SFTP, 21 for FTP; taken from the URL for WebDAV)
    #[arg(short, long)]
    port: Option<u16>,

    /// Path to SSH identity file (SSH/SFTP only)
    #[arg(short, long)]
    identity: Option<String>,
}

/// Built once, then borrowed for the rest of the process: clap wants a
/// `&'static str`, and `concat!` cannot take `std::env::consts` because those
/// are consts rather than literals. Paying a `LazyLock` is what buys the OS and
/// arch being read *here*, in the binary, where they describe the target — a
/// build script only knows the host, and would mislabel every cross build.
static LONG_VERSION: LazyLock<String> = LazyLock::new(|| {
    format!(
        "version={}, commit={}, build date={}, os={}, arch={}",
        env!("CARGO_PKG_VERSION"),
        env!("LT_GIT_COMMIT"),
        env!("LT_BUILD_DATE"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
});

fn long_version() -> &'static str {
    LONG_VERSION.as_str()
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

    // Unknown values keep falling back to Ssh, as before.
    let protocol =
        lazy_transfer::transfer::types::Protocol::from_str_opt(&cli.protocol).unwrap_or_default();
    let port = cli.port.unwrap_or_else(|| protocol.default_port());

    let mut app =
        lazy_transfer::ui::app::App::new(cfg, cli.host, cli.user, port, cli.identity, protocol);

    if let Err(e) = app.run() {
        log::error!("program error: {e}");
        eprintln!("Error: {e}");
        process::exit(1);
    }
}
