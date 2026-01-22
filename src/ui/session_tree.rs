use gpui::*;
use gpui::prelude::*;
use std::collections::HashSet;
use uuid::Uuid;

use crate::app::AppState;
use crate::session::{Session, SessionGroup, SshSession};
use super::session_dialog::SessionDialog;
use super::group_dialog::GroupDialog;
use super::delete_confirm_dialog::DeleteConfirmDialog;

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

/// Context menu target
#[derive(Clone, Debug)]
enum ContextMenuTarget {
    Group { id: Uuid, name: String },
    Session { id: Uuid, name: String },
}

/// State for an open context menu
struct ContextMenuState {
    position: Point<Pixels>,
    target: ContextMenuTarget,
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
    pending_edit_session: Option<Uuid>,
    pending_edit_group: Option<Uuid>,
    pending_delete_session: Option<(Uuid, String)>,
    pending_delete_group: Option<(Uuid, String)>,
    context_menu: Option<ContextMenuState>,
}

impl SessionTree {
    pub fn new() -> Self {
        Self {
            state: SessionTreeState::new(),
            pending_new_session_group: None,
            pending_new_group_parent: None,
            pending_edit_session: None,
            pending_edit_group: None,
            pending_delete_session: None,
            pending_delete_group: None,
            context_menu: None,
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
            let runtime = app_state.tokio_runtime.clone();
            let _ = app_state.app.lock().open_ssh_session(session_id, &runtime);
        }
        cx.emit(SessionTreeEvent::OpenSession(session_id));
        cx.notify();
    }

