use gpui::*;
use gpui::prelude::*;
use std::collections::HashSet;
use uuid::Uuid;

use crate::app::AppState;
use crate::session::{Session, SessionGroup};
use super::session_dialog::SessionDialog;
use super::group_dialog::GroupDialog;

/// Actions for the session tree
#[derive(Clone, Debug)]
pub enum SessionTreeAction {
    OpenSession(Uuid),
    NewGroup(Option<Uuid>),
    NewSshSession(Option<Uuid>),
    NewLocalSession(Option<Uuid>),
    EditSession(Uuid),
    EditGroup(Uuid),
    DeleteSession(Uuid),
    DeleteGroup(Uuid),
    MassConnect(Uuid),
    ToggleGroup(Uuid),
}

/// Events emitted by the session tree
pub enum SessionTreeEvent {
    OpenSession(Uuid),
    SessionCreated,
    SessionDeleted(Uuid),
    GroupCreated,
    GroupDeleted(Uuid),
}

impl EventEmitter<SessionTreeEvent> for SessionTree {}

/// State for expanded groups
pub struct SessionTreeState {
    expanded_groups: HashSet<Uuid>,
    selected_item: Option<TreeItem>,
}

impl SessionTreeState {
    pub fn new() -> Self {
        Self {
            expanded_groups: HashSet::new(),
            selected_item: None,
        }
    }

    pub fn is_expanded(&self, group_id: Uuid) -> bool {
        self.expanded_groups.contains(&group_id)
    }

    pub fn toggle_expanded(&mut self, group_id: Uuid) {
        if self.expanded_groups.contains(&group_id) {
            self.expanded_groups.remove(&group_id);
        } else {
            self.expanded_groups.insert(group_id);
        }
    }

    pub fn expand(&mut self, group_id: Uuid) {
        self.expanded_groups.insert(group_id);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TreeItem {
    Group(Uuid),
    Session(Uuid),
}

/// Cached data for rendering the tree
struct TreeRenderData {
    groups: Vec<SessionGroup>,
    sessions: Vec<Session>,
}

impl TreeRenderData {
    fn top_level_groups(&self) -> impl Iterator<Item = &SessionGroup> {
        self.groups.iter().filter(|g| g.parent_id.is_none())
    }

    fn child_groups(&self, parent_id: Uuid) -> impl Iterator<Item = &SessionGroup> {
        self.groups.iter().filter(move |g| g.parent_id == Some(parent_id))
    }

    fn sessions_in_group(&self, group_id: Uuid) -> impl Iterator<Item = &Session> {
        self.sessions.iter().filter(move |s| s.group_id() == Some(group_id))
    }

    fn ungrouped_sessions(&self) -> impl Iterator<Item = &Session> {
        self.sessions.iter().filter(|s| s.group_id().is_none())
    }
}

/// Session tree panel component
pub struct SessionTree {
    state: SessionTreeState,
    pending_new_session_group: Option<Uuid>,
    pending_new_group_parent: Option<Uuid>,
}

impl SessionTree {
    pub fn new() -> Self {
        Self {
            state: SessionTreeState::new(),
            pending_new_session_group: None,
            pending_new_group_parent: None,
        }
    }

    /// Handle clicking on a group header
    fn handle_toggle_group(&mut self, group_id: Uuid, cx: &mut Context<Self>) {
        self.state.toggle_expanded(group_id);
        cx.notify();
    }

    /// Handle clicking on a session
    fn handle_open_session(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if let Some(app_state) = cx.try_global::<AppState>() {
            let _ = app_state.app.lock().open_ssh_session(session_id);
        }
        cx.emit(SessionTreeEvent::OpenSession(session_id));
        cx.notify();
    }

    /// Handle mass connect for a group
    fn handle_mass_connect(&mut self, group_id: Uuid, cx: &mut Context<Self>) {
        if let Some(app_state) = cx.try_global::<AppState>() {
            let results = app_state.app.lock().mass_connect(group_id);
            for result in results {
                if let Err(e) = result {
                    tracing::error!("Mass connect error: {}", e);
                }
            }
        }
        cx.notify();
    }

    /// Handle clicking the new session button - just set flag for later
    fn request_new_session(&mut self, group_id: Option<Uuid>, cx: &mut Context<Self>) {
        if let Some(gid) = group_id {
            self.state.expand(gid);
        }
        self.pending_new_session_group = Some(group_id.unwrap_or_else(Uuid::nil));
        cx.notify();
    }

    /// Handle clicking new group button - just set flag for later
    fn request_new_group(&mut self, parent_id: Option<Uuid>, cx: &mut Context<Self>) {
        if let Some(pid) = parent_id {
            self.state.expand(pid);
        }
        self.pending_new_group_parent = Some(parent_id.unwrap_or_else(Uuid::nil));
        cx.notify();
    }

    fn render_group_header(
        &self,
        group: &SessionGroup,
        is_expanded: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let group_id = group.id;
        let group_name = group.name.clone();
        let group_color = group.color.clone();

        div()
            .id(ElementId::Name(format!("group-{}", group_id).into()))
            .flex()
            .items_center()
            .justify_between()
            .gap_1()
            .px_2()
            .py_1()
            .rounded_sm()
            .cursor_pointer()
            .hover(|style| style.bg(rgb(0x313244)))
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.handle_toggle_group(group_id, cx);
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, _event, _window, cx| {
                this.handle_mass_connect(group_id, cx);
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x6c7086))
                            .child(if is_expanded { "▼" } else { "▶" }),
                    )
                    .child(
                        div().text_sm().child(if is_expanded { "📂" } else { "📁" }),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xcdd6f4))
                            .when_some(group_color, |this, color| {
                                let color_val = u32::from_str_radix(&color[1..], 16).unwrap_or(0xcdd6f4);
                                this.text_color(rgb(color_val))
                            })
                            .child(group_name),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    // Connect All button for this group
                    .child(
                        div()
                            .id(ElementId::Name(format!("group-connect-all-{}", group_id).into()))
                            .px_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .hover(|style| style.bg(rgb(0x45475a)).text_color(rgb(0xa6e3a1)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.handle_mass_connect(group_id, cx);
                            }))
                            .child(">>"),
                    )
                    // Add session button for this group
                    .child(
                        div()
                            .id(ElementId::Name(format!("group-add-{}", group_id).into()))
                            .px_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .hover(|style| style.bg(rgb(0x45475a)).text_color(rgb(0x89b4fa)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.request_new_session(Some(group_id), cx);
                            }))
                            .child("+"),
                    ),
            )
    }

    fn render_session_item(
        &self,
        session: &Session,
        indent: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let session_id = session.id();
        let session_name = session.name().to_string();
        let icon = match session {
            Session::Ssh(_) => "🖥️",
            Session::Local(_) => "💻",
        };

        div()
            .id(ElementId::Name(format!("session-{}", session_id).into()))
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .ml(px(indent))
            .rounded_sm()
            .cursor_pointer()
            .hover(|style| style.bg(rgb(0x313244)))
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.handle_open_session(session_id, cx);
            }))
            .child(div().text_sm().child(icon))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0xcdd6f4))
                    .child(session_name),
            )
    }

    fn render_tree_content(&self, data: &TreeRenderData, cx: &mut Context<Self>) -> Div {
        let mut content = div().flex().flex_col().gap_1();

        // Render top-level groups
        for group in data.top_level_groups() {
            let is_expanded = self.state.is_expanded(group.id);
            let group_id = group.id;

            content = content.child(
                div()
                    .flex()
                    .flex_col()
                    .child(self.render_group_header(group, is_expanded, cx)),
            );

            if is_expanded {
                // Render sessions in this group
                for session in data.sessions_in_group(group_id) {
                    content = content.child(self.render_session_item(session, 16.0, cx));
                }

                // Render child groups
                for child_group in data.child_groups(group_id) {
                    let child_expanded = self.state.is_expanded(child_group.id);
                    let child_id = child_group.id;

                    content = content.child(
                        div()
                            .ml(px(12.0))
                            .child(self.render_group_header(child_group, child_expanded, cx)),
                    );

                    if child_expanded {
                        for session in data.sessions_in_group(child_id) {
                            content = content.child(self.render_session_item(session, 28.0, cx));
                        }
                    }
                }
            }
        }

        // Render ungrouped sessions
        let ungrouped: Vec<_> = data.ungrouped_sessions().collect();
        if !ungrouped.is_empty() {
            content = content.child(
                div()
                    .mt_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(rgb(0x313244))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .px_2()
                            .mb_1()
                            .child("Ungrouped"),
                    ),
            );

            for session in ungrouped {
                content = content.child(self.render_session_item(session, 0.0, cx));
            }
        }

        content
    }
}

