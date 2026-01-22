use parking_lot::Mutex;
use russh::ChannelMsg;
use std::sync::Arc;
use tokio::runtime::Runtime as TokioRuntime;
use tokio::sync::Mutex as TokioMutex;
use uuid::Uuid;

use gpui::*;

use crate::config::AppConfig;
use crate::session::{LocalSession, Session, SessionGroup, SessionManager, SshSession};
use crate::terminal::{SshBackend, Terminal, TerminalConfig, TerminalSize};

/// Represents an open terminal tab
pub struct TerminalTab {
    /// Unique ID for this tab
    pub id: Uuid,
    /// Reference to the session (if any)
    pub session_id: Option<Uuid>,
    /// The terminal instance
    pub terminal: Arc<Mutex<Terminal>>,
    /// Tab title (may differ from terminal title)
    pub title: String,
    /// Whether the tab has unsaved state
    pub dirty: bool,
    /// Color scheme override for this tab
    pub color_scheme: Option<String>,
}

impl TerminalTab {
    /// Create a new terminal tab
    pub fn new(terminal: Terminal, session_id: Option<Uuid>, title: String, color_scheme: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            session_id,
            terminal: Arc::new(Mutex::new(terminal)),
            title,
            dirty: false,
            color_scheme,
        }
    }
}

/// Main application state
pub struct RedPillApp {
    /// Application configuration
    pub config: AppConfig,
    /// Session manager for CRUD operations
    pub session_manager: SessionManager,
    /// Open terminal tabs
    pub tabs: Vec<TerminalTab>,
    /// Currently active tab index
    pub active_tab: Option<usize>,
    /// Whether the session tree is visible
    pub session_tree_visible: bool,
}

impl RedPillApp {
    /// Create a new application instance
    pub fn new() -> Self {
        let config = AppConfig::load().unwrap_or_default();
        let session_tree_visible = config.session_tree.visible;
        let session_manager = SessionManager::new().unwrap_or_else(|e| {
            tracing::error!("Failed to load sessions: {}", e);
            SessionManager::default()
        });

        Self {
            config,
            session_manager,
            tabs: Vec::new(),
            active_tab: None,
            session_tree_visible,
        }
    }

    /// Open a new local terminal tab
    pub fn open_local_terminal(&mut self) -> Result<Uuid, String> {
        let config = TerminalConfig::default();
        let terminal =
            Terminal::new_local(config).map_err(|e| format!("Failed to create terminal: {}", e))?;

        let tab = TerminalTab::new(terminal, None, "Local".to_string(), None);
        let id = tab.id;

        self.tabs.push(tab);
        self.active_tab = Some(self.tabs.len() - 1);

        tracing::info!("Opened local terminal tab: {}", id);
        Ok(id)
    }

