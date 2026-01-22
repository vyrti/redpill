use parking_lot::Mutex;
use std::sync::Arc;
use tokio::runtime::Runtime as TokioRuntime;
use tokio::sync::Mutex as TokioMutex;
use uuid::Uuid;

use gpui::*;

use crate::config::AppConfig;
use crate::session::{LocalSession, Session, SessionGroup, SessionManager, SshSession};
use crate::terminal::{SshBackend, Terminal, TerminalConfig};

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
}

impl TerminalTab {
    /// Create a new terminal tab
    pub fn new(terminal: Terminal, session_id: Option<Uuid>, title: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            session_id,
            terminal: Arc::new(Mutex::new(terminal)),
            title,
            dirty: false,
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
        let session_manager = SessionManager::new().unwrap_or_else(|e| {
            tracing::error!("Failed to load sessions: {}", e);
            SessionManager::default()
        });

        Self {
            config,
            session_manager,
            tabs: Vec::new(),
            active_tab: None,
            session_tree_visible: true,
        }
    }

    /// Open a new local terminal tab
    pub fn open_local_terminal(&mut self) -> Result<Uuid, String> {
        let config = TerminalConfig::default();
        let terminal =
            Terminal::new_local(config).map_err(|e| format!("Failed to create terminal: {}", e))?;

        let tab = TerminalTab::new(terminal, None, "Local".to_string());
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
        let ssh_session = match session {
            Session::Ssh(ssh) => ssh.clone(),
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
            // Connect to SSH server
            let connect_result = {
                let mut backend = backend_for_connect.lock().await;
                backend.connect().await
            };

            match connect_result {
                Ok(()) => {
                    tracing::info!("SSH connection established");
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

            // Start reading from SSH and feeding to terminal
            spawn_ssh_reader(terminal_weak, backend_for_connect).await;
        });

        let tab = TerminalTab {
            id: Uuid::new_v4(),
            session_id: Some(session_id),
            terminal: terminal_arc,
            title,
            dirty: false,
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

/// Spawn an async task to read from SSH and feed data to terminal
async fn spawn_ssh_reader(
    terminal: std::sync::Weak<Mutex<Terminal>>,
    backend: Arc<TokioMutex<SshBackend>>,
) {
    let mut buf = vec![0u8; 8192];

    'outer: loop {
        // Read from SSH
        let read_result = {
            let mut b = backend.lock().await;
            b.read(&mut buf).await
        };

        match read_result {
            Ok(0) => {
                tracing::info!("SSH connection closed (EOF)");
                // Connection was cleanly closed, try to reconnect
                if !attempt_reconnect(&terminal, &backend).await {
                    break 'outer;
                }
            }
            Ok(n) => {
                // Feed data to terminal for display
                if let Some(term_arc) = terminal.upgrade() {
                    let term = term_arc.lock();
                    term.write_to_pty(&buf[..n]);
                } else {
                    // Terminal was dropped
                    tracing::info!("Terminal dropped, stopping SSH reader");
                    break 'outer;
                }
            }
            Err(e) => {
                tracing::error!("SSH read error: {}", e);
                // Connection error, try to reconnect
                if !attempt_reconnect(&terminal, &backend).await {
                    break 'outer;
                }
            }
        }
    }

    // Close SSH connection
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
