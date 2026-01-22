use gpui::*;
use gpui::prelude::*;
use std::path::PathBuf;
use uuid::Uuid;

use crate::app::AppState;
use crate::session::{AuthMethod, SshSession, SsmSession};
use super::text_field::TextField;

/// Result of the session dialog
#[derive(Clone, Debug)]
pub enum SessionDialogResult {
    /// Dialog was canceled
    Canceled,
    /// Session was created/updated
    Saved(SshSession),
}

/// Events emitted by the session dialog
pub enum SessionDialogEvent {
    Saved(SshSession),
    SavedSsm(SsmSession),
    Canceled,
}

impl EventEmitter<SessionDialogEvent> for SessionDialog {}

/// Type of session being created/edited
#[derive(Clone, Copy, PartialEq, Default, Debug)]
enum SessionType {
    #[default]
    Ssh,
    Ssm,
}

#[derive(Clone, Copy, PartialEq, Default, Debug)]
enum AuthType {
    #[default]
    Password,
    PrivateKey,
    Agent,
}

/// Session dialog for creating/editing SSH and SSM sessions
pub struct SessionDialog {
    /// Session ID if editing (None for new session)
    session_id: Option<Uuid>,
    /// Group ID if adding to a group
    group_id: Option<Uuid>,
    /// Session type (SSH or SSM)
    session_type: SessionType,
    /// Whether we're editing (locks session type)
    is_editing: bool,
    /// Common field
    name_field: Entity<TextField>,
    /// SSH-specific fields
    host_field: Entity<TextField>,
    port_field: Entity<TextField>,
    username_field: Entity<TextField>,
    password_field: Entity<TextField>,
    key_path_field: Entity<TextField>,
    key_passphrase_field: Entity<TextField>,
    /// SSM-specific fields
    instance_id_field: Entity<TextField>,
    region_field: Entity<TextField>,
    profile_field: Entity<TextField>,
    /// Auth settings (SSH only)
    auth_type: AuthType,
    save_password: bool,
    save_passphrase: bool,
    /// Color scheme override (None = use default)
    color_scheme: Option<String>,
    /// Validation errors
    errors: Vec<String>,
}

