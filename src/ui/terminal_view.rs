use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point as TermPoint, Side};
use alacritty_terminal::selection::SelectionType;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use gpui::*;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::terminal::{keystroke_to_escape, terminal::color_to_rgb, Terminal, TerminalSize};

/// Cursor blink interval in milliseconds
const CURSOR_BLINK_INTERVAL_MS: u64 = 500;

/// Terminal view element for rendering a terminal
pub struct TerminalView {
    terminal: Arc<Mutex<Terminal>>,
    focus_handle: FocusHandle,
    font_family: SharedString,
    font_size: Pixels,
    /// Cell dimensions for mouse coordinate conversion
    cell_width: Pixels,
    cell_height: Pixels,
    /// View bounds origin for mouse coordinate conversion
    /// Mouse events in GPUI are in window coordinates, so we need to subtract the origin
    /// This is shared with the canvas callback via Arc so it can be updated during paint
    bounds_origin: Arc<Mutex<Point<Pixels>>>,
    /// Whether mouse is currently selecting
    is_selecting: bool,
    /// Cursor blink state - true means cursor is visible in the blink cycle
    cursor_visible: bool,
    /// Last cursor blink toggle time
    last_blink_toggle: Instant,
    /// Whether terminal was focused in previous frame
    was_focused: bool,
}

