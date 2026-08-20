<p align="center">
  <svg width="120" height="120" viewBox="0 0 120 120" xmlns="http://www.w3.org/2000/svg">
    <rect x="20" y="25" width="80" height="70" rx="8" fill="none" stroke="#E0E0E0" stroke-width="4"/>
    <path d="M20 45 L50 45 L60 35 L80 35" fill="none" stroke="#E0E0E0" stroke-width="4" stroke-linecap="round"/>
    <path d="M30 25 L30 20 Q30 10 40 10 L80 10 Q90 10 90 20 L90 25" fill="none" stroke="#E0E0E0" stroke-width="4"/>
    <path d="M45 65 L60 80 L75 65" fill="none" stroke="#50FA7B" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/>
    <path d="M45 55 L75 55" stroke="#50FA7B" stroke-width="4" stroke-linecap="round"/>
    <path d="M75 75 L75 55" stroke="#50FA7B" stroke-width="4" stroke-linecap="round"/>
    <path d="M75 55 L45 55 L45 75 L75 75" fill="none" stroke="#FFB86C" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" stroke-dasharray="4 2"/>
  </svg>
</p>

<h1 align="center">Lazy-Transfer</h1>

<p align="center">
  A terminal user interface (TUI) dual-pane file manager for remote file transfers, inspired by <a href="https://github.com/jesseduffield/lazygit">lazygit</a>.
</p>

<p align="center">
  <strong>Beta</strong> — This project is under active development. Expect rough edges and breaking changes.
</p>

<p align="center">
  Built with <a href="https://ratatui.rs">ratatui</a> + <a href="https://github.com/crossterm-rs/crossterm">crossterm</a>.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/rust-stable-orange" alt="Rust">
  <img src="https://img.shields.io/github/v/release/RomainMILLAN/Lazy-Transfer" alt="Release">
  <img src="https://img.shields.io/github/license/RomainMILLAN/Lazy-Transfer" alt="License">
</p>

## Features

- **SSH/SCP** — Connect via SSH and transfer files using scp with tar for directories
- **SFTP** — Native SFTP support via libssh2 for efficient file operations
- **FTP** — FTP support for legacy servers with recursive transfers
- **WebDAV** — Nextcloud/ownCloud, Synology and Apache `mod_dav`, over HTTP or HTTPS
- **Dual-pane navigation** — Browse local and remote files side by side
- **File operations** — Upload, download, mkdir, delete, rename, and more
- **Progress tracking** — Real-time progress bars for all transfers
- **Inline search** — Filter any list with `/` (fuzzy matching)
- **Light/Dark theme** — Auto-detects terminal background, toggle with `Ctrl+L`
- **Saved connections** — Store and reuse connection configurations

## Prerequisites

- Rust (stable)
- Linux: `libssh2-dev` package for SFTP support

```bash
# Ubuntu/Debian
sudo apt install libssh2-1-dev

# Fedora/RHEL
sudo dnf install libssh2-devel

# macOS (Homebrew)
brew install libssh2
```

## Installation

### From GitHub releases (recommended)