    /// Open a terminal for an SSH session (sync wrapper that spawns async task)
    pub fn open_ssh_session(&mut self, session_id: Uuid, runtime: &TokioRuntime) -> Result<Uuid, String> {
        let session = self
            .session_manager
            .get_session(session_id)
            .ok_or_else(|| "Session not found".to_string())?;

        let title = session.name().to_string();

        // Get SSH session config
        let (ssh_session, color_scheme) = match session {
            Session::Ssh(ssh) => (ssh.clone(), ssh.color_scheme.clone()),
            Session::Local(_) => {
                // For local sessions, just open a local terminal
                return self.open_local_terminal();
            }
        };

        // Create SSH backend (not connected yet)
        let backend = SshBackend::new(ssh_session);

        // Create terminal in SSH mode with tokio handle for async operations
        let config = TerminalConfig::default();
        let terminal = Terminal::new_ssh(config, backend, runtime.handle().clone())
            .map_err(|e| format!("Failed to create SSH terminal: {}", e))?;

        // Get the backend for the reader task
        let backend_arc = terminal
            .ssh_backend()
            .expect("SSH terminal should have backend");

        let terminal_arc = Arc::new(Mutex::new(terminal));

        // Spawn the async connection and reader task on Tokio runtime
        let terminal_weak = Arc::downgrade(&terminal_arc);
        let backend_for_connect = backend_arc.clone();

        runtime.spawn(async move {
            // Connect to SSH server and take channel for I/O
            let io_handles = {
                let mut backend = backend_for_connect.lock().await;
                match backend.connect().await {
                    Ok(()) => {
                        tracing::info!("SSH connection established");
                        // Take the channel out of the backend for direct I/O
                        backend.take_channel_for_io()
                    }
                    Err(e) => {
                        tracing::error!("SSH connection failed: {}", e);
                        // Display error message in terminal with nice formatting
                        if let Some(term_arc) = terminal_weak.upgrade() {
                            let term = term_arc.lock();
                            let error_text = e.to_string();
                            let error_msg = format!(
                                "\x1b[2J\x1b[H\r\n\
                                \x1b[1;31m  Connection Failed\x1b[0m\r\n\
                                \r\n\
                                \x1b[33m  {}\x1b[0m\r\n",
                                error_text
                            );
                            term.write_to_pty(error_msg.as_bytes());
                        }
                        return;
                    }
                }
            };

            let (channel, write_rx) = match io_handles {
                Some(handles) => handles,
                None => {
                    tracing::error!("Failed to get SSH channel for I/O");
                    return;
                }
            };

            // Create resize channel for sending window size changes to I/O loop
            let (resize_tx, resize_rx) = tokio::sync::mpsc::unbounded_channel();

            // Update the terminal's write_tx and resize_tx to point to our new channels
            // Also get the current size to send immediately after setup
            let write_tx = backend_for_connect.lock().await.get_write_sender();
            let current_size = if let (Some(term_arc), Some(tx)) = (terminal_weak.upgrade(), write_tx) {
                let mut term = term_arc.lock();
                term.set_write_tx(tx);
                term.set_resize_tx(resize_tx);
                Some(term.size())
            } else {
                None
            };

            // Send immediate resize with the terminal's current size
            // This ensures the SSH server gets correct dimensions even if the first
            // UI paint happened before the channels were connected
            if let Some(size) = current_size {
                if size.cols > 0 && size.rows > 0 {
                    tracing::info!("SSH immediate resize after channel setup: {}x{} ({}x{} px)",
                        size.cols, size.rows, size.pixel_width, size.pixel_height);
                    if let Err(e) = channel.window_change(
                        size.cols as u32,
                        size.rows as u32,
                        size.pixel_width as u32,
                        size.pixel_height as u32,
                    ).await {
                        tracing::error!("SSH immediate resize error: {}", e);
                    }
                }
            }

            // Start the combined I/O loop using select!
            spawn_ssh_io_loop(terminal_weak, backend_for_connect, channel, write_rx, resize_rx).await;
        });

        let tab = TerminalTab {
            id: Uuid::new_v4(),
            session_id: Some(session_id),
            terminal: terminal_arc,
            title,
            dirty: false,
            color_scheme,
        };
        let id = tab.id;

        self.tabs.push(tab);
        self.active_tab = Some(self.tabs.len() - 1);

        tracing::info!(
            "Opened SSH session tab: {} for session: {}",
            id,
            session_id
        );
        Ok(id)
    }

    /// Close a terminal tab
    pub fn close_tab(&mut self, tab_id: Uuid) {
        if let Some(index) = self.tabs.iter().position(|t| t.id == tab_id) {
            self.tabs.remove(index);

            // Adjust active tab
            if self.tabs.is_empty() {
                self.active_tab = None;
            } else if let Some(active) = self.active_tab {
                if active >= self.tabs.len() {
                    self.active_tab = Some(self.tabs.len() - 1);
                } else if active > index {
                    self.active_tab = Some(active - 1);
                }
            }

            tracing::info!("Closed tab: {}", tab_id);
        }
    }

    /// Get the currently active tab
    pub fn active_tab(&self) -> Option<&TerminalTab> {
        self.active_tab.and_then(|i| self.tabs.get(i))
    }

    /// Get a mutable reference to the active tab
    pub fn active_tab_mut(&mut self) -> Option<&mut TerminalTab> {
        self.active_tab.and_then(move |i| self.tabs.get_mut(i))
    }

