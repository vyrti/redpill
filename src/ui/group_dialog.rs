use gpui::*;
use gpui::prelude::*;
use uuid::Uuid;

use crate::app::AppState;
use crate::session::SessionGroup;
use super::text_field::TextField;

/// Result of the group dialog
#[derive(Clone, Debug)]
pub enum GroupDialogResult {
    /// Dialog was canceled
    Canceled,
    /// Group was created/updated
    Saved(SessionGroup),
}

/// Events emitted by the group dialog
pub enum GroupDialogEvent {
    Saved(SessionGroup),
    Canceled,
}

impl EventEmitter<GroupDialogEvent> for GroupDialog {}

/// Group dialog for creating/editing session groups
pub struct GroupDialog {
    /// Group ID if editing (None for new group)
    group_id: Option<Uuid>,
    /// Parent group ID
    parent_id: Option<Uuid>,
    /// Name text field
    name_field: Entity<TextField>,
    /// Selected color
    color: Option<String>,
    /// Validation errors
    errors: Vec<String>,
    /// Available colors
    available_colors: Vec<(&'static str, &'static str)>,
}

impl GroupDialog {
    /// Create a new group dialog
    pub fn new(parent_id: Option<Uuid>, cx: &mut Context<Self>) -> Self {
        Self {
            group_id: None,
            parent_id,
            name_field: cx.new(|cx| TextField::new(cx, "Group Name")),
            color: None,
            errors: Vec::new(),
            available_colors: vec![
                ("Red", "#f38ba8"),
                ("Orange", "#fab387"),
                ("Yellow", "#f9e2af"),
                ("Green", "#a6e3a1"),
                ("Teal", "#94e2d5"),
                ("Blue", "#89b4fa"),
                ("Purple", "#cba6f7"),
                ("Pink", "#f5c2e7"),
            ],
        }
    }

    /// Create a dialog for editing an existing group
    pub fn edit(group: &SessionGroup, cx: &mut Context<Self>) -> Self {
        Self {
            group_id: Some(group.id),
            parent_id: group.parent_id,
            name_field: cx.new(|cx| TextField::with_content(cx, "Group Name", group.name.clone())),
            color: group.color.clone(),
            errors: Vec::new(),
            available_colors: vec![
                ("Red", "#f38ba8"),
                ("Orange", "#fab387"),
                ("Yellow", "#f9e2af"),
                ("Green", "#a6e3a1"),
                ("Teal", "#94e2d5"),
                ("Blue", "#89b4fa"),
                ("Purple", "#cba6f7"),
                ("Pink", "#f5c2e7"),
            ],
        }
    }

    /// Open as a modal window for new group
    pub fn open_new(parent_id: Option<Uuid>, cx: &mut App) {
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(400.0), px(300.0)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("New Group".into()),
                appears_transparent: false,
                ..Default::default()
            }),
            kind: WindowKind::Normal,
            ..Default::default()
        };

        let _ = cx.open_window(window_options, |_window, cx| {
            cx.new(|cx| GroupDialog::new(parent_id, cx))
        });
    }

    /// Open as a modal window for editing
    pub fn open_edit(group: &SessionGroup, cx: &mut App) {
        let group = group.clone();
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(400.0), px(300.0)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Edit Group".into()),
                appears_transparent: false,
                ..Default::default()
            }),
            kind: WindowKind::Normal,
            ..Default::default()
        };

        let _ = cx.open_window(window_options, |_window, cx| {
            cx.new(|cx| GroupDialog::edit(&group, cx))
        });
    }

    /// Validate the form
    fn validate(&mut self, cx: &mut Context<Self>) -> bool {
        self.errors.clear();

        let name = self.name_field.read(cx).content().trim().to_string();

        if name.is_empty() {
            self.errors.push("Name is required".to_string());
        }

        self.errors.is_empty()
    }

    /// Build the group from form fields
    fn build_group(&self, cx: &Context<Self>) -> SessionGroup {
        let name = self.name_field.read(cx).content().to_string();

        let mut group = if let Some(parent_id) = self.parent_id {
            SessionGroup::new_nested(name, parent_id)
        } else {
            SessionGroup::new(name)
        };

        group.color = self.color.clone();

        // Preserve ID if editing
        if let Some(id) = self.group_id {
            group.id = id;
        }

        group
    }

    /// Get the built group if valid
    pub fn get_group(&self, cx: &Context<Self>) -> Option<SessionGroup> {
        if self.errors.is_empty() {
            Some(self.build_group(cx))
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

        let group = self.build_group(cx);

        // Save to app state
        if let Some(app_state) = cx.try_global::<AppState>() {
            let mut app = app_state.app.lock();
            if self.group_id.is_some() {
                // Update existing group
                let _ = app.session_manager.update_group(group.id, group.clone());
            } else {
                // Add new group
                app.session_manager.add_group(group.clone());
            }
            let _ = app.save();
        }

        cx.emit(GroupDialogEvent::Saved(group));

        // Close the window
        window.remove_window();
    }

    /// Handle cancel button click
    fn handle_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(GroupDialogEvent::Canceled);
        window.remove_window();
    }
}

impl Render for GroupDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self.group_id.is_some() {
            "Edit Group"
        } else {
            "New Group"
        };

        let current_color = self.color.clone();

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
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap_4()
                    .p_4()
                    // Errors
                    .when(!self.errors.is_empty(), |this| {
                        this.child(
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
                                })),
                        )
                    })
                    // Name field
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xcdd6f4))
                                    .child("Name"),
                            )
                            .child(self.name_field.clone()),
                    )
                    // Color picker
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xcdd6f4))
                                    .child("Color (optional)"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .children(self.available_colors.iter().map(|(name, hex)| {
                                        let is_selected = current_color.as_deref() == Some(*hex);
                                        let color_value = u32::from_str_radix(&hex[1..], 16).unwrap_or(0);
                                        let hex_string = hex.to_string();

                                        div()
                                            .id(ElementId::Name(format!("color-{}", name).into()))
                                            .w(px(28.0))
                                            .h(px(28.0))
                                            .rounded_full()
                                            .cursor_pointer()
                                            .bg(rgb(color_value))
                                            .when(is_selected, |this| {
                                                this.border_2().border_color(rgb(0xcdd6f4))
                                            })
                                            .when(!is_selected, |this| {
                                                this.border_1()
                                                    .border_color(rgb(0x45475a))
                                                    .hover(|style| style.border_color(rgb(0x6c7086)))
                                            })
                                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                                if this.color.as_deref() == Some(hex_string.as_str()) {
                                                    // Deselect if already selected
                                                    this.color = None;
                                                } else {
                                                    this.color = Some(hex_string.clone());
                                                }
                                                cx.notify();
                                            }))
                                    })),
                            ),
                    ),
            )
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

/// Create a group dialog view
pub fn group_dialog(parent_id: Option<Uuid>, cx: &mut App) -> Entity<GroupDialog> {
    cx.new(|cx| GroupDialog::new(parent_id, cx))
}

/// Create a group dialog for editing
pub fn edit_group_dialog(group: &SessionGroup, cx: &mut App) -> Entity<GroupDialog> {
    cx.new(|cx| GroupDialog::edit(group, cx))
}
