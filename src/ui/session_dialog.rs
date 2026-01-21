use gpui::*;
use gpui::prelude::*;
use std::path::PathBuf;
use uuid::Uuid;

use crate::app::AppState;
use crate::session::{AuthMethod, SshSession};
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
    Canceled,
}

impl EventEmitter<SessionDialogEvent> for SessionDialog {}

#[derive(Clone, Copy, PartialEq, Default, Debug)]
enum AuthType {
    #[default]
    Password,
    PrivateKey,
    Agent,
}

/// Session dialog for creating/editing SSH sessions
pub struct SessionDialog {
    /// Session ID if editing (None for new session)
    session_id: Option<Uuid>,
    /// Group ID if adding to a group
    group_id: Option<Uuid>,
    /// Text fields
    name_field: Entity<TextField>,
    host_field: Entity<TextField>,
    port_field: Entity<TextField>,
    username_field: Entity<TextField>,
    password_field: Entity<TextField>,
    key_path_field: Entity<TextField>,
    key_passphrase_field: Entity<TextField>,
    /// Auth settings
    auth_type: AuthType,
    save_password: bool,
    save_passphrase: bool,
    /// Validation errors
    errors: Vec<String>,
}

impl SessionDialog {
    /// Create a new session dialog
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            session_id: None,
            group_id: None,
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
            auth_type: AuthType::Password,
            save_password: false,
            save_passphrase: false,
            errors: Vec::new(),
        }
    }

    /// Create a new session dialog for a specific group
    pub fn new_for_group(group_id: Option<Uuid>, cx: &mut Context<Self>) -> Self {
        let mut dialog = Self::new(cx);
        dialog.group_id = group_id;
        dialog
    }

    /// Create a dialog for editing an existing session
    pub fn edit(session: &SshSession, cx: &mut Context<Self>) -> Self {
        let (auth_type, password, save_password, key_path, key_passphrase, save_passphrase) =
            match &session.auth {
                AuthMethod::Password {
                    password,
                    save_password,
                } => (
                    AuthType::Password,
                    password.clone().unwrap_or_default(),
                    *save_password,
                    String::new(),
                    String::new(),
                    false,
                ),
                AuthMethod::PrivateKey {
                    path,
                    passphrase,
                    save_passphrase,
                } => (
                    AuthType::PrivateKey,
                    String::new(),
                    false,
                    path.to_string_lossy().to_string(),
                    passphrase.clone().unwrap_or_default(),
                    *save_passphrase,
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
            auth_type,
            save_password,
            save_passphrase,
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
                size(px(420.0), px(520.0)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("New SSH Session".into()),
                appears_transparent: false,
                ..Default::default()
            }),
            kind: WindowKind::PopUp,
            ..Default::default()
        };

        let _ = cx.open_window(window_options, |_window, cx| {
            cx.new(|cx| SessionDialog::new_for_group(group_id, cx))
        });
    }

    /// Open as a modal window for editing
    pub fn open_edit(session: &SshSession, cx: &mut App) {
        let session = session.clone();
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(420.0), px(520.0)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Edit SSH Session".into()),
                appears_transparent: false,
                ..Default::default()
            }),
            kind: WindowKind::PopUp,
            ..Default::default()
        };

        let _ = cx.open_window(window_options, |_window, cx| {
            cx.new(|cx| SessionDialog::edit(&session, cx))
        });
    }

    /// Validate the form
    fn validate(&mut self, cx: &mut Context<Self>) -> bool {
        self.errors.clear();

        let name = self.name_field.read(cx).content().trim().to_string();
        let host = self.host_field.read(cx).content().trim().to_string();
        let port = self.port_field.read(cx).content().trim().to_string();
        let username = self.username_field.read(cx).content().trim().to_string();
        let key_path = self.key_path_field.read(cx).content().trim().to_string();

        if name.is_empty() {
            self.errors.push("Name is required".to_string());
        }

        if host.is_empty() {
            self.errors.push("Host is required".to_string());
        }

        if port.parse::<u16>().is_err() {
            self.errors.push("Port must be a valid number (1-65535)".to_string());
        }

        if username.is_empty() {
            self.errors.push("Username is required".to_string());
        }

        if self.auth_type == AuthType::PrivateKey && key_path.is_empty() {
            self.errors.push("Private key path is required".to_string());
        }

        self.errors.is_empty()
    }

    /// Build the session from form fields
    fn build_session(&self, cx: &Context<Self>) -> SshSession {
        let name = self.name_field.read(cx).content().to_string();
        let host = self.host_field.read(cx).content().to_string();
        let port = self.port_field.read(cx).content().parse().unwrap_or(22);
        let username = self.username_field.read(cx).content().to_string();
        let password = self.password_field.read(cx).content().to_string();
        let key_path = self.key_path_field.read(cx).content().to_string();
        let key_passphrase = self.key_passphrase_field.read(cx).content().to_string();

        let auth = match self.auth_type {
            AuthType::Password => AuthMethod::Password {
                password: if password.is_empty() {
                    None
                } else {
                    Some(password)
                },
                save_password: self.save_password,
            },
            AuthType::PrivateKey => AuthMethod::PrivateKey {
                path: PathBuf::from(&key_path),
                passphrase: if key_passphrase.is_empty() {
                    None
                } else {
                    Some(key_passphrase)
                },
                save_passphrase: self.save_passphrase,
            },
            AuthType::Agent => AuthMethod::Agent,
        };

        let mut session = SshSession::new(name, host, username);
        session.port = port;
        session.auth = auth;
        session.group_id = self.group_id;

        // Preserve ID if editing
        if let Some(id) = self.session_id {
            session.id = id;
        }

        session
    }

    /// Get the built session if valid
    pub fn get_session(&self, cx: &Context<Self>) -> Option<SshSession> {
        if self.errors.is_empty() {
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

        let session = self.build_session(cx);

        // Save to app state
        if let Some(app_state) = cx.try_global::<AppState>() {
            let mut app = app_state.app.lock();
            if self.session_id.is_some() {
                // Update existing session
                let _ = app.session_manager.update_ssh_session(session.id, session.clone());
            } else {
                // Add new session
                app.add_ssh_session(session.clone());
            }
            let _ = app.save();
        }

        cx.emit(SessionDialogEvent::Saved(session));

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
}

impl Render for SessionDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self.session_id.is_some() {
            "Edit Session"
        } else {
            "New SSH Session"
        };

        let auth_type = self.auth_type;
        let has_errors = !self.errors.is_empty();

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

                // Form fields
                form = form
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(self.render_label("Name"))
                            .child(self.name_field.clone()),
                    )
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

                // Auth-specific fields
                if auth_type == AuthType::Password {
                    form = form.child(self.render_password_field());
                } else if auth_type == AuthType::PrivateKey {
                    form = form.child(self.render_key_fields());
                }

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
                            .bg(rgb(0x89b4fa))
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x74c7ec)))
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