impl SessionDialog {
    /// Create a new session dialog
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            session_id: None,
            group_id: None,
            session_type: SessionType::Ssh,
            is_editing: false,
            name_field: cx.new(|cx| TextField::new(cx, "My Server")),
            host_field: cx.new(|cx| TextField::new(cx, "hostname or IP")),
            port_field: cx.new(|cx| TextField::with_content(cx, "22", "22".to_string())),
            username_field: cx.new(|cx| TextField::new(cx, "username")),
            password_field: cx.new(|cx| {
                let mut field = TextField::new(cx, "password");
                field.set_password(true);
                field
            }),
            key_path_field: cx.new(|cx| TextField::new(cx, "~/.ssh/id_rsa")),
            key_passphrase_field: cx.new(|cx| {
                let mut field = TextField::new(cx, "passphrase (optional)");
                field.set_password(true);
                field
            }),
            instance_id_field: cx.new(|cx| TextField::new(cx, "i-0123456789abcdef0")),
            region_field: cx.new(|cx| TextField::new(cx, "us-east-1 (optional)")),
            profile_field: cx.new(|cx| TextField::new(cx, "default (optional)")),
            auth_type: AuthType::Password,
            save_password: false,
            save_passphrase: false,
            color_scheme: None,
            errors: Vec::new(),
        }
    }

    /// Create a new session dialog for a specific group
    pub fn new_for_group(group_id: Option<Uuid>, cx: &mut Context<Self>) -> Self {
        let mut dialog = Self::new(cx);
        dialog.group_id = group_id;
        dialog
    }

    /// Create a dialog for editing an existing SSH session
    pub fn edit(session: &SshSession, cx: &mut Context<Self>) -> Self {
        let (auth_type, password, save_password, key_path, key_passphrase, save_passphrase) =
            match &session.auth {
                AuthMethod::Password {
                    password,
                    use_keychain,
                } => (
                    AuthType::Password,
                    password.clone().unwrap_or_default(),
                    *use_keychain,
                    String::new(),
                    String::new(),
                    false,
                ),
                AuthMethod::PrivateKey {
                    path,
                    passphrase,
                    use_keychain,
                } => (
                    AuthType::PrivateKey,
                    String::new(),
                    false,
                    path.to_string_lossy().to_string(),
                    passphrase.clone().unwrap_or_default(),
                    *use_keychain,
                ),
                AuthMethod::Agent => (
                    AuthType::Agent,
                    String::new(),
                    false,
                    String::new(),
                    String::new(),
                    false,
                ),
            };

        Self {
            session_id: Some(session.id),
            group_id: session.group_id,
            session_type: SessionType::Ssh,
            is_editing: true,
            name_field: cx.new(|cx| TextField::with_content(cx, "My Server", session.name.clone())),
            host_field: cx.new(|cx| TextField::with_content(cx, "hostname or IP", session.host.clone())),
            port_field: cx.new(|cx| TextField::with_content(cx, "22", session.port.to_string())),
            username_field: cx.new(|cx| TextField::with_content(cx, "username", session.username.clone())),
            password_field: cx.new(|cx| {
                let mut field = TextField::with_content(cx, "password", password);
                field.set_password(true);
                field
            }),
            key_path_field: cx.new(|cx| TextField::with_content(cx, "~/.ssh/id_rsa", key_path)),
            key_passphrase_field: cx.new(|cx| {
                let mut field = TextField::with_content(cx, "passphrase (optional)", key_passphrase);
                field.set_password(true);
                field
            }),
            instance_id_field: cx.new(|cx| TextField::new(cx, "i-0123456789abcdef0")),
            region_field: cx.new(|cx| TextField::new(cx, "us-east-1 (optional)")),
            profile_field: cx.new(|cx| TextField::new(cx, "default (optional)")),
            auth_type,
            save_password,
            save_passphrase,
            color_scheme: session.color_scheme.clone(),
            errors: Vec::new(),
        }
    }

    /// Create a dialog for editing an existing SSM session
    pub fn edit_ssm(session: &SsmSession, cx: &mut Context<Self>) -> Self {
        Self {
            session_id: Some(session.id),
            group_id: session.group_id,
            session_type: SessionType::Ssm,
            is_editing: true,
            name_field: cx.new(|cx| TextField::with_content(cx, "My EC2 Instance", session.name.clone())),
            host_field: cx.new(|cx| TextField::new(cx, "hostname or IP")),
            port_field: cx.new(|cx| TextField::with_content(cx, "22", "22".to_string())),
            username_field: cx.new(|cx| TextField::new(cx, "username")),
            password_field: cx.new(|cx| {
                let mut field = TextField::new(cx, "password");
                field.set_password(true);
                field
            }),
            key_path_field: cx.new(|cx| TextField::new(cx, "~/.ssh/id_rsa")),
            key_passphrase_field: cx.new(|cx| {
                let mut field = TextField::new(cx, "passphrase (optional)");
                field.set_password(true);
                field
            }),
            instance_id_field: cx.new(|cx| TextField::with_content(cx, "i-0123456789abcdef0", session.instance_id.clone())),
            region_field: cx.new(|cx| TextField::with_content(cx, "us-east-1 (optional)", session.region.clone().unwrap_or_default())),
            profile_field: cx.new(|cx| TextField::with_content(cx, "default (optional)", session.profile.clone().unwrap_or_default())),
            auth_type: AuthType::Password,
            save_password: false,
            save_passphrase: false,
            color_scheme: session.color_scheme.clone(),
            errors: Vec::new(),
        }
    }

    /// Open as a modal window
    pub fn open_new(cx: &mut App) {
        Self::open_with_group(None, cx);
    }

    /// Open as a modal window for a specific group
    pub fn open_with_group(group_id: Option<Uuid>, cx: &mut App) {
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(450.0), px(720.0)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("New Session".into()),
                appears_transparent: false,
                ..Default::default()
            }),
            kind: WindowKind::Normal,
            ..Default::default()
        };

        let _ = cx.open_window(window_options, |_window, cx| {
            cx.new(|cx| SessionDialog::new_for_group(group_id, cx))
        });
    }

    /// Open as a modal window for editing an SSH session
    pub fn open_edit(session: SshSession, cx: &mut App) {
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(450.0), px(720.0)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Edit SSH Session".into()),
                appears_transparent: false,
                ..Default::default()
            }),
            kind: WindowKind::Normal,
            ..Default::default()
        };

        let _ = cx.open_window(window_options, |_window, cx| {
            cx.new(|cx| SessionDialog::edit(&session, cx))
        });
    }

    /// Open as a modal window for editing an SSM session
    pub fn open_edit_ssm(session: SsmSession, cx: &mut App) {
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(450.0), px(720.0)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Edit SSM Session".into()),
                appears_transparent: false,
                ..Default::default()
            }),
            kind: WindowKind::Normal,
            ..Default::default()
        };

        let _ = cx.open_window(window_options, |_window, cx| {
            cx.new(|cx| SessionDialog::edit_ssm(&session, cx))
        });
    }

    /// Validate the form
    fn validate(&mut self, cx: &mut Context<Self>) -> bool {
        self.errors.clear();

        let name = self.name_field.read(cx).content();
        if name.trim().is_empty() {
            self.errors.push("Name is required".into());
        }

        match self.session_type {
            SessionType::Ssh => {
                let host = self.host_field.read(cx).content();
                let port = self.port_field.read(cx).content();
                let username = self.username_field.read(cx).content();
                let key_path = self.key_path_field.read(cx).content();

                if host.trim().is_empty() {
                    self.errors.push("Host is required".into());
                }

                if port.trim().parse::<u16>().is_err() {
                    self.errors.push("Port must be a valid number (1-65535)".into());
                }

                if username.trim().is_empty() {
                    self.errors.push("Username is required".into());
                }

                if self.auth_type == AuthType::PrivateKey && key_path.trim().is_empty() {
                    self.errors.push("Private key path is required".into());
                }
            }
            SessionType::Ssm => {
                let instance_id = self.instance_id_field.read(cx).content();

                if instance_id.trim().is_empty() {
                    self.errors.push("Instance ID is required".into());
                } else {
                    let id = instance_id.trim();
                    if !id.starts_with("i-") && !id.starts_with("mi-") {
                        self.errors.push("Instance ID must start with 'i-' (EC2) or 'mi-' (on-prem)".into());
                    }
                }
            }
        }

        self.errors.is_empty()
    }

    /// Build the session from form fields
    fn build_session(&self, cx: &Context<Self>) -> SshSession {
        // Read fields only once, trim and convert to owned strings only when needed
        let name = self.name_field.read(cx).content().trim();
        let host = self.host_field.read(cx).content().trim();
        let port = self.port_field.read(cx).content().parse().unwrap_or(22);
        let username = self.username_field.read(cx).content().trim();
        let password = self.password_field.read(cx).content();
        let key_path = self.key_path_field.read(cx).content();
        let key_passphrase = self.key_passphrase_field.read(cx).content();

        let auth = match self.auth_type {
            AuthType::Password => AuthMethod::Password {
                password: if password.is_empty() {
                    None
                } else {
                    Some(password.to_string())
                },
                use_keychain: self.save_password,
            },
            AuthType::PrivateKey => AuthMethod::PrivateKey {
                path: PathBuf::from(key_path.trim()),
                passphrase: if key_passphrase.is_empty() {
                    None
                } else {
                    Some(key_passphrase.to_string())
                },
                use_keychain: self.save_passphrase,
            },
            AuthType::Agent => AuthMethod::Agent,
        };

        let mut session = SshSession::new(name, host, username);
        session.port = port;
        session.auth = auth;
        session.group_id = self.group_id;
        session.color_scheme = self.color_scheme.clone();

        // Preserve ID if editing
        if let Some(id) = self.session_id {
            session.id = id;
        }

        session
    }

    /// Build an SSM session from form fields
    fn build_ssm_session(&self, cx: &Context<Self>) -> SsmSession {
        let name = self.name_field.read(cx).content().trim().to_string();
        let instance_id = self.instance_id_field.read(cx).content().trim().to_string();
        let region = {
            let r = self.region_field.read(cx).content().trim().to_string();
            if r.is_empty() { None } else { Some(r) }
        };
        let profile = {
            let p = self.profile_field.read(cx).content().trim().to_string();
            if p.is_empty() { None } else { Some(p) }
        };

        let mut session = SsmSession::with_config(name, instance_id, region, profile);
        session.group_id = self.group_id;
        session.color_scheme = self.color_scheme.clone();

        // Preserve ID if editing
        if let Some(id) = self.session_id {
            session.id = id;
        }

        session
    }

    /// Get the built session if valid
    pub fn get_session(&self, cx: &Context<Self>) -> Option<SshSession> {
        if self.errors.is_empty() && self.session_type == SessionType::Ssh {
            Some(self.build_session(cx))
        } else {
            None
        }
    }

    /// Handle save button click
    fn handle_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.validate(cx) {
            cx.notify();
            return;
        }

        match self.session_type {
            SessionType::Ssh => {
                let session = self.build_session(cx);

                // Save to app state
                if let Some(app_state) = cx.try_global::<AppState>() {
                    let mut app = app_state.app.lock();
                    if self.session_id.is_some() {
                        let _ = app.session_manager.update_ssh_session(session.id, session.clone());
                    } else {
                        app.add_ssh_session(session.clone());
                    }
                    let _ = app.save();
                }

                cx.emit(SessionDialogEvent::Saved(session));
            }
            SessionType::Ssm => {
                let session = self.build_ssm_session(cx);

                // Save to app state
                if let Some(app_state) = cx.try_global::<AppState>() {
                    let mut app = app_state.app.lock();
                    if self.session_id.is_some() {
                        let _ = app.session_manager.update_ssm_session(session.id, session.clone());
                    } else {
                        app.add_ssm_session(session.clone());
                    }
                    let _ = app.save();
                }

                cx.emit(SessionDialogEvent::SavedSsm(session));
            }
        }

        // Close the window
        window.remove_window();
    }

    /// Handle cancel button click
    fn handle_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(SessionDialogEvent::Canceled);
        window.remove_window();
    }

    fn render_label(&self, text: &str) -> impl IntoElement {
        div()
            .text_sm()
            .text_color(rgb(0xcdd6f4))
            .child(text.to_string())
    }

    fn render_auth_option(
        &self,
        label: impl Into<SharedString>,
        auth_type: AuthType,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = label.into();
        let is_selected = self.auth_type == auth_type;

        div()
            .id(ElementId::Name(format!("auth-{:?}", auth_type).into()))
            .px_3()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .when(is_selected, |this| {
                this.bg(rgb(0x89b4fa)).text_color(rgb(0x1e1e2e))
            })
            .when(!is_selected, |this| {
                this.bg(rgb(0x313244))
                    .text_color(rgb(0xcdd6f4))
                    .hover(|style| style.bg(rgb(0x45475a)))
            })
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.auth_type = auth_type;
                cx.notify();
            }))
            .child(div().text_sm().child(label))
    }

    fn render_color_scheme_option(
        &self,
        label: impl Into<SharedString>,
        scheme_value: Option<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = label.into();
        let is_selected = self.color_scheme == scheme_value;
        let scheme_for_click = scheme_value.clone();

        div()
            .id(ElementId::Name(format!("scheme-{}", scheme_value.as_deref().unwrap_or("default")).into()))
            .px_3()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .when(is_selected, |this| {
                this.bg(rgb(0x89b4fa)).text_color(rgb(0x1e1e2e))
            })
            .when(!is_selected, |this| {
                this.bg(rgb(0x313244))
                    .text_color(rgb(0xcdd6f4))
                    .hover(|style| style.bg(rgb(0x45475a)))
            })
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.color_scheme = scheme_for_click.clone();
                cx.notify();
            }))
            .child(div().text_sm().child(label))
    }

    fn render_color_scheme_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.render_label("Color Scheme"))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.render_color_scheme_option("Default", None, cx))
                    .child(self.render_color_scheme_option("Light", Some("light".to_string()), cx))
                    .child(self.render_color_scheme_option("Matrix", Some("matrix".to_string()), cx))
                    .child(self.render_color_scheme_option("Red", Some("red".to_string()), cx)),
            )
    }

    fn render_errors(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .bg(rgba(0xf38ba833))
            .rounded_md()
            .children(self.errors.iter().map(|e| {
                div()
                    .text_sm()
                    .text_color(rgb(0xf38ba8))
                    .child(e.clone())
            }))
    }

    fn render_password_field(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(self.render_label("Password"))
            .child(self.password_field.clone())
    }

    fn render_key_fields(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.render_label("Key Path"))
                    .child(self.key_path_field.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.render_label("Key Passphrase"))
                    .child(self.key_passphrase_field.clone()),
            )
    }

    fn render_session_type_option(
        &self,
        label: impl Into<SharedString>,
        icon: &'static str,
        session_type: SessionType,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = label.into();
        let is_selected = self.session_type == session_type;
        let is_disabled = self.is_editing;

        div()
            .id(ElementId::Name(format!("type-{:?}", session_type).into()))
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .px_4()
            .py_2()
            .rounded_md()
            .when(is_disabled, |this| this.cursor_default().opacity(0.7))
            .when(!is_disabled, |this| this.cursor_pointer())
            .when(is_selected, |this| {
                this.bg(rgb(0x89b4fa)).text_color(rgb(0x1e1e2e))
            })
            .when(!is_selected, |this| {
                this.bg(rgb(0x313244))
                    .text_color(rgb(0xcdd6f4))
                    .when(!is_disabled, |inner| inner.hover(|style| style.bg(rgb(0x45475a))))
            })
            .when(!is_disabled, |this| {
                this.on_click(cx.listener(move |this, _event, _window, cx| {
                    this.session_type = session_type;
                    cx.notify();
                }))
            })
            .child(div().text_base().child(icon))
            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
    }

    fn render_session_type_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.render_label("Connection Type"))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(self.render_session_type_option("SSH", "🖥️", SessionType::Ssh, cx))
                    .child(self.render_session_type_option("AWS SSM", "☁️", SessionType::Ssm, cx)),
            )
    }

    fn render_ssh_fields(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let auth_type = self.auth_type;

        let mut fields = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.render_label("Host"))
                    .child(self.host_field.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.render_label("Port"))
                    .child(self.port_field.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.render_label("Username"))
                    .child(self.username_field.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(self.render_label("Authentication"))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.render_auth_option("Password", AuthType::Password, cx))
                            .child(self.render_auth_option("Key", AuthType::PrivateKey, cx))
                            .child(self.render_auth_option("Agent", AuthType::Agent, cx)),
                    ),
            );

        if auth_type == AuthType::Password {
            fields = fields.child(self.render_password_field());
        } else if auth_type == AuthType::PrivateKey {
            fields = fields.child(self.render_key_fields());
        }

        fields
    }

    fn render_ssm_fields(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .p_3()
                    .bg(rgb(0x313244))
                    .rounded_md()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xfab387))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("AWS SSM Session Manager"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x6c7086))
                                    .child("Requires AWS credentials and SSM agent on target"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.render_label("Instance ID"))
                    .child(self.instance_id_field.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.render_label("AWS Region (optional)"))
                    .child(self.region_field.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.render_label("AWS Profile (optional)"))
                    .child(self.profile_field.clone()),
            )
    }
}

