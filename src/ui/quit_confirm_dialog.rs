use gpui::*;

/// Events emitted by the quit confirmation dialog
pub enum QuitConfirmEvent {
    ConfirmedQuit,
    Canceled,
}

impl EventEmitter<QuitConfirmEvent> for QuitConfirmDialog {}

/// Quit confirmation dialog shown when closing app with active SSH connections
pub struct QuitConfirmDialog {
    /// Number of active SSH connections
    ssh_connection_count: usize,
}

impl QuitConfirmDialog {
    /// Create a new quit confirmation dialog
    pub fn new(ssh_connection_count: usize) -> Self {
        Self { ssh_connection_count }
    }

    /// Open as a modal window
    pub fn open(ssh_connection_count: usize, cx: &mut App) {
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(420.0), px(220.0)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Quit RedPill?".into()),
                appears_transparent: false,
                ..Default::default()
            }),
            kind: WindowKind::Normal,
            ..Default::default()
        };

        let _ = cx.open_window(window_options, |_window, cx| {
            cx.new(|_cx| QuitConfirmDialog::new(ssh_connection_count))
        });
    }

    /// Handle quit confirmation
    fn handle_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(QuitConfirmEvent::ConfirmedQuit);
        window.remove_window();
        // Actually quit the application
        cx.quit();
    }

    /// Handle cancel
    fn handle_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(QuitConfirmEvent::Canceled);
        window.remove_window();
    }
}

impl Render for QuitConfirmDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let connection_text = if self.ssh_connection_count == 1 {
            "1 active SSH connection".to_string()
        } else {
            format!("{} active SSH connections", self.ssh_connection_count)
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
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(0x313244))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0xfab387)) // Orange/peach for warning
                            .child("Quit RedPill?"),
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
                            .child(format!(
                                "You have {}. Quitting will disconnect all sessions.",
                                connection_text
                            )),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x6c7086))
                            .child("Are you sure you want to quit?"),
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
                            .id("quit-btn")
                            .px_4()
                            .py_2()
                            .bg(rgb(0xfab387)) // Orange/peach for warning
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0xf9e2af)))
                            .on_click(cx.listener(|this, _event, window, cx| {
                                this.handle_quit(window, cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x1e1e2e))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Quit"),
                            ),
                    ),
            )
    }
}
