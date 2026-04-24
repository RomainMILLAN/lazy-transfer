use std::io;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent, MouseButton, MouseEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, Widget};
use ratatui::Terminal;

use crate::config::Config;
use crate::transfer::backend::RemoteBackend;
use crate::transfer::exec::RealExecutor;
use crate::transfer::runner::SshRunner;
use crate::transfer::ssh_config::parse_ssh_config;
use crate::transfer::types::*;
use crate::ui::components::*;
use crate::ui::keys::default_key_map;
use crate::ui::layout::{compute_connection_layout, compute_layout, Layout};
use crate::ui::messages::Action;
use crate::ui::style::theme;

use super::components::statusbar::{browser_hints, connection_hints};

// --- Background messages ---

enum BgMsg {
    ConnectionSuccess {
        home_dir: String,
    },
    ConnectionReady {
        backend: Arc<dyn RemoteBackend>,
        home_dir: String,
    },
    ConnectionError(String),
    RemoteFilesLoaded(Vec<FileEntry>),
    RemoteFilesError(String),
    TransferProgress {
        job_id: usize,
        percent: u8,
        speed: String,
    },
    TransferComplete {
        job_id: usize,
    },
    TransferError {
        job_id: usize,
        error: String,
    },
    OperationSuccess {
        is_remote: bool,
        message: String,
    },
    OperationError(String),
}

// --- App screens ---

#[derive(PartialEq)]
enum AppScreen {
    ConnectionSelect,
    FileBrowser,
}

#[derive(PartialEq, Clone, Copy)]
enum ActivePane {
    Local,
    Remote,
}

#[derive(PartialEq)]
enum InputMode {
    None,
    Mkdir,
    Rename,
    ManualHost,
    ManualUser,
    ManualPort,
    ManualAuthChoice,
    ManualKeyPath,
    ManualPassword,
    SaveConnectionName,
    SortChoice,
}

/// Tracks what action triggered a confirm dialog.
enum PendingAction {
    DeleteLocal {
        path: String,
    },
    DeleteRemote {
        path: String,
    },
    OverwriteUpload {
        local_path: String,
        remote_path: String,
        tar: bool,
    },
    OverwriteDownload {
        remote_path: String,
        local_path: String,
        tar: bool,
    },
    DeleteSavedConnection {
        index: usize,
    },
    SaveConnection,
}

pub struct App {
    // Screen state
    screen: AppScreen,

    // Domain
    runner: Option<Arc<dyn RemoteBackend>>,
    connection: Option<ConnectionConfig>,
    config: Config,

    // Layout
    layout: Layout,

    // Panels
    connection_panel: super::panels::ConnectionPanel,
    local_files: super::panels::LocalFilesPanel,
    remote_files: super::panels::RemoteFilesPanel,
    transfers: super::panels::TransfersPanel,

    // Components
    confirm: ConfirmDialog,
    choice: ChoiceDialog,
    help: HelpPopup,
    input: InputBox,
    spinner: LoadingSpinner,
    status_bar: StatusBar,

    // State
    active_pane: ActivePane,
    input_mode: InputMode,
    filtering: bool,
    pending_action: Option<PendingAction>,
    next_job_id: usize,

    // Loading flags
    loading_remote: bool,
    connecting: bool,

    // Pending interactive operations (suspend TUI)
    pending_password_connect: Option<ConnectionConfig>,

    // Background channel
    bg_rx: mpsc::Receiver<BgMsg>,
    bg_tx: mpsc::Sender<BgMsg>,

    // Error/info message
    info_msg: Option<String>,

    // Manual connection temp state
    pending_host: String,
    pending_user: String,
    pending_port: String,
    pending_password: Option<String>,
    is_manual_connect: bool,

    // CLI direct connect
    cli_host: Option<String>,
    cli_user: Option<String>,
    cli_port: u16,
    cli_identity: Option<String>,
    cli_protocol: Protocol,
}

impl App {
    pub fn new(
        config: Config,
        cli_host: Option<String>,
        cli_user: Option<String>,
        cli_port: u16,
        cli_identity: Option<String>,
        cli_protocol: Protocol,
    ) -> Self {
        let (bg_tx, bg_rx) = mpsc::channel();
        let ssh_hosts = parse_ssh_config();

        let local_files = super::panels::LocalFilesPanel::new(&config.start_dir);

        App {
            screen: AppScreen::ConnectionSelect,
            runner: None,
            connection: None,
            config,
            layout: Layout::default(),
            connection_panel: super::panels::ConnectionPanel::new(
                ssh_hosts,
                crate::transfer::connections::load().entries,
            ),
            local_files,
            remote_files: super::panels::RemoteFilesPanel::new(),
            transfers: super::panels::TransfersPanel::new(),
            confirm: ConfirmDialog::new(),
            choice: ChoiceDialog::new(),
            help: HelpPopup::new(),
            input: InputBox::new(),
            spinner: LoadingSpinner::new(),
            status_bar: StatusBar::new(),
            active_pane: ActivePane::Local,
            input_mode: InputMode::None,
            filtering: false,
            pending_action: None,
            next_job_id: 1,
            loading_remote: false,
            connecting: false,
            pending_password_connect: None,
            bg_rx,
            bg_tx,
            info_msg: None,
            pending_host: String::new(),
            pending_user: String::new(),
            pending_port: String::new(),
            pending_password: None,
            is_manual_connect: false,
            cli_host,
            cli_user,
            cli_port,
            cli_identity,
            cli_protocol,
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Initial layout
        let (w, h) = crossterm::terminal::size()?;
        self.layout = compute_connection_layout(w, h);
        self.status_bar.set_hints(connection_hints());

        // If CLI host was provided, connect directly
        if self.cli_host.is_some() {
            self.connect_direct();
        }

        loop {
            // Recalculate layout dynamically (transfer panel may grow/shrink)
            if self.screen == AppScreen::FileBrowser {
                let (w, h) = crossterm::terminal::size().unwrap_or((120, 40));
                self.layout = compute_layout(w, h, self.transfers.has_active_transfers());
            }

            // Draw
            terminal.draw(|f| self.render(f))?;

            // Process background messages
            self.process_bg_messages();

            // Tick spinner
            self.spinner.tick();

            // Handle events
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) if self.handle_key(key) => break,
                    Event::Key(_) => {}
                    Event::Mouse(mouse) => self.handle_mouse(mouse),
                    Event::Resize(w, h) => {
                        self.layout = match self.screen {
                            AppScreen::ConnectionSelect => compute_connection_layout(w, h),
                            AppScreen::FileBrowser => {
                                compute_layout(w, h, self.transfers.has_active_transfers())
                            }
                        };
                    }
                    _ => {}
                }
            }

