//! Split pane container for terminal views

use gpui::*;
use gpui::prelude::*;
use parking_lot::Mutex;
use std::sync::Arc;

use crate::terminal::Terminal;
use super::terminal_view::TerminalView;

/// Split orientation
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitOrientation {
    Horizontal, // Side by side (left/right)
    Vertical,   // Stacked (top/bottom)
}

/// Events emitted by SplitContainer
pub enum SplitContainerEvent {
    /// A pane was closed (index of closed pane)
    PaneClosed(usize),
    /// Active pane changed
    ActivePaneChanged(usize),
}

impl EventEmitter<SplitContainerEvent> for SplitContainer {}

/// A container that holds multiple terminal panes with resize handles
pub struct SplitContainer {
    /// Terminal views in this container
    panes: Vec<Entity<TerminalView>>,
    /// Terminal instances corresponding to panes
    terminals: Vec<Arc<Mutex<Terminal>>>,
    /// Split positions as percentages (0.0 to 1.0) - one less than panes count
    split_positions: Vec<f32>,
    /// Currently active pane index
    active_pane: usize,
    /// Split orientation
    orientation: SplitOrientation,
    /// Whether currently resizing a divider
    is_resizing: Option<usize>,
    /// Focus handle
    focus_handle: FocusHandle,
    /// Color scheme for new panes
    color_scheme: Option<String>,
}

impl SplitContainer {
    /// Create a new split container with a single pane
    pub fn new(
        terminal: Arc<Mutex<Terminal>>,
        color_scheme: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let view = cx.new(|cx| TerminalView::new(terminal.clone(), color_scheme.clone(), cx));

        Self {
            panes: vec![view],
            terminals: vec![terminal],
            split_positions: Vec::new(),
            active_pane: 0,
            orientation: SplitOrientation::Horizontal,
            is_resizing: None,
            focus_handle: cx.focus_handle(),
            color_scheme,
        }
    }

    /// Get the number of panes
    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    /// Get the active pane index
    pub fn active_pane(&self) -> usize {
        self.active_pane
    }

    /// Get the active terminal
    pub fn active_terminal(&self) -> Option<Arc<Mutex<Terminal>>> {
        self.terminals.get(self.active_pane).cloned()
    }

    /// Split the active pane horizontally (left/right)
    pub fn split_horizontal(&mut self, new_terminal: Arc<Mutex<Terminal>>, cx: &mut Context<Self>) {
        self.split(new_terminal, SplitOrientation::Horizontal, cx);
    }

    /// Split the active pane vertically (top/bottom)
    pub fn split_vertical(&mut self, new_terminal: Arc<Mutex<Terminal>>, cx: &mut Context<Self>) {
        self.split(new_terminal, SplitOrientation::Vertical, cx);
    }

    /// Split the active pane
    fn split(&mut self, new_terminal: Arc<Mutex<Terminal>>, orientation: SplitOrientation, cx: &mut Context<Self>) {
        // Limit to 4 panes
        if self.panes.len() >= 4 {
            return;
        }

        // If this is the second pane, set the orientation
        if self.panes.len() == 1 {
            self.orientation = orientation;
        }

        let view = cx.new(|cx| TerminalView::new(new_terminal.clone(), self.color_scheme.clone(), cx));

        // Insert after active pane
        let insert_idx = self.active_pane + 1;
        self.panes.insert(insert_idx, view);
        self.terminals.insert(insert_idx, new_terminal);

        // Add a split position for the new divider
        // Calculate equal spacing
        let num_panes = self.panes.len();
        self.split_positions = (1..num_panes)
            .map(|i| i as f32 / num_panes as f32)
            .collect();

        // Focus the new pane
        self.active_pane = insert_idx;
        cx.emit(SplitContainerEvent::ActivePaneChanged(self.active_pane));
        cx.notify();
    }

    /// Close the active pane
    pub fn close_active_pane(&mut self, cx: &mut Context<Self>) -> bool {
        if self.panes.len() <= 1 {
            // Can't close the last pane
            return false;
        }

        let closed_idx = self.active_pane;
        self.panes.remove(closed_idx);
        self.terminals.remove(closed_idx);

        // Recalculate split positions
        let num_panes = self.panes.len();
        self.split_positions = (1..num_panes)
            .map(|i| i as f32 / num_panes as f32)
            .collect();

        // Adjust active pane
        if self.active_pane >= self.panes.len() {
            self.active_pane = self.panes.len() - 1;
        }

        cx.emit(SplitContainerEvent::PaneClosed(closed_idx));
        cx.emit(SplitContainerEvent::ActivePaneChanged(self.active_pane));
        cx.notify();
        true
    }