    /// Handle mass connect for a group
    fn handle_mass_connect(&mut self, group_id: Uuid, cx: &mut Context<Self>) {
        if let Some(app_state) = cx.try_global::<AppState>() {
            let runtime = app_state.tokio_runtime.clone();
            let results = app_state.app.lock().mass_connect(group_id, &runtime);
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

    /// Request edit session dialog
    fn request_edit_session(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        tracing::info!("request_edit_session called for: {}", session_id);
        self.pending_edit_session = Some(session_id);
        self.context_menu = None;
        cx.notify();
    }

    /// Request edit group dialog
    fn request_edit_group(&mut self, group_id: Uuid, cx: &mut Context<Self>) {
        tracing::info!("request_edit_group called for: {}", group_id);
        self.pending_edit_group = Some(group_id);
        self.context_menu = None;
        cx.notify();
    }

    /// Request delete session confirmation
    fn request_delete_session(&mut self, id: Uuid, name: String, cx: &mut Context<Self>) {
        self.pending_delete_session = Some((id, name));
        self.context_menu = None;
        cx.notify();
    }

    /// Request delete group confirmation
    fn request_delete_group(&mut self, id: Uuid, name: String, cx: &mut Context<Self>) {
        self.pending_delete_group = Some((id, name));
        self.context_menu = None;
        cx.notify();
    }

    /// Show context menu for a target
    fn show_context_menu(&mut self, position: Point<Pixels>, target: ContextMenuTarget, cx: &mut Context<Self>) {
        tracing::info!("show_context_menu called at position: {:?}, target: {:?}", position, target);
        self.context_menu = Some(ContextMenuState { position, target });
        cx.notify();
    }

    /// Close context menu
    fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        self.context_menu = None;
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
        let group_name_for_menu = group.name.clone();
        let group_name_for_delete = group.name.clone();
        let group_color = group.color.clone();

        div()
            .id(ElementId::Name(format!("group-{}", group_id).into()))
            .group("group-row")
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
            .on_mouse_up(MouseButton::Right, cx.listener(move |this, event: &MouseUpEvent, _window, cx| {
                cx.stop_propagation();
                let target = ContextMenuTarget::Group { id: group_id, name: group_name_for_menu.clone() };
                this.show_context_menu(event.position, target, cx);
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
                    .opacity(0.0)
                    .group_hover("group-row", |this| this.opacity(1.0))
                    // Edit button
                    .child(
                        div()
                            .id(ElementId::Name(format!("group-edit-{}", group_id).into()))
                            .px_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .hover(|style| style.bg(rgb(0x45475a)).text_color(rgb(0xf9e2af)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                cx.stop_propagation();
                                this.request_edit_group(group_id, cx);
                            }))
                            .child("✏"),
                    )
                    // Delete button
                    .child(
                        div()
                            .id(ElementId::Name(format!("group-delete-{}", group_id).into()))
                            .px_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .hover(|style| style.bg(rgb(0x45475a)).text_color(rgb(0xf38ba8)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                cx.stop_propagation();
                                this.request_delete_group(group_id, group_name_for_delete.clone(), cx);
                            }))
                            .child("🗑"),
                    )
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
                                cx.stop_propagation();
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
                                cx.stop_propagation();
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
        let session_name_for_menu = session.name().to_string();
        let session_name_for_delete = session.name().to_string();
        let icon = match session {
            Session::Ssh(_) => "🖥️",
            Session::Local(_) => "💻",
        };
        let group_id = format!("session-row-{}", session_id);

        div()
            .id(ElementId::Name(format!("session-{}", session_id).into()))
            .group(SharedString::from(group_id.clone()))
            .flex()
            .items_center()
            .justify_between()
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
            .on_mouse_up(MouseButton::Right, cx.listener(move |this, event: &MouseUpEvent, _window, cx| {
                cx.stop_propagation();
                let target = ContextMenuTarget::Session { id: session_id, name: session_name_for_menu.clone() };
                this.show_context_menu(event.position, target, cx);
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().text_sm().child(icon))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xcdd6f4))
                            .child(session_name),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .opacity(0.0)
                    .group_hover(SharedString::from(group_id), |this| this.opacity(1.0))
                    // Edit button
                    .child(
                        div()
                            .id(ElementId::Name(format!("session-edit-{}", session_id).into()))
                            .px_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .hover(|style| style.bg(rgb(0x45475a)).text_color(rgb(0xf9e2af)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                cx.stop_propagation();
                                this.request_edit_session(session_id, cx);
                            }))
                            .child("✏"),
                    )
                    // Delete button
                    .child(
                        div()
                            .id(ElementId::Name(format!("session-delete-{}", session_id).into()))
                            .px_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .hover(|style| style.bg(rgb(0x45475a)).text_color(rgb(0xf38ba8)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                cx.stop_propagation();
                                this.request_delete_session(session_id, session_name_for_delete.clone(), cx);
                            }))
                            .child("🗑"),
                    ),
            )
    }

    fn render_context_menu(&self, menu: &ContextMenuState, cx: &mut Context<Self>) -> impl IntoElement {
        // Clamp position to stay within panel bounds (250px wide panel, 160px menu)
        let menu_width = px(160.0);
        let panel_width = px(250.0);
        let max_x = panel_width - menu_width - px(8.0);
        let x = if menu.position.x > max_x {
            max_x
        } else {
            menu.position.x
        };
        let y = menu.position.y;

        match &menu.target {
            ContextMenuTarget::Group { id, name } => {
                let group_id = *id;
                let group_name_delete = name.clone();

                div()
                    .absolute()
                    .left(x)
                    .top(y)
                    .w(px(160.0))
                    .bg(rgb(0x313244))
                    .border_1()
                    .border_color(rgb(0x45475a))
                    .rounded_md()
                    .shadow_lg()
                    .py_1()
                    .child(
                        div()
                            .id("ctx-edit-group")
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.request_edit_group(group_id, cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xcdd6f4))
                                    .child("Edit Group"),
                            ),
                    )
                    .child(
                        div()
                            .id("ctx-connect-all")
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.handle_mass_connect(group_id, cx);
                                this.close_context_menu(cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xcdd6f4))
                                    .child("Connect All"),
                            ),
                    )
                    .child(
                        div()
                            .id("ctx-add-session")
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.request_new_session(Some(group_id), cx);
                                this.close_context_menu(cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xcdd6f4))
                                    .child("Add Session"),
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
                    .child(
                        div()
                            .id("ctx-delete-group")
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.request_delete_group(group_id, group_name_delete.clone(), cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xf38ba8))
                                    .child("Delete Group"),
                            ),
                    )
            }
            ContextMenuTarget::Session { id, name } => {
                let session_id = *id;
                let session_name_delete = name.clone();

                div()
                    .absolute()
                    .left(x)
                    .top(y)
                    .w(px(160.0))
                    .bg(rgb(0x313244))
                    .border_1()
                    .border_color(rgb(0x45475a))
                    .rounded_md()
                    .shadow_lg()
                    .py_1()
                    .child(
                        div()
                            .id("ctx-connect")
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.handle_open_session(session_id, cx);
                                this.close_context_menu(cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xcdd6f4))
                                    .child("Connect"),
                            ),
                    )
                    .child(
                        div()
                            .id("ctx-edit-session")
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.request_edit_session(session_id, cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xcdd6f4))
                                    .child("Edit Session"),
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
                    .child(
                        div()
                            .id("ctx-delete-session")
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.request_delete_session(session_id, session_name_delete.clone(), cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xf38ba8))
                                    .child("Delete Session"),
                            ),
                    )
            }
        }
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

        // Handle pending edit session request
        if let Some(session_id) = self.pending_edit_session.take() {
            tracing::info!("Edit session requested for: {}", session_id);
            let mut session_to_edit: Option<SshSession> = None;
            if let Some(app_state) = cx.try_global::<AppState>() {
                let app = app_state.app.lock();
                if let Some(session) = app.session_manager.get_session(session_id) {
                    tracing::info!("Found session: {:?}", session.name());
                    if let Session::Ssh(ssh_session) = session {
                        session_to_edit = Some(ssh_session.clone());
                    } else {
                        tracing::info!("Session is not SSH, skipping edit");
                    }
                } else {
                    tracing::warn!("Session not found: {}", session_id);
                }
            } else {
                tracing::warn!("AppState not available");
            }
            if let Some(session) = session_to_edit {
                tracing::info!("Opening edit dialog for session");
                cx.defer(move |cx| {
                    SessionDialog::open_edit(session, cx);
                });
            }
        }

        // Handle pending edit group request
        if let Some(group_id) = self.pending_edit_group.take() {
            tracing::info!("Edit group requested for: {}", group_id);
            let mut group_to_edit: Option<SessionGroup> = None;
            if let Some(app_state) = cx.try_global::<AppState>() {
                let app = app_state.app.lock();
                if let Some(group) = app.session_manager.get_group(group_id) {
                    tracing::info!("Found group: {}", group.name);
                    group_to_edit = Some(group.clone());
                } else {
                    tracing::warn!("Group not found: {}", group_id);
                }
            } else {
                tracing::warn!("AppState not available");
            }
            if let Some(group) = group_to_edit {
                tracing::info!("Opening edit dialog for group");
                cx.defer(move |cx| {
                    GroupDialog::open_edit(&group, cx);
                });
            }
        }

        // Handle pending delete session request
        if let Some((id, name)) = self.pending_delete_session.take() {
            cx.defer(move |cx| {
                DeleteConfirmDialog::open_for_session(id, name, cx);
            });
        }

        // Handle pending delete group request
        if let Some((id, name)) = self.pending_delete_group.take() {
            cx.defer(move |cx| {
                DeleteConfirmDialog::open_for_group(id, name, cx);
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

        // Check if context menu is open
        let has_context_menu = self.context_menu.is_some();
        if has_context_menu {
            tracing::info!("Rendering with context menu open");
        }

        let mut root = div()
            .relative()
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
            );

        // Add context menu if open
        if has_context_menu {
            // Invisible overlay to capture clicks outside the menu
            root = root.child(
                div()
                    .id("context-menu-backdrop")
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

/// Create a session tree view
pub fn session_tree(cx: &mut App) -> Entity<SessionTree> {
    cx.new(|_| SessionTree::new())
}