impl Render for SessionTree {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Handle pending dialog requests
        if let Some(group_id) = self.pending_new_session_group.take() {
            let group_id = if group_id.is_nil() { None } else { Some(group_id) };
            cx.defer(move |cx| {
                SessionDialog::open_with_group(group_id, cx);
            });
        }

        if let Some(parent_id) = self.pending_new_group_parent.take() {
            let parent_id = if parent_id.is_nil() { None } else { Some(parent_id) };
            cx.defer(move |cx| {
                GroupDialog::open_new(parent_id, cx);
            });
        }

        // Get data from app state (clone it to avoid borrow conflicts)
        let render_data = cx.try_global::<AppState>().map(|app_state| {
            let app = app_state.app.lock();
            TreeRenderData {
                groups: app.session_manager.all_groups().to_vec(),
                sessions: app.session_manager.all_sessions().to_vec(),
            }
        });

        div()
            .flex()
            .flex_col()
            .w(px(250.0))
            .h_full()
            .bg(rgb(0x1e1e2e))
            .border_r_1()
            .border_color(rgb(0x313244))
            .child(
                // Header
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(0x313244))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0xcdd6f4))
                            .child("Sessions"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            // New group button
                            .child(
                                div()
                                    .id("new-group-btn")
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(0x313244)))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.request_new_group(None, cx);
                                    }))
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(0xa6e3a1))
                                            .child("📁"),
                                    ),
                            )
                            // New session button
                            .child(
                                div()
                                    .id("new-session-btn")
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(0x313244)))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.request_new_session(None, cx);
                                    }))
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(0x89b4fa))
                                            .child("+"),
                                    ),
                            ),
                    ),
            )
            .child(
                // Tree content
                div()
                    .flex_1()
                    .overflow_y_hidden()
                    .p_2()
                    .child(
                        if let Some(data) = render_data {
                            self.render_tree_content(&data, cx)
                        } else {
                            div().child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x6c7086))
                                    .child("No sessions"),
                            )
                        },
                    ),
            )
    }
}

/// Create a session tree view
pub fn session_tree(cx: &mut App) -> Entity<SessionTree> {
    cx.new(|_| SessionTree::new())
}
