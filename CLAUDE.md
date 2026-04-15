# lazy-transfer

TUI dual-pane file manager for remote transfers (SSH/SCP, SFTP, FTP), built with Rust + ratatui.

## Build & Run

Requires `libssh2-dev` on Linux for SFTP support (`ssh2` crate).

```bash
cargo build
cargo run -- --light                                      # light theme
cargo run -- --host myserver --user admin                  # direct SSH connect
cargo run -- --protocol sftp --host ftp.example.com --user admin  # direct SFTP connect
cargo run -- --protocol ftp --host ftp.example.com --user admin   # direct FTP connect
```

## Test

```bash
cargo test
```

## Architecture

Follows the same patterns as `lazycomposer`:

```
src/
  transfer/              # Domain layer (NO UI imports)
    backend.rs           # RemoteBackend trait — protocol-agnostic interface
    stream.rs            # StreamHandle, StreamLine — shared progress types
    exec.rs              # Executor trait, RealExecutor (ssh/scp via std::process)
    runner.rs            # SshRunner implements RemoteBackend (shell out to ssh/scp)
    sftp_backend.rs      # SftpBackend implements RemoteBackend (native ssh2 crate)
    ftp_backend.rs       # FtpBackend implements RemoteBackend (native suppaftp crate)
    ssh_config.rs        # Parse ~/.ssh/config
    connections.rs       # Save/load connections to ~/.config/lazy-transfer/connections.json
    ls_parse.rs          # Parse ls -la output (shared by SSH runner and FTP backend)
    types.rs             # FileEntry, ConnectionConfig, Protocol, SortColumn, etc.
  config/                # CLI config resolution
  logger/                # File-based debug logger (~/.local/state/lazy-transfer/debug.log)
  ui/
    app.rs               # Main event loop, state machine, key routing, render
    panels/
      connection.rs      # Tabbed connection screen [1:SSH] [2:SFTP] [3:FTP]
      local_files.rs     # Local file browser panel
      remote_files.rs    # Remote file browser panel
      transfers.rs       # Transfer queue with progress bars
    components/          # Reusable: ConfirmDialog, InputBox, ChoiceDialog, HelpPopup, Spinner, StatusBar
    style/               # Theme system (dark/light), style presets
    keys.rs              # KeyMap with all keybindings
    layout.rs            # Split-pane layout computation
```

## Key Patterns

- **RemoteBackend trait**: Protocol-agnostic interface (`list_dir`, `upload`, `download`, `mkdir`, `delete`, `rename`, `upload_dir`, `download_dir`). Three implementations: `SshRunner`, `SftpBackend`, `FtpBackend`.
- **`App.runner`**: Stored as `Arc<dyn RemoteBackend>` — the UI never knows which protocol is active.
- **Background threading**: `mpsc::channel` with `BgMsg` enum. Main loop drains with `try_recv()` every 50ms.
- **Panel pattern**: Each panel has `files: Vec<T>`, `filtered: Vec<usize>`, `cursor`, `filter`. Uses `SkimMatcherV2` for fuzzy search.
- **SftpBackend**: Uses a persistent `SftpHandle` (self-referential struct with `Session` + `Sftp`) to avoid channel startup failures on SFTP-only servers. `unsafe impl Send/Sync` justified by Mutex serialization.
- **FtpBackend**: `suppaftp::FtpStream` behind `Mutex`. Directory operations use recursive walk (no tar on FTP servers).
- **SSH ControlMaster**: All SSH/SCP commands use `-o ControlMaster=auto -o ControlPersist=600` for session reuse. Password auth suspends the TUI for interactive prompt.
- **Directory transfers**: SSH uses tar + scp + extract. SFTP and FTP use recursive walk via native protocol.
- **Saved connections**: `~/.config/lazy-transfer/connections.json`, passwords base64-encoded, file permissions 0600.
- **Progress tracking**: SSH/SCP parses PTY output (byte-by-byte split on `\r`/`\n`). SFTP/FTP emit synthetic `X%` via `ProgressReader`/`ProgressWriter`. Unified regex `(\d+)%(?:\s+\S+\s+(\S+/s))?` in `monitor_transfer`.

## Conventions

- Connection screen has protocol tabs [1:SSH] [2:SFTP] [3:FTP] — not a global tab bar
- Two screens only: `ConnectionSelect` and `FileBrowser`
- Inline fuzzy filter (`/` to activate, type to filter, Esc to clear) — no modal dialogs for search
- Sort via `s` key -> ChoiceDialog (name/size/date), re-select same column toggles ASC/DESC
- `h` key is used for "go to parent directory" (like vim), NOT for help
- `.` toggles hidden files visibility
- Theme: `color_*()` functions from `ui::style::theme`, never hardcoded colors
- Transfers auto-refresh the destination pane (upload -> refresh remote, download -> refresh local)
- After manual connection success, prompt to save. Saved connections show `e` to edit, `x` to delete.
