# lazy-transfer

TUI dual-pane file manager for remote transfers (SSH/SCP, SFTP, FTP, WebDAV), built with Rust + ratatui.

## Build & Run

Requires `libssh2-dev` on Linux for SFTP support (`ssh2` crate).

```bash
cargo build
cargo run -- --light                                      # light theme
cargo run -- --host myserver --user admin                  # direct SSH connect
cargo run -- --protocol sftp --host ftp.example.com --user admin  # direct SFTP connect
cargo run -- --protocol ftp --host ftp.example.com --user admin   # direct FTP connect
# WebDAV is URL-addressed: set it up from the connection screen (tab 4), not the CLI
```

## Test

```bash
cargo test                                    # offline: unit tests + the wire-format test

# WebDAV integration tests (ignored by default, need a server):
docker run -d --name lt-dav -p 18080:80 \
  -e AUTH_TYPE=Basic -e USERNAME=alice -e PASSWORD=s3cret bytemark/webdav
cargo test --test webdav_live -- --ignored --test-threads=1
```

`tests/webdav_wire.rs` runs offline against an in-process HTTP server and locks the PUT
wire format (explicit `Content-Length`, never chunked). `tests/webdav_mem.rs` and
`tests/webdav_tls.rs` are ignored live checks for streaming and self-signed certificates.

## Architecture

Follows the same patterns as `lazycomposer`:

```
src/
  transfer/              # Domain layer (NO UI imports)
    backend.rs           # RemoteBackend trait — protocol-agnostic interface
    stream.rs            # StreamHandle/StreamLine + ByteProgress, ProgressReader, spawn_transfer
    exec.rs              # Executor trait, RealExecutor (ssh/scp via std::process)
    runner.rs            # SshRunner implements RemoteBackend (shell out to ssh/scp)
    sftp_backend.rs      # SftpBackend implements RemoteBackend (native ssh2 crate)
    ftp_backend.rs       # FtpBackend implements RemoteBackend (native suppaftp crate)
    digest_auth.rs       # HTTP Digest (RFC 7616) challenge parsing + response, pure fns
    webdav_backend.rs    # WebDavBackend implements RemoteBackend (ureq + roxmltree)
    ssh_config.rs        # Parse ~/.ssh/config
    connections.rs       # Save/load connections to ~/.config/lazy-transfer/connections.json
    ls_parse.rs          # Parse ls -la output (shared by SSH runner and FTP backend)
    types.rs             # FileEntry, ConnectionConfig, Protocol, SortColumn, etc.
  config/                # CLI config resolution
  logger/                # File-based debug logger (~/.local/state/lazy-transfer/debug.log)
  ui/
    app.rs               # Main event loop, state machine, key routing, render
    panels/
      connection.rs      # Tabbed connection screen [1:SSH] [2:SFTP] [3:FTP] [4:WebDAV]
      local_files.rs     # Local file browser panel
      remote_files.rs    # Remote file browser panel
      transfers.rs       # Transfer queue with progress bars
    components/          # Reusable: ConfirmDialog, InputBox, ChoiceDialog, HelpPopup, Spinner, StatusBar
    style/               # Theme system (dark/light), style presets
    keys.rs              # KeyMap with all keybindings
    layout.rs            # Split-pane layout computation
    webdav_form.rs       # WebDAV connection flow state machine (unit-testable)
```

## Key Patterns