impl TerminalView {
    pub fn new(terminal: Arc<Mutex<Terminal>>, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();

        let terminal_weak = Arc::downgrade(&terminal);

        // Event-driven update loop - polls for terminal events and handles cursor blink
        cx.spawn(async move |entity, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;

                let should_notify = terminal_weak.upgrade().map(|t| {
                    let mut term = t.lock();
                    !term.poll_events().is_empty()
                }).unwrap_or(false);

                // Handle cursor blinking - always update, render will check focus state
                let _ = entity.update(cx, |view, cx| {
                    let now = Instant::now();
                    if now.duration_since(view.last_blink_toggle).as_millis() >= CURSOR_BLINK_INTERVAL_MS as u128 {
                        view.cursor_visible = !view.cursor_visible;
                        view.last_blink_toggle = now;
                        cx.notify();
                    }
                    if should_notify {
                        cx.notify();
                    }
                });
            }
        })
        .detach();

        Self {
            terminal,
            focus_handle,
            font_family: "Monaco".into(),
            font_size: px(14.0),
            cell_width: px(8.0),
            cell_height: px(14.0),
            bounds_origin: Arc::new(Mutex::new(point(px(0.0), px(0.0)))),
            is_selecting: false,
            cursor_visible: true,
            last_blink_toggle: Instant::now(),
            was_focused: false,
        }
    }

    /// Focus this terminal view
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
    }

    fn handle_key_input(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // Reset cursor blink on any input
        self.cursor_visible = true;
        self.last_blink_toggle = Instant::now();

        let keystroke = &event.keystroke;
        eprintln!("[KEY EVENT] key={:?} modifiers={:?}", keystroke.key, keystroke.modifiers);

        // Skip if platform modifier (Cmd on Mac) is pressed - those are usually shortcuts
        if keystroke.modifiers.platform {
            return;
        }

        // Get mode and try escape sequence conversion
        let escape_result = {
            let term = self.terminal.lock();
            let mode = term.mode();
            keystroke_to_escape(keystroke, &mode, false)
        };

        // If we have an escape sequence, send it
        if let Some(escape_str) = escape_result {
            eprintln!("[KEY] Sending escape sequence: {:?}", escape_str.as_bytes());
            let term = self.terminal.lock();
            term.write(escape_str.as_bytes());
            drop(term);
            cx.stop_propagation();
            cx.notify();
            return;
        }

        // Skip if control or alt is pressed without a matching escape sequence
        // (those were handled above by keystroke_to_escape)
        if keystroke.modifiers.control || keystroke.modifiers.alt {
            return;
        }

        // Handle regular character input
        // Use key_char if available (for proper Unicode handling), otherwise key
        let input = if let Some(key_char) = &keystroke.key_char {
            if !key_char.is_empty() && key_char.chars().all(|c| !c.is_control() || c == '\t') {
                Some(key_char.to_string())
            } else {
                None
            }
        } else if keystroke.key.len() == 1 {
            let c = keystroke.key.chars().next().unwrap();
            // Only send printable ASCII characters
            if c.is_ascii_graphic() || c == ' ' {
                Some(keystroke.key.clone())
            } else {
                None
            }
        } else {
            None
        };

        if let Some(input) = input {
            eprintln!("[KEY] Sending character input: {:?}", input.as_bytes());
            let term = self.terminal.lock();
            term.write(input.as_bytes());
            drop(term);
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn handle_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Focus on click
        cx.focus_self(window);

        // Adjust mouse position from window coordinates to view-local coordinates
        let bounds_origin = *self.bounds_origin.lock();
        let local_position = point(
            event.position.x - bounds_origin.x,
            event.position.y - bounds_origin.y,
        );

        let term = self.terminal.lock();
        let mode = term.mode();
        let term_size = term.size();

        // Debug: print mouse mode status and terminal size
        eprintln!("[MOUSE] DOWN window_pos={:?} bounds_origin={:?} local_pos={:?}, term_size={}x{}, cell={}x{}, mode: REPORT_CLICK={}, SGR={}",
            event.position,
            bounds_origin,
            local_position,
            term_size.cols, term_size.rows,
            f32::from(self.cell_width), f32::from(self.cell_height),
            mode.contains(TermMode::MOUSE_REPORT_CLICK),
            mode.contains(TermMode::SGR_MOUSE)
        );

        // Check if terminal wants mouse events
        if mode.contains(TermMode::MOUSE_REPORT_CLICK)
            || mode.contains(TermMode::MOUSE_DRAG)
            || mode.contains(TermMode::MOUSE_MOTION)
        {
            // Send mouse event to terminal application (use local coordinates)
            let point = self.mouse_to_point(local_position);
            let button = match event.button {
                MouseButton::Left => 0,
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
                _ => 0,
            };
            let col = point.column.0 as u32 + 1; // 1-based
            let row = point.line.0 as u32 + 1; // 1-based

            // Clamp coordinates to terminal dimensions (and ensure positive)
            let col = col.max(1).min(term_size.cols as u32);
            let row = row.max(1).min(term_size.rows as u32);

            // Use SGR mouse protocol if enabled, otherwise use normal protocol
            let mouse_report = if mode.contains(TermMode::SGR_MOUSE) {
                format!("\x1b[<{};{};{}M", button, col, row)
            } else {
                // X10 mouse protocol (limited to 223 columns/rows)
                let cb = (32 + button) as u8;
                let cx_char = (32 + col.min(223)) as u8;
                let cy_char = (32 + row.min(223)) as u8;
                format!("\x1b[M{}{}{}", cb as char, cx_char as char, cy_char as char)
            };
            eprintln!("[MOUSE] sending report: {:?} at col={} row={}", mouse_report.as_bytes(), col, row);
            term.write(mouse_report.as_bytes());
            drop(term);
            cx.notify();
            return;
        }

        // Normal selection behavior
        term.clear_selection();

        // Start new selection
        let point = self.mouse_to_point(event.position);
        let side = self.mouse_to_side(event.position);
        term.start_selection(SelectionType::Simple, point, side);
        self.is_selecting = true;

        cx.notify();
    }

    fn handle_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            // Adjust mouse position from window coordinates to view-local coordinates
            let bounds_origin = *self.bounds_origin.lock();
            let local_position = point(
                event.position.x - bounds_origin.x,
                event.position.y - bounds_origin.y,
            );
            let point = self.mouse_to_point(local_position);
            let side = self.mouse_to_side(local_position);
            let term = self.terminal.lock();
            term.update_selection(point, side);
            cx.notify();
        }
    }

    fn handle_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // Adjust mouse position from window coordinates to view-local coordinates
        let bounds_origin = *self.bounds_origin.lock();
        let local_position = point(
            event.position.x - bounds_origin.x,
            event.position.y - bounds_origin.y,
        );

        let term = self.terminal.lock();
        let mode = term.mode();
        let term_size = term.size();

        // Check if terminal wants mouse events
        if mode.contains(TermMode::MOUSE_REPORT_CLICK)
            || mode.contains(TermMode::MOUSE_DRAG)
            || mode.contains(TermMode::MOUSE_MOTION)
        {
            // Send mouse release event to terminal application (use local coordinates)
            let point = self.mouse_to_point(local_position);
            let button = match event.button {
                MouseButton::Left => 0,
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
                _ => 0,
            };
            // Clamp coordinates to terminal dimensions (and ensure positive)
            let col = (point.column.0 as u32 + 1).max(1).min(term_size.cols as u32);
            let row = (point.line.0 as u32 + 1).max(1).min(term_size.rows as u32);

            // Use SGR mouse protocol for release (lowercase 'm')
            let mouse_report = if mode.contains(TermMode::SGR_MOUSE) {
                format!("\x1b[<{};{};{}m", button, col, row)
            } else {
                // X10 protocol uses button 3 for release
                let cb = (32 + 3) as u8; // release
                let cx_char = (32 + col.min(223)) as u8;
                let cy_char = (32 + row.min(223)) as u8;
                format!("\x1b[M{}{}{}", cb as char, cx_char as char, cy_char as char)
            };
            eprintln!("[MOUSE] UP local_pos={:?} col={} row={}, sending: {:?}", local_position, col, row, mouse_report.as_bytes());
            term.write(mouse_report.as_bytes());
            drop(term);
            cx.notify();
            return;
        }

        drop(term);
        self.is_selecting = false;
        cx.notify();
    }

    fn handle_scroll(&mut self, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // Adjust mouse position from window coordinates to view-local coordinates
        let bounds_origin = *self.bounds_origin.lock();
        let local_position = point(
            event.position.x - bounds_origin.x,
            event.position.y - bounds_origin.y,
        );

        let term = self.terminal.lock();
        let mode = term.mode();

        // Check if terminal wants mouse events (scroll = mouse button 64/65)
        if mode.contains(TermMode::MOUSE_REPORT_CLICK)
            || mode.contains(TermMode::MOUSE_DRAG)
            || mode.contains(TermMode::MOUSE_MOTION)
        {
            let lines = match event.delta {
                ScrollDelta::Lines(lines) => -lines.y as i32,
                ScrollDelta::Pixels(pixels) => {
                    let cell_h: f32 = self.cell_height.into();
                    if cell_h > 0.0 {
                        let px_y: f32 = pixels.y.into();
                        -(px_y / cell_h).round() as i32
                    } else {
                        0
                    }
                }
            };

            if lines != 0 {
                let point = self.mouse_to_point(local_position);
                let term_size = term.size();
                // Clamp coordinates to terminal dimensions (and ensure positive)
                let col = (point.column.0 as u32 + 1).max(1).min(term_size.cols as u32);
                let row = (point.line.0 as u32 + 1).max(1).min(term_size.rows as u32);

                // Button 64 = scroll up, 65 = scroll down
                let button = if lines < 0 { 64 } else { 65 };
                let scroll_count = lines.abs();

                for _ in 0..scroll_count {
                    let mouse_report = if mode.contains(TermMode::SGR_MOUSE) {
                        format!("\x1b[<{};{};{}M", button, col, row)
                    } else {
                        let cb = (32 + button) as u8;
                        let cx_char = (32 + col.min(223)) as u8;
                        let cy_char = (32 + row.min(223)) as u8;
                        format!("\x1b[M{}{}{}", cb as char, cx_char as char, cy_char as char)
                    };
                    term.write(mouse_report.as_bytes());
                }
                drop(term);
                cx.notify();
                return;
            }
        }

        // Normal scroll behavior (scrollback)
        let lines = match event.delta {
            ScrollDelta::Lines(lines) => -lines.y as i32,
            ScrollDelta::Pixels(pixels) => {
                let cell_h: f32 = self.cell_height.into();
                if cell_h > 0.0 {
                    let px_y: f32 = pixels.y.into();
                    -(px_y / cell_h).round() as i32
                } else {
                    0
                }
            }
        };

        if lines != 0 {
            term.scroll(lines);
            cx.notify();
        }
    }

    /// Convert mouse position to terminal point
    fn mouse_to_point(&self, position: Point<Pixels>) -> TermPoint {
        let cell_w: f32 = self.cell_width.into();
        let cell_h: f32 = self.cell_height.into();
        let px_x: f32 = position.x.into();
        let px_y: f32 = position.y.into();

        let col = if cell_w > 0.0 { (px_x / cell_w).floor() as usize } else { 0 };
        let line = if cell_h > 0.0 { (px_y / cell_h).floor() as i32 } else { 0 };
        TermPoint::new(Line(line), Column(col))
    }

    /// Determine which side of the cell the mouse is on
    fn mouse_to_side(&self, position: Point<Pixels>) -> Side {
        let cell_w: f32 = self.cell_width.into();
        let px_x: f32 = position.x.into();
        if cell_w > 0.0 {
            let col_frac = (px_x / cell_w).fract();
            if col_frac < 0.5 { Side::Left } else { Side::Right }
        } else {
            Side::Left
        }
    }

    pub fn clear_selection(&mut self) {
        let term = self.terminal.lock();
        term.clear_selection();
    }

    pub fn selected_text(&self) -> Option<String> {
        let term = self.terminal.lock();
        term.selected_text()
    }

    pub fn terminal(&self) -> Arc<Mutex<Terminal>> {
        self.terminal.clone()
    }
}

