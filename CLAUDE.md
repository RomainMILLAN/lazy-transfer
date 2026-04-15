# lazy-transfer

TUI dual-pane file manager for SSH/SCP remote transfers, built with Rust + ratatui.

## Build & Run

```bash
cargo build
cargo run -- --light                    # light theme
cargo run -- --host myserver --user admin  # direct connect
```

## Test

```bash
cargo test
```

## Architecture

Follows the same patterns as `lazycomposer`:

```
src/
  transfer/         # Domain layer (NO UI imports)
    backend.rs      # RemoteBackend trait (future: FTP, SFTP)
    exec.rs         # Executor trait, RealExecutor (ssh/scp via std::process)
    runner.rs       # SshRunner implements RemoteBackend
    ssh_config.rs   # Parse ~/.ssh/config
    types.rs        # FileEntry, ConnectionConfig, TransferJob, etc.
  config/           # CLI config resolution, binary validation
  logger/           # File-based debug logger (~/.local/state/lazy-transfer/debug.log)
  ui/
    app.rs          # Main event loop, state machine, key routing, render
    panels/         # ConnectionPanel, LocalFilesPanel, RemoteFilesPanel, TransfersPanel
    components/     # Reusable: ConfirmDialog, InputBox, ChoiceDialog, HelpPopup, Spinner, StatusBar
    style/          # Theme system (dark/light), style presets
    keys.rs         # KeyMap with all keybindings
    layout.rs       # Split-pane layout computation
```

## Key Patterns

- **Background threading**: `mpsc::channel` with `BgMsg` enum. Main loop drains with `try_recv()` every 50ms.
- **Panel pattern**: Each panel has `files: Vec<T>`, `filtered: Vec<usize>`, `cursor`, `filter`. Uses `SkimMatcherV2` for fuzzy search.
- **Executor trait**: Testable abstraction over ssh/scp commands. `RealExecutor` uses `script -qefc` PTY trick for real-time SCP progress.
- **SSH ControlMaster**: All SSH/SCP commands use `-o ControlMaster=auto -o ControlPersist=600` for session reuse.
- **Directory transfers**: tar + scp + extract pattern (not `scp -r`). Temp files in `/tmp/lt-*.tar.gz`, cleaned up after.

## Conventions

- No tab bar (only 2 screens: ConnectionSelect, FileBrowser)
- Inline fuzzy filter (`/` to activate, type to filter, Esc to clear) — no modal dialogs for search
- `h` key is used for "go to parent directory" (like vim), NOT for help
- `.` toggles hidden files visibility
- Theme: `color_*()` functions from `ui::style::theme`, never hardcoded colors