Pre-built binaries are available on the [releases page](https://github.com/RomainMILLAN/Lazy-Transfer/releases).

Download the archive matching your platform, extract it, and move the binary to a directory in your `PATH`:

```bash
# Example for Linux x86_64 — adjust the version and asset name as needed
curl -L https://github.com/RomainMILLAN/Lazy-Transfer/releases/latest/download/lazy-transfer-linux-x86_64.tar.gz \
  | tar -xz
mv lazy-transfer ~/.local/bin/
```

Available assets: `lazy-transfer-linux-x86_64`, `lazy-transfer-linux-aarch64`, `lazy-transfer-macos-x86_64`, `lazy-transfer-macos-aarch64`.

### From source

```bash
git clone https://github.com/RomainMILLAN/Lazy-Transfer.git
cd Lazy-Transfer
cargo build --release
```

The binary will be at `./target/release/lazy-transfer`.

```bash
cp ./target/release/lazy-transfer ~/.local/bin/
```

> Make sure `~/.local/bin` is in your `PATH`. If not, add this to your shell config:
>
> ```bash
> export PATH="$HOME/.local/bin:$PATH"
> ```

## Usage

```bash
# Launch and connect interactively
lazy-transfer

# Direct SSH connection
lazy-transfer --protocol ssh --host myserver --user admin

# Direct SFTP connection
lazy-transfer --protocol sftp --host myserver --user admin

# Direct FTP connection
lazy-transfer --protocol ftp --host ftp.example.com --user admin

# WebDAV is addressed by URL, so it is set up from the connection screen (tab 4)
# rather than on the command line.

# Force light theme
lazy-transfer --light
```

On first launch, a connection screen appears where you can select the protocol (SSH/SFTP/FTP/WebDAV), enter host and credentials, or choose from saved connections.

For WebDAV you enter a single **collection URL** — for example
`https://cloud.example.com/remote.php/dav/files/alice/` for Nextcloud — and then pick
Basic (user + password), a Bearer token, or anonymous access. If the server presents an
untrusted certificate (self-signed, common on NAS devices), the error is shown and you are
offered a retry that accepts it; that choice is remembered per connection.

## Configuration

### Saved connections

Connections are stored in:

```
~/.config/lazy-transfer/connections.json
```

The file is created with `0600` permissions (owner read/write only). Passwords are base64-encoded (not stored in plain text).

```json
{
  "connections": [
    {
      "name": "Production Server",
      "protocol": "ssh",
      "host": "server.example.com",
      "user": "admin",
      "port": 22
    },
    {
      "name": "FTP Backup",
      "protocol": "ftp",
      "host": "ftp.example.com",
      "user": "backup",
      "port": 21
    },
    {
      "name": "Nextcloud",
      "protocol": "webdav",
      "host": "cloud.example.com",
      "user": "alice",
      "port": 443,
      "auth_method": "basic",
      "url": "https://cloud.example.com/remote.php/dav/files/alice/"
    }
  ]
}
```

For WebDAV, `url` is the field that matters — `host`, `user` and `port` are derived from it
and kept only for display. Add `"insecure_tls": true` to accept an untrusted certificate.

On subsequent launches, saved connections are available for quick access. Press `e` to edit or `x` to delete a saved connection.

## Keybindings

### Global

| Key | Action |
|-----|--------|
| `1` `2` `3` `4` | Switch protocol tab (SSH / SFTP / FTP / WebDAV) |
| `Tab` / `Shift+Tab` | Next / previous panel |
| `j` / `k` or `Up` / `Down` | Navigate up / down |
| `Enter` | Select / drill-down |
| `h` | Go to parent directory |
| `/` | Filter current list |
| `Esc` | Clear filter / cancel |
| `s` | Sort menu (name/size/date) |
| `.` | Toggle hidden files |
| `R` | Refresh current directory |
| `Ctrl+L` | Toggle light/dark theme |
| `?` | Show help |
| `q` / `Ctrl+C` | Quit |

### File operations

| Key | Action |
|-----|--------|
| `u` | Upload selected file(s) to remote |
| `d` | Download selected file(s) to local |
| `n` | Create new directory |
| `N` | Create new file |
| `r` | Rename selected item |
| `x` | Delete selected item |
| `y` | Copy selected item name/path |

### Transfers panel

| Key | Action |
|-----|--------|
| `t` | Open transfers panel |
| `x` | Cancel selected transfer |
| `j` / `k` | Navigate up / down |
| `Enter` | View transfer details |

## Development

```bash
cargo build             # Debug build
cargo build --release   # Release build
cargo test              # Run all tests
cargo clippy            # Lint
```

Debug logs are written to `~/.local/state/lazy-transfer/debug.log`.

## Architecture

```
src/
├── transfer/           # Domain layer (no UI dependency)
│   ├── backend.rs      # RemoteBackend trait — protocol-agnostic interface
│   ├── runner.rs       # SshRunner — SSH/SCP via shell commands
│   ├── sftp_backend.rs # SftpBackend — native SFTP via ssh2 crate
│   ├── ftp_backend.rs   # FtpBackend — native FTP via suppaftp crate
│   ├── webdav_backend.rs # WebDavBackend — WebDAV via ureq + roxmltree
│   ├── ls_parse.rs      # Parse ls -la output
│   ├── stream.rs        # Shared progress types (ByteProgress, ProgressReader)
│   ├── connections.rs    # Load/save connections
│   └── types.rs        # FileEntry, ConnectionConfig, Protocol...
├── config/             # CLI config resolution
├── logger/             # File-based debug logger
└── ui/
    ├── app.rs          # Main event loop, state machine, key routing
    ├── panels/
    │   ├── connection.rs  # Connection setup screen
    │   ├── local_files.rs # Local file browser
    │   ├── remote_files.rs # Remote file browser
    │   └── transfers.rs   # Transfer queue with progress
    ├── components/     # ConfirmDialog, InputBox, ChoiceDialog...
    └── style/          # Theme system (dark/light)
```

All file operations run in background threads with `mpsc` channels, keeping the UI responsive. Progress updates stream back to the UI in real-time.

## License

MIT
