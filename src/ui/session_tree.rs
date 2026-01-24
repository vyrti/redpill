use gpui::*;
use gpui::prelude::*;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::app::AppState;
use crate::kubernetes::{KubeConfig, KubeContext, KubeClient};
use crate::kubernetes::client::{KubeNamespace, KubePod};
use crate::session::{Session, SessionGroup, SshSession, SsmSession};
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

/// Message for async K8s data updates
#[derive(Debug)]
pub enum K8sUpdate {
    Namespaces { context: String, namespaces: Vec<KubeNamespace> },
    NamespacesError { context: String, error: String },
    Pods { context: String, namespace: String, pods: Vec<KubePod> },
    PodsError { context: String, namespace: String, error: String },
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
    /// Kubernetes config loaded from kubeconfig
    kube_config: Option<KubeConfig>,
    /// Expanded K8s contexts
    expanded_k8s_contexts: HashSet<String>,
    /// Whether the K8s root group is expanded
    k8s_expanded: bool,
    /// Loaded namespaces per context
    k8s_namespaces: HashMap<String, Vec<KubeNamespace>>,
    /// Loaded pods per context+namespace (key: "context:namespace")
    k8s_pods: HashMap<String, Vec<KubePod>>,
    /// Expanded namespaces per context (key: "context:namespace")
    expanded_k8s_namespaces: HashSet<String>,
    /// Contexts currently loading namespaces
    loading_contexts: HashSet<String>,
    /// Namespaces currently loading pods (key: "context:namespace")
    loading_namespaces: HashSet<String>,
    /// Channel sender for K8s data updates (cloned for async tasks)
    k8s_update_tx: async_channel::Sender<K8sUpdate>,
}

impl SessionTree {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Try to load kubeconfig
        let kube_config = KubeConfig::load_default().ok();
        if let Some(ref config) = kube_config {
            tracing::info!("Loaded kubeconfig with {} contexts", config.contexts.len());
        }

        let (k8s_update_tx, k8s_update_rx) = async_channel::unbounded();

        // Spawn async task to listen for K8s updates and notify UI immediately
        cx.spawn({
            let rx = k8s_update_rx;
            async move |this: WeakEntity<SessionTree>, cx: &mut AsyncApp| {
                while let Ok(update) = rx.recv().await {
                    if let Some(entity) = this.upgrade() {
                        let _ = cx.update_entity(&entity, |tree: &mut SessionTree, cx: &mut Context<SessionTree>| {
                            tree.handle_k8s_update(update);
                            cx.notify();
                        });
                    } else {
                        break; // Entity was dropped, stop the loop
                    }
                }
            }
        }).detach();