    /// Set the active tab by index
    pub fn set_active_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = Some(index);
        }
    }

    /// Set the active tab by ID
    pub fn set_active_tab_by_id(&mut self, tab_id: Uuid) {
        if let Some(index) = self.tabs.iter().position(|t| t.id == tab_id) {
            self.active_tab = Some(index);
        }
    }

    /// Get a tab by ID
    pub fn get_tab(&self, tab_id: Uuid) -> Option<&TerminalTab> {
        self.tabs.iter().find(|t| t.id == tab_id)
    }

    /// Toggle session tree visibility
    pub fn toggle_session_tree(&mut self) {
        self.session_tree_visible = !self.session_tree_visible;
        self.config.session_tree.visible = self.session_tree_visible;
        let _ = self.config.save();
    }

    /// Count the number of active SSH connections (tabs with session_id)
    #[must_use]
    pub fn active_ssh_connection_count(&self) -> usize {
        self.tabs.iter().filter(|tab| tab.session_id.is_some()).count()
    }

    /// Mass connect to all sessions in a group
    pub fn mass_connect(&mut self, group_id: Uuid, runtime: &TokioRuntime) -> Vec<Result<Uuid, String>> {
        let session_ids = self
            .session_manager
            .get_all_sessions_in_group_recursive(group_id);

        session_ids
            .into_iter()
            .map(|id| self.open_ssh_session(id, runtime))
            .collect()
    }

    /// Save application state
    pub fn save(&mut self) -> Result<(), String> {
        self.session_manager
            .save()
            .map_err(|e| format!("Failed to save sessions: {}", e))?;

        self.config
            .save()
            .map_err(|e| format!("Failed to save config: {}", e))?;

        Ok(())
    }

    /// Get all top-level groups
    pub fn top_level_groups(&self) -> Vec<&SessionGroup> {
        self.session_manager.top_level_groups()
    }

    /// Get child groups of a parent
    pub fn child_groups(&self, parent_id: Uuid) -> Vec<&SessionGroup> {
        self.session_manager.child_groups(parent_id)
    }

    /// Get sessions in a group
    pub fn sessions_in_group(&self, group_id: Uuid) -> Vec<&Session> {
        self.session_manager.sessions_in_group(group_id)
    }

    /// Get ungrouped sessions
    pub fn ungrouped_sessions(&self) -> Vec<&Session> {
        self.session_manager.ungrouped_sessions()
    }

    /// Add a new group
    pub fn add_group(&mut self, name: String, parent_id: Option<Uuid>) -> Uuid {
        let group = if let Some(pid) = parent_id {
            SessionGroup::new_nested(name, pid)
        } else {
            SessionGroup::new(name)
        };
        self.session_manager.add_group(group)
    }

    /// Add a new SSH session
    pub fn add_ssh_session(&mut self, session: SshSession) -> Uuid {
        self.session_manager.add_ssh_session(session)
    }

    /// Add a new local session
    pub fn add_local_session(&mut self, session: LocalSession) -> Uuid {
        self.session_manager.add_local_session(session)
    }

    /// Delete a session
    pub fn delete_session(&mut self, id: Uuid) -> Result<(), String> {
        // Close any tabs using this session
        let tabs_to_close: Vec<Uuid> = self
            .tabs
            .iter()
            .filter(|t| t.session_id == Some(id))
            .map(|t| t.id)
            .collect();

        for tab_id in tabs_to_close {
            self.close_tab(tab_id);
        }

        self.session_manager
            .delete_session(id)
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Delete a group
    pub fn delete_group(&mut self, id: Uuid, recursive: bool) -> Result<(), String> {
        if recursive {
            self.session_manager
                .delete_group_recursive(id)
                .map_err(|e| e.to_string())
        } else {
            self.session_manager
                .delete_group(id)
                .map_err(|e| e.to_string())?;
            Ok(())
        }
    }
}