impl Render for SessionDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self.session_id.is_some() {
            match self.session_type {
                SessionType::Ssh => "Edit SSH Session",
                SessionType::Ssm => "Edit SSM Session",
            }
        } else {
            "New Session"
        };

        let session_type = self.session_type;
        let has_errors = !self.errors.is_empty();

        // Button color based on session type
        let button_bg = match session_type {
            SessionType::Ssh => rgb(0x89b4fa),  // Blue for SSH
            SessionType::Ssm => rgb(0xfab387),  // Orange for AWS
        };
        let button_hover = match session_type {
            SessionType::Ssh => rgb(0x74c7ec),
            SessionType::Ssm => rgb(0xf9e2af),
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1e2e))
            // Header
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(0x313244))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0xcdd6f4))
                            .child(title),
                    ),
            )
            // Form content
            .child({
                let mut form = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap_3()
                    .p_4()
                    .overflow_y_hidden();

                // Errors
                if has_errors {
                    form = form.child(self.render_errors());
                }

                // Session type selector (only for new sessions)
                form = form.child(self.render_session_type_selector(cx));

                // Name field (common to both)
                form = form.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(self.render_label("Name"))
                        .child(self.name_field.clone()),
                );

                // Type-specific fields
                match session_type {
                    SessionType::Ssh => {
                        form = form.child(self.render_ssh_fields(cx));
                    }
                    SessionType::Ssm => {
                        form = form.child(self.render_ssm_fields());
                    }
                }

                // Color scheme selector (common to both)
                form = form.child(self.render_color_scheme_selector(cx));

                form
            })
            // Footer with buttons
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(rgb(0x313244))
                    .child(
                        div()
                            .id("cancel-btn")
                            .px_4()
                            .py_2()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x313244)))
                            .on_click(cx.listener(|this, _event, window, cx| {
                                this.handle_cancel(window, cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x6c7086))
                                    .child("Cancel"),
                            ),
                    )
                    .child(
                        div()
                            .id("save-btn")
                            .px_4()
                            .py_2()
                            .bg(button_bg)
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|style| style.bg(button_hover))
                            .on_click(cx.listener(|this, _event, window, cx| {
                                this.handle_save(window, cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x1e1e2e))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Save"),
                            ),
                    ),
            )
    }
}

/// Create a session dialog view
pub fn session_dialog(cx: &mut App) -> Entity<SessionDialog> {
    cx.new(|cx| SessionDialog::new(cx))
}

/// Create a session dialog for editing
pub fn edit_session_dialog(session: &SshSession, cx: &mut App) -> Entity<SessionDialog> {
    cx.new(|cx| SessionDialog::edit(session, cx))
}
