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
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use std::collections::HashMap;
use std::io;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use uuid::Uuid;

use super::events::{event_channel, TerminalEvent, TerminalEventSender};
use super::ssh_backend::SshBackend;

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
    /// Remote mode - display-only, data fed via write_to_pty
    Remote {
        notifier: Notifier,
        backend: Arc<TokioMutex<SshBackend>>,
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
}

impl Terminal {
    /// Create a new terminal with a local PTY
    pub fn new_local(config: TerminalConfig) -> io::Result<Self> {
        let id = Uuid::new_v4();
        let (event_tx, event_rx) = event_channel();

        // Create terminal config
        let term_config = TermConfig::default();

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
            hold: false,
            env,
        };

        // Create PTY
        let pty = tty::new(&pty_config, window_size, id.as_u128() as u64)?;

        // Create event loop (uses cloned event sender)
        let event_loop = EventLoop::new(term.clone(), event_tx, pty, pty_config.hold, false)?;

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
        })
    }

    /// Create an SSH terminal (display-only mode)
    pub fn new_ssh(config: TerminalConfig, backend: SshBackend) -> io::Result<Self> {
        let id = Uuid::new_v4();
        let (event_tx, event_rx) = event_channel();

        // Create terminal config
        let term_config = TermConfig::default();

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
        // The PTY runs a default shell that stays idle - keyboard input goes to SSH instead
        let pty_config = PtyOptions {
            shell: None, // Use default shell (it will stay idle)
            working_directory: None,
            hold: false,
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

        let backend_arc = Arc::new(TokioMutex::new(backend));

        Ok(Self {
            id,
            term,
            mode: TerminalMode2::Remote {
                notifier,
                backend: backend_arc,
            },
            event_rx,
            config,
            title: "SSH".to_string(),
        })
    }

    /// Get the SSH backend (for spawning reader task)
    pub fn ssh_backend(&self) -> Option<Arc<TokioMutex<SshBackend>>> {
        match &self.mode {
            TerminalMode2::Remote { backend, .. } => Some(backend.clone()),
            TerminalMode2::Local { .. } => None,
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

    /// Write data TO the terminal for display (from SSH output)
    ///
    /// This feeds data into alacritty for parsing and display.
    /// Use this for data received FROM SSH that needs to be rendered.
    pub fn write_to_pty(&self, data: &[u8]) {
        let notifier = match &self.mode {
            TerminalMode2::Local { notifier } => notifier,
            TerminalMode2::Remote { notifier, .. } => notifier,
        };
        // Use Notify trait like Zed does
        notifier.notify(data.to_vec());
    }

    /// Write keyboard input (goes to PTY for local, SSH for remote)
    ///
    /// This sends user keyboard input to the shell/remote process.
    pub fn write(&self, data: &[u8]) {
        match &self.mode {
            TerminalMode2::Local { notifier } => {
                // For local terminals, use Notify trait like Zed does
                notifier.notify(data.to_vec());
            }
            TerminalMode2::Remote { backend, .. } => {
                // For SSH terminals, send to SSH backend
                let backend = backend.clone();
                let data = data.to_vec();
                tokio::spawn(async move {
                    let mut b = backend.lock().await;
                    let _ = b.write(&data).await;
                });
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

        // Notify the PTY / SSH backend
        match &self.mode {
            TerminalMode2::Local { notifier } => {
                let _ = notifier.0.send(Msg::Resize(window_size));
            }
            TerminalMode2::Remote { notifier, backend } => {
                // Notify the event loop
                let _ = notifier.0.send(Msg::Resize(window_size));

                // Notify SSH backend
                let backend = backend.clone();
                tokio::spawn(async move {
                    let mut b = backend.lock().await;
                    let ssh_size = super::ssh_backend::TerminalSize {
                        cols: size.cols,
                        rows: size.rows,
                        pixel_width: size.pixel_width,
                        pixel_height: size.pixel_height,
                    };
                    let _ = b.resize(ssh_size).await;
                });
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
        };
        let _ = notifier.0.send(Msg::Shutdown);
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
