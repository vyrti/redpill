use gpui::*;
use gpui::prelude::*;
use uuid::Uuid;

use crate::app::{AppState, TerminalTab};

/// Actions for terminal tabs
#[derive(Clone, Debug)]
pub enum TabAction {
    /// Select a tab
    Select(Uuid),
    /// Close a tab
    Close(Uuid),
    /// Create a new tab
    NewTab,
    /// Reorder tabs (from_index, to_index)
    Reorder(usize, usize),
}

/// Events emitted by the terminal tabs
pub enum TabEvent {
    SelectTab(Uuid),
    CloseTab(Uuid),
    NewTab,
}

impl EventEmitter<TabEvent> for TerminalTabs {}

/// State for tab context menu (public for rendering in MainWindow)
#[derive(Clone)]
pub struct TabContextMenuState {
    pub position: Point<Pixels>,
    pub tab_id: Uuid,
    pub tab_index: usize,
    pub tab_count: usize,
}

/// Tab bar component for terminal tabs
pub struct TerminalTabs {
    tabs: Vec<TabInfo>,
    active_tab: Option<Uuid>,
    scroll_offset: f32,
    prev_tab_count: usize,
    context_menu: Option<TabContextMenuState>,
}

/// Information about a tab for display
#[derive(Clone)]
pub struct TabInfo {
    pub id: Uuid,
    pub title: String,
    pub dirty: bool,
}

impl From<&TerminalTab> for TabInfo {
    fn from(tab: &TerminalTab) -> Self {
        Self {
            id: tab.id,
            title: tab.title.clone(),
            dirty: tab.dirty,
        }
    }
}

impl TerminalTabs {
    pub fn new(tabs: Vec<TabInfo>, active_tab: Option<Uuid>) -> Self {
        let tab_count = tabs.len();
        Self {
            tabs,
            active_tab,
            scroll_offset: 0.0,
            prev_tab_count: tab_count,
            context_menu: None,
        }
    }

    pub fn set_tabs(&mut self, tabs: Vec<TabInfo>) {
        self.tabs = tabs;
    }

    pub fn set_active_tab(&mut self, tab_id: Option<Uuid>) {
        self.active_tab = tab_id;
    }

    /// Get the current context menu state (for rendering in MainWindow)
    pub fn context_menu_state(&self) -> Option<TabContextMenuState> {
        self.context_menu.clone()
    }

    /// Close context menu (public for MainWindow to call)
    pub fn dismiss_context_menu(&mut self, cx: &mut Context<Self>) {
        self.context_menu = None;
        cx.notify();
    }

    /// Close other tabs (public for MainWindow to call)
    pub fn close_other_tabs_action(&mut self, keep_id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        self.close_other_tabs(keep_id, window, cx);
    }

    /// Close tabs to right (public for MainWindow to call)
    pub fn close_tabs_to_right_action(&mut self, from_index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tabs_to_right(from_index, window, cx);
    }

    /// Close tabs to left (public for MainWindow to call)
    pub fn close_tabs_to_left_action(&mut self, from_index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tabs_to_left(from_index, window, cx);
    }

