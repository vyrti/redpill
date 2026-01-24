use gpui::*;
use gpui::prelude::*;
use parking_lot::Mutex;
use std::sync::Arc;
use uuid::Uuid;

use crate::app::AppState;
use crate::terminal::Terminal;

use super::agent_panel::{AgentPanel, AgentPanelEvent};
use super::quit_confirm_dialog::QuitConfirmDialog;
use super::session_tree::SessionTree;
use super::split_container::SplitContainer;
use super::terminal_tabs::{TabContextMenuState, TabInfo, TerminalTabs};

/// Minimum session tree width in pixels
const MIN_TREE_WIDTH: f32 = 150.0;
/// Maximum session tree width in pixels
const MAX_TREE_WIDTH: f32 = 500.0;
/// Minimum agent panel width in pixels
const MIN_AGENT_WIDTH: f32 = 280.0;
/// Maximum agent panel width in pixels
const MAX_AGENT_WIDTH: f32 = 800.0;

/// Main window component
pub struct MainWindow {
    /// Session tree view
    session_tree: Entity<SessionTree>,
    /// Terminal tabs view
    tabs_view: Entity<TerminalTabs>,
    /// Agent panel view
    agent_panel: Entity<AgentPanel>,
    /// Current split containers (one per tab)
    split_containers: Vec<(Uuid, Entity<SplitContainer>)>,
    /// Active terminal tab ID
    active_tab_id: Option<Uuid>,
    /// Previously active tab ID (to detect changes)
    prev_active_tab_id: Option<Uuid>,
    /// Session tree width in pixels
    session_tree_width: f32,
    /// Whether currently resizing the session tree
    is_resizing: bool,
    /// Agent panel width in pixels
    agent_panel_width: f32,
    /// Whether currently resizing the agent panel
    is_resizing_agent: bool,
    /// Whether the agent panel is visible
    agent_panel_visible: bool,
    /// Subscriptions
    _subscriptions: Vec<Subscription>,
}

impl MainWindow {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Create session tree
        let session_tree = cx.new(|cx| {
            SessionTree::new(cx)
        });

        // Create tabs view with empty tabs
        let tabs_view = cx.new(|_| TerminalTabs::new(Vec::new(), None));

        // Create agent panel
        let agent_panel = cx.new(|cx| AgentPanel::new(cx));

        // Subscribe to agent panel events
        let agent_subscription = cx.subscribe(&agent_panel, |this, _agent_panel, event, cx| {
            match event {
                AgentPanelEvent::ToggleVisibility => {
                    this.agent_panel_visible = !this.agent_panel_visible;
                    cx.notify();
                }
                _ => {}
            }
        });

        // Get initial widths from config
        let (session_tree_width, agent_panel_width) = cx
            .try_global::<AppState>()
            .map(|state| {
                let app = state.app.lock();
                (
                    app.config.session_tree.width as f32,
                    app.config.agent_panel.width as f32,
                )
            })
            .unwrap_or((250.0, 360.0));