/// Cursor shape for rendering
#[derive(Clone, Copy, Debug)]
enum CursorShape {
    Block,
    Hollow,
    Bar,
    Underline,
}

/// A batched text run with position and styling
struct PositionedTextRun {
    col: usize,
    line: usize,
    text: String,
    fg_color: Hsla,
    bold: bool,
}

/// Data prepared in prepaint for use in paint
struct TerminalPaintData {
    cell_width: Pixels,
    cell_height: Pixels,
    cols: usize,
    rows: usize,
    bg_rects: Vec<(usize, usize, Hsla)>,
    selected_cells: Vec<(usize, usize)>,
    text_runs: Vec<PositionedTextRun>,
    cursor: Option<(usize, usize, CursorShape)>,
}

fn color_to_hsla(color: Color, colors: &alacritty_terminal::term::color::Colors) -> Hsla {
    let rgb = color_to_rgb(color, colors);
    let r = rgb.r as f32 / 255.0;
    let g = rgb.g as f32 / 255.0;
    let b = rgb.b as f32 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;

    if max == min {
        Hsla { h: 0.0, s: 0.0, l, a: 1.0 }
    } else {
        let d = max - min;
        let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        let h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        Hsla { h: h / 6.0, s, l, a: 1.0 }
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let terminal = self.terminal.clone();
        let focused = self.focus_handle.is_focused(window);

        // Reset cursor blink when focus changes
        if focused != self.was_focused {
            if focused {
                // Just gained focus - reset blink to visible
                self.cursor_visible = true;
                self.last_blink_toggle = Instant::now();
            }
            self.was_focused = focused;
        }

        let font_family = self.font_family.clone();
        let font_family_paint = self.font_family.clone();
        let font_size = self.font_size;

        // Update cell dimensions from font metrics for accurate mouse coordinate conversion
        let text_system = window.text_system();
        let font_for_measure = font(font_family.clone());
        let font_id = text_system.resolve_font(&font_for_measure);
        self.cell_width = text_system
            .advance(font_id, font_size, 'M')
            .map(|a| a.width)
            .unwrap_or(px(8.0));
        self.cell_height = font_size * 1.4;

        // Cursor is visible if blink state is true, or if we're not focused (hollow cursor always visible)
        let cursor_blink_visible = self.cursor_visible || !focused;

        // Clone bounds_origin for the canvas callback
        let bounds_origin_for_canvas = self.bounds_origin.clone();

        div()
            .size_full()
            .bg(rgb(0x1e1e2e))
            .track_focus(&self.focus_handle)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::handle_mouse_down))
            .on_mouse_move(cx.listener(Self::handle_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::handle_mouse_up))
            .on_scroll_wheel(cx.listener(Self::handle_scroll))
            .on_key_down(cx.listener(Self::handle_key_input))
            .child(
                canvas(
                    {
                        let terminal = terminal.clone();
                        let bounds_origin = bounds_origin_for_canvas.clone();
                        move |bounds, window, _cx| {
                            // Update bounds origin for mouse coordinate conversion
                            *bounds_origin.lock() = bounds.origin;

                            let terminal = terminal.lock();
                            let colors = terminal.colors();
                            let cursor_pos = terminal.cursor_position();
                            let term_mode = terminal.mode();

                            // Check if cursor should be visible:
                            // 1. Terminal's SHOW_CURSOR mode must be on
                            // 2. If focused and blinking: use blink state
                            // 3. If not focused: always show (but rendered as hollow)
                            let show_cursor = term_mode.contains(TermMode::SHOW_CURSOR);
                            let cursor_should_show = show_cursor && (cursor_blink_visible || !focused);

                            // Calculate cell dimensions from font metrics
                            let text_system = window.text_system();
                            let font = font(font_family.clone());
                            let font_id = text_system.resolve_font(&font);
                            let cell_width = text_system
                                .advance(font_id, font_size, 'M')
                                .map(|a| a.width)
                                .unwrap_or(px(8.0));
                            let cell_height = font_size * 1.4;

                            // Calculate grid size based on bounds
                            let cols = (bounds.size.width / cell_width).floor() as usize;
                            let rows = (bounds.size.height / cell_height).floor() as usize;

                            let mut bg_rects = Vec::new();
                            let mut selected_cells = Vec::new();
                            let mut text_runs = Vec::new();

                            terminal.with_term(|term| {
                                let content = term.grid();
                                let screen_lines = term.screen_lines();
                                let columns = term.columns();

                                // Get selection range for checking points
                                let selection_range = term.selection.as_ref().and_then(|s| s.to_range(term));

                                for line_idx in 0..screen_lines {
                                    let line = Line(line_idx as i32);
                                    let mut current_run: Option<PositionedTextRun> = None;

                                    for col_idx in 0..columns {
                                        let col = Column(col_idx);
                                        let pt = TermPoint::new(line, col);
                                        let cell = &content[pt];

                                        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                                            continue;
                                        }

                                        // Check selection using alacritty's built-in
                                        if let Some(ref range) = selection_range {
                                            if range.contains(pt) {
                                                selected_cells.push((col_idx, line_idx));
                                            }
                                        }

                                        // Handle INVERSE flag (reverse video) - swap fg and bg
                                        let is_inverse = cell.flags.contains(Flags::INVERSE);
                                        let (cell_fg, cell_bg) = if is_inverse {
                                            (cell.bg, cell.fg)
                                        } else {
                                            (cell.fg, cell.bg)
                                        };

                                        // Background color
                                        if cell_bg != Color::Named(NamedColor::Background) || is_inverse {
                                            let bg_color = if is_inverse && cell_bg == Color::Named(NamedColor::Foreground) {
                                                // Default foreground as background in inverse mode
                                                color_to_hsla(Color::Named(NamedColor::Foreground), &colors)
                                            } else if is_inverse && cell.fg == Color::Named(NamedColor::Foreground) {
                                                // Use default foreground color for inverse
                                                color_to_hsla(Color::Named(NamedColor::Foreground), &colors)
                                            } else {
                                                color_to_hsla(cell_bg, &colors)
                                            };
                                            bg_rects.push((col_idx, line_idx, bg_color));
                                        }

                                        let c = cell.c;
                                        if c == ' ' || c == '\0' {
                                            if let Some(run) = current_run.take() {
                                                text_runs.push(run);
                                            }
                                            // Still need to draw background for spaces in inverse mode
                                            continue;
                                        }

                                        let fg_color = color_to_hsla(cell_fg, &colors);
                                        let bold = cell.flags.contains(Flags::BOLD);

                                        let can_extend = current_run.as_ref().map_or(false, |run| {
                                            run.line == line_idx
                                                && run.col + run.text.chars().count() == col_idx
                                                && run.fg_color == fg_color
                                                && run.bold == bold
                                        });

                                        if can_extend {
                                            current_run.as_mut().unwrap().text.push(c);
                                        } else {
                                            if let Some(run) = current_run.take() {
                                                text_runs.push(run);
                                            }
                                            current_run = Some(PositionedTextRun {
                                                col: col_idx,
                                                line: line_idx,
                                                text: c.to_string(),
                                                fg_color,
                                                bold,
                                            });
                                        }
                                    }

                                    if let Some(run) = current_run.take() {
                                        text_runs.push(run);
                                    }
                                }
                            });

                            // Determine cursor position and shape
                            let cursor = if cursor_should_show {
                                let col = cursor_pos.column.0;
                                let line = cursor_pos.line.0;

                                // Only show cursor if it's within visible area
                                if line >= 0 && (line as usize) < rows && col < cols {
                                    let shape = if focused {
                                        CursorShape::Block
                                    } else {
                                        CursorShape::Hollow
                                    };
                                    Some((col, line as usize, shape))
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            TerminalPaintData {
                                cell_width,
                                cell_height,
                                cols,
                                rows,
                                bg_rects,
                                selected_cells,
                                text_runs,
                                cursor,
                            }
                        }
                    },
                    {
                        let terminal = terminal.clone();
                        move |bounds, data, window, cx| {
                            let origin = bounds.origin;

                            // Draw background rects
                            for (col, line, color) in &data.bg_rects {
                                let x = origin.x + data.cell_width * *col as f32;
                                let y = origin.y + data.cell_height * *line as f32;
                                window.paint_quad(fill(
                                    Bounds::new(point(x, y), size(data.cell_width, data.cell_height)),
                                    *color,
                                ));
                            }

                            // Draw selection highlight
                            for (col, line) in &data.selected_cells {
                                let x = origin.x + data.cell_width * *col as f32;
                                let y = origin.y + data.cell_height * *line as f32;
                                window.paint_quad(fill(
                                    Bounds::new(point(x, y), size(data.cell_width, data.cell_height)),
                                    hsla(0.6, 0.6, 0.5, 0.3),
                                ));
                            }

                            // Draw text runs
                            for run in &data.text_runs {
                                let x = origin.x + data.cell_width * run.col as f32;
                                let y = origin.y + data.cell_height * run.line as f32;

                                let text: SharedString = run.text.clone().into();
                                let font_weight = if run.bold { FontWeight::BOLD } else { FontWeight::NORMAL };

                                let text_run = gpui::TextRun {
                                    len: text.len(),
                                    font: Font {
                                        family: font_family_paint.clone(),
                                        weight: font_weight,
                                        ..Default::default()
                                    },
                                    color: run.fg_color,
                                    background_color: None,
                                    underline: None,
                                    strikethrough: None,
                                };

                                let shaped = window.text_system().shape_line(
                                    text,
                                    font_size,
                                    &[text_run],
                                    Some(data.cell_width),
                                );

                                let _ = shaped.paint(
                                    point(x, y),
                                    data.cell_height,
                                    TextAlign::Left,
                                    None,
                                    window,
                                    cx,
                                );
                            }

                            // Draw cursor
                            if let Some((col, line, shape)) = data.cursor {
                                let x = origin.x + data.cell_width * col as f32;
                                let y = origin.y + data.cell_height * line as f32;

                                let cursor_color = hsla(0.0, 0.0, 0.9, 0.9);

                                match shape {
                                    CursorShape::Block => {
                                        // Filled block cursor
                                        window.paint_quad(fill(
                                            Bounds::new(point(x, y), size(data.cell_width, data.cell_height)),
                                            cursor_color,
                                        ));
                                    }
                                    CursorShape::Hollow => {
                                        // Hollow block (just outline) for unfocused
                                        let border_width = px(1.0);
                                        // Top
                                        window.paint_quad(fill(
                                            Bounds::new(point(x, y), size(data.cell_width, border_width)),
                                            cursor_color,
                                        ));
                                        // Bottom
                                        window.paint_quad(fill(
                                            Bounds::new(point(x, y + data.cell_height - border_width), size(data.cell_width, border_width)),
                                            cursor_color,
                                        ));
                                        // Left
                                        window.paint_quad(fill(
                                            Bounds::new(point(x, y), size(border_width, data.cell_height)),
                                            cursor_color,
                                        ));
                                        // Right
                                        window.paint_quad(fill(
                                            Bounds::new(point(x + data.cell_width - border_width, y), size(border_width, data.cell_height)),
                                            cursor_color,
                                        ));
                                    }
                                    CursorShape::Bar => {
                                        // Thin vertical bar
                                        let bar_width = px(2.0);
                                        window.paint_quad(fill(
                                            Bounds::new(point(x, y), size(bar_width, data.cell_height)),
                                            cursor_color,
                                        ));
                                    }
                                    CursorShape::Underline => {
                                        // Underline cursor
                                        let underline_height = px(2.0);
                                        window.paint_quad(fill(
                                            Bounds::new(point(x, y + data.cell_height - underline_height), size(data.cell_width, underline_height)),
                                            cursor_color,
                                        ));
                                    }
                                }
                            }

                            // Check if resize is needed
                            let cols = data.cols as u16;
                            let rows = data.rows as u16;
                            if cols > 0 && rows > 0 {
                                let mut term = terminal.lock();
                                let current_size = term.size();
                                if current_size.cols != cols || current_size.rows != rows {
                                    let cell_w: f32 = data.cell_width.into();
                                    let cell_h: f32 = data.cell_height.into();
                                    let pixel_width = (cell_w * cols as f32) as u16;
                                    let pixel_height = (cell_h * rows as f32) as u16;
                                    term.resize(TerminalSize::with_pixels(cols, rows, pixel_width, pixel_height));
                                }
                            }
                        }
                    },
                )
                .size_full(),
            )
    }
}

pub fn terminal_view(terminal: Arc<Mutex<Terminal>>, _window: &mut Window, cx: &mut App) -> Entity<TerminalView> {
    cx.new(|cx| TerminalView::new(terminal, cx))
}