            // Handle pending password connection (suspend TUI)
            if self.pending_password_connect.is_some() {
                self.run_password_connect_interactive(&mut terminal)?;
            }
        }

        // Cleanup
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        Ok(())
    }

    // --- Rendering ---

    fn render(&mut self, f: &mut ratatui::Frame) {
        let area = f.area();
        let buf = f.buffer_mut();

        match self.screen {
            AppScreen::ConnectionSelect => self.render_connection_screen(area, buf),
            AppScreen::FileBrowser => self.render_file_browser(area, buf),
        }

        // Modals (on top of everything)
        self.render_modals(area, buf);
    }

    fn render_connection_screen(&self, area: Rect, buf: &mut Buffer) {
        let status_h = self.layout.status_bar_h;
        let content_h = area.height.saturating_sub(status_h);

        // Center the connection panel
        let panel_w = 70.min(area.width.saturating_sub(4));
        let panel_h = content_h.saturating_sub(4);
        let panel_x = (area.width.saturating_sub(panel_w)) / 2;
        let panel_y = 2;

        let panel_area = Rect::new(panel_x, panel_y, panel_w, panel_h);
        self.connection_panel
            .render(panel_area, buf, self.connecting, self.filtering);

        // Info message
        if let Some(ref msg) = self.info_msg {
            let msg_y = panel_y + panel_h + 1;
            if msg_y < area.height {
                let style = Style::default().fg(theme::color_danger());
                buf.set_string(panel_x + 1, msg_y, msg, style);
            }
        }

        // Status bar
        let status_area = Rect::new(0, area.height.saturating_sub(1), area.width, 1);
        self.status_bar.render(status_area, buf);
    }

    fn render_file_browser(&mut self, area: Rect, buf: &mut Buffer) {
        let l = &self.layout;
        let mut y = 0u16;

        // Connection bar
        if l.connection_bar_h > 0 {
            let conn_area = Rect::new(0, y, area.width, 1);
            self.render_connection_bar(conn_area, buf);
            y += l.connection_bar_h;
        }

        // File browser panels (side by side)
        if l.browser_h > 0 {
            let local_area = Rect::new(0, y, l.left_width, l.browser_h);
            let remote_area = Rect::new(l.left_width, y, l.right_width, l.browser_h);

            self.local_files.render(
                local_area,
                buf,
                self.active_pane == ActivePane::Local,
                self.filtering,
            );
            self.remote_files.render(
                remote_area,
                buf,
                self.active_pane == ActivePane::Remote,
                self.loading_remote,
                self.filtering,
            );
            y += l.browser_h;
        }

        // Transfers panel
        if l.transfer_h > 0 {
            let transfer_area = Rect::new(0, y, area.width, l.transfer_h);
            self.transfers.render(transfer_area, buf);
            y += l.transfer_h;
        }

        // Status bar
        if l.status_bar_h > 0 && y < area.height {
            let status_area = Rect::new(0, y, area.width, 1);
            self.status_bar.render(status_area, buf);
        }
    }

    fn render_connection_bar(&self, area: Rect, buf: &mut Buffer) {
        let bar_bg = if theme::mode() == theme::ThemeMode::Light {
            Color::Rgb(0xE8, 0xEB, 0xF0)
        } else {
            Color::Rgb(0x1A, 0x2A, 0x3A)
        };
        let bg = Style::default().fg(theme::color_text()).bg(bar_bg);

        for x in area.x..area.x + area.width {
            buf.set_string(x, area.y, " ", bg);
        }

        if let Some(ref conn) = self.connection {
            let label = format!(" Connection: {} via {} ", conn.label, conn.protocol.label());
            let style = Style::default()
                .fg(theme::color_primary())
                .bg(bar_bg)
                .add_modifier(Modifier::BOLD);
            buf.set_string(area.x, area.y, &label, style);
        }
    }

    fn render_modals(&mut self, area: Rect, buf: &mut Buffer) {
        // Confirm dialog
        if self.confirm.is_visible() {
            let w = 50.min(area.width.saturating_sub(4));
            let h = 5;
            let x = (area.width.saturating_sub(w)) / 2;
            let y = (area.height.saturating_sub(h)) / 2;
            let modal_area = Rect::new(x, y, w, h);

            Clear.render(modal_area, buf);
            let block = Block::default()
                .title(" Confirm ")
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(theme::color_warning())
                        .add_modifier(Modifier::BOLD),
                );
            let inner = block.inner(modal_area);
            block.render(modal_area, buf);

            let style = Style::default().fg(theme::color_text());
            buf.set_string(inner.x + 1, inner.y, self.confirm.message(), style);

            let hint = "[y]es / [n]o";
            let hint_style = Style::default().fg(theme::color_muted());
            buf.set_string(inner.x + 1, inner.y + 2, hint, hint_style);
        }

        // Choice dialog
        if self.choice.is_visible() {
            let w = 50.min(area.width.saturating_sub(4));
            let h = 10;
            let x = (area.width.saturating_sub(w)) / 2;
            let y = (area.height.saturating_sub(h)) / 2;
            let modal_area = Rect::new(x, y, w, h);

            Clear.render(modal_area, buf);
            let block = Block::default()
                .title(" Choose ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::color_primary()));
            let inner = block.inner(modal_area);
            block.render(modal_area, buf);

            let view = self.choice.view();
            for (i, line) in view.lines().enumerate() {
                if i >= inner.height as usize {
                    break;
                }
                buf.set_string(
                    inner.x + 1,
                    inner.y + i as u16,
                    line,
                    Style::default().fg(theme::color_text()),
                );
            }
        }

        // Input box
        if self.input.is_visible() {
            let w = 60.min(area.width.saturating_sub(4));
            let h = 7;
            let x = (area.width.saturating_sub(w)) / 2;
            let y = (area.height.saturating_sub(h)) / 2;
            let modal_area = Rect::new(x, y, w, h);
            self.input.render(modal_area, buf);
        }

        // Help popup
        if self.help.is_visible() {
            let w = 60.min(area.width.saturating_sub(4));
            let h = (area.height as f32 * 0.8) as u16;
            let x = (area.width.saturating_sub(w)) / 2;
            let y = (area.height.saturating_sub(h)) / 2;
            let modal_area = Rect::new(x, y, w, h);
            self.help.render(modal_area, buf);
        }
    }

    // --- Key handling ---

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        let km = default_key_map();

        // 1. Modal dialogs (highest priority)
        if self.confirm.is_visible() {
            if let Some(_action) = self.confirm.handle_key(key) {
                if self.confirm.confirmed {
                    self.execute_pending_action();
                } else {
                    self.pending_action = None;
                }
                return false;
            }
            return false;
        }

        if self.choice.is_visible() {
            if let Some(c) = self.choice.handle_key(key) {
                if c != '\x1b' {
                    self.handle_choice_result(c);
                }
                return false;
            }
            return false;
        }

        if self.input.is_visible() {
            if let Some(action) = self.input.handle_key(key) {
                match action {
                    Action::InputSubmit(value) => self.handle_input_submit(value),
                    Action::InputCancel => {
                        self.input_mode = InputMode::None;
                    }
                    _ => {}
                }
                return false;
            }
            return false;
        }

        if self.help.is_visible() {
            self.help.handle_key(key);
            return false;
        }

        // 2. Inline filter mode — all keystrokes go to the filter
        if self.filtering {
            return self.handle_filter_key(key);
        }

        // 3. Global keys
        if km.quit.matches(&key) {
            return true;
        }

        if km.help.matches(&key) {
            self.help.show();
            return false;
        }

        if km.toggle_theme.matches(&key) {
            theme::toggle_mode();
            return false;
        }

        // 4. Screen-specific keys
        match self.screen {
            AppScreen::ConnectionSelect => self.handle_connection_key(key),
            AppScreen::FileBrowser => self.handle_browser_key(key),
        }

        false
    }

    /// Handle keystrokes while in inline fzf filter mode.
    fn handle_filter_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                // Exit filter mode and clear filter
                self.filtering = false;
                match self.screen {
                    AppScreen::ConnectionSelect => self.connection_panel.clear_filter(),
                    AppScreen::FileBrowser => match self.active_pane {
                        ActivePane::Local => self.local_files.clear_filter(),
                        ActivePane::Remote => self.remote_files.clear_filter(),
                    },
                }
            }
            crossterm::event::KeyCode::Enter => {
                // Exit filter mode, keep filter applied
                self.filtering = false;
            }
            crossterm::event::KeyCode::Backspace => match self.screen {
                AppScreen::ConnectionSelect => {
                    if self.connection_panel.filter.is_empty() {
                        self.filtering = false;
                    } else {
                        self.connection_panel.filter_pop();
                    }
                }
                AppScreen::FileBrowser => match self.active_pane {
                    ActivePane::Local => {
                        if self.local_files.filter.is_empty() {
                            self.filtering = false;
                        } else {
                            self.local_files.filter_pop();
                        }
                    }
                    ActivePane::Remote => {
                        if self.remote_files.filter.is_empty() {
                            self.filtering = false;
                        } else {
                            self.remote_files.filter_pop();
                        }
                    }
                },
            },
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Down => {
                // Allow navigation while filtering
                let is_up = key.code == crossterm::event::KeyCode::Up;
                match self.screen {
                    AppScreen::ConnectionSelect => {
                        if is_up {
                            self.connection_panel.move_up()
                        } else {
                            self.connection_panel.move_down()
                        }
                    }
                    AppScreen::FileBrowser => match self.active_pane {
                        ActivePane::Local => {
                            if is_up {
                                self.local_files.move_up()
                            } else {
                                self.local_files.move_down()
                            }
                        }
                        ActivePane::Remote => {
                            if is_up {
                                self.remote_files.move_up()
                            } else {
                                self.remote_files.move_down()
                            }
                        }
                    },
                }
            }
            crossterm::event::KeyCode::Char(c) => match self.screen {
                AppScreen::ConnectionSelect => self.connection_panel.filter_push(c),
                AppScreen::FileBrowser => match self.active_pane {
                    ActivePane::Local => self.local_files.filter_push(c),
                    ActivePane::Remote => self.remote_files.filter_push(c),
                },
            },
            _ => {}
        }
        false
    }

    fn handle_connection_key(&mut self, key: KeyEvent) {
        let km = default_key_map();

        // Tab switching: 1=SSH, 2=SFTP, 3=FTP
        if let crossterm::event::KeyCode::Char('1') = key.code {
            self.connection_panel.select_protocol(Protocol::Ssh);
            return;
        }
        if let crossterm::event::KeyCode::Char('2') = key.code {
            self.connection_panel.select_protocol(Protocol::Sftp);
            return;
        }
        if let crossterm::event::KeyCode::Char('3') = key.code {
            self.connection_panel.select_protocol(Protocol::Ftp);
            return;
        }

        // Delete saved connection with 'x'
        if let crossterm::event::KeyCode::Char('x') = key.code {
            if self.connection_panel.selected_saved_index().is_some() {
                if let Some(saved) = self.connection_panel.selected_saved() {
                    let name = saved.name.clone();
                    self.confirm
                        .show(&format!("Remove saved connection '{}'?", name));
                    // Store the index for deletion after confirmation
                    if let Some(idx) = self.connection_panel.selected_saved_index() {
                        self.pending_action =
                            Some(PendingAction::DeleteSavedConnection { index: idx });
                    }
                }
            }
            return;
        }

        // Edit saved connection with 'e'
        if let crossterm::event::KeyCode::Char('e') = key.code {
            if let Some(saved) = self.connection_panel.selected_saved().cloned() {
                // Pre-fill the manual connection flow with saved values
                self.pending_host = saved.host.clone();
                self.pending_user = saved.user.clone();
                self.pending_port = saved.port.to_string();
                self.pending_password = saved.decoded_password();

                // Delete the old saved connection
                if let Some(idx) = self.connection_panel.selected_saved_index() {
                    let mut conns = crate::transfer::connections::load();
                    if idx < conns.entries.len() {
                        conns.entries.remove(idx);
                        let _ = crate::transfer::connections::save(&conns);
                        self.connection_panel.reload_saved();
                    }
                }

                // Start the manual flow at the host step, pre-filled
                self.is_manual_connect = true;
                self.input_mode = InputMode::ManualHost;
                self.input
                    .show_with_value("Host", "hostname or IP...", &saved.host);
            }
            return;
        }

        if km.up.matches(&key) {
            self.connection_panel.move_up();
        } else if km.down.matches(&key) {
            self.connection_panel.move_down();
        } else if km.enter.matches(&key) {
            if self.connection_panel.is_manual_selected() {
                self.start_manual_connection();
            } else if let Some(host) = self.connection_panel.selected_ssh_host().cloned() {
                let conn = ConnectionConfig::from_ssh_host(&host);
                self.connect(conn);
            } else if let Some(saved) = self.connection_panel.selected_saved().cloned() {
                self.is_manual_connect = false;
                let conn = saved.to_connection_config();
                let password = saved.decoded_password();
                self.connect_saved(conn, password);
            }
        } else if km.search.matches(&key) {
            self.filtering = true;
        }
    }

    fn handle_browser_key(&mut self, key: KeyEvent) {
        let km = default_key_map();

        if km.switch_pane.matches(&key) {
            self.active_pane = match self.active_pane {
                ActivePane::Local => ActivePane::Remote,
                ActivePane::Remote => ActivePane::Local,
            };
            return;
        }

        if km.copy_file.matches(&key) {
            self.start_copy();
            return;
        }

        if km.copy_tar.matches(&key) {
            self.start_copy_tar();
            return;
        }

        if km.delete.matches(&key) {
            self.start_delete();
            return;
        }

        if km.rename.matches(&key) {
            self.start_rename();
            return;
        }

        if km.mkdir.matches(&key) {
            self.input_mode = InputMode::Mkdir;
            self.input.show("New Directory", "enter directory name...");
            return;
        }

        if km.search.matches(&key) {
            self.filtering = true;
            return;
        }

        if km.refresh.matches(&key) {
            self.refresh_current_panel();
            return;
        }

        if km.toggle_hidden.matches(&key) {
            self.local_files.toggle_hidden();
            self.remote_files.toggle_hidden();
            return;
        }

        if km.sort.matches(&key) {
            self.choice.show(
                "Sort by:",
                vec![
                    Choice {
                        key: 'n',
                        label: "Name".to_string(),
                    },
                    Choice {
                        key: 's',
                        label: "Size".to_string(),
                    },
                    Choice {
                        key: 't',
                        label: "Date".to_string(),
                    },
                ],
            );
            self.input_mode = InputMode::SortChoice;
            return;
        }

        if km.escape.matches(&key) {
            match self.active_pane {
                ActivePane::Local => self.local_files.clear_filter(),
                ActivePane::Remote => self.remote_files.clear_filter(),
            }
            return;
        }

        // Navigation keys for active pane
        match self.active_pane {
            ActivePane::Local => {
                if km.up.matches(&key) {
                    self.local_files.move_up();
                } else if km.down.matches(&key) {
                    self.local_files.move_down();
                } else if km.enter.matches(&key) {
                    self.local_files.enter_selected();
                } else if km.back.matches(&key) {
                    self.local_files.go_parent();
                } else if km.top.matches(&key) {
                    self.local_files.go_to_top();
                } else if km.bottom.matches(&key) {
                    self.local_files.go_to_bottom();
                }
            }
            ActivePane::Remote => {
                if km.up.matches(&key) {
                    self.remote_files.move_up();
                } else if km.down.matches(&key) {
                    self.remote_files.move_down();
                } else if km.enter.matches(&key) {
                    self.enter_remote_dir();
                } else if km.back.matches(&key) {
                    self.go_remote_parent();
                } else if km.top.matches(&key) {
                    self.remote_files.go_to_top();
                } else if km.bottom.matches(&key) {
                    self.remote_files.go_to_bottom();
                }
            }
        }
    }

    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => match self.active_pane {
                ActivePane::Local => self.local_files.move_up(),
                ActivePane::Remote => self.remote_files.move_up(),
            },
            MouseEventKind::ScrollDown => match self.active_pane {
                ActivePane::Local => self.local_files.move_down(),
                ActivePane::Remote => self.remote_files.move_down(),
            },
            MouseEventKind::Down(MouseButton::Left) if self.screen == AppScreen::FileBrowser => {
                if mouse.column < self.layout.left_width {
                    self.active_pane = ActivePane::Local;
                } else {
                    self.active_pane = ActivePane::Remote;
                }
            }
            _ => {}
        }
    }

    // --- Input handling ---

    fn handle_input_submit(&mut self, value: String) {
        match self.input_mode {
            InputMode::Mkdir => {
                self.do_mkdir(&value);
            }
            InputMode::Rename => {
                self.do_rename(&value);
            }
            InputMode::ManualHost => {
                self.pending_host = value;
                self.input_mode = InputMode::ManualUser;
                // Pre-fill user if editing a saved connection
                let user = &self.pending_user;
                if user.is_empty() {
                    self.input.show("User", "username...");
                } else {
                    self.input.show_with_value("User", "username...", user);
                }
                return;
            }
            InputMode::ManualUser => {
                self.pending_user = value;
                self.input_mode = InputMode::ManualPort;
                // Pre-fill port from pending (edit) or default
                let port = if self.pending_port.is_empty() {
                    self.connection_panel
                        .selected_protocol
                        .default_port()
                        .to_string()
                } else {
                    self.pending_port.clone()
                };
                self.input.show_with_value("Port", &port, &port);
                return;
            }
            InputMode::ManualPort => {
                self.pending_port = value;
                // FTP: skip auth choice, go directly to password
                if self.connection_panel.selected_protocol == Protocol::Ftp {
                    self.input_mode = InputMode::ManualPassword;
                    let hint = if self.pending_password.is_some() {
                        "leave empty to keep current password"
                    } else {
                        "FTP password..."
                    };
                    self.input.show_password("Password", hint);
                    return;
                }
                self.input_mode = InputMode::ManualAuthChoice;
                self.choice.show(
                    "Authentication method:",
                    vec![
                        Choice {
                            key: 'k',
                            label: "SSH Key".to_string(),
                        },
                        Choice {
                            key: 'a',
                            label: "SSH Agent (default)".to_string(),
                        },
                        Choice {
                            key: 'p',
                            label: "Password".to_string(),
                        },
                    ],
                );
                return;
            }
            InputMode::ManualKeyPath => {
                let port = self.pending_port.parse().unwrap_or(22);
                let protocol = self.connection_panel.selected_protocol.clone();
                let conn = ConnectionConfig {
                    protocol,
                    host: self.pending_host.clone(),
                    user: self.pending_user.clone(),
                    port,
                    auth: AuthMethod::Key(value),
                    label: format!("{}@{}:{}", self.pending_user, self.pending_host, port),
                    ssh_alias: None,
                };
                self.connect(conn);
            }
            InputMode::ManualPassword => {
                // If empty and we have a saved password, reuse it
                let password = if value.is_empty() {
                    self.pending_password.clone().unwrap_or_default()
                } else {
                    value
                };
                self.pending_password = Some(password.clone());
                let port = self.pending_port.parse().unwrap_or(21);
                let protocol = self.connection_panel.selected_protocol.clone();
                let conn = ConnectionConfig {
                    protocol: protocol.clone(),
                    host: self.pending_host.clone(),
                    user: self.pending_user.clone(),
                    port,
                    auth: AuthMethod::Password,
                    label: format!("{}@{}:{}", self.pending_user, self.pending_host, port),
                    ssh_alias: None,
                };
                match protocol {
                    Protocol::Ftp => self.connect_ftp(conn, password),
                    Protocol::Sftp => self.connect_sftp(conn, Some(password)),
                    Protocol::Ssh => {
                        self.pending_password_connect = Some(conn);
                    }
                }
            }
            InputMode::SaveConnectionName => {
                if let Some(ref conn) = self.connection {
                    let pw = self.pending_password.as_deref();
                    let saved =
                        crate::transfer::connections::SavedConnection::from_connection_config(
                            &value, conn, pw,
                        );
                    let mut conns = crate::transfer::connections::load();
                    conns.entries.push(saved);
                    if let Err(e) = crate::transfer::connections::save(&conns) {
                        self.info_msg = Some(format!("Save error: {}", e));
                    } else {
                        self.connection_panel.reload_saved();
                    }
                }
            }
            _ => {}
        }
        self.input_mode = InputMode::None;
    }

    fn handle_choice_result(&mut self, choice: char) {
        if self.input_mode == InputMode::ManualAuthChoice {
            match choice {
                'k' => {
                    self.input_mode = InputMode::ManualKeyPath;
                    self.input.show("Identity File", "path to SSH key...");
                }
                'a' => {
                    let port = self.pending_port.parse().unwrap_or(22);
                    let protocol = self.connection_panel.selected_protocol.clone();
                    let conn = ConnectionConfig {
                        protocol,
                        host: self.pending_host.clone(),
                        user: self.pending_user.clone(),
                        port,
                        auth: AuthMethod::Agent,
                        label: format!("{}@{}:{}", self.pending_user, self.pending_host, port),
                        ssh_alias: None,
                    };
                    self.input_mode = InputMode::None;
                    self.connect(conn);
                }
                'p' => {
                    // For SFTP, ask for password inline
                    if self.connection_panel.selected_protocol == Protocol::Sftp {
                        self.input_mode = InputMode::ManualPassword;
                        let hint = if self.pending_password.is_some() {
                            "leave empty to keep current password"
                        } else {
                            "SSH password..."
                        };
                        self.input.show_password("Password", hint);
                        return;
                    }
                    let port = self.pending_port.parse().unwrap_or(22);
                    let conn = ConnectionConfig {
                        protocol: Protocol::Ssh,
                        host: self.pending_host.clone(),
                        user: self.pending_user.clone(),
                        port,
                        auth: AuthMethod::Password,
                        label: format!("{}@{}:{}", self.pending_user, self.pending_host, port),
                        ssh_alias: None,
                    };
                    self.input_mode = InputMode::None;
                    self.connect(conn);
                }
                _ => {
                    self.input_mode = InputMode::None;
                }
            }
        } else if self.input_mode == InputMode::SortChoice {
            let col = match choice {
                'n' => Some(SortColumn::Name),
                's' => Some(SortColumn::Size),
                't' => Some(SortColumn::Date),
                _ => None,
            };
            if let Some(col) = col {
                match self.active_pane {
                    ActivePane::Local => self.local_files.cycle_sort(col),
                    ActivePane::Remote => self.remote_files.cycle_sort(col),
                }
            }
            self.input_mode = InputMode::None;
        }
    }

    // --- Connection ---

    fn start_manual_connection(&mut self) {
        self.pending_host.clear();
        self.pending_user.clear();
        self.pending_port.clear();
        self.pending_password = None;
        self.is_manual_connect = true;
        self.input_mode = InputMode::ManualHost;
        self.input.show("Host", "hostname or IP...");
    }

    fn connect_direct(&mut self) {
        if let Some(host) = self.cli_host.take() {
            let user = self.cli_user.take().unwrap_or_default();
            let port = self.cli_port;
            let identity = self.cli_identity.take();
            let protocol = self.cli_protocol.clone();
            let auth = match identity {
                Some(path) => AuthMethod::Key(path),
                None => AuthMethod::Agent,
            };
            let label = if user.is_empty() {
                format!("{}:{}", host, port)
            } else {
                format!("{}@{}:{}", user, host, port)
            };
            let conn = ConnectionConfig {
                protocol,
                host,
                user,
                port,
                auth,
                label,
                ssh_alias: None,
            };
            self.connect(conn);
        }
    }

    fn connect(&mut self, conn: ConnectionConfig) {
        match conn.protocol {
            Protocol::Ssh => {
                if matches!(conn.auth, AuthMethod::Password) {
                    self.pending_password_connect = Some(conn);
                } else {
                    self.connect_ssh(conn);
                }
            }
            Protocol::Sftp => {
                self.connect_sftp(conn, None);
            }
            Protocol::Ftp => {
                // FTP needs a password — this path is for manual connections
                // where password is not yet known. For saved connections, use connect_saved.
                self.info_msg = Some(
                    "FTP requires a password. Use manual connection or saved connections."
                        .to_string(),
                );
            }
        }
    }

    fn connect_saved(&mut self, conn: ConnectionConfig, password: Option<String>) {
        match conn.protocol {
            Protocol::Ssh => {
                if matches!(conn.auth, AuthMethod::Password) {
                    self.pending_password_connect = Some(conn);
                } else {
                    self.connect_ssh(conn);
                }
            }
            Protocol::Sftp => {
                self.connect_sftp(conn, password);
            }
            Protocol::Ftp => {
                if let Some(pw) = password {
                    self.connect_ftp(conn, pw);
                } else {
                    self.info_msg = Some("FTP requires a password.".to_string());
                }
            }
        }
    }

    fn connect_ssh(&mut self, conn: ConnectionConfig) {
        self.connecting = true;
        self.info_msg = None;
        self.spinner.start("Connecting...");

        let ssh_bin = self.config.ssh_bin.clone();
        let scp_bin = self.config.scp_bin.clone();

        let exec = if let Some(alias) = &conn.ssh_alias {
            Arc::new(RealExecutor::from_alias(&ssh_bin, &scp_bin, alias))
        } else {
            let identity = match &conn.auth {
                AuthMethod::Key(path) => Some(path.clone()),
                _ => None,
            };
            Arc::new(RealExecutor::new(
                &ssh_bin, &scp_bin, &conn.user, &conn.host, conn.port, identity,
            ))
        };
        let runner: Arc<dyn RemoteBackend> = Arc::new(SshRunner::new(exec));

        self.connection = Some(conn);
        self.runner = Some(Arc::clone(&runner));

        let tx = self.bg_tx.clone();
        thread::spawn(move || match runner.test_connection() {
            Ok(home_dir) => {
                let _ = tx.send(BgMsg::ConnectionSuccess { home_dir });
            }
            Err(e) => {
                let _ = tx.send(BgMsg::ConnectionError(e));
            }
        });
    }

    fn connect_sftp(&mut self, conn: ConnectionConfig, password: Option<String>) {
        self.connecting = true;
        self.info_msg = None;
        self.spinner.start("Connecting via SFTP...");

        let host = conn.host.clone();
        let port = conn.port;
        let user = conn.user.clone();
        let auth = conn.auth.clone();

        log::info!(
            "connect_sftp: {}@{}:{} (password={})",
            user,
            host,
            port,
            password.is_some()
        );

        self.connection = Some(conn);
        let tx = self.bg_tx.clone();

        thread::spawn(move || {
            log::info!("connect_sftp thread: starting SftpBackend::connect");
            let result = if let Some(pw) = password {
                crate::transfer::sftp_backend::SftpBackend::connect_with_password(
                    &host, port, &user, &pw,
                )
            } else {
                crate::transfer::sftp_backend::SftpBackend::connect(&host, port, &user, &auth)
            };

            match result {
                Ok(backend) => {
                    let home = backend.home_dir().unwrap_or_else(|_| "/".to_string());
                    log::info!("connect_sftp thread: success, home={}", home);
                    let _ = tx.send(BgMsg::ConnectionReady {
                        backend: std::sync::Arc::new(backend),
                        home_dir: home,
                    });
                }
                Err(e) => {
                    log::error!("connect_sftp thread: failed: {}", e);
                    let _ = tx.send(BgMsg::ConnectionError(e));
                }
            }
            log::info!("connect_sftp thread: done");
        });
    }

    fn connect_ftp(&mut self, conn: ConnectionConfig, password: String) {
        self.connecting = true;
        self.info_msg = None;
        self.spinner.start("Connecting via FTP...");

        let host = conn.host.clone();
        let port = conn.port;
        let user = conn.user.clone();

        log::info!("connect_ftp: {}@{}:{}", user, host, port);

        self.connection = Some(conn);
        let tx = self.bg_tx.clone();

        thread::spawn(move || {
            log::info!("connect_ftp thread: starting FtpBackend::connect");
            match crate::transfer::ftp_backend::FtpBackend::connect(&host, port, &user, &password) {
                Ok(backend) => {
                    let home = backend.home_dir().unwrap_or_else(|_| "/".to_string());
                    log::info!("connect_ftp thread: success, home={}", home);
                    let _ = tx.send(BgMsg::ConnectionReady {
                        backend: std::sync::Arc::new(backend),
                        home_dir: home,
                    });
                }
                Err(e) => {
                    log::error!("connect_ftp thread: failed: {}", e);
                    let _ = tx.send(BgMsg::ConnectionError(e));
                }
            }
            log::info!("connect_ftp thread: done");
        });
    }

    fn run_password_connect_interactive(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> io::Result<()> {
        let conn = match self.pending_password_connect.take() {
            Some(c) => c,
            None => return Ok(()),
        };

        // Suspend TUI
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            crossterm::event::DisableMouseCapture,
            LeaveAlternateScreen
        )?;

        // Build SSH command to establish ControlMaster session.
        // When connecting via an ssh_config alias, pass only the alias so ssh
        // applies all matching Host blocks (wildcards, ProxyJump, IdentityFile...).
        let (target, control_path, use_port) = if let Some(alias) = &conn.ssh_alias {
            let sanitized: String = alias
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            (
                alias.clone(),
                format!("/tmp/lt-ssh-alias-{}", sanitized),
                false,
            )
        } else {
            let target = if conn.user.is_empty() {
                conn.host.clone()
            } else {
                format!("{}@{}", conn.user, conn.host)
            };
            let control_path = format!("/tmp/lt-ssh-{}@{}:{}", conn.user, conn.host, conn.port);
            (target, control_path, true)
        };

        eprintln!("Connecting to {} (password auth)...", conn.label);
        eprintln!("Type your password when prompted.\n");

        let mut args: Vec<String> = vec![
            "-o".into(),
            "ConnectTimeout=10".into(),
            "-o".into(),
            format!("ControlPath={}", control_path),
            "-o".into(),
            "ControlMaster=auto".into(),
            "-o".into(),
            "ControlPersist=600".into(),
        ];
        if use_port {
            args.push("-p".into());
            args.push(conn.port.to_string());
        }
        args.push(target);
        args.push("echo ok && echo $HOME".into());

        let status = std::process::Command::new(&self.config.ssh_bin)
            .args(&args)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .output();

        // Resume TUI
        enable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;
        terminal.clear()?;

        match status {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let lines: Vec<&str> = stdout.trim().lines().collect();
                let home_dir = if lines.len() >= 2 && lines[0] == "ok" {
                    lines[1].to_string()
                } else {
                    "/".to_string()
                };

                // Now connect with the established ControlMaster session
                self.connect_ssh(conn);
                // Directly send success since ControlMaster is already up
                let _ = self.bg_tx.send(BgMsg::ConnectionSuccess { home_dir });
            }
            Ok(_) => {
                self.info_msg = Some("Connection failed: authentication error".to_string());
            }
            Err(e) => {
                self.info_msg = Some(format!("Connection failed: {}", e));
            }
        }

        Ok(())
    }

    fn spawn_load_remote(&mut self, path: &str) {
        let runner = match &self.runner {
            Some(r) => Arc::clone(r),
            None => return,
        };
        let path = path.to_string();
        let tx = self.bg_tx.clone();
        self.loading_remote = true;
        self.spinner.start("Loading remote files...");

        thread::spawn(move || match runner.list_dir(&path) {
            Ok(files) => {
                let _ = tx.send(BgMsg::RemoteFilesLoaded(files));
            }
            Err(e) => {
                let _ = tx.send(BgMsg::RemoteFilesError(e));
            }
        });
    }

    // --- Background message processing ---

    fn process_bg_messages(&mut self) {
        while let Ok(msg) = self.bg_rx.try_recv() {
            match msg {
                BgMsg::ConnectionSuccess { home_dir } => {
                    self.connecting = false;
                    self.spinner.stop();
                    self.screen = AppScreen::FileBrowser;

                    let (w, h) = crossterm::terminal::size().unwrap_or((120, 40));
                    self.layout = compute_layout(w, h, false);
                    self.status_bar.set_hints(browser_hints());

                    if let Some(ref conn) = self.connection {
                        self.status_bar.set_connection_info(&conn.label);
                    }

                    self.remote_files.set_dir(&home_dir);
                    self.spawn_load_remote(&home_dir);
                }
                BgMsg::ConnectionReady { backend, home_dir } => {
                    self.connecting = false;
                    self.spinner.stop();
                    self.runner = Some(backend);
                    self.screen = AppScreen::FileBrowser;

                    let (w, h) = crossterm::terminal::size().unwrap_or((120, 40));
                    self.layout = compute_layout(w, h, false);
                    self.status_bar.set_hints(browser_hints());

                    if let Some(ref conn) = self.connection {
                        self.status_bar.set_connection_info(&conn.label);
                    }

                    self.remote_files.set_dir(&home_dir);
                    self.spawn_load_remote(&home_dir);

                    // Propose to save only for manual connections (not already saved ones)
                    if self.is_manual_connect {
                        self.is_manual_connect = false;
                        self.confirm.show("Save this connection for later?");
                        self.pending_action = Some(PendingAction::SaveConnection);
                    }
                }
                BgMsg::ConnectionError(e) => {
                    self.connecting = false;
                    self.spinner.stop();
                    self.info_msg = Some(format!("Connection failed: {}", e));
                    self.connection = None;
                    self.runner = None;
                    log::error!("connection error: {e}");
                }
                BgMsg::RemoteFilesLoaded(files) => {
                    self.loading_remote = false;
                    self.spinner.stop();
                    self.remote_files.set_files(files);
                }
                BgMsg::RemoteFilesError(e) => {
                    self.loading_remote = false;
                    self.spinner.stop();
                    self.info_msg = Some(format!("Error: {}", e));
                    log::error!("remote files error: {e}");
                }
                BgMsg::TransferProgress {
                    job_id,
                    percent,
                    speed,
                } => {
                    self.transfers.update_progress(job_id, percent, speed);
                }
                BgMsg::TransferComplete { job_id } => {
                    // Determine direction before marking complete
                    let direction = self
                        .transfers
                        .jobs
                        .iter()
                        .find(|j| j.id == job_id)
                        .map(|j| j.direction.clone());
                    self.transfers.complete_job(job_id);
                    // Refresh the destination pane
                    match direction {
                        Some(TransferDirection::Upload) => {
                            // Uploaded to remote -> refresh remote
                            let dir = self.remote_files.current_dir.clone();
                            if !dir.is_empty() {
                                self.spawn_load_remote(&dir);
                            }
                        }
                        Some(TransferDirection::Download) => {
                            // Downloaded to local -> refresh local
                            self.local_files.load_dir();
                        }
                        None => {}
                    }
                }
                BgMsg::TransferError { job_id, error } => {
                    self.transfers.fail_job(job_id, error.clone());
                    self.info_msg = Some(format!("Transfer failed: {}", error));
                }
                BgMsg::OperationSuccess { is_remote, message } => {
                    self.spinner.stop();
                    log::info!("operation success: {message}");
                    if is_remote {
                        let dir = self.remote_files.current_dir.clone();
                        self.spawn_load_remote(&dir);
                    } else {
                        self.local_files.load_dir();
                    }
                }
                BgMsg::OperationError(e) => {
                    self.spinner.stop();
                    self.info_msg = Some(format!("Error: {}", e));
                    log::error!("operation error: {e}");
                }
            }
        }
    }

    // --- File operations ---

    fn start_copy(&mut self) {
        match self.active_pane {
            ActivePane::Local => {
                let entry = match self.local_files.selected() {
                    Some(e) if e.name != ".." => e.clone(),
                    _ => return,
                };
                let local_path = format!("{}/{}", self.local_files.current_dir, entry.name);
                let remote_dir = self.remote_files.current_dir.clone();
                let remote_path = if remote_dir.ends_with('/') {
                    format!("{}{}", remote_dir, entry.name)
                } else {
                    format!("{}/{}", remote_dir, entry.name)
                };

                // Check if destination exists on remote (look in loaded file list)
                let exists_on_remote = self.remote_files.files.iter().any(|f| f.name == entry.name);
                if exists_on_remote {
                    self.pending_action = Some(PendingAction::OverwriteUpload {
                        local_path,
                        remote_path: if entry.is_dir {
                            remote_dir
                        } else {
                            remote_path
                        },
                        tar: false,
                    });
                    self.confirm.show(&format!(
                        "'{}' already exists on remote. Overwrite?",
                        entry.name
                    ));
                } else if entry.is_dir {
                    self.do_upload_dir(&local_path, &remote_dir, &entry.name);
                } else {
                    self.do_upload(&local_path, &remote_path, &entry.name, entry.size);
                }
            }
            ActivePane::Remote => {
                let entry = match self.remote_files.selected() {
                    Some(e) if e.name != ".." => e.clone(),
                    _ => return,
                };
                let remote_path = if self.remote_files.current_dir.ends_with('/') {
                    format!("{}{}", self.remote_files.current_dir, entry.name)
                } else {
                    format!("{}/{}", self.remote_files.current_dir, entry.name)
                };
                let local_dest = self.local_files.current_dir.clone();
                let local_path = format!("{}/{}", local_dest, entry.name);

                // Check if destination exists locally
                if std::path::Path::new(&local_path).exists() {
                    self.pending_action = Some(PendingAction::OverwriteDownload {
                        remote_path,
                        local_path: if entry.is_dir { local_dest } else { local_path },
                        tar: false,
                    });
                    self.confirm.show(&format!(
                        "'{}' already exists locally. Overwrite?",
                        entry.name
                    ));
                } else if entry.is_dir {
                    self.do_download_dir(&remote_path, &local_dest, &entry.name);
                } else {
                    self.do_download(&remote_path, &local_path, &entry.name, entry.size);
                }
            }
        }
    }

    fn start_copy_tar(&mut self) {
        match self.active_pane {
            ActivePane::Local => {
                let entry = match self.local_files.selected() {
                    Some(e) if e.name != ".." => e.clone(),
                    _ => return,
                };
                if entry.is_dir {
                    return;
                }
                let local_path = format!("{}/{}", self.local_files.current_dir, entry.name);
                let remote_dir = self.remote_files.current_dir.clone();

                let exists_on_remote = self.remote_files.files.iter().any(|f| f.name == entry.name);
                if exists_on_remote {
                    self.pending_action = Some(PendingAction::OverwriteUpload {
                        local_path,
                        remote_path: remote_dir,
                        tar: true,
                    });
                    self.confirm.show(&format!(
                        "'{}' already exists on remote. Overwrite?",
                        entry.name
                    ));
                } else {
                    self.do_upload_tar(&local_path, &remote_dir, &entry.name, entry.size);
                }
            }
            ActivePane::Remote => {
                let entry = match self.remote_files.selected() {
                    Some(e) if e.name != ".." => e.clone(),
                    _ => return,
                };
                if entry.is_dir {
                    return;
                }
                let remote_path = if self.remote_files.current_dir.ends_with('/') {
                    format!("{}{}", self.remote_files.current_dir, entry.name)
                } else {
                    format!("{}/{}", self.remote_files.current_dir, entry.name)
                };
                let local_dest = self.local_files.current_dir.clone();
                let local_path = format!("{}/{}", local_dest, entry.name);

                if std::path::Path::new(&local_path).exists() {
                    self.pending_action = Some(PendingAction::OverwriteDownload {
                        remote_path,
                        local_path: local_dest,
                        tar: true,
                    });
                    self.confirm.show(&format!(
                        "'{}' already exists locally. Overwrite?",
                        entry.name
                    ));
                } else {
                    self.do_download_tar(&remote_path, &local_dest, &entry.name, entry.size);
                }
            }
        }
    }

    fn do_upload(&mut self, local_path: &str, remote_path: &str, file_name: &str, file_size: u64) {
        let job_id = self.next_job_id;
        self.next_job_id += 1;

        self.transfers.jobs.push(TransferJob {
            id: job_id,
            source: local_path.to_string(),
            destination: remote_path.to_string(),
            direction: TransferDirection::Upload,
            file_name: file_name.to_string(),
            file_size,
            status: TransferStatus::Queued,
        });

        let runner = match &self.runner {
            Some(r) => Arc::clone(r),
            None => return,
        };

        let local = local_path.to_string();
        let remote = remote_path.to_string();
        let tx = self.bg_tx.clone();

        thread::spawn(move || match runner.upload(&local, &remote) {
            Ok(handle) => {
                Self::monitor_transfer(handle, job_id, tx);
            }
            Err(e) => {
                let _ = tx.send(BgMsg::TransferError { job_id, error: e });
            }
        });
    }

    fn do_download(
        &mut self,
        remote_path: &str,
        local_path: &str,
        file_name: &str,
        file_size: u64,
    ) {
        let job_id = self.next_job_id;
        self.next_job_id += 1;

        self.transfers.jobs.push(TransferJob {
            id: job_id,
            source: remote_path.to_string(),
            destination: local_path.to_string(),
            direction: TransferDirection::Download,
            file_name: file_name.to_string(),
            file_size,
            status: TransferStatus::Queued,
        });

        let runner = match &self.runner {
            Some(r) => Arc::clone(r),
            None => return,
        };

        let remote = remote_path.to_string();
        let local = local_path.to_string();
        let tx = self.bg_tx.clone();

        thread::spawn(move || match runner.download(&remote, &local) {
            Ok(handle) => {
                Self::monitor_transfer(handle, job_id, tx);
            }
            Err(e) => {
                let _ = tx.send(BgMsg::TransferError { job_id, error: e });
            }
        });
    }

    fn do_upload_dir(&mut self, local_path: &str, remote_dest: &str, dir_name: &str) {
        let job_id = self.next_job_id;
        self.next_job_id += 1;

        self.transfers.jobs.push(TransferJob {
            id: job_id,
            source: local_path.to_string(),
            destination: remote_dest.to_string(),
            direction: TransferDirection::Upload,
            file_name: format!("{}/", dir_name),
            file_size: 0,
            status: TransferStatus::Queued,
        });

        let runner = match &self.runner {
            Some(r) => Arc::clone(r),
            None => return,
        };
        let local = local_path.to_string();
        let remote = remote_dest.to_string();
        let tx = self.bg_tx.clone();

        thread::spawn(move || match runner.upload_dir(&local, &remote) {
            Ok(handle) => Self::monitor_transfer(handle, job_id, tx),
            Err(e) => {
                let _ = tx.send(BgMsg::TransferError { job_id, error: e });
            }
        });
    }

    fn do_download_dir(&mut self, remote_path: &str, local_dest: &str, dir_name: &str) {
        let job_id = self.next_job_id;
        self.next_job_id += 1;

        self.transfers.jobs.push(TransferJob {
            id: job_id,
            source: remote_path.to_string(),
            destination: local_dest.to_string(),
            direction: TransferDirection::Download,
            file_name: format!("{}/", dir_name),
            file_size: 0,
            status: TransferStatus::Queued,
        });

        let runner = match &self.runner {
            Some(r) => Arc::clone(r),
            None => return,
        };
        let remote = remote_path.to_string();
        let local = local_dest.to_string();
        let tx = self.bg_tx.clone();

        thread::spawn(move || match runner.download_dir(&remote, &local) {
            Ok(handle) => Self::monitor_transfer(handle, job_id, tx),
            Err(e) => {
                let _ = tx.send(BgMsg::TransferError { job_id, error: e });
            }
        });
    }

    fn do_upload_tar(
        &mut self,
        local_path: &str,
        remote_dest: &str,
        file_name: &str,
        file_size: u64,
    ) {
        let job_id = self.next_job_id;
        self.next_job_id += 1;

        self.transfers.jobs.push(TransferJob {
            id: job_id,
            source: local_path.to_string(),
            destination: remote_dest.to_string(),
            direction: TransferDirection::Upload,
            file_name: file_name.to_string(),
            file_size,
            status: TransferStatus::Queued,
        });

        let runner = match &self.runner {
            Some(r) => Arc::clone(r),
            None => return,
        };
        let local = local_path.to_string();
        let remote = remote_dest.to_string();
        let tx = self.bg_tx.clone();

        thread::spawn(move || match runner.upload_tar(&local, &remote) {
            Ok(handle) => Self::monitor_transfer(handle, job_id, tx),
            Err(e) => {
                let _ = tx.send(BgMsg::TransferError { job_id, error: e });
            }
        });
    }

    fn do_download_tar(
        &mut self,
        remote_path: &str,
        local_dest: &str,
        file_name: &str,
        file_size: u64,
    ) {
        let job_id = self.next_job_id;
        self.next_job_id += 1;

        self.transfers.jobs.push(TransferJob {
            id: job_id,
            source: remote_path.to_string(),
            destination: local_dest.to_string(),
            direction: TransferDirection::Download,
            file_name: file_name.to_string(),
            file_size,
            status: TransferStatus::Queued,
        });

        let runner = match &self.runner {
            Some(r) => Arc::clone(r),
            None => return,
        };
        let remote = remote_path.to_string();
        let local = local_dest.to_string();
        let tx = self.bg_tx.clone();

        thread::spawn(move || match runner.download_tar(&remote, &local) {
            Ok(handle) => Self::monitor_transfer(handle, job_id, tx),
            Err(e) => {
                let _ = tx.send(BgMsg::TransferError { job_id, error: e });
            }
        });
    }

    fn monitor_transfer(
        handle: crate::transfer::exec::StreamHandle,
        job_id: usize,
        tx: mpsc::Sender<BgMsg>,
    ) {
        let progress_re = regex::Regex::new(r"(\d+)%(?:\s+\S+\s+(\S+/s))?").ok();

        while let Ok(line) = handle.rx.recv() {
            if line.done {
                if let Some(ref err) = line.err {
                    let _ = tx.send(BgMsg::TransferError {
                        job_id,
                        error: err.clone(),
                    });
                } else {
                    let _ = tx.send(BgMsg::TransferComplete { job_id });
                }
                return;
            }

            // Try to parse SCP progress
            if let Some(ref re) = progress_re {
                if let Some(caps) = re.captures(&line.text) {
                    if let Ok(pct) = caps[1].parse::<u8>() {
                        let speed = caps.get(2).map_or("", |m| m.as_str()).to_string();
                        let _ = tx.send(BgMsg::TransferProgress {
                            job_id,
                            percent: pct,
                            speed,
                        });
                    }
                }
            }
        }
    }

    fn start_delete(&mut self) {
        match self.active_pane {
            ActivePane::Local => {
                if let Some(entry) = self.local_files.selected() {
                    if entry.name == ".." {
                        return;
                    }
                    let path = format!("{}/{}", self.local_files.current_dir, entry.name);
                    self.pending_action = Some(PendingAction::DeleteLocal { path });
                    self.confirm.show(&format!("Delete '{}'?", entry.name));
                }
            }
            ActivePane::Remote => {
                if let Some(entry) = self.remote_files.selected() {
                    if entry.name == ".." {
                        return;
                    }
                    let path = if self.remote_files.current_dir.ends_with('/') {
                        format!("{}{}", self.remote_files.current_dir, entry.name)
                    } else {
                        format!("{}/{}", self.remote_files.current_dir, entry.name)
                    };
                    self.pending_action = Some(PendingAction::DeleteRemote { path });
                    self.confirm.show(&format!("Delete '{}'?", entry.name));
                }
            }
        }
    }

    fn start_rename(&mut self) {
        let current_name = match self.active_pane {
            ActivePane::Local => self.local_files.selected().map(|e| e.name.clone()),
            ActivePane::Remote => self.remote_files.selected().map(|e| e.name.clone()),
        };

        if let Some(name) = current_name {
            if name == ".." {
                return;
            }
            self.input_mode = InputMode::Rename;
            self.input.show_with_value("Rename", "new name...", &name);
        }
    }

    fn execute_pending_action(&mut self) {
        if let Some(action) = self.pending_action.take() {
            match action {
                PendingAction::DeleteLocal { path } => {
                    if let Err(e) =
                        std::fs::remove_file(&path).or_else(|_| std::fs::remove_dir_all(&path))
                    {
                        self.info_msg = Some(format!("Delete failed: {}", e));
                    } else {
                        self.local_files.load_dir();
                    }
                }
                PendingAction::DeleteRemote { path } => {
                    let runner = match &self.runner {
                        Some(r) => Arc::clone(r),
                        None => return,
                    };
                    let tx = self.bg_tx.clone();
                    self.spinner.start("Deleting...");
                    thread::spawn(move || match runner.delete(&path) {
                        Ok(()) => {
                            let _ = tx.send(BgMsg::OperationSuccess {
                                is_remote: true,
                                message: format!("Deleted {}", path),
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(BgMsg::OperationError(e));
                        }
                    });
                }
                PendingAction::OverwriteUpload {
                    local_path,
                    remote_path,
                    tar,
                } => {
                    let name = std::path::Path::new(&local_path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    if tar {
                        self.do_upload_tar(&local_path, &remote_path, &name, 0);
                    } else if std::path::Path::new(&local_path).is_dir() {
                        self.do_upload_dir(&local_path, &remote_path, &name);
                    } else {
                        self.do_upload(&local_path, &remote_path, &name, 0);
                    }
                }
                PendingAction::OverwriteDownload {
                    remote_path,
                    local_path,
                    tar,
                } => {
                    let name = remote_path
                        .trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or(&remote_path)
                        .to_string();
                    if tar {
                        self.do_download_tar(&remote_path, &local_path, &name, 0);
                    } else {
                        // Check if remote is a dir by looking at the file list
                        let is_dir = self
                            .remote_files
                            .files
                            .iter()
                            .any(|f| f.name == name && f.is_dir);
                        if is_dir {
                            self.do_download_dir(&remote_path, &local_path, &name);
                        } else {
                            self.do_download(&remote_path, &local_path, &name, 0);
                        }
                    }
                }
                PendingAction::SaveConnection => {
                    self.input_mode = InputMode::SaveConnectionName;
                    self.input
                        .show("Connection Name", "name for this connection...");
                }
                PendingAction::DeleteSavedConnection { index } => {
                    let mut conns = crate::transfer::connections::load();
                    if index < conns.entries.len() {
                        conns.entries.remove(index);
                        if let Err(e) = crate::transfer::connections::save(&conns) {
                            self.info_msg = Some(format!("Save error: {}", e));
                        }
                        self.connection_panel.reload_saved();
                    }
                }
            }
        }
    }

    fn do_mkdir(&mut self, name: &str) {
        if name.is_empty() {
            return;
        }

        match self.active_pane {
            ActivePane::Local => {
                let path = format!("{}/{}", self.local_files.current_dir, name);
                if let Err(e) = std::fs::create_dir_all(&path) {
                    self.info_msg = Some(format!("mkdir failed: {}", e));
                } else {
                    self.local_files.load_dir();
                }
            }
            ActivePane::Remote => {
                let runner = match &self.runner {
                    Some(r) => Arc::clone(r),
                    None => return,
                };
                let dir = self.remote_files.current_dir.clone();
                let path = if dir.ends_with('/') {
                    format!("{}{}", dir, name)
                } else {
                    format!("{}/{}", dir, name)
                };
                let tx = self.bg_tx.clone();
                self.spinner.start("Creating directory...");
                thread::spawn(move || match runner.mkdir(&path) {
                    Ok(()) => {
                        let _ = tx.send(BgMsg::OperationSuccess {
                            is_remote: true,
                            message: format!("Created {}", path),
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(BgMsg::OperationError(e));
                    }
                });
            }
        }
    }

    fn do_rename(&mut self, new_name: &str) {
        if new_name.is_empty() {
            return;
        }

        match self.active_pane {
            ActivePane::Local => {
                if let Some(entry) = self.local_files.selected() {
                    let old_path = format!("{}/{}", self.local_files.current_dir, entry.name);
                    let new_path = format!("{}/{}", self.local_files.current_dir, new_name);
                    if let Err(e) = std::fs::rename(&old_path, &new_path) {
                        self.info_msg = Some(format!("Rename failed: {}", e));
                    } else {
                        self.local_files.load_dir();
                    }
                }
            }
            ActivePane::Remote => {
                if let Some(entry) = self.remote_files.selected() {
                    let runner = match &self.runner {
                        Some(r) => Arc::clone(r),
                        None => return,
                    };
                    let dir = &self.remote_files.current_dir;
                    let old_path = if dir.ends_with('/') {
                        format!("{}{}", dir, entry.name)
                    } else {
                        format!("{}/{}", dir, entry.name)
                    };
                    let new_path = if dir.ends_with('/') {
                        format!("{}{}", dir, new_name)
                    } else {
                        format!("{}/{}", dir, new_name)
                    };
                    let tx = self.bg_tx.clone();
                    self.spinner.start("Renaming...");
                    thread::spawn(move || match runner.rename(&old_path, &new_path) {
                        Ok(()) => {
                            let _ = tx.send(BgMsg::OperationSuccess {
                                is_remote: true,
                                message: format!("Renamed {} -> {}", old_path, new_path),
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(BgMsg::OperationError(e));
                        }
                    });
                }
            }
        }
    }

    fn enter_remote_dir(&mut self) {
        if let Some(path) = self.remote_files.enter_path() {
            self.remote_files.set_dir(&path);
            self.remote_files.files.clear();
            self.remote_files.set_files(vec![]);
            self.spawn_load_remote(&path);
        }
    }

    fn go_remote_parent(&mut self) {
        if let Some(path) = self.remote_files.parent_path() {
            self.remote_files.set_dir(&path);
            self.remote_files.files.clear();
            self.remote_files.set_files(vec![]);
            self.spawn_load_remote(&path);
        }
    }

    fn refresh_current_panel(&mut self) {
        match self.active_pane {
            ActivePane::Local => {
                self.local_files.load_dir();
            }
            ActivePane::Remote => {
                let dir = self.remote_files.current_dir.clone();
                if !dir.is_empty() {
                    self.spawn_load_remote(&dir);
                }
            }
        }
    }
}