        Self {
            session_tree,
            tabs_view,
            agent_panel,
            split_containers: Vec::new(),
            active_tab_id: None,
            prev_active_tab_id: None,
            session_tree_width,
            is_resizing: false,
            agent_panel_width,
            is_resizing_agent: false,
            agent_panel_visible: true,
            _subscriptions: vec![agent_subscription],
        }
    }

    /// Synchronize tabs with app state (call in render)
    fn sync_tabs_from_state(&mut self, cx: &mut Context<Self>) {
        // First, extract all the data we need from AppState
        let (tab_infos, active_tab, new_tabs, tab_ids) = {
            let Some(state) = cx.try_global::<AppState>() else {
                return;
            };
            let app = state.app.lock();

            let tab_infos: Vec<TabInfo> = app.tabs.iter().map(TabInfo::from).collect();
            let active_tab = app.active_tab().map(|t| t.id);

            // Collect info for new tabs that need views created (including color_scheme)
            let new_tabs: Vec<_> = app
                .tabs
                .iter()
                .filter(|tab| !self.split_containers.iter().any(|(id, _)| *id == tab.id))
                .map(|tab| (tab.id, tab.terminal.clone(), tab.color_scheme.clone()))
                .collect();

            let tab_ids: Vec<Uuid> = app.tabs.iter().map(|t| t.id).collect();

            (tab_infos, active_tab, new_tabs, tab_ids)
        };
        // AppState borrow is now dropped

        // Update tabs view
        self.tabs_view.update(cx, |view, _| {
            view.set_tabs(tab_infos);
            view.set_active_tab(active_tab);
        });

        self.active_tab_id = active_tab;

        // Create split containers for new tabs
        for (tab_id, terminal, color_scheme) in new_tabs {
            let container = cx.new(|cx| SplitContainer::new(terminal, color_scheme, cx));
            self.split_containers.push((tab_id, container));
        }

        // Remove containers for closed tabs
        self.split_containers.retain(|(id, _)| tab_ids.contains(id));
    }

    /// Get the active split container
    fn active_split_container(&self) -> Option<&Entity<SplitContainer>> {
        self.active_tab_id.and_then(|id| {
            self.split_containers.iter().find(|(tid, _)| *tid == id).map(|(_, v)| v)
        })
    }

    /// Render tab context menu at window level
    fn render_tab_context_menu(&self, menu: &TabContextMenuState, cx: &mut Context<Self>) -> impl IntoElement {
        let tab_id = menu.tab_id;
        let tab_index = menu.tab_index;
        let tab_count = menu.tab_count;
        let has_tabs_to_right = tab_index < tab_count.saturating_sub(1);
        let has_tabs_to_left = tab_index > 0;
        let has_other_tabs = tab_count > 1;

        let tabs_view = self.tabs_view.clone();

        div()
            .absolute()
            .left(menu.position.x)
            .top(menu.position.y)
            .w(px(180.0))
            .bg(rgb(0x313244))
            .border_1()
            .border_color(rgb(0x45475a))
            .rounded_md()
            .shadow_lg()
            .py_1()
            // Close Tab
            .child(
                div()
                    .id("ctx-close-tab")
                    .px_3()
                    .py_1()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x45475a)))
                    .on_click({
                        let tabs_view = tabs_view.clone();
                        cx.listener(move |_this, _event, window, cx| {
                            tabs_view.update(cx, |view, cx| {
                                view.close_tab_action(tab_id, window, cx);
                            });
                        })
                    })
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xcdd6f4))
                            .child("Close Tab"),
                    ),
            )
            // Separator
            .child(
                div()
                    .h(px(1.0))
                    .mx_2()
                    .my_1()
                    .bg(rgb(0x45475a)),
            )
            // Close Other Tabs
            .child(
                div()
                    .id("ctx-close-other")
                    .px_3()
                    .py_1()
                    .when(has_other_tabs, |this| {
                        let tabs_view = tabs_view.clone();
                        this.cursor_pointer()
                            .hover(|s| s.bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |_this, _event, window, cx| {
                                tabs_view.update(cx, |view, cx| {
                                    view.close_other_tabs_action(tab_id, window, cx);
                                });
                            }))
                    })
                    .child(
                        div()
                            .text_sm()
                            .text_color(if has_other_tabs { rgb(0xcdd6f4) } else { rgb(0x6c7086) })
                            .child("Close Other Tabs"),
                    ),
            )
            // Close Tabs to the Right
            .child(
                div()
                    .id("ctx-close-right")
                    .px_3()
                    .py_1()
                    .when(has_tabs_to_right, |this| {
                        let tabs_view = tabs_view.clone();
                        this.cursor_pointer()
                            .hover(|s| s.bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |_this, _event, window, cx| {
                                tabs_view.update(cx, |view, cx| {
                                    view.close_tabs_to_right_action(tab_index, window, cx);
                                });
                            }))
                    })
                    .child(
                        div()
                            .text_sm()
                            .text_color(if has_tabs_to_right { rgb(0xcdd6f4) } else { rgb(0x6c7086) })
                            .child("Close Tabs to the Right"),
                    ),
            )
            // Close Tabs to the Left
            .child(
                div()
                    .id("ctx-close-left")
                    .px_3()
                    .py_1()
                    .when(has_tabs_to_left, |this| {
                        let tabs_view = tabs_view.clone();
                        this.cursor_pointer()
                            .hover(|s| s.bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |_this, _event, window, cx| {
                                tabs_view.update(cx, |view, cx| {
                                    view.close_tabs_to_left_action(tab_index, window, cx);
                                });
                            }))
                    })
                    .child(
                        div()
                            .text_sm()
                            .text_color(if has_tabs_to_left { rgb(0xcdd6f4) } else { rgb(0x6c7086) })
                            .child("Close Tabs to the Left"),
                    ),
            )
            // Separator
            .child(
                div()
                    .h(px(1.0))
                    .mx_2()
                    .my_1()
                    .bg(rgb(0x45475a)),
            )
            // Close All Tabs
            .child(
                div()
                    .id("ctx-close-all")
                    .px_3()
                    .py_1()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x45475a)))
                    .on_click({
                        let tabs_view = tabs_view.clone();
                        cx.listener(move |_this, _event, window, cx| {
                            tabs_view.update(cx, |view, cx| {
                                view.close_all_tabs_action(window, cx);
                            });
                        })
                    })
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xf38ba8))
                            .child("Close All Tabs"),
                    ),
            )
    }

    /// Handle resize end - save width to config
    fn finish_resize(&mut self, cx: &mut Context<Self>) {
        if self.is_resizing {
            self.is_resizing = false;
            // Save width to config
            if let Some(app_state) = cx.try_global::<AppState>() {
                let mut app = app_state.app.lock();
                app.config.session_tree.width = self.session_tree_width as u32;
                let _ = app.config.save();
            }
            cx.notify();
        }
    }

    /// Handle agent panel resize end - save width to config
    fn finish_agent_resize(&mut self, cx: &mut Context<Self>) {
        if self.is_resizing_agent {
            self.is_resizing_agent = false;
            // Save width to config
            if let Some(app_state) = cx.try_global::<AppState>() {
                let mut app = app_state.app.lock();
                app.config.agent_panel.width = self.agent_panel_width as u32;
                let _ = app.config.save();
            }
            cx.notify();
        }
    }

    /// Split the active pane horizontally (left/right)
    fn split_horizontal(&mut self, cx: &mut Context<Self>) {
        if let Some(container) = self.active_split_container().cloned() {
            // Create a new local terminal for the split
            if let Some(new_terminal) = self.create_local_terminal(cx) {
                container.update(cx, |split, cx| {
                    split.split_horizontal(new_terminal, cx);
                });
            }
        }
    }

    /// Split the active pane vertically (top/bottom)
    fn split_vertical(&mut self, cx: &mut Context<Self>) {
        if let Some(container) = self.active_split_container().cloned() {
            // Create a new local terminal for the split
            if let Some(new_terminal) = self.create_local_terminal(cx) {
                container.update(cx, |split, cx| {
                    split.split_vertical(new_terminal, cx);
                });
            }
        }
    }

    /// Close the active pane (or tab if single pane)
    fn close_active_pane(&mut self, cx: &mut Context<Self>) {
        if let Some(container) = self.active_split_container().cloned() {
            let closed = container.update(cx, |split, cx| {
                split.close_active_pane(cx)
            });
            if !closed {
                // Single pane - close the tab instead
                if let Some(tab_id) = self.active_tab_id {
                    if let Some(app_state) = cx.try_global::<AppState>() {
                        let mut app = app_state.app.lock();
                        app.close_tab(tab_id);
                    }
                }
            }
            cx.notify();
        }
    }

    /// Focus next pane in the active split container
    fn focus_next_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(container) = self.active_split_container().cloned() {
            container.update(cx, |split, cx| {
                split.focus_next_pane(window, cx);
            });
        }
    }

    /// Focus previous pane in the active split container
    fn focus_prev_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(container) = self.active_split_container().cloned() {
            container.update(cx, |split, cx| {
                split.focus_prev_pane(window, cx);
            });
        }
    }

    /// Create a new local terminal
    fn create_local_terminal(&self, cx: &Context<Self>) -> Option<Arc<Mutex<Terminal>>> {
        let scrollback = cx
            .try_global::<AppState>()
            .map(|state| state.app.lock().config.scrollback_lines)
            .unwrap_or(10000);

        let config = crate::terminal::TerminalConfig {
            scrollback_lines: scrollback,
            ..Default::default()
        };

        match Terminal::new_local(config) {
            Ok(terminal) => Some(Arc::new(Mutex::new(terminal))),
            Err(e) => {
                tracing::error!("Failed to create terminal for split: {}", e);
                None
            }
        }
    }

    /// Handle keyboard shortcuts
    fn handle_key_input(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;

        // Split horizontal: Cmd+D (Mac) or Ctrl+Shift+D
        if (keystroke.modifiers.platform && keystroke.key == "d")
            || (keystroke.modifiers.control && keystroke.modifiers.shift && keystroke.key == "d")
        {
            self.split_horizontal(cx);
            cx.stop_propagation();
            return;
        }

        // Split vertical: Cmd+Shift+D (Mac) or Ctrl+Shift+E
        if (keystroke.modifiers.platform && keystroke.modifiers.shift && keystroke.key == "d")
            || (keystroke.modifiers.control && keystroke.modifiers.shift && keystroke.key == "e")
        {
            self.split_vertical(cx);
            cx.stop_propagation();
            return;
        }

        // Focus next pane: Cmd+] or Ctrl+]
        if (keystroke.modifiers.platform || keystroke.modifiers.control) && keystroke.key == "]" {
            self.focus_next_pane(window, cx);
            cx.stop_propagation();
            return;
        }

        // Focus prev pane: Cmd+[ or Ctrl+[
        if (keystroke.modifiers.platform || keystroke.modifiers.control) && keystroke.key == "[" {
            self.focus_prev_pane(window, cx);
            cx.stop_propagation();
            return;
        }
    }
}

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Sync terminal views with app state
        self.sync_tabs_from_state(cx);

        // Focus split container when active tab changes
        if self.active_tab_id != self.prev_active_tab_id {
            self.prev_active_tab_id = self.active_tab_id;
            if let Some(container) = self.active_split_container().cloned() {
                container.update(cx, |split_container, cx| {
                    split_container.focus_active_pane(window, cx);
                });
            }
        }

        let session_tree_visible = if let Some(state) = cx.try_global::<AppState>() {
            state.app.lock().session_tree_visible
        } else {
            true
        };

        let tree_width = self.session_tree_width;
        let is_resizing = self.is_resizing;
        let agent_width = self.agent_panel_width;
        let is_resizing_agent = self.is_resizing_agent;

        // Get tab context menu state
        let tab_context_menu = self.tabs_view.read(cx).context_menu_state();

        // Get window width for agent panel resize calculation
        let window_bounds = window.bounds();
        let window_width: f32 = window_bounds.size.width.into();

        // Root container with window-level mouse handlers for drag tracking
        let mut root = div()
            .id("main-window-root")
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1e2e))
            // Handle split keyboard shortcuts
            .on_key_down(cx.listener(Self::handle_key_input))
            // Window-level mouse move handler for resize dragging
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
                if this.is_resizing {
                    let x: f32 = event.position.x.into();
                    let new_width = x.clamp(MIN_TREE_WIDTH, MAX_TREE_WIDTH);
                    this.session_tree_width = new_width;
                    cx.notify();
                }
                if this.is_resizing_agent {
                    let x: f32 = event.position.x.into();
                    // Agent panel width is measured from the right edge
                    let new_width = (window_width - x).clamp(MIN_AGENT_WIDTH, MAX_AGENT_WIDTH);
                    this.agent_panel_width = new_width;
                    cx.notify();
                }
            }))
            // Window-level mouse up handler to end resize
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _event, _window, cx| {
                this.finish_resize(cx);
                this.finish_agent_resize(cx);
            }))
            // Also handle mouse up outside window (when dragged out)
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _event, _window, cx| {
                this.finish_resize(cx);
                this.finish_agent_resize(cx);
            }))
            .child(
                // Main content area
                div()
                    .flex()
                    .flex_1()
                    .overflow_hidden()
                    // Expand button (when tree is collapsed)
                    .when(!session_tree_visible, |this| {
                        this.child(
                            div()
                                .id("expand-tree-btn")
                                .w(px(24.0))
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(rgb(0x1e1e2e))
                                .border_r_1()
                                .border_color(rgb(0x313244))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(0x313244)))
                                .on_click(cx.listener(|_this, _event, _window, cx| {
                                    if let Some(app_state) = cx.try_global::<AppState>() {
                                        let mut app = app_state.app.lock();
                                        app.toggle_session_tree();
                                    }
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(0x6c7086))
                                        .child("\u{25B6}"),
                                ),
                        )
                    })
                    // Session tree (left panel) with dynamic width
                    .when(session_tree_visible, |this| {
                        this.child(
                            div()
                                .w(px(tree_width))
                                .h_full()
                                .border_r_1()
                                .border_color(rgb(0x313244))
                                .child(self.session_tree.clone()),
                        )
                    })
                    // Resize handle - only handles mouse down to start resize
                    .when(session_tree_visible, |this| {
                        this.child(
                            div()
                                .id("resize-handle")
                                .w(px(6.0))
                                .h_full()
                                .cursor_col_resize()
                                .when(is_resizing, |s| s.bg(rgb(0x89b4fa)))
                                .when(!is_resizing, |s| s.hover(|h| h.bg(rgb(0x45475a))))
                                .on_mouse_down(MouseButton::Left, cx.listener(|this, _event, _window, cx| {
                                    this.is_resizing = true;
                                    cx.notify();
                                })),
                        )
                    })
                    // Terminal area (center)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .overflow_hidden()
                            // Tab bar
                            .child(self.tabs_view.clone())
                            // Terminal split container
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .when_some(self.active_split_container().cloned(), |el, container| {
                                        el.child(container)
                                    })
                                    .when(self.active_split_container().is_none(), |this| {
                                        this.flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                div()
                                                    .text_color(rgb(0x6c7086))
                                                    .child("Press Ctrl+Shift+T to open a new terminal"),
                                            )
                                    }),
                            ),
                    )
                    // Agent panel resize handle
                    .when(self.agent_panel_visible, |this| {
                        this.child(
                            div()
                                .id("agent-resize-handle")
                                .w(px(6.0))
                                .h_full()
                                .cursor_col_resize()
                                .when(is_resizing_agent, |s| s.bg(rgb(0x89b4fa)))
                                .when(!is_resizing_agent, |s| s.hover(|h| h.bg(rgb(0x45475a))))
                                .on_mouse_down(MouseButton::Left, cx.listener(|this, _event, _window, cx| {
                                    this.is_resizing_agent = true;
                                    cx.notify();
                                })),
                        )
                    })
                    // Agent panel (right sidebar) with dynamic width
                    .when(self.agent_panel_visible, |this| {
                        this.child(
                            div()
                                .w(px(agent_width))
                                .h_full()
                                .child(self.agent_panel.clone())
                        )
                    })
                    // Agent panel toggle button (when panel is hidden)
                    .when(!self.agent_panel_visible, |this| {
                        this.child(
                            div()
                                .id("expand-agent-btn")
                                .w(px(24.0))
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(rgb(0x1e1e2e))
                                .border_l_1()
                                .border_color(rgb(0x313244))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(0x313244)))
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.agent_panel_visible = true;
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(0x6c7086))
                                        .child("\u{25C0}"),
                                ),
                        )
                    }),
            )
            // Status bar
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(24.0))
                    .px_3()
                    .bg(rgb(0x181825))
                    .border_t_1()
                    .border_color(rgb(0x313244))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .child("RedPill - SSH / Kube Terminal Manager"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .child(format!(
                                "{} tab{}",
                                self.split_containers.len(),
                                if self.split_containers.len() == 1 { "" } else { "s" }
                            )),
                    ),
            );

        // Add tab context menu if open (rendered at window level to avoid clipping)
        if let Some(menu) = tab_context_menu {
            let tabs_view = self.tabs_view.clone();
            // Backdrop to dismiss menu
            root = root.child(
                div()
                    .id("tab-context-menu-backdrop")
                    .absolute()
                    .inset_0()
                    .on_mouse_up(MouseButton::Left, {
                        let tabs_view = tabs_view.clone();
                        cx.listener(move |_this, _event: &MouseUpEvent, _window, cx| {
                            tabs_view.update(cx, |view, cx| {
                                view.dismiss_context_menu(cx);
                            });
                        })
                    })
                    .on_mouse_up(MouseButton::Right, {
                        let tabs_view = tabs_view.clone();
                        cx.listener(move |_this, _event: &MouseUpEvent, _window, cx| {
                            tabs_view.update(cx, |view, cx| {
                                view.dismiss_context_menu(cx);
                            });
                        })
                    }),
            );
            // Context menu
            root = root.child(self.render_tab_context_menu(&menu, cx));
        }

        root
    }
}

/// Create the main window
pub fn main_window(_window: &mut Window, cx: &mut App) -> Entity<MainWindow> {
    cx.new(|cx| MainWindow::new(cx))
}

/// Open the main application window
pub fn open_main_window(cx: &mut App) -> WindowHandle<MainWindow> {
    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1200.0), px(800.0)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("RedPill".into()),
            appears_transparent: false,
            ..Default::default()
        }),
        ..Default::default()
    };

    cx.open_window(window_options, |window, cx| {
        // Initialize app state
        let app_state = AppState::new();
        cx.set_global(app_state);

        // Register window close handler to check for active SSH connections
        window.on_window_should_close(cx, |_window, cx| {
            // Check for active SSH connections
            let ssh_count = if let Some(state) = cx.try_global::<AppState>() {
                state.app.lock().active_ssh_connection_count()
            } else {
                0
            };

            if ssh_count > 0 {
                // Show confirmation dialog and prevent close
                QuitConfirmDialog::open(ssh_count, cx);
                false // Don't close the window yet
            } else {
                true // Allow the window to close
            }
        });

        // Activate window to bring to foreground
        window.activate_window();

        main_window(window, cx)
    })
    .expect("Failed to open window")
}
