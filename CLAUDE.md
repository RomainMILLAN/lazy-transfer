# lazy-transfer

TUI dual-pane file manager for remote transfers (SSH/SCP, SFTP, FTP, WebDAV), built with Rust + ratatui.

## Build & Run

Requires OpenSSL headers + `pkg-config` (libssh2 is always vendored by `libssh2-sys`,
so there is no `libssh2-dev` to install). `--features vendored-openssl` compiles OpenSSL
in too and needs nothing from the system — that is what the release binaries use.

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

## Release

Tag-driven. `.github/workflows/release.yml` builds four targets (linux gnu x86_64/aarch64,
macOS x86_64/aarch64) with `--features vendored-openssl`, tars each binary with `LICENSE`,
and publishes them plus `SHA256SUMS` via `softprops/action-gh-release`.

```bash
# bump `version` in Cargo.toml first — the workflow refuses a tag that disagrees with it
git tag -a v0.3.0 -m "v0.3.0" && git push origin v0.3.0
```

- **`workflow_dispatch` is the rehearsal room.** Run it to exercise all four targets without
  cutting a tag; the `release` job is gated on `refs/tags/` so no release appears. Use it
  before touching anything in that workflow — a pushed tag is not something you take back.
- The `verify` job must keep running on **every** trigger. A job-level `if:` restricting it to
  tags would skip it off-tag, and a skipped `needs:` skips its dependents — which would make
  `workflow_dispatch` build nothing at all.
- Tag patterns accept `v0.3.0` **and** `0.3.0`: the repo already carries a bare `0.2.0` tag,
  and a `v*`-only trigger is precisely why it never produced a release.
- **`.cargo/config.toml` owns the cross-compilation knowledge** (linker, `CC`, `AR`). The
  workflow only installs the toolchain packages, so any CI build reproduces verbatim locally.
- **OS/arch are read in the binary, never in `build.rs`** — in a build script
  `std::env::consts` describes the *host*, so every cross build would mislabel itself.
  `build.rs` stamps only what the binary cannot know: the commit and the build date (which
  honours `SOURCE_DATE_EPOCH`).
- `Cargo.lock` is committed and every `cargo` invocation passes `--locked`. Release archives
  stay flat (binary + `LICENSE`) because the documented install is `tar -xz lazy-transfer`.

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
- **`DavClient::builder` is the only request factory**: `http::Request::builder()` must not appear anywhere else in `webdav_backend.rs`. Reading the `auth` field directly silently drops Digest (that field is `None` for it), which is exactly how `get_file` came to send bare GETs — listing, mkdir, rename, delete and upload all worked while **every download** returned 401 with a message blaming the password. `tests/webdav_wire.rs::digest_download_authenticates_the_get` locks it, with a server that refuses an unauthenticated GET. The 401 message is asked of the client (`DavClient::unauthorized`), never assembled by the caller from `digest.is_some()`.
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
- **Keyboard hints are derived, never written out**: `KeyBinding::hint()`/`hint_as()` build them, `help_key`/`help_desc` are **private**, and `Hint` lives in `ui::components::hint` so no surface can recompose a label of its own. A hardcoded list is a second copy of the truth and this one drifted badly — the bar advertised `u`/`x`/`y` (bound to nothing) and told users `d` downloads when `d` **deletes**, so pressing the advertised download key opened the delete confirmation. The transfer key is `c` in both directions, labelled by the focused pane (`browser_hints(km, pane, supports_tar)` in `statusbar` — it composes bindings for a screen, so it does not belong on `KeyMap`, which would then have to know panes and backends exist). `copy_tar` (`C`) is shown only when `RemoteBackend::supports_tar()` says so, and `start_copy_tar` refuses early otherwise — asked of the backend, never derived from the protocol. `connection_hints()` stays a literal list on purpose: `1`-`4`, `e` and `x` are matched directly in `handle_connection_key`, and a `KeyBinding` nothing matches would be the very unread field this fixes; `connection_hints_are_all_handled` in `app` pays for that choice. Labels are checked against the bindings, never recopied: `names_only_bound_keys` requires **every** key a label names to be bound (`all`, not `any` — `"u/c"` must fail on the dead `u`), and `no_two_bindings_claim_the_same_key` guards the one invariant construction does not give for free
- **`StatusBar` holds no state**: hints are passed to `render` each frame, like every other panel gets `focused`/`filtering`. It used to keep a copy refreshed from a few push sites, and that cache is where `d download` survived a rebinding. Nothing to refresh means nothing to forget — a mouse click that changes the focused pane relabels the bar for free
- `h` key is used for "go to parent directory" (like vim), NOT for help
- `.` toggles hidden files visibility
- **Naming**: `lazy-transfer` is canonical everywhere (crate, binary, config path, wordmark, **GitHub repository** — `github.com/RomainMILLAN/lazy-transfer`, all lowercase). `lazy_transfer` is the Rust module path. Never introduce a third spelling — the README badges were broken for exactly that reason
- Theme: `color_*()` functions from `ui::style::theme`, never hardcoded colors — `theme.rs` is the only file in the tree naming an RGB value, and a test enforces that every text color clears 4.5:1 against both `color_background` and `color_surface` in both modes. `color_accent_dim` is for non-text tones only (it fails contrast as text by design)
- Reach for a preset in `ui::style::styles` before writing `Style::default().fg(..)` at a call site: `border_style`/`block_title_style`/`selected_style` all take `focused: bool`, so "what does focus look like" has one answer. The panels used to make that decision in ~80 places
- `ui::brand` owns the product name, the box-drawing mark and the rules for folding them away on a small terminal. Screen geometry lives in `ui::layout` as a value (`compute_connection_screen`) so tests exercise the real arithmetic instead of a copy of it
- Artwork in `docs/assets/` is generated: `cargo run --example screenshots` renders the real widgets into a buffer and dumps HTML, then `docs/assets/src/render.sh` rasterises it. Never hand-edit a PNG there, and re-run both after a palette change
- Transfers auto-refresh the destination pane (upload -> refresh remote, download -> refresh local)
- After manual connection success, prompt to save. Saved connections show `e` to edit, `x` to delete.