    /// Focus next pane
    pub fn focus_next_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panes.len() > 1 {
            self.active_pane = (self.active_pane + 1) % self.panes.len();
            self.focus_active_pane(window, cx);
            cx.emit(SplitContainerEvent::ActivePaneChanged(self.active_pane));
        }
    }

    /// Focus previous pane
    pub fn focus_prev_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panes.len() > 1 {
            if self.active_pane == 0 {
                self.active_pane = self.panes.len() - 1;
            } else {
                self.active_pane -= 1;
            }
            self.focus_active_pane(window, cx);
            cx.emit(SplitContainerEvent::ActivePaneChanged(self.active_pane));
        }
    }

    /// Focus the active pane
    pub fn focus_active_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.panes.get(self.active_pane).cloned() {
            view.update(cx, |v, cx| v.focus(window, cx));
        }
        cx.notify();
    }

    /// Set active pane by index
    fn set_active_pane(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.panes.len() && index != self.active_pane {
            self.active_pane = index;
            self.focus_active_pane(window, cx);
            cx.emit(SplitContainerEvent::ActivePaneChanged(self.active_pane));
        }
    }

    /// Handle resize drag
    fn handle_resize_drag(&mut self, divider_index: usize, position: f32, total_size: f32, cx: &mut Context<Self>) {
        if divider_index < self.split_positions.len() {
            let ratio = (position / total_size).clamp(0.1, 0.9);

            // Ensure this position doesn't cross adjacent dividers
            let min_ratio = if divider_index > 0 {
                self.split_positions[divider_index - 1] + 0.1
            } else {
                0.1
            };
            let max_ratio = if divider_index + 1 < self.split_positions.len() {
                self.split_positions[divider_index + 1] - 0.1
            } else {
                0.9
            };

            self.split_positions[divider_index] = ratio.clamp(min_ratio, max_ratio);
            cx.notify();
        }
    }

    /// Calculate pane bounds based on split positions
    fn calculate_pane_sizes(&self, total_size: f32) -> Vec<f32> {
        if self.panes.len() == 1 {
            return vec![total_size];
        }

        let mut sizes = Vec::with_capacity(self.panes.len());
        let mut prev_pos = 0.0;

        for &pos in self.split_positions.iter() {
            sizes.push((pos - prev_pos) * total_size);
            prev_pos = pos;
        }
        // Last pane
        sizes.push((1.0 - prev_pos) * total_size);

        sizes
    }
}

impl Focusable for SplitContainer {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SplitContainer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pane_count = self.panes.len();
        let active_pane = self.active_pane;
        let orientation = self.orientation;
        let is_resizing = self.is_resizing;

        if pane_count == 1 {
            // Single pane - no split
            return div()
                .size_full()
                .child(self.panes[0].clone())
                .into_any_element();
        }

        // Build split layout
        let mut container = div()
            .size_full()
            .flex()
            .when(orientation == SplitOrientation::Horizontal, |el| el.flex_row())
            .when(orientation == SplitOrientation::Vertical, |el| el.flex_col());

        // Clone split positions for closures
        let split_positions = self.split_positions.clone();

        for (idx, view) in self.panes.iter().enumerate() {
            let is_active = idx == active_pane;

            // Calculate flex basis from split positions
            let flex_basis = if idx == 0 {
                split_positions.first().copied().unwrap_or(0.5)
            } else if idx == pane_count - 1 {
                1.0 - split_positions.last().copied().unwrap_or(0.5)
            } else {
                split_positions.get(idx).copied().unwrap_or(0.5)
                    - split_positions.get(idx - 1).copied().unwrap_or(0.0)
            };

            // Pane wrapper with border highlighting for active pane
            let pane_wrapper = div()
                .flex_1()
                .flex_basis(px(flex_basis * 1000.0)) // Use large number for flex calculation
                .min_w(px(100.0))
                .min_h(px(50.0))
                .overflow_hidden()
                .when(is_active, |el| {
                    el.border_2().border_color(rgb(0x89b4fa))
                })
                .when(!is_active, |el| {
                    el.border_1().border_color(rgb(0x313244))
                })
                .on_mouse_down(MouseButton::Left, {
                    let idx = idx;
                    cx.listener(move |this, _event, window, cx| {
                        this.set_active_pane(idx, window, cx);
                    })
                })
                .child(view.clone());

            container = container.child(pane_wrapper);

            // Add divider between panes (not after the last one)
            if idx < pane_count - 1 {
                let divider_idx = idx;
                let is_divider_resizing = is_resizing == Some(divider_idx);

                let divider = div()
                    .id(ElementId::Name(format!("divider-{}", divider_idx).into()))
                    .when(orientation == SplitOrientation::Horizontal, |el| {
                        el.w(px(6.0)).h_full().cursor_col_resize()
                    })
                    .when(orientation == SplitOrientation::Vertical, |el| {
                        el.h(px(6.0)).w_full().cursor_row_resize()
                    })
                    .when(is_divider_resizing, |el| el.bg(rgb(0x89b4fa)))
                    .when(!is_divider_resizing, |el| {
                        el.bg(rgb(0x313244)).hover(|h| h.bg(rgb(0x45475a)))
                    })
                    .on_mouse_down(MouseButton::Left, {
                        let divider_idx = divider_idx;
                        cx.listener(move |this, _event, _window, cx| {
                            this.is_resizing = Some(divider_idx);
                            cx.notify();
                        })
                    })
;

                container = container.child(divider);
            }
        }

        // Handle mouse move for resizing at container level
        let orientation_for_move = orientation;
        container = container.on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
            if let Some(divider_idx) = this.is_resizing {
                let position = match orientation_for_move {
                    SplitOrientation::Horizontal => event.position.x.into(),
                    SplitOrientation::Vertical => event.position.y.into(),
                };
                // Estimate total size (this is approximate, actual bounds would be better)
                let total_size = 1000.0; // Will be scaled by flex
                this.handle_resize_drag(divider_idx, position, total_size, cx);
            }
        }));

        // Handle mouse up to end resizing
        container = container.on_mouse_up(MouseButton::Left, cx.listener(|this, _event, _window, cx| {
            if this.is_resizing.is_some() {
                this.is_resizing = None;
                cx.notify();
            }
        }));

        container = container.on_mouse_up_out(MouseButton::Left, cx.listener(|this, _event, _window, cx| {
            if this.is_resizing.is_some() {
                this.is_resizing = None;
                cx.notify();
            }
        }));

        container.into_any_element()
    }
}
