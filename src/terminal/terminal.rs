use alacritty_terminal::event::{Notify, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Config as TermConfig, Term, TermMode};
use alacritty_terminal::tty::{self, Options as PtyOptions};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor, Rgb, StdSyncHandler};
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use tokio::runtime::Handle as TokioHandle;
use tokio::sync::Mutex as TokioMutex;
use uuid::Uuid;

use super::events::{event_channel, TerminalEvent, TerminalEventSender};
use super::k8s_backend::K8sBackend;
use super::ssh_backend::SshBackend;
use super::ssm_backend::SsmBackend;

/// Terminal size in characters and pixels
#[derive(Debug, Clone, Copy, Default)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl TerminalSize {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols,
            rows,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    pub fn with_pixels(cols: u16, rows: u16, pixel_width: u16, pixel_height: u16) -> Self {
        Self {
            cols,
            rows,
            pixel_width,
            pixel_height,
        }
    }
}

/// Size info struct that implements Dimensions for alacritty
#[derive(Debug, Clone, Copy)]
pub struct SizeInfo {
    cols: usize,
    rows: usize,
}

impl SizeInfo {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols as usize,
            rows: rows as usize,
        }
    }
}

impl Dimensions for SizeInfo {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// Terminal configuration
#[derive(Debug, Clone)]
pub struct TerminalConfig {
    /// Number of lines to keep in scrollback
    pub scrollback_lines: usize,
    /// Terminal size
    pub size: TerminalSize,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            scrollback_lines: 10000,
            size: TerminalSize::new(80, 24),
        }
    }
}

/// Terminal operating mode
pub enum TerminalMode2 {
    /// Local mode - uses PTY for local shell
    Local {
        notifier: Notifier,
    },
    /// Remote mode - display-only, data fed via write_to_pty (SSH)
    Remote {
        notifier: Notifier,
        backend: Arc<TokioMutex<SshBackend>>,
        /// Sender for write data (doesn't require backend lock)
        write_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        /// Sender for resize requests (doesn't require backend lock)
        resize_tx: tokio::sync::mpsc::UnboundedSender<TerminalSize>,
        tokio_handle: TokioHandle,
    },
    /// K8s mode - Kubernetes pod exec connection
    K8s {
        notifier: Notifier,
        backend: Arc<TokioMutex<K8sBackend>>,
        /// Sender for write data
        write_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        /// Sender for resize requests
        resize_tx: tokio::sync::mpsc::UnboundedSender<TerminalSize>,
        tokio_handle: TokioHandle,
    },
    /// SSM mode - AWS Systems Manager Session Manager connection
    Ssm {
        notifier: Notifier,
        backend: Arc<TokioMutex<SsmBackend>>,
        /// Sender for write data (doesn't require backend lock)
        write_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        /// Sender for resize requests (doesn't require backend lock)
        resize_tx: tokio::sync::mpsc::UnboundedSender<TerminalSize>,
        tokio_handle: TokioHandle,
    },
}

/// A terminal instance wrapping alacritty_terminal
pub struct Terminal {
    /// Unique identifier
    id: Uuid,
    /// The alacritty terminal
    term: Arc<FairMutex<Term<TerminalEventSender>>>,
    /// Terminal mode (local or remote)
    mode: TerminalMode2,
    /// Event receiver for terminal events
    event_rx: Receiver<TerminalEvent>,
    /// Terminal configuration
    config: TerminalConfig,
    /// Current title (updated from events)
    title: String,
    /// Flag indicating new content has been written (for SSH mode)
    /// This allows the UI to know when to redraw without polling events
    dirty: Arc<AtomicBool>,
}