        Self {
            state: SessionTreeState::new(),
            pending_new_session_group: None,
            pending_new_group_parent: None,
            pending_edit_session: None,
            pending_edit_group: None,
            pending_delete_session: None,
            pending_delete_group: None,
            context_menu: None,
            kube_config,
            expanded_k8s_contexts: HashSet::new(),
            k8s_expanded: false,
            k8s_namespaces: HashMap::new(),
            k8s_pods: HashMap::new(),
            expanded_k8s_namespaces: HashSet::new(),
            loading_contexts: HashSet::new(),
            loading_namespaces: HashSet::new(),
            k8s_update_tx,
        }
    }

    /// Handle a K8s update from the async channel
    fn handle_k8s_update(&mut self, update: K8sUpdate) {
        match update {
            K8sUpdate::Namespaces { context, namespaces } => {
                tracing::info!("Loaded {} namespaces for context {}", namespaces.len(), context);
                self.loading_contexts.remove(&context);
                self.k8s_namespaces.insert(context, namespaces);
            }
            K8sUpdate::NamespacesError { context, error } => {
                tracing::warn!("Failed to load namespaces for {}: {}", context, error);
                self.loading_contexts.remove(&context);
                self.k8s_namespaces.insert(context, vec![]);
            }
            K8sUpdate::Pods { context, namespace, pods } => {
                tracing::info!("Loaded {} pods for {}:{}", pods.len(), context, namespace);
                let key = format!("{}:{}", context, namespace);
                self.loading_namespaces.remove(&key);
                self.k8s_pods.insert(key, pods);
            }
            K8sUpdate::PodsError { context, namespace, error } => {
                tracing::warn!("Failed to load pods for {}:{}: {}", context, namespace, error);
                let key = format!("{}:{}", context, namespace);
                self.loading_namespaces.remove(&key);
                self.k8s_pods.insert(key, vec![]);
            }
        }
    }

    /// Toggle K8s root group expansion
    fn toggle_k8s_expanded(&mut self, _cx: &mut Context<Self>) {
        self.k8s_expanded = !self.k8s_expanded;
    }

    /// Toggle K8s context expansion and load namespaces if needed
    fn toggle_k8s_context(&mut self, context_name: String, cx: &mut Context<Self>) {
        if self.expanded_k8s_contexts.contains(&context_name) {
            self.expanded_k8s_contexts.remove(&context_name);
        } else {
            self.expanded_k8s_contexts.insert(context_name.clone());
            // Load namespaces if not already loaded/loading
            if !self.k8s_namespaces.contains_key(&context_name) && !self.loading_contexts.contains(&context_name) {
                self.load_namespaces(context_name, cx);
            }
        }
    }

    /// Toggle K8s namespace expansion and load pods if needed
    fn toggle_k8s_namespace(&mut self, context_name: String, namespace: String, cx: &mut Context<Self>) {
        let key = format!("{}:{}", context_name, namespace);
        if self.expanded_k8s_namespaces.contains(&key) {
            self.expanded_k8s_namespaces.remove(&key);
        } else {
            self.expanded_k8s_namespaces.insert(key.clone());
            // Load pods if not already loaded/loading
            if !self.k8s_pods.contains_key(&key) && !self.loading_namespaces.contains(&key) {
                self.load_pods(context_name, namespace, cx);
            }
        }
    }

    /// Load namespaces for a K8s context
    fn load_namespaces(&mut self, context_name: String, cx: &mut Context<Self>) {
        self.loading_contexts.insert(context_name.clone());
        let tx = self.k8s_update_tx.clone();

        if let Some(app_state) = cx.try_global::<AppState>() {
            let runtime = app_state.tokio_runtime.clone();
            let ctx_name = context_name.clone();
            runtime.spawn(async move {
                match KubeClient::for_context(&ctx_name).await {
                    Ok(client) => {
                        match client.list_namespaces().await {
                            Ok(namespaces) => {
                                let _ = tx.send(K8sUpdate::Namespaces {
                                    context: ctx_name,
                                    namespaces
                                }).await;
                            }
                            Err(e) => {
                                let _ = tx.send(K8sUpdate::NamespacesError {
                                    context: ctx_name,
                                    error: e.to_string()
                                }).await;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(K8sUpdate::NamespacesError {
                            context: ctx_name,
                            error: e.to_string()
                        }).await;
                    }
                }
            });
        }
    }

    /// Load pods for a K8s namespace
    fn load_pods(&mut self, context_name: String, namespace: String, cx: &mut Context<Self>) {
        let key = format!("{}:{}", context_name, namespace);
        self.loading_namespaces.insert(key);
        let tx = self.k8s_update_tx.clone();

        if let Some(app_state) = cx.try_global::<AppState>() {
            let runtime = app_state.tokio_runtime.clone();
            let ctx_name = context_name.clone();
            let ns = namespace.clone();
            runtime.spawn(async move {
                match KubeClient::for_context(&ctx_name).await {
                    Ok(client) => {
                        match client.list_pods(&ns).await {
                            Ok(pods) => {
                                let _ = tx.send(K8sUpdate::Pods {
                                    context: ctx_name,
                                    namespace: ns,
                                    pods
                                }).await;
                            }
                            Err(e) => {
                                let _ = tx.send(K8sUpdate::PodsError {
                                    context: ctx_name,
                                    namespace: ns,
                                    error: e.to_string()
                                }).await;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(K8sUpdate::PodsError {
                            context: ctx_name,
                            namespace: ns,
                            error: e.to_string()
                        }).await;
                    }
                }
            });
        }
    }

    /// Handle clicking on a pod to exec into it
    fn handle_pod_exec(&mut self, context: String, namespace: String, pod: String, container: Option<String>, cx: &mut Context<Self>) {
        tracing::info!("Exec into pod: {}:{}:{}", context, namespace, pod);
        // Create a K8s session and open it
        use crate::session::K8sSession;

        let session = if let Some(container) = container {
            K8sSession::with_container(&pod, &context, &namespace, &pod, container)
        } else {
            K8sSession::new(&pod, &context, &namespace, &pod)
        };

        if let Some(app_state) = cx.try_global::<AppState>() {
            let runtime = app_state.tokio_runtime.clone();
            let mut app = app_state.app.lock();
            // Add session temporarily (or we could have a transient exec)
            let session_id = session.id;
            app.session_manager.add_k8s_session(session);
            if let Err(e) = app.open_k8s_session(session_id, &runtime) {
                tracing::error!("Failed to exec into pod: {}", e);
            }
        }
        cx.notify();
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
            let mut app = app_state.app.lock();
            // Check session type and call appropriate method
            if let Some(session) = app.session_manager.get_session(session_id) {
                let result = match session {
                    Session::Ssh(_) => app.open_ssh_session(session_id, &runtime),
                    Session::Ssm(_) => app.open_ssm_session(session_id, &runtime),
                    Session::Local(_) => app.open_local_terminal(),
                    Session::K8s(_) => app.open_k8s_session(session_id, &runtime),
                };
                if let Err(e) = result {
                    tracing::error!("Failed to open session: {}", e);
                }
            }
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
        let icon = match session {
            Session::Ssh(_) => "🖥️",
            Session::Local(_) => "💻",
            Session::Ssm(_) => "☁️",
            Session::K8s(_) => "⎈",
        };

        div()
            .id(ElementId::Name(format!("session-{}", session_id).into()))
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
                    .child(
                        div()
                            .id("ctx-add-subgroup")
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.request_new_group(Some(group_id), cx);
                                this.close_context_menu(cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xcdd6f4))
                                    .child("Add Sub-group"),
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

    /// Recursively render a group and all its descendants
    fn render_group_recursive(
        &self,
        data: &TreeRenderData,
        group: &SessionGroup,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let is_expanded = self.state.is_expanded(group.id);
        let group_id = group.id;
        let group_indent = (depth as f32) * 12.0;
        let session_indent = group_indent + 16.0;

        let mut container = div().flex().flex_col();

        // Render group header with indent
        container = container.child(
            div()
                .ml(px(group_indent))
                .child(self.render_group_header(group, is_expanded, cx)),
        );

        if is_expanded {
            // Render sessions in this group
            for session in data.sessions_in_group(group_id) {
                container = container.child(self.render_session_item(session, session_indent, cx));
            }

            // Recursively render child groups
            for child_group in data.child_groups(group_id) {
                container = container.child(self.render_group_recursive(data, child_group, depth + 1, cx));
            }
        }

        container
    }

    fn render_tree_content(&self, data: &TreeRenderData, cx: &mut Context<Self>) -> Div {
        let mut content = div().flex().flex_col().gap_1();

        // Render top-level groups recursively
        for group in data.top_level_groups() {
            content = content.child(self.render_group_recursive(data, group, 0, cx));
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

        // Render Kubernetes section if kubeconfig exists
        if let Some(ref kube_config) = self.kube_config {
            content = content.child(self.render_k8s_section(kube_config, cx));
        }

        content
    }

    /// Render the Kubernetes section with contexts
    fn render_k8s_section(&self, config: &KubeConfig, cx: &mut Context<Self>) -> Div {
        let k8s_expanded = self.k8s_expanded;
        let chevron = if k8s_expanded { "▼" } else { "▶" };
        let context_count = config.contexts.len();

        let mut section = div()
            .mt_2()
            .pt_2()
            .border_t_1()
            .border_color(rgb(0x313244))
            // K8s header
            .child(
                div()
                    .id("k8s-header")
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0x313244)))
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.toggle_k8s_expanded(cx);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .child(chevron),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0x89b4fa))
                            .child("⎈ Kubernetes"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .child(format!("({})", context_count)),
                    ),
            );

        // Render contexts if expanded
        if k8s_expanded {
            for context in &config.contexts {
                section = section.child(self.render_k8s_context(context, config, cx));
            }
        }

        section
    }

    /// Render a K8s context item
    fn render_k8s_context(&self, context: &KubeContext, config: &KubeConfig, cx: &mut Context<Self>) -> Div {
        let context_name = context.name.clone();
        let context_name_for_click = context.name.clone();
        let is_current = config.current_context.as_ref() == Some(&context.name);
        let is_expanded = self.expanded_k8s_contexts.contains(&context.name);
        let chevron = if is_expanded { "▼" } else { "▶" };
        let is_loading = self.loading_contexts.contains(&context.name);

        let current_marker = if is_current { " ●" } else { "" };

        let mut container = div()
            .ml(px(12.0))
            .child(
                // Context header
                div()
                    .id(ElementId::Name(format!("k8s-ctx-{}", context.name).into()))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0x313244)))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.toggle_k8s_context(context_name_for_click.clone(), cx);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .child(chevron),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(if is_current { rgb(0xa6e3a1) } else { rgb(0xcdd6f4) })
                            .child(format!("{}{}", context_name, current_marker)),
                    )
                    .when(is_loading, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x6c7086))
                                .child("⏳")
                        )
                    }),
            );

        // Show namespaces if expanded
        if is_expanded {
            if let Some(namespaces) = self.k8s_namespaces.get(&context.name) {
                for ns in namespaces {
                    container = container.child(self.render_k8s_namespace(&context.name, ns, cx));
                }
            } else if is_loading {
                container = container.child(
                    div()
                        .ml(px(24.0))
                        .text_xs()
                        .text_color(rgb(0x6c7086))
                        .child("Loading namespaces...")
                );
            }
        }

        container
    }

    /// Render a K8s namespace item
    fn render_k8s_namespace(&self, context_name: &str, namespace: &KubeNamespace, cx: &mut Context<Self>) -> Div {
        let ctx = context_name.to_string();
        let ctx_for_click = context_name.to_string();
        let ns = namespace.name.clone();
        let ns_for_click = namespace.name.clone();
        let key = format!("{}:{}", context_name, namespace.name);
        let is_expanded = self.expanded_k8s_namespaces.contains(&key);
        let is_loading = self.loading_namespaces.contains(&key);
        let chevron = if is_expanded { "▼" } else { "▶" };

        let mut container = div()
            .ml(px(24.0))
            .child(
                div()
                    .id(ElementId::Name(format!("k8s-ns-{}", key).into()))
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_0p5()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0x313244)))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.toggle_k8s_namespace(ctx_for_click.clone(), ns_for_click.clone(), cx);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .child(chevron),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x89b4fa))
                            .child("📦"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0xcdd6f4))
                            .child(ns.clone()),
                    )
                    .when(is_loading, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x6c7086))
                                .child("⏳")
                        )
                    }),
            );

        // Show pods if expanded
        if is_expanded {
            if let Some(pods) = self.k8s_pods.get(&key) {
                if pods.is_empty() {
                    container = container.child(
                        div()
                            .ml(px(36.0))
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .child("No pods")
                    );
                } else {
                    for pod in pods {
                        container = container.child(self.render_k8s_pod(&ctx, &namespace.name, pod, cx));
                    }
                }
            } else if is_loading {
                container = container.child(
                    div()
                        .ml(px(36.0))
                        .text_xs()
                        .text_color(rgb(0x6c7086))
                        .child("Loading pods...")
                );
            }
        }

        container
    }

    /// Render a K8s pod item
    fn render_k8s_pod(&self, context: &str, namespace: &str, pod: &KubePod, cx: &mut Context<Self>) -> impl IntoElement {
        let ctx = context.to_string();
        let ns = namespace.to_string();
        let pod_name = pod.name.clone();
        let container = pod.containers.first().cloned();

        // Color based on status
        let status_color = match pod.status.as_str() {
            "Running" => rgb(0xa6e3a1), // green
            "Pending" => rgb(0xf9e2af), // yellow
            "Succeeded" => rgb(0x6c7086), // gray
            "Failed" => rgb(0xf38ba8), // red
            _ => rgb(0x6c7086), // gray
        };

        div()
            .id(ElementId::Name(format!("k8s-pod-{}:{}:{}", context, namespace, pod.name).into()))
            .ml(px(36.0))
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_0p5()
            .rounded_sm()
            .cursor_pointer()
            .hover(|style| style.bg(rgb(0x313244)))
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.handle_pod_exec(ctx.clone(), ns.clone(), pod_name.clone(), container.clone(), cx);
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(status_color)
                    .child("●"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0xcdd6f4))
                    .child(pod.name.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x6c7086))
                    .child(format!("({})", pod.ready)),
            )
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
            let mut ssh_session_to_edit: Option<SshSession> = None;
            let mut ssm_session_to_edit: Option<SsmSession> = None;
            if let Some(app_state) = cx.try_global::<AppState>() {
                let app = app_state.app.lock();
                if let Some(session) = app.session_manager.get_session(session_id) {
                    tracing::info!("Found session: {:?}", session.name());
                    match session {
                        Session::Ssh(ssh_session) => {
                            ssh_session_to_edit = Some(ssh_session.clone());
                        }
                        Session::Ssm(ssm_session) => {
                            ssm_session_to_edit = Some(ssm_session.clone());
                        }
                        Session::Local(_) => {
                            tracing::info!("Local sessions don't have edit dialogs yet");
                        }
                        Session::K8s(_) => {
                            tracing::info!("K8s sessions don't have edit dialogs yet");
                        }
                    }
                } else {
                    tracing::warn!("Session not found: {}", session_id);
                }
            } else {
                tracing::warn!("AppState not available");
            }
            if let Some(session) = ssh_session_to_edit {
                tracing::info!("Opening edit dialog for SSH session");
                cx.defer(move |cx| {
                    SessionDialog::open_edit(session, cx);
                });
            } else if let Some(session) = ssm_session_to_edit {
                tracing::info!("Opening edit dialog for SSM session");
                cx.defer(move |cx| {
                    SessionDialog::open_edit_ssm(session, cx);
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
            .min_w(px(150.0))
            .h_full()
            .bg(rgb(0x1e1e2e))
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
                            .flex()
                            .items_center()
                            .gap_2()
                            // Collapse button
                            .child(
                                div()
                                    .id("collapse-tree-btn")
                                    .px_1()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .text_xs()
                                    .text_color(rgb(0x6c7086))
                                    .hover(|style| style.bg(rgb(0x313244)).text_color(rgb(0xcdd6f4)))
                                    .on_click(cx.listener(|_this, _event, _window, cx| {
                                        if let Some(app_state) = cx.try_global::<AppState>() {
                                            let mut app = app_state.app.lock();
                                            app.toggle_session_tree();
                                        }
                                        cx.notify();
                                    }))
                                    .child("\u{25C0}"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xcdd6f4))
                                    .child("Sessions"),
                            ),
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
    cx.new(|cx| SessionTree::new(cx))
}
