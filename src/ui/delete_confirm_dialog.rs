use gpui::*;
use gpui::prelude::*;
use uuid::Uuid;

use crate::app::AppState;

/// Target for deletion
#[derive(Clone, Debug)]
pub enum DeleteTarget {
    Session { id: Uuid, name: String },
    Group { id: Uuid, name: String },
}

/// Events emitted by the delete confirmation dialog
pub enum DeleteConfirmEvent {
    Confirmed,
    Canceled,
}

impl EventEmitter<DeleteConfirmEvent> for DeleteConfirmDialog {}

/// Delete confirmation dialog
pub struct DeleteConfirmDialog {
    target: DeleteTarget,
    /// For groups: whether to delete all sessions in the group
    recursive: bool,
}

impl DeleteConfirmDialog {
    /// Create a new delete confirmation dialog
    pub fn new(target: DeleteTarget) -> Self {
        Self {
            target,
            recursive: false,
        }
    }

    /// Open as a modal window for session deletion
    pub fn open_for_session(id: Uuid, name: String, cx: &mut App) {
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(380.0), px(200.0)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Delete Session".into()),
                appears_transparent: false,
                ..Default::default()
            }),
            kind: WindowKind::Normal,
            ..Default::default()
        };

        let _ = cx.open_window(window_options, |_window, cx| {
            cx.new(|_cx| DeleteConfirmDialog::new(DeleteTarget::Session { id, name }))
        });
    }

    /// Open as a modal window for group deletion
    pub fn open_for_group(id: Uuid, name: String, cx: &mut App) {
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(380.0), px(240.0)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Delete Group".into()),
                appears_transparent: false,
                ..Default::default()
            }),
            kind: WindowKind::Normal,
            ..Default::default()
        };

        let _ = cx.open_window(window_options, |_window, cx| {
            cx.new(|_cx| DeleteConfirmDialog::new(DeleteTarget::Group { id, name }))
        });
    }

    /// Handle delete confirmation
    fn handle_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app_state) = cx.try_global::<AppState>() {
            let mut app = app_state.app.lock();
            match &self.target {
                DeleteTarget::Session { id, .. } => {
                    if let Err(e) = app.delete_session(*id) {
                        tracing::error!("Failed to delete session: {}", e);
                    }
                }
                DeleteTarget::Group { id, .. } => {
                    if let Err(e) = app.delete_group(*id, self.recursive) {
                        tracing::error!("Failed to delete group: {}", e);
                    }
                }
            }
            let _ = app.save();
        }

        cx.emit(DeleteConfirmEvent::Confirmed);
        window.remove_window();
    }

    /// Handle cancel
    fn handle_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DeleteConfirmEvent::Canceled);
        window.remove_window();
    }
}

impl Render for DeleteConfirmDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, name, is_group) = match &self.target {
            DeleteTarget::Session { name, .. } => ("Delete Session?", name.clone(), false),
            DeleteTarget::Group { name, .. } => ("Delete Group?", name.clone(), true),
        };

        let recursive = self.recursive;

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
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(0x313244))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0xf38ba8))
                            .child(title),
                    ),
            )
            // Content
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap_3()
                    .p_4()
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xcdd6f4))
                            .child(format!("Are you sure you want to delete '{}'?", name)),
                    )
                    // Show recursive checkbox only for groups
                    .when(is_group, |this| {
                        this.child(
                            div()
                                .id("recursive-checkbox")
                                .flex()
                                .items_center()
                                .gap_2()
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.recursive = !this.recursive;
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .w(px(16.0))
                                        .h(px(16.0))
                                        .rounded_sm()
                                        .border_1()
                                        .border_color(rgb(0x6c7086))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when(recursive, |this| {
                                            this.bg(rgb(0xf38ba8))
                                                .border_color(rgb(0xf38ba8))
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(0x1e1e2e))
                                                        .child("✓"),
                                                )
                                        }),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0xcdd6f4))
                                        .child("Delete all sessions in this group"),
                                ),
                        )
                    })
                    .when(is_group && !recursive, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x6c7086))
                                .child("Sessions will be moved to ungrouped."),
                        )
                    }),
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
                            .id("delete-btn")
                            .px_4()
                            .py_2()
                            .bg(rgb(0xf38ba8))
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0xeba0ac)))
                            .on_click(cx.listener(|this, _event, window, cx| {
                                this.handle_delete(window, cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x1e1e2e))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Delete"),
                            ),
                    ),
            )
    }
}