    /// Close all tabs (public for MainWindow to call)
    pub fn close_all_tabs_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_all_tabs(window, cx);
    }

    /// Close single tab (public for MainWindow to call)
    pub fn close_tab_action(&mut self, tab_id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.handle_close_tab(tab_id, window, cx);
    }

    fn handle_select_tab(&mut self, tab_id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app_state) = cx.try_global::<AppState>() {
            app_state.app.lock().set_active_tab_by_id(tab_id);
        }
        self.active_tab = Some(tab_id);
        cx.emit(TabEvent::SelectTab(tab_id));
        cx.notify();
        window.refresh();
    }

    fn handle_close_tab(&mut self, tab_id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app_state) = cx.try_global::<AppState>() {
            app_state.app.lock().close_tab(tab_id);

            // Update local state
            self.tabs.retain(|t| t.id != tab_id);
            if self.active_tab == Some(tab_id) {
                self.active_tab = self.tabs.first().map(|t| t.id);
            }
        }
        cx.emit(TabEvent::CloseTab(tab_id));
        cx.notify();
        window.refresh();
    }

    fn handle_new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app_state) = cx.try_global::<AppState>() {
            if let Ok(id) = app_state.app.lock().open_local_terminal() {
                self.active_tab = Some(id);
            }
        }
        cx.emit(TabEvent::NewTab);
        cx.notify();
        window.refresh();
    }

    /// Show context menu for a tab
    fn show_context_menu(&mut self, position: Point<Pixels>, tab_id: Uuid, tab_index: usize, cx: &mut Context<Self>) {
        let tab_count = self.tabs.len();
        self.context_menu = Some(TabContextMenuState { position, tab_id, tab_index, tab_count });
        cx.notify();
    }

    /// Close context menu
    fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        self.context_menu = None;
        cx.notify();
    }

    /// Close all tabs except the specified one
    fn close_other_tabs(&mut self, keep_id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app_state) = cx.try_global::<AppState>() {
            let tabs_to_close: Vec<Uuid> = self.tabs.iter()
                .filter(|t| t.id != keep_id)
                .map(|t| t.id)
                .collect();
            for tab_id in tabs_to_close {
                app_state.app.lock().close_tab(tab_id);
            }
            self.tabs.retain(|t| t.id == keep_id);
            self.active_tab = Some(keep_id);
        }
        self.context_menu = None;
        cx.notify();
        window.refresh();
    }

    /// Close tabs to the right of the specified index
    fn close_tabs_to_right(&mut self, from_index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app_state) = cx.try_global::<AppState>() {
            let tabs_to_close: Vec<Uuid> = self.tabs.iter()
                .enumerate()
                .filter(|(i, _)| *i > from_index)
                .map(|(_, t)| t.id)
                .collect();
            for tab_id in tabs_to_close {
                app_state.app.lock().close_tab(tab_id);
            }
            self.tabs.truncate(from_index + 1);
            // If active tab was closed, select the last remaining tab
            if let Some(active) = self.active_tab {
                if !self.tabs.iter().any(|t| t.id == active) {
                    self.active_tab = self.tabs.last().map(|t| t.id);
                }
            }
        }
        self.context_menu = None;
        cx.notify();
        window.refresh();
    }

    /// Close tabs to the left of the specified index
    fn close_tabs_to_left(&mut self, from_index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app_state) = cx.try_global::<AppState>() {
            let tabs_to_close: Vec<Uuid> = self.tabs.iter()
                .enumerate()
                .filter(|(i, _)| *i < from_index)
                .map(|(_, t)| t.id)
                .collect();
            for tab_id in tabs_to_close {
                app_state.app.lock().close_tab(tab_id);
            }
            self.tabs = self.tabs.split_off(from_index);
            // If active tab was closed, select the first remaining tab
            if let Some(active) = self.active_tab {
                if !self.tabs.iter().any(|t| t.id == active) {
                    self.active_tab = self.tabs.first().map(|t| t.id);
                }
            }
        }
        self.context_menu = None;
        cx.notify();
        window.refresh();
    }

    /// Close all tabs
    fn close_all_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app_state) = cx.try_global::<AppState>() {
            let tabs_to_close: Vec<Uuid> = self.tabs.iter().map(|t| t.id).collect();
            for tab_id in tabs_to_close {
                app_state.app.lock().close_tab(tab_id);
            }
            self.tabs.clear();
            self.active_tab = None;
        }
        self.context_menu = None;
        cx.notify();
        window.refresh();
    }

    fn render_tab(&self, tab: &TabInfo, tab_index: usize, is_active: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let tab_id = tab.id;
        let title = tab.title.clone();
        let dirty = tab.dirty;

        div()
            .id(ElementId::Name(format!("tab-{}", tab_id).into()))
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .cursor_pointer()
            .border_b_2()
            .flex_shrink_0()
            .when(is_active, |this| {
                this.border_color(rgb(0x89b4fa))
                    .bg(rgb(0x313244))
            })
            .when(!is_active, |this| {
                this.border_color(transparent_black())
                    .hover(|style| style.bg(rgb(0x313244)))
            })
            // Click handler for selecting tab
            .on_click(cx.listener(move |this, _event, window, cx| {
                this.handle_select_tab(tab_id, window, cx);
            }))
            // Right-click handler for context menu
            .on_mouse_up(MouseButton::Right, cx.listener(move |this, event: &MouseUpEvent, _window, cx| {
                cx.stop_propagation();
                this.show_context_menu(event.position, tab_id, tab_index, cx);
            }))
            .child(
                // Tab title
                div()
                    .text_sm()
                    .text_color(if is_active {
                        rgb(0xcdd6f4)
                    } else {
                        rgb(0x6c7086)
                    })
                    .when(dirty, |this| this.child(format!("● {}", title)))
                    .when(!dirty, |this| this.child(title)),
            )
            .child(
                // Close button
                div()
                    .id(ElementId::Name(format!("tab-close-{}", tab_id).into()))
                    .px_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0x45475a)))
                    .on_click(cx.listener(move |this, _event, window, cx| {
                        this.handle_close_tab(tab_id, window, cx);
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .child("×"),
                    ),
            )
    }

    fn scroll_left(&mut self, cx: &mut Context<Self>) {
        self.scroll_offset = (self.scroll_offset - 120.0).max(0.0);
        cx.notify();
    }

    fn scroll_right(&mut self, max_scroll: f32, cx: &mut Context<Self>) {
        self.scroll_offset = (self.scroll_offset + 120.0).min(max_scroll);
        cx.notify();
    }

    fn render_scroll_button(&self, direction: &str, enabled: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let is_left = direction == "left";
        let arrow = if is_left { "◀" } else { "▶" };

        div()
            .id(ElementId::Name(format!("scroll-{}", direction).into()))
            .flex()
            .items_center()
            .justify_center()
            .w(px(24.0))
            .h_full()
            .flex_shrink_0()
            .border_b_1()
            .border_color(rgb(0x313244))
            .when(enabled, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(rgb(0x313244)))
                    .when(is_left, |this| {
                        this.on_click(cx.listener(|this, _event, _window, cx| {
                            this.scroll_left(cx);
                        }))
                    })
                    .when(!is_left, |this| {
                        this.on_click(cx.listener(|this, _event, _window, cx| {
                            let max = this.calculate_max_scroll();
                            this.scroll_right(max, cx);
                        }))
                    })
            })
            .child(
                div()
                    .text_xs()
                    .text_color(if enabled { rgb(0x6c7086) } else { rgb(0x45475a) })
                    .child(arrow),
            )
    }

    fn calculate_max_scroll(&self) -> f32 {
        // Each tab is ~120px, keep at least 2 tabs visible
        let tab_count = self.tabs.len();
        if tab_count <= 2 {
            0.0
        } else {
            (tab_count - 2) as f32 * 120.0
        }
    }
}

