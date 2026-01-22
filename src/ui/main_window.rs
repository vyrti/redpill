use gpui::*;
use gpui::prelude::*;
use uuid::Uuid;

use crate::app::AppState;

use super::quit_confirm_dialog::QuitConfirmDialog;
use super::session_tree::SessionTree;
use super::terminal_tabs::{TabInfo, TerminalTabs};
use super::terminal_view::TerminalView;

/// Main window component
pub struct MainWindow {
    /// Session tree view
    session_tree: Entity<SessionTree>,
    /// Terminal tabs view
    tabs_view: Entity<TerminalTabs>,
    /// Current terminal views (one per tab)
    terminal_views: Vec<(Uuid, Entity<TerminalView>)>,
    /// Active terminal tab ID
    active_tab_id: Option<Uuid>,
    /// Previously active tab ID (to detect changes)
    prev_active_tab_id: Option<Uuid>,
}

impl MainWindow {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Create session tree
        let session_tree = cx.new(|_| {
            SessionTree::new()
        });

        // Create tabs view with empty tabs
        let tabs_view = cx.new(|_| TerminalTabs::new(Vec::new(), None));

        Self {
            session_tree,
            tabs_view,
            terminal_views: Vec::new(),
            active_tab_id: None,
            prev_active_tab_id: None,
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

            // Collect info for new tabs that need views created
            let new_tabs: Vec<_> = app
                .tabs
                .iter()
                .filter(|tab| !self.terminal_views.iter().any(|(id, _)| *id == tab.id))
                .map(|tab| (tab.id, tab.terminal.clone()))
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

        // Create terminal views for new tabs
        for (tab_id, terminal) in new_tabs {
            let view = cx.new(|cx| TerminalView::new(terminal, cx));
            self.terminal_views.push((tab_id, view));
        }

        // Remove views for closed tabs
        self.terminal_views.retain(|(id, _)| tab_ids.contains(id));
    }

    /// Get the active terminal view
    fn active_terminal_view(&self) -> Option<&Entity<TerminalView>> {
        self.active_tab_id.and_then(|id| {
            self.terminal_views.iter().find(|(tid, _)| *tid == id).map(|(_, v)| v)
        })
    }
}

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Sync terminal views with app state
        self.sync_tabs_from_state(cx);

        // Focus terminal view when active tab changes
        if self.active_tab_id != self.prev_active_tab_id {
            self.prev_active_tab_id = self.active_tab_id;
            if let Some(view) = self.active_terminal_view().cloned() {
                view.update(cx, |terminal_view, cx| {
                    terminal_view.focus(window, cx);
                });
            }
        }

        let session_tree_visible = if let Some(state) = cx.try_global::<AppState>() {
            state.app.lock().session_tree_visible
        } else {
            true
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1e2e))
            .child(
                // Main content area
                div()
                    .flex()
                    .flex_1()
                    .overflow_hidden()
                    // Session tree (left panel)
                    .when(session_tree_visible, |this| {
                        this.child(self.session_tree.clone())
                    })
                    // Terminal area (right side)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .overflow_hidden()
                            // Tab bar
                            .child(self.tabs_view.clone())
                            // Terminal view
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .when_some(self.active_terminal_view().cloned(), |el, view| {
                                        el.child(view)
                                    })
                                    .when(self.active_terminal_view().is_none(), |this| {
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
                    ),
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
                            .child("RedPill - SSH Terminal Manager"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .child(format!(
                                "{} tab{}",
                                self.terminal_views.len(),
                                if self.terminal_views.len() == 1 { "" } else { "s" }
                            )),
                    ),
            )
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
