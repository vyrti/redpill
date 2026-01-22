use gpui::*;
use gpui::prelude::*;
use uuid::Uuid;

use crate::app::AppState;
use crate::session::SsmSession;
use super::text_field::TextField;

/// Result of the SSM session dialog
#[derive(Clone, Debug)]
pub enum SsmSessionDialogResult {
    /// Dialog was canceled
    Canceled,
    /// Session was created/updated
    Saved(SsmSession),
}

/// Events emitted by the SSM session dialog
pub enum SsmSessionDialogEvent {
    Saved(SsmSession),
    Canceled,
}

impl EventEmitter<SsmSessionDialogEvent> for SsmSessionDialog {}

/// SSM session dialog for creating/editing AWS SSM sessions
pub struct SsmSessionDialog {
    /// Session ID if editing (None for new session)
    session_id: Option<Uuid>,
    /// Group ID if adding to a group
    group_id: Option<Uuid>,
    /// Text fields
    name_field: Entity<TextField>,
    instance_id_field: Entity<TextField>,
    region_field: Entity<TextField>,
    profile_field: Entity<TextField>,
    /// Color scheme override (None = use default)
    color_scheme: Option<String>,
    /// Validation errors
    errors: Vec<String>,
}

impl SsmSessionDialog {
    /// Create a new SSM session dialog
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            session_id: None,
            group_id: None,
            name_field: cx.new(|cx| TextField::new(cx, "My EC2 Instance")),
            instance_id_field: cx.new(|cx| TextField::new(cx, "i-0123456789abcdef0")),
            region_field: cx.new(|cx| TextField::new(cx, "us-east-1 (optional)")),
            profile_field: cx.new(|cx| TextField::new(cx, "default (optional)")),
            color_scheme: None,
            errors: Vec::new(),
        }
    }

    /// Create a new SSM session dialog for a specific group
    pub fn new_for_group(group_id: Option<Uuid>, cx: &mut Context<Self>) -> Self {
        let mut dialog = Self::new(cx);
        dialog.group_id = group_id;
        dialog
    }

    /// Create a dialog for editing an existing SSM session
    pub fn edit(session: &SsmSession, cx: &mut Context<Self>) -> Self {
        Self {
            session_id: Some(session.id),
            group_id: session.group_id,
            name_field: cx.new(|cx| TextField::with_content(cx, "My EC2 Instance", session.name.clone())),
            instance_id_field: cx.new(|cx| TextField::with_content(cx, "i-0123456789abcdef0", session.instance_id.clone())),
            region_field: cx.new(|cx| TextField::with_content(cx, "us-east-1 (optional)", session.region.clone().unwrap_or_default())),
            profile_field: cx.new(|cx| TextField::with_content(cx, "default (optional)", session.profile.clone().unwrap_or_default())),
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
                size(px(450.0), px(480.0)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("New SSM Session".into()),
                appears_transparent: false,
                ..Default::default()
            }),
            kind: WindowKind::Normal,
            ..Default::default()
        };

        let _ = cx.open_window(window_options, |_window, cx| {
            cx.new(|cx| SsmSessionDialog::new_for_group(group_id, cx))
        });
    }

    /// Open as a modal window for editing
    pub fn open_edit(session: SsmSession, cx: &mut App) {
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(450.0), px(480.0)),
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
            cx.new(|cx| SsmSessionDialog::edit(&session, cx))
        });
    }

    /// Validate the form
    fn validate(&mut self, cx: &mut Context<Self>) -> bool {
        self.errors.clear();

        let name = self.name_field.read(cx).content();
        let instance_id = self.instance_id_field.read(cx).content();

        if name.trim().is_empty() {
            self.errors.push("Name is required".into());
        }

        if instance_id.trim().is_empty() {
            self.errors.push("Instance ID is required".into());
        } else {
            let id = instance_id.trim();
            // Validate instance ID format (i-xxx or mi-xxx)
            if !id.starts_with("i-") && !id.starts_with("mi-") {
                self.errors.push("Instance ID must start with 'i-' (EC2) or 'mi-' (on-prem)".into());
            }
        }

        self.errors.is_empty()
    }

    /// Build the session from form fields
    fn build_session(&self, cx: &Context<Self>) -> SsmSession {
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
                let _ = app.session_manager.update_ssm_session(session.id, session.clone());
            } else {
                // Add new session
                app.add_ssm_session(session.clone());
            }
            let _ = app.save();
        }

        cx.emit(SsmSessionDialogEvent::Saved(session));

        // Close the window
        window.remove_window();
    }

    /// Handle cancel button click
    fn handle_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(SsmSessionDialogEvent::Canceled);
        window.remove_window();
    }

    fn render_label(&self, text: &str) -> impl IntoElement {
        div()
            .text_sm()
            .text_color(rgb(0xcdd6f4))
            .child(text.to_string())
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

    fn render_help_text(&self) -> impl IntoElement {
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
                            .text_color(rgb(0x89b4fa))
                            .font_weight(FontWeight::MEDIUM)
                            .child("AWS SSM Session Manager"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .child("Connects via AWS SSM. Requires:"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .child("- AWS credentials configured"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x6c7086))
                            .child("- SSM agent running on target"),
                    ),
            )
    }
}

impl Render for SsmSessionDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self.session_id.is_some() {
            "Edit SSM Session"
        } else {
            "New SSM Session"
        };

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

                // Help text
                form = form.child(self.render_help_text());

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
                    // Color scheme selector
                    .child(self.render_color_scheme_selector(cx));

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
                            .bg(rgb(0xfab387)) // Orange for AWS
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0xf9e2af)))
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

/// Create an SSM session dialog view
pub fn ssm_session_dialog(cx: &mut App) -> Entity<SsmSessionDialog> {
    cx.new(|cx| SsmSessionDialog::new(cx))
}

/// Create an SSM session dialog for editing
pub fn edit_ssm_session_dialog(session: &SsmSession, cx: &mut App) -> Entity<SsmSessionDialog> {
    cx.new(|cx| SsmSessionDialog::edit(session, cx))
}