- **RemoteBackend trait**: Protocol-agnostic interface (`list_dir`, `upload`, `download`, `mkdir`, `delete`, `rename`, `upload_dir`, `download_dir`). Four implementations: `SshRunner`, `SftpBackend`, `FtpBackend`, `WebDavBackend`.
- **`App.runner`**: Stored as `Arc<dyn RemoteBackend>` — the UI never knows which protocol is active.
- **Background threading**: `mpsc::channel` with `BgMsg` enum. Main loop drains with `try_recv()` every 50ms.
- **Panel pattern**: Each panel has `files: Vec<T>`, `filtered: Vec<usize>`, `cursor`, `filter`. Uses `SkimMatcherV2` for fuzzy search.
- **SftpBackend**: Uses a persistent `SftpHandle` (self-referential struct with `Session` + `Sftp`) to avoid channel startup failures on SFTP-only servers. `unsafe impl Send/Sync` justified by Mutex serialization.
- **FtpBackend**: `suppaftp::FtpStream` behind `Mutex`. Directory operations use recursive walk (no tar on FTP servers).
- **SSH ControlMaster**: All SSH/SCP commands use `-o ControlMaster=auto -o ControlPersist=600` for session reuse. Password auth suspends the TUI for interactive prompt.
- **Directory transfers**: SSH uses tar + scp + extract. SFTP and FTP use recursive walk via native protocol.
- **Saved connections**: `~/.config/lazy-transfer/connections.json`, passwords base64-encoded, file permissions 0600.
- **Progress tracking**: SSH/SCP parses PTY output (byte-by-byte split on `\r`/`\n`). SFTP/FTP/WebDAV emit synthetic `X%`. Unified regex `(\d+)%(?:\s+\S+\s+(\S+/s))?` in `monitor_transfer`. `ByteProgress` in `stream.rs` is the only place new code encodes that text — the day `StreamLine` gains a typed `percent`, one function changes.
- **WebDavBackend**: `ureq` 3 (blocking, no tokio) + `roxmltree` for PROPFIND. A cloned `ureq::Agent` is `Send + Sync`, so transfer threads get an owned client — **no `Mutex`, no `unsafe`**, unlike the FTP/SFTP backends, and transfers can run concurrently. Streams both ways (never buffers a file in RAM). Non-negotiable agent settings: `allow_non_standard_methods(true)` (else PROPFIND is refused), `http_status_as_error(false)`, and **both** `max_redirects(0)` + `max_redirects_will_error(false)` — a followed 3xx is rewritten to GET, so a redirected MOVE/DELETE would silently do nothing, and without the second flag a 301 arrives as an error instead of a readable status.
- **WebDAV collections**: `RemoteBackend::delete`/`rename` do not carry `is_dir`, and Apache `mod_dav` answers 301 for a collection addressed without a trailing slash. Both therefore try the resource form, then retry once as a collection on 301/308/404/409, reporting the **first** status.
- **WebDAV paths**: the remote panel speaks POSIX paths rooted at `/`, relative to the DAV root; `url_for` translates via `path_segments_mut` (never string concatenation). PROPFIND hrefs are percent-decoded before comparison (encoding is not canonical, decoding is), and the "self" entry is found by path **equality** — never by `strip_prefix`, whose failure would make entries vanish silently.
- **WebDAV has no POSIX bits**: `FileEntry.permissions` is left empty rather than fabricated.
- **WebDAV auth**: Basic/Bearer are pre-computed into a header at connect time (`WebDavAuth::header_value`), so the secret never reaches the backend. **Digest** (`digest_auth.rs`, RFC 7616, MD5 + `qop=auth`) cannot be: it is challenge-response. The client therefore **caches the server challenge** — a nonce is reusable, that is what the incrementing `nc` is for — so only the first request pays a 401 round trip, and the streamed PUT can authenticate up front even though its body cannot be replayed. A 401 is retried **only** when Digest is configured; re-sending an identical Basic header would waste a round trip and mislabel the failure. `ByteProgress::rewind` un-counts a replayed PUT body without moving the bar backwards.
- **401 messages distinguish three cases**: Digest required but not selected ("choose Digest"), a Digest answer refused (bad credentials), and a plain credential failure. Saying "check your password" against a Digest-only server sends the user after the wrong problem.
- **`dav://` / `davs://`** are accepted as aliases for `http://` / `https://` (the GVFS/Dolphin/Cyberduck spelling).

## Conventions

- Connection screen has protocol tabs [1:SSH] [2:SFTP] [3:FTP] [4:WebDAV] — not a global tab bar
- `ConnectionConfig` has **private fields**: build it through the named constructors (`ssh`/`sftp`/`ftp`/`webdav`/`webdav_saved`) so `webdav: Some(..)` with `protocol: Ssh` stays unrepresentable. `display_identity()` is the single renderer of "who is this connection" and owns the "empty user" branch
- `Protocol::as_str`/`from_str_opt` are the single source of truth for the JSON and CLI spellings; `SavedConnection.protocol` stays a tolerant `String` (a strict enum would make `load()` fall back to `Default` and wipe every saved connection) and must never be read raw outside `connections.rs`
- Delete confirmation branches on `entry.is_dir` **only**, never on the active protocol — `start_delete` is protocol-blind and must stay so
- Two screens only: `ConnectionSelect` and `FileBrowser`
- Inline fuzzy filter (`/` to activate, type to filter, Esc to clear) — no modal dialogs for search
- Sort via `s` key -> ChoiceDialog (name/size/date), re-select same column toggles ASC/DESC
- `h` key is used for "go to parent directory" (like vim), NOT for help
- `.` toggles hidden files visibility
- **Naming**: `lazy-transfer` is canonical everywhere (crate, binary, config path, wordmark, **GitHub repository** — `github.com/RomainMILLAN/lazy-transfer`, all lowercase). `lazy_transfer` is the Rust module path. Never introduce a third spelling — the README badges were broken for exactly that reason
- Theme: `color_*()` functions from `ui::style::theme`, never hardcoded colors — `theme.rs` is the only file in the tree naming an RGB value, and a test enforces that every text color clears 4.5:1 against both `color_background` and `color_surface` in both modes. `color_accent_dim` is for non-text tones only (it fails contrast as text by design)
- Reach for a preset in `ui::style::styles` before writing `Style::default().fg(..)` at a call site: `border_style`/`block_title_style`/`selected_style` all take `focused: bool`, so "what does focus look like" has one answer. The panels used to make that decision in ~80 places
- `ui::brand` owns the product name, the box-drawing mark and the rules for folding them away on a small terminal. Screen geometry lives in `ui::layout` as a value (`compute_connection_screen`) so tests exercise the real arithmetic instead of a copy of it
- Artwork in `docs/assets/` is generated: `cargo run --example screenshots` renders the real widgets into a buffer and dumps HTML, then `docs/assets/src/render.sh` rasterises it. Never hand-edit a PNG there, and re-run both after a palette change
- Transfers auto-refresh the destination pane (upload -> refresh remote, download -> refresh local)
- After manual connection success, prompt to save. Saved connections show `e` to edit, `x` to delete.