impl Terminal {
    /// Create a new terminal with a local PTY
    pub fn new_local(config: TerminalConfig) -> io::Result<Self> {
        let id = Uuid::new_v4();
        let (event_tx, event_rx) = event_channel();

        // Create terminal config with scrollback history
        let term_config = TermConfig {
            scrolling_history: config.scrollback_lines,
            ..TermConfig::default()
        };

        // Create terminal size (implements Dimensions)
        let term_size = SizeInfo::new(config.size.cols, config.size.rows);

        // Create window size (for PTY)
        let window_size = WindowSize {
            num_cols: config.size.cols,
            num_lines: config.size.rows,
            cell_width: 1,
            cell_height: 1,
        };

        // Create the terminal
        let term = Term::new(term_config, &term_size, event_tx.clone());
        let term = Arc::new(FairMutex::new(term));

        // Create PTY options with proper TERM environment variable
        let mut env = HashMap::new();
        env.insert("TERM".to_string(), "xterm-256color".to_string());
        env.insert("COLORTERM".to_string(), "truecolor".to_string());

        let pty_config = PtyOptions {
            shell: None, // Use default shell
            working_directory: None,
            drain_on_exit: false,
            env,
        };

        // Create PTY
        let pty = tty::new(&pty_config, window_size, id.as_u128() as u64)?;

        // Create event loop (uses cloned event sender)
        let event_loop = EventLoop::new(term.clone(), event_tx, pty, pty_config.drain_on_exit, false)?;

        // Get notifier before starting the loop
        let notifier = Notifier(event_loop.channel());

        // Spawn the event loop
        let _join_handle = event_loop.spawn();

        Ok(Self {
            id,
            term,
            mode: TerminalMode2::Local { notifier },
            event_rx,
            config,
            title: "Terminal".to_string(),
            dirty: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create an SSH terminal (display-only mode)
    pub fn new_ssh(config: TerminalConfig, backend: SshBackend, tokio_handle: TokioHandle) -> io::Result<Self> {
        let id = Uuid::new_v4();
        let (event_tx, event_rx) = event_channel();

        // Create terminal config with scrollback history
        let term_config = TermConfig {
            scrolling_history: config.scrollback_lines,
            ..TermConfig::default()
        };

        // Create terminal size
        let term_size = SizeInfo::new(config.size.cols, config.size.rows);

        // Create window size (for PTY)
        let window_size = WindowSize {
            num_cols: config.size.cols,
            num_lines: config.size.rows,
            cell_width: 1,
            cell_height: 1,
        };

        // Create the terminal
        let term = Term::new(term_config, &term_size, event_tx.clone());
        let term = Arc::new(FairMutex::new(term));

        // Create PTY options (we still need a PTY for the EventLoop, but it won't be used for SSH data)
        // Use a null placeholder that blocks waiting for stdin and consumes no resources.
        // SSH data is fed directly via the VT processor, bypassing this dummy PTY entirely.
        #[cfg(windows)]
        let dummy_shell = tty::Shell::new("cmd.exe".to_string(), vec!["/c".to_string(), "pause>nul".to_string()]);
        #[cfg(not(windows))]
        let dummy_shell = tty::Shell::new("/bin/cat".to_string(), vec![]);

        let pty_config = PtyOptions {
            shell: Some(dummy_shell),
            working_directory: None,
            drain_on_exit: false,
            env: HashMap::new(),
        };

        // Create a dummy PTY - we'll feed SSH data via Msg::Input
        let pty = tty::new(&pty_config, window_size, id.as_u128() as u64)?;

        // Create event loop
        let event_loop = EventLoop::new(term.clone(), event_tx, pty, false, false)?;

        // Get notifier before starting the loop
        let notifier = Notifier(event_loop.channel());

        // Spawn the event loop
        let _join_handle = event_loop.spawn();

        // Set up write and resize channels - the actual channel setup happens in take_channel_for_io
        // after connection, so we just create placeholder senders here
        let (write_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (resize_tx, _) = tokio::sync::mpsc::unbounded_channel();

        let backend_arc = Arc::new(TokioMutex::new(backend));

        Ok(Self {
            id,
            term,
            mode: TerminalMode2::Remote {
                notifier,
                backend: backend_arc,
                write_tx,
                resize_tx,
                tokio_handle,
            },
            event_rx,
            config,
            title: "SSH".to_string(),
            dirty: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create an SSM terminal (display-only mode for AWS SSM Session Manager)
    pub fn new_ssm(config: TerminalConfig, backend: SsmBackend, tokio_handle: TokioHandle) -> io::Result<Self> {
        let id = Uuid::new_v4();
        let (event_tx, event_rx) = event_channel();

        // Create terminal config with scrollback history
        let term_config = TermConfig {
            scrolling_history: config.scrollback_lines,
            ..TermConfig::default()
        };

        // Create terminal size
        let term_size = SizeInfo::new(config.size.cols, config.size.rows);

        // Create window size (for PTY)
        let window_size = WindowSize {
            num_cols: config.size.cols,
            num_lines: config.size.rows,
            cell_width: 1,
            cell_height: 1,
        };

        // Create the terminal
        let term = Term::new(term_config, &term_size, event_tx.clone());
        let term = Arc::new(FairMutex::new(term));

        // Create PTY options (we still need a PTY for the EventLoop, but it won't be used for SSM data)
        // Use a null placeholder that blocks waiting for stdin and consumes no resources.
        // SSM data is fed directly via the VT processor, bypassing this dummy PTY entirely.
        #[cfg(windows)]
        let dummy_shell = tty::Shell::new("cmd.exe".to_string(), vec!["/c".to_string(), "pause>nul".to_string()]);
        #[cfg(not(windows))]
        let dummy_shell = tty::Shell::new("/bin/cat".to_string(), vec![]);

        let pty_config = PtyOptions {
            shell: Some(dummy_shell),
            working_directory: None,
            drain_on_exit: false,
            env: HashMap::new(),
        };

        // Create a dummy PTY - we'll feed SSM data via write_to_pty
        let pty = tty::new(&pty_config, window_size, id.as_u128() as u64)?;

        // Create event loop
        let event_loop = EventLoop::new(term.clone(), event_tx, pty, false, false)?;

        // Get notifier before starting the loop
        let notifier = Notifier(event_loop.channel());

        // Spawn the event loop
        let _join_handle = event_loop.spawn();

        // Set up write and resize channels - the actual channel setup happens in setup_channels
        // after connection, so we just create placeholder senders here
        let (write_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (resize_tx, _) = tokio::sync::mpsc::unbounded_channel();

        let backend_arc = Arc::new(TokioMutex::new(backend));

        Ok(Self {
            id,
            term,
            mode: TerminalMode2::Ssm {
                notifier,
                backend: backend_arc,
                write_tx,
                resize_tx,
                tokio_handle,
            },
            event_rx,
            config,
            title: "SSM".to_string(),
            dirty: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create a K8s terminal (display-only mode for Kubernetes pod exec)
    pub fn new_k8s(config: TerminalConfig, backend: K8sBackend, tokio_handle: TokioHandle) -> io::Result<Self> {
        let id = Uuid::new_v4();
        let (event_tx, event_rx) = event_channel();

        // Create terminal config with scrollback history
        let term_config = TermConfig {
            scrolling_history: config.scrollback_lines,
            ..TermConfig::default()
        };

        // Create terminal size
        let term_size = SizeInfo::new(config.size.cols, config.size.rows);

        // Create window size (for PTY)
        let window_size = WindowSize {
            num_cols: config.size.cols,
            num_lines: config.size.rows,
            cell_width: 1,
            cell_height: 1,
        };

        // Create the terminal
        let term = Term::new(term_config, &term_size, event_tx.clone());
        let term = Arc::new(FairMutex::new(term));

        // Create PTY options - use a null placeholder that blocks
        #[cfg(windows)]
        let dummy_shell = tty::Shell::new("cmd.exe".to_string(), vec!["/c".to_string(), "pause>nul".to_string()]);
        #[cfg(not(windows))]
        let dummy_shell = tty::Shell::new("/bin/cat".to_string(), vec![]);

        let pty_config = PtyOptions {
            shell: Some(dummy_shell),
            working_directory: None,
            drain_on_exit: false,
            env: HashMap::new(),
        };

        // Create a dummy PTY
        let pty = tty::new(&pty_config, window_size, id.as_u128() as u64)?;

        // Create event loop
        let event_loop = EventLoop::new(term.clone(), event_tx, pty, false, false)?;

        // Get notifier before starting the loop
        let notifier = Notifier(event_loop.channel());

        // Spawn the event loop
        let _join_handle = event_loop.spawn();

        // Set up write and resize channels
        let (write_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (resize_tx, _) = tokio::sync::mpsc::unbounded_channel();

        let backend_arc = Arc::new(TokioMutex::new(backend));

        Ok(Self {
            id,
            term,
            mode: TerminalMode2::K8s {
                notifier,
                backend: backend_arc,
                write_tx,
                resize_tx,
                tokio_handle,
            },
            event_rx,
            config,
            title: "K8s".to_string(),
            dirty: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Update the write sender after I/O setup
    pub fn set_write_tx(&mut self, tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>) {
        match &mut self.mode {
            TerminalMode2::Remote { write_tx, .. } => *write_tx = tx,
            TerminalMode2::Ssm { write_tx, .. } => *write_tx = tx,
            TerminalMode2::K8s { write_tx, .. } => *write_tx = tx,
            _ => {}
        }
    }

    /// Update the resize sender after I/O setup
    pub fn set_resize_tx(&mut self, tx: tokio::sync::mpsc::UnboundedSender<TerminalSize>) {
        match &mut self.mode {
            TerminalMode2::Remote { resize_tx, .. } => *resize_tx = tx,
            TerminalMode2::Ssm { resize_tx, .. } => *resize_tx = tx,
            TerminalMode2::K8s { resize_tx, .. } => *resize_tx = tx,
            _ => {}
        }
    }

    /// Get the SSH backend (for spawning reader task)
    pub fn ssh_backend(&self) -> Option<Arc<TokioMutex<SshBackend>>> {
        match &self.mode {
            TerminalMode2::Remote { backend, .. } => Some(backend.clone()),
            _ => None,
        }
    }

    /// Get the K8s backend (for spawning I/O task)
    pub fn k8s_backend(&self) -> Option<Arc<TokioMutex<K8sBackend>>> {
        match &self.mode {
            TerminalMode2::K8s { backend, .. } => Some(backend.clone()),
            _ => None,
        }
    }

    /// Get the SSM backend (for spawning I/O task)
    pub fn ssm_backend(&self) -> Option<Arc<TokioMutex<SsmBackend>>> {
        match &self.mode {
            TerminalMode2::Ssm { backend, .. } => Some(backend.clone()),
            _ => None,
        }
    }

    /// Get the terminal ID
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Get the current title
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Write data TO the terminal for display (from SSH/SSM output)
    ///
    /// This feeds data into alacritty for parsing and display.
    /// Use this for data received FROM SSH/SSM that needs to be rendered.
    pub fn write_to_pty(&self, data: &[u8]) {
        match &self.mode {
            TerminalMode2::Local { notifier } => {
                // For local terminals, send through the PTY event loop
                notifier.notify(data.to_vec());
            }
            TerminalMode2::Remote { .. } | TerminalMode2::Ssm { .. } | TerminalMode2::K8s { .. } => {
                // For SSH/SSM/K8s terminals, directly process data through the VT parser
                // This ensures escape sequences (like mouse mode) are handled correctly
                let mut processor = Processor::<StdSyncHandler>::new();
                let mut term = self.term.lock();
                processor.advance(&mut *term, data);
                // Signal that new content is available for rendering
                self.dirty.store(true, Ordering::Release);
            }
        }
    }

    /// Write keyboard input (goes to PTY for local, SSH/SSM for remote)
    ///
    /// This sends user keyboard input to the shell/remote process.
    pub fn write(&self, data: &[u8]) {
        match &self.mode {
            TerminalMode2::Local { notifier } => {
                notifier.notify(data.to_vec());
            }
            TerminalMode2::Remote { write_tx, .. } => {
                // Send through the write channel (processed by ssh_io_loop)
                tracing::debug!("SSH write: queuing {} bytes", data.len());
                if let Err(e) = write_tx.send(data.to_vec()) {
                    tracing::error!("SSH write send error: {}", e);
                }
            }
            TerminalMode2::Ssm { write_tx, .. } => {
                // Send through the write channel (processed by ssm_io_loop)
                tracing::debug!("SSM write: queuing {} bytes", data.len());
                if let Err(e) = write_tx.send(data.to_vec()) {
                    tracing::error!("SSM write send error: {}", e);
                }
            }
            TerminalMode2::K8s { write_tx, .. } => {
                // Send through the write channel (processed by k8s_io_loop)
                tracing::debug!("K8s write: queuing {} bytes", data.len());
                if let Err(e) = write_tx.send(data.to_vec()) {
                    tracing::error!("K8s write send error: {}", e);
                }
            }
        }
    }

    /// Poll for events (non-blocking)
    ///
    /// Returns all pending terminal events. Call this to check if the
    /// terminal needs to be redrawn.
    pub fn poll_events(&mut self) -> Vec<TerminalEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            // Update title if changed
            if let TerminalEvent::TitleChanged(ref new_title) = event {
                self.title = new_title.clone();
            }
            events.push(event);
        }
        events
    }

    /// Check if new content has been written (for SSH mode)
    /// Returns true if dirty and clears the flag
    #[must_use]
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    /// Get the dirty flag Arc for external polling without locking
    pub fn dirty_flag(&self) -> Arc<AtomicBool> {
        self.dirty.clone()
    }

    /// Resize the terminal
    pub fn resize(&mut self, size: TerminalSize) {
        self.config.size = size;

        // cell_width/cell_height are per-cell dimensions, not total window size
        // Calculate from total pixel dimensions if available, otherwise use defaults
        let cell_width = if size.pixel_width > 0 && size.cols > 0 {
            size.pixel_width / size.cols
        } else {
            8 // Default cell width
        };
        let cell_height = if size.pixel_height > 0 && size.rows > 0 {
            size.pixel_height / size.rows
        } else {
            14 // Default cell height
        };

        let window_size = WindowSize {
            num_cols: size.cols,
            num_lines: size.rows,
            cell_width: cell_width.max(1),
            cell_height: cell_height.max(1),
        };

        let size_info = SizeInfo::new(size.cols, size.rows);

        // Resize the terminal grid
        {
            let mut term = self.term.lock();
            term.resize(size_info);
        }

        // Notify the PTY / SSH / SSM backend
        match &self.mode {
            TerminalMode2::Local { notifier } => {
                let _ = notifier.0.send(Msg::Resize(window_size));
            }
            TerminalMode2::Remote { notifier, resize_tx, .. } => {
                // Notify the event loop
                let _ = notifier.0.send(Msg::Resize(window_size));

                // Send resize through channel (handled by I/O loop)
                tracing::debug!("SSH resize: queuing {}x{}", size.cols, size.rows);
                if let Err(e) = resize_tx.send(size) {
                    tracing::error!("SSH resize send error: {}", e);
                }
            }
            TerminalMode2::Ssm { notifier, resize_tx, .. } => {
                // Notify the event loop
                let _ = notifier.0.send(Msg::Resize(window_size));

                // Send resize through channel (handled by I/O loop)
                tracing::debug!("SSM resize: queuing {}x{}", size.cols, size.rows);
                if let Err(e) = resize_tx.send(size) {
                    tracing::error!("SSM resize send error: {}", e);
                }
            }
            TerminalMode2::K8s { notifier, resize_tx, .. } => {
                // Notify the event loop
                let _ = notifier.0.send(Msg::Resize(window_size));

                // Send resize through channel (handled by I/O loop)
                tracing::debug!("K8s resize: queuing {}x{}", size.cols, size.rows);
                if let Err(e) = resize_tx.send(size) {
                    tracing::error!("K8s resize send error: {}", e);
                }
            }
        }
    }

    /// Get the current terminal size
    pub fn size(&self) -> TerminalSize {
        self.config.size
    }

    /// Get the number of columns
    pub fn cols(&self) -> u16 {
        let term = self.term.lock();
        term.columns() as u16
    }

    /// Get the number of rows
    pub fn rows(&self) -> u16 {
        let term = self.term.lock();
        term.screen_lines() as u16
    }

    /// Get the terminal mode flags
    pub fn mode(&self) -> TermMode {
        let term = self.term.lock();
        *term.mode()
    }

    /// Get a cell at the given position
    pub fn cell(&self, point: Point) -> Option<Cell> {
        let term = self.term.lock();
        let content = term.grid();
        if point.line.0 < content.screen_lines() as i32 && point.column.0 < content.columns() {
            Some(content[point].clone())
        } else {
            None
        }
    }

    /// Get the cursor position
    pub fn cursor_position(&self) -> Point {
        let term = self.term.lock();
        term.grid().cursor.point
    }

    /// Check if cursor is visible
    #[must_use]
    pub fn cursor_visible(&self) -> bool {
        let term = self.term.lock();
        term.mode().contains(TermMode::SHOW_CURSOR)
    }

    /// Get the color palette
    pub fn colors(&self) -> Colors {
        let term = self.term.lock();
        *term.colors()
    }

    /// Get the number of screen lines (visible area)
    pub fn screen_lines(&self) -> usize {
        let term = self.term.lock();
        term.screen_lines()
    }

    /// Get the number of columns
    pub fn columns(&self) -> usize {
        let term = self.term.lock();
        term.columns()
    }

    /// Get direct access to the terminal (for rendering)
    pub fn with_term<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Term<TerminalEventSender>) -> R,
    {
        let term = self.term.lock();
        f(&term)
    }

    /// Get mutable access to the terminal
    pub fn with_term_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Term<TerminalEventSender>) -> R,
    {
        let mut term = self.term.lock();
        f(&mut term)
    }

    /// Scroll the terminal
    pub fn scroll(&self, lines: i32) {
        let mut term = self.term.lock();
        term.scroll_display(alacritty_terminal::grid::Scroll::Delta(lines));
    }

    // --- Selection methods using alacritty's built-in selection ---

    /// Start a selection at the given point
    pub fn start_selection(&self, ty: SelectionType, point: Point, side: Side) {
        let mut term = self.term.lock();
        let selection = Selection::new(ty, point, side);
        term.selection = Some(selection);
    }

    /// Update the current selection to a new point
    pub fn update_selection(&self, point: Point, side: Side) {
        let mut term = self.term.lock();
        if let Some(ref mut selection) = term.selection {
            selection.update(point, side);
        }
    }

    /// Get the selected text
    pub fn selected_text(&self) -> Option<String> {
        let term = self.term.lock();
        term.selection_to_string()
    }

    /// Clear the current selection
    pub fn clear_selection(&self) {
        let mut term = self.term.lock();
        term.selection = None;
    }

    /// Check if there is an active selection
    #[must_use]
    pub fn has_selection(&self) -> bool {
        let term = self.term.lock();
        term.selection.is_some()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Signal shutdown to the event loop
        let notifier = match &self.mode {
            TerminalMode2::Local { notifier } => notifier,
            TerminalMode2::Remote { notifier, .. } => notifier,
            TerminalMode2::Ssm { notifier, .. } => notifier,
            TerminalMode2::K8s { notifier, .. } => notifier,
        };
        let _ = notifier.0.send(Msg::Shutdown);
    }
}

use crate::config::ColorScheme;

/// Convert a hex color (0xRRGGBB) to Rgb
pub fn hex_to_rgb(hex: u32) -> Rgb {
    Rgb {
        r: ((hex >> 16) & 0xff) as u8,
        g: ((hex >> 8) & 0xff) as u8,
        b: (hex & 0xff) as u8,
    }
}

/// Convert an alacritty color to RGB
pub fn color_to_rgb(color: Color, colors: &Colors) -> Rgb {
    match color {
        Color::Named(named) => named_color_to_rgb(named, colors),
        Color::Spec(rgb) => rgb,
        Color::Indexed(idx) => {
            if let Some(rgb) = colors[idx as usize] {
                rgb
            } else {
                // Fallback for standard 256 colors
                index_to_rgb(idx)
            }
        }
    }
}

/// Convert an alacritty color to RGB using a color scheme
pub fn color_to_rgb_with_scheme(color: Color, colors: &Colors, scheme: &ColorScheme) -> Rgb {
    match color {
        Color::Named(named) => named_color_to_rgb_with_scheme(named, colors, scheme),
        Color::Spec(rgb) => rgb,
        Color::Indexed(idx) => {
            // Use scheme colors for standard 16 colors
            if idx < 16 {
                index_to_rgb_with_scheme(idx, scheme)
            } else if let Some(rgb) = colors[idx as usize] {
                rgb
            } else {
                index_to_rgb(idx)
            }
        }
    }
}

/// Convert a named color to RGB using a color scheme
fn named_color_to_rgb_with_scheme(named: NamedColor, colors: &Colors, scheme: &ColorScheme) -> Rgb {
    match colors[named] {
        Some(rgb) => rgb,
        None => match named {
            NamedColor::Black => hex_to_rgb(scheme.black),
            NamedColor::Red => hex_to_rgb(scheme.red),
            NamedColor::Green => hex_to_rgb(scheme.green),
            NamedColor::Yellow => hex_to_rgb(scheme.yellow),
            NamedColor::Blue => hex_to_rgb(scheme.blue),
            NamedColor::Magenta => hex_to_rgb(scheme.magenta),
            NamedColor::Cyan => hex_to_rgb(scheme.cyan),
            NamedColor::White => hex_to_rgb(scheme.white),
            NamedColor::BrightBlack => hex_to_rgb(scheme.bright_black),
            NamedColor::BrightRed => hex_to_rgb(scheme.bright_red),
            NamedColor::BrightGreen => hex_to_rgb(scheme.bright_green),
            NamedColor::BrightYellow => hex_to_rgb(scheme.bright_yellow),
            NamedColor::BrightBlue => hex_to_rgb(scheme.bright_blue),
            NamedColor::BrightMagenta => hex_to_rgb(scheme.bright_magenta),
            NamedColor::BrightCyan => hex_to_rgb(scheme.bright_cyan),
            NamedColor::BrightWhite => hex_to_rgb(scheme.bright_white),
            NamedColor::Foreground => hex_to_rgb(scheme.foreground),
            NamedColor::Background => hex_to_rgb(scheme.background),
            _ => hex_to_rgb(scheme.foreground),
        },
    }
}

/// Convert a 256-color index to RGB using scheme for first 16 colors
fn index_to_rgb_with_scheme(idx: u8, scheme: &ColorScheme) -> Rgb {
    match idx {
        0 => hex_to_rgb(scheme.black),
        1 => hex_to_rgb(scheme.red),
        2 => hex_to_rgb(scheme.green),
        3 => hex_to_rgb(scheme.yellow),
        4 => hex_to_rgb(scheme.blue),
        5 => hex_to_rgb(scheme.magenta),
        6 => hex_to_rgb(scheme.cyan),
        7 => hex_to_rgb(scheme.white),
        8 => hex_to_rgb(scheme.bright_black),
        9 => hex_to_rgb(scheme.bright_red),
        10 => hex_to_rgb(scheme.bright_green),
        11 => hex_to_rgb(scheme.bright_yellow),
        12 => hex_to_rgb(scheme.bright_blue),
        13 => hex_to_rgb(scheme.bright_magenta),
        14 => hex_to_rgb(scheme.bright_cyan),
        15 => hex_to_rgb(scheme.bright_white),
        _ => index_to_rgb(idx),
    }
}

/// Convert a named color to RGB
fn named_color_to_rgb(named: NamedColor, colors: &Colors) -> Rgb {
    match colors[named] {
        Some(rgb) => rgb,
        None => match named {
            NamedColor::Black => Rgb { r: 0, g: 0, b: 0 },
            NamedColor::Red => Rgb { r: 205, g: 0, b: 0 },
            NamedColor::Green => Rgb { r: 0, g: 205, b: 0 },
            NamedColor::Yellow => Rgb { r: 205, g: 205, b: 0 },
            NamedColor::Blue => Rgb { r: 0, g: 0, b: 238 },
            NamedColor::Magenta => Rgb { r: 205, g: 0, b: 205 },
            NamedColor::Cyan => Rgb { r: 0, g: 205, b: 205 },
            NamedColor::White => Rgb { r: 229, g: 229, b: 229 },
            NamedColor::BrightBlack => Rgb {
                r: 127,
                g: 127,
                b: 127,
            },
            NamedColor::BrightRed => Rgb { r: 255, g: 0, b: 0 },
            NamedColor::BrightGreen => Rgb { r: 0, g: 255, b: 0 },
            NamedColor::BrightYellow => Rgb {
                r: 255,
                g: 255,
                b: 0,
            },
            NamedColor::BrightBlue => Rgb {
                r: 92,
                g: 92,
                b: 255,
            },
            NamedColor::BrightMagenta => Rgb {
                r: 255,
                g: 0,
                b: 255,
            },
            NamedColor::BrightCyan => Rgb {
                r: 0,
                g: 255,
                b: 255,
            },
            NamedColor::BrightWhite => Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            NamedColor::Foreground => Rgb {
                r: 229,
                g: 229,
                b: 229,
            },
            NamedColor::Background => Rgb { r: 0, g: 0, b: 0 },
            _ => Rgb {
                r: 229,
                g: 229,
                b: 229,
            },
        },
    }
}

/// Convert a 256-color index to RGB
fn index_to_rgb(idx: u8) -> Rgb {
    if idx < 16 {
        // Standard colors
        match idx {
            0 => Rgb { r: 0, g: 0, b: 0 },
            1 => Rgb { r: 205, g: 0, b: 0 },
            2 => Rgb { r: 0, g: 205, b: 0 },
            3 => Rgb { r: 205, g: 205, b: 0 },
            4 => Rgb { r: 0, g: 0, b: 238 },
            5 => Rgb { r: 205, g: 0, b: 205 },
            6 => Rgb { r: 0, g: 205, b: 205 },
            7 => Rgb {
                r: 229,
                g: 229,
                b: 229,
            },
            8 => Rgb {
                r: 127,
                g: 127,
                b: 127,
            },
            9 => Rgb { r: 255, g: 0, b: 0 },
            10 => Rgb { r: 0, g: 255, b: 0 },
            11 => Rgb {
                r: 255,
                g: 255,
                b: 0,
            },
            12 => Rgb {
                r: 92,
                g: 92,
                b: 255,
            },
            13 => Rgb {
                r: 255,
                g: 0,
                b: 255,
            },
            14 => Rgb {
                r: 0,
                g: 255,
                b: 255,
            },
            15 => Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            _ => unreachable!(),
        }
    } else if idx < 232 {
        // 6x6x6 color cube
        let idx = idx - 16;
        let r = (idx / 36) % 6;
        let g = (idx / 6) % 6;
        let b = idx % 6;
        let to_component = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
        Rgb {
            r: to_component(r),
            g: to_component(g),
            b: to_component(b),
        }
    } else {
        // Grayscale ramp
        let gray = 8 + (idx - 232) * 10;
        Rgb {
            r: gray,
            g: gray,
            b: gray,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_config_default() {
        let config = TerminalConfig::default();
        assert_eq!(config.scrollback_lines, 10000);
        assert_eq!(config.size.cols, 80);
        assert_eq!(config.size.rows, 24);
    }

    #[test]
    fn test_color_conversion() {
        let colors = Colors::default();

        // Test named color
        let rgb = color_to_rgb(Color::Named(NamedColor::Red), &colors);
        assert!(rgb.r > 0);

        // Test indexed color
        let rgb = index_to_rgb(1);
        assert_eq!(rgb.r, 205);
        assert_eq!(rgb.g, 0);
        assert_eq!(rgb.b, 0);
    }
}
