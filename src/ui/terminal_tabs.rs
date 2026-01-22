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

/// State for tab context menu
struct TabContextMenuState {
    position: Point<Pixels>,
    tab_id: Uuid,
    tab_index: usize,
}

/// Tab bar component for terminal tabs
pub struct TerminalTabs {
    tabs: Vec<TabInfo>,
    active_tab: Option<Uuid>,
    scroll_offset: f32,
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
        Self {
            tabs,
            active_tab,
            scroll_offset: 0.0,
            context_menu: None,
        }
    }

    pub fn set_tabs(&mut self, tabs: Vec<TabInfo>) {
        self.tabs = tabs;
    }

    pub fn set_active_tab(&mut self, tab_id: Option<Uuid>) {
        self.active_tab = tab_id;
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
        self.context_menu = Some(TabContextMenuState { position, tab_id, tab_index });
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
        self.scroll_offset = 0.0;
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
        self.scroll_offset = 0.0;
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
        self.scroll_offset = 0.0;
        cx.notify();
        window.refresh();
    }

    /// Scroll tabs left
    fn scroll_left(&mut self, cx: &mut Context<Self>) {
        self.scroll_offset = (self.scroll_offset - 120.0).max(0.0);
        cx.notify();
    }

    /// Scroll tabs right
    fn scroll_right(&mut self, cx: &mut Context<Self>) {
        self.scroll_offset += 120.0;
        cx.notify();
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

    fn render_context_menu(&self, menu: &TabContextMenuState, cx: &mut Context<Self>) -> impl IntoElement {
        let tab_id = menu.tab_id;
        let tab_index = menu.tab_index;
        let tab_count = self.tabs.len();
        let has_tabs_to_right = tab_index < tab_count.saturating_sub(1);
        let has_tabs_to_left = tab_index > 0;
        let has_other_tabs = tab_count > 1;

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
                    .on_click(cx.listener(move |this, _event, window, cx| {
                        this.context_menu = None;
                        this.handle_close_tab(tab_id, window, cx);
                    }))
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
                        this.cursor_pointer()
                            .hover(|s| s.bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |this, _event, window, cx| {
                                this.close_other_tabs(tab_id, window, cx);
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
                        this.cursor_pointer()
                            .hover(|s| s.bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |this, _event, window, cx| {
                                this.close_tabs_to_right(tab_index, window, cx);
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
                        this.cursor_pointer()
                            .hover(|s| s.bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |this, _event, window, cx| {
                                this.close_tabs_to_left(tab_index, window, cx);
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
                    .on_click(cx.listener(move |this, _event, window, cx| {
                        this.close_all_tabs(window, cx);
                    }))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xf38ba8))
                            .child("Close All Tabs"),
                    ),
            )
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
                            this.scroll_right(cx);
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
        let scroll_offset = self.scroll_offset;

        // Estimate content width (each tab is roughly 120px)
        let estimated_tab_width = 120.0;
        let estimated_content_width = tabs.len() as f32 * estimated_tab_width;

        // Show scroll buttons when there are many tabs
        let show_scroll_buttons = tabs.len() > 5;
        let can_scroll_left = scroll_offset > 0.0;
        let can_scroll_right = scroll_offset < estimated_content_width - 400.0; // rough estimate

        let has_context_menu = self.context_menu.is_some();

        let mut root = div()
            .relative()
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

        // Tab list with scroll transform
        root = root.child(
            div()
                .flex()
                .flex_1()
                .overflow_hidden()
                .child(
                    div()
                        .flex()
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

        // Context menu overlay and menu
        if has_context_menu {
            // Invisible overlay to capture clicks outside the menu
            root = root.child(
                div()
                    .id("tab-context-menu-backdrop")
                    .absolute()
                    .inset_0()
                    .on_mouse_up(MouseButton::Left, cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                        this.close_context_menu(cx);
                    }))
                    .on_mouse_up(MouseButton::Right, cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                        this.close_context_menu(cx);
                    })),
            );

            if let Some(menu) = &self.context_menu {
                root = root.child(self.render_context_menu(menu, cx));
            }
        }

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
