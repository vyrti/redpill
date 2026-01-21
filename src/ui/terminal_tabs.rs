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

/// Tab bar component for terminal tabs
pub struct TerminalTabs {
    tabs: Vec<TabInfo>,
    active_tab: Option<Uuid>,
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

    fn render_tab(&self, tab: &TabInfo, is_active: bool, cx: &mut Context<Self>) -> impl IntoElement {
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

        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(40.0))
            .bg(rgb(0x1e1e2e))
            .border_b_1()
            .border_color(rgb(0x313244))
            .child(
                // Tab list
                div()
                    .flex()
                    .flex_1()
                    .overflow_hidden()
                    .children(
                        tabs.iter().map(|tab| {
                            let is_active = active_tab == Some(tab.id);
                            self.render_tab(tab, is_active, cx)
                        }),
                    ),
            )
            .child(
                // New tab button
                div()
                    .id("new-tab-btn")
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(40.0))
                    .h_full()
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
            )
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