impl Render for TerminalTabs {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Sync tabs from app state
        if let Some(app_state) = cx.try_global::<AppState>() {
            let app = app_state.app.lock();
            self.tabs = app.tabs.iter().map(TabInfo::from).collect();
            self.active_tab = app.active_tab().map(|t| t.id);
        }

        let tabs: Vec<_> = self.tabs.clone();
        let active_tab = self.active_tab;
        let tab_count = tabs.len();

        // Calculate max scroll (keep at least 2 tabs visible)
        let max_scroll = if tab_count <= 2 {
            0.0
        } else {
            (tab_count - 2) as f32 * 120.0
        };

        // When a new tab is added, scroll to show it (scroll by one tab)
        if tab_count > self.prev_tab_count && self.prev_tab_count > 0 && tab_count > 6 {
            self.scroll_offset = (self.scroll_offset + 120.0).min(max_scroll);
        }

        // Clamp scroll offset
        self.scroll_offset = self.scroll_offset.clamp(0.0, max_scroll);

        // Update state for next render
        self.prev_tab_count = tab_count;

        let scroll_offset = self.scroll_offset;
        let show_scroll_buttons = tab_count > 6;
        let can_scroll_left = scroll_offset > 0.0;
        let can_scroll_right = scroll_offset < max_scroll;

        let mut root = div()
            .id("tab-bar")
            .flex()
            .items_center()
            .w_full()
            .h(px(40.0))
            .bg(rgb(0x1e1e2e))
            .border_b_1()
            .border_color(rgb(0x313244));

        // Left scroll button
        if show_scroll_buttons {
            root = root.child(self.render_scroll_button("left", can_scroll_left, cx));
        }

        // Tab container with manual scroll via negative margin
        root = root.child(
            div()
                .flex_1()
                .h_full()
                .overflow_hidden()
                .child(
                    div()
                        .id("tabs")
                        .flex()
                        .h_full()
                        .ml(px(-scroll_offset))
                        .children(
                            tabs.iter().enumerate().map(|(index, tab)| {
                                let is_active = active_tab == Some(tab.id);
                                self.render_tab(tab, index, is_active, cx)
                            }),
                        ),
                ),
        );

        // Right scroll button
        if show_scroll_buttons {
            root = root.child(self.render_scroll_button("right", can_scroll_right, cx));
        }

        // New tab button
        root = root.child(
            div()
                .id("new-tab-btn")
                .flex()
                .items_center()
                .justify_center()
                .w(px(40.0))
                .h_full()
                .flex_shrink_0()
                .border_b_1()
                .border_color(rgb(0x313244))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(0x313244)))
                .on_click(cx.listener(|this, _event, window, cx| {
                    this.handle_new_tab(window, cx);
                }))
                .child(
                    div()
                        .text_lg()
                        .text_color(rgb(0x6c7086))
                        .child("+"),
                ),
        );

        root
    }
}

/// Create a terminal tabs view
pub fn terminal_tabs(
    tabs: Vec<TabInfo>,
    active_tab: Option<Uuid>,
    cx: &mut App,
) -> Entity<TerminalTabs> {
    cx.new(|_| TerminalTabs::new(tabs, active_tab))
}