/// Combined SSH I/O loop using tokio::select! for concurrent read/write/resize
///
/// This follows the recommended russh pattern where a single task handles
/// both reading from the channel and writing user input, using select!
/// to multiplex between them without locks.
async fn spawn_ssh_io_loop(
    terminal: std::sync::Weak<Mutex<Terminal>>,
    backend: Arc<TokioMutex<SshBackend>>,
    mut channel: russh::Channel<russh::client::Msg>,
    mut write_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    mut resize_rx: tokio::sync::mpsc::UnboundedReceiver<TerminalSize>,
) {
    loop {
        tokio::select! {
            // Handle user input (keyboard -> SSH)
            Some(data) = write_rx.recv() => {
                tracing::debug!("SSH write: sending {} bytes", data.len());
                if let Err(e) = channel.data(&data[..]).await {
                    tracing::error!("SSH write error: {}", e);
                    break;
                }
            }

            // Handle resize requests (window resize -> SSH PTY)
            Some(size) = resize_rx.recv() => {
                tracing::debug!("SSH resize: sending {}x{}", size.cols, size.rows);
                if let Err(e) = channel.window_change(
                    size.cols as u32,
                    size.rows as u32,
                    size.pixel_width as u32,
                    size.pixel_height as u32,
                ).await {
                    tracing::error!("SSH resize error: {}", e);
                    // Don't break on resize error - connection may still be usable
                }
            }

            // Handle SSH channel messages (SSH -> terminal)
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => {
                        if let Some(term_arc) = terminal.upgrade() {
                            let term = term_arc.lock();
                            term.write_to_pty(&data);
                        } else {
                            tracing::info!("Terminal dropped, stopping SSH I/O");
                            break;
                        }
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        // Handle stderr
                        if let Some(term_arc) = terminal.upgrade() {
                            let term = term_arc.lock();
                            term.write_to_pty(&data);
                        } else {
                            break;
                        }
                    }
                    Some(ChannelMsg::Eof) => {
                        tracing::info!("SSH channel EOF");
                        break;
                    }
                    Some(ChannelMsg::Close) => {
                        tracing::info!("SSH channel closed");
                        break;
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        tracing::info!("Remote process exited with status: {}", exit_status);
                        break;
                    }
                    Some(_) => {
                        // Other protocol messages (WindowAdjust, Success, etc.)
                        // Just continue - these don't need special handling
                    }
                    None => {
                        tracing::info!("SSH channel closed (None)");
                        break;
                    }
                }
            }
        }
    }

    // Clean up - close the channel
    let _ = channel.eof().await;
    let _ = channel.close().await;

    // Update backend state
    let mut b = backend.lock().await;
    let _ = b.close().await;
}

/// Attempt to reconnect to SSH server with exponential backoff
///
/// Returns true if reconnection succeeded and we should continue reading,
/// false if reconnection failed or terminal was dropped.
async fn attempt_reconnect(
    terminal: &std::sync::Weak<Mutex<Terminal>>,
    backend: &Arc<TokioMutex<SshBackend>>,
) -> bool {
    // Check if terminal still exists
    let term_arc = match terminal.upgrade() {
        Some(t) => t,
        None => {
            tracing::info!("Terminal dropped during reconnection");
            return false;
        }
    };

    // Display reconnection message to user
    {
        let term = term_arc.lock();
        let msg = "\r\n\x1b[1;33m  Connection lost. Attempting to reconnect...\x1b[0m\r\n";
        term.write_to_pty(msg.as_bytes());
    }

    // Attempt reconnection
    let result = {
        let mut b = backend.lock().await;
        b.reconnect().await
    };

    match result {
        Ok(()) => {
            // Display success message
            if let Some(term_arc) = terminal.upgrade() {
                let term = term_arc.lock();
                let msg = "\r\n\x1b[1;32m  Reconnected successfully!\x1b[0m\r\n";
                term.write_to_pty(msg.as_bytes());
            }
            true
        }
        Err(e) => {
            // Display failure message
            if let Some(term_arc) = terminal.upgrade() {
                let term = term_arc.lock();
                let msg = format!(
                    "\r\n\x1b[1;31m  Reconnection failed: {}\x1b[0m\r\n",
                    e
                );
                term.write_to_pty(msg.as_bytes());
            }
            false
        }
    }
}

/// Global application state wrapper
pub struct AppState {
    pub app: Arc<Mutex<RedPillApp>>,
    /// Tokio runtime for async SSH operations
    pub tokio_runtime: Arc<TokioRuntime>,
}

impl AppState {
    pub fn new() -> Self {
        let tokio_runtime = TokioRuntime::new().expect("Failed to create Tokio runtime");
        Self {
            app: Arc::new(Mutex::new(RedPillApp::new())),
            tokio_runtime: Arc::new(tokio_runtime),
        }
    }
}

impl Global for AppState {}
