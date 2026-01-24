//! SFTP file browser panel

use gpui::*;
use gpui::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

use crate::sftp::{DirEntry, EntryType, SftpBrowser, SftpError, TransferProgress, format_size};

/// Events emitted by SftpPanel
pub enum SftpPanelEvent {
    /// Close the panel
    Close,
}

impl EventEmitter<SftpPanelEvent> for SftpPanel {}

/// SFTP panel state
pub struct SftpPanel {
    /// SFTP browser (wrapped for async access)
    browser: Arc<TokioMutex<SftpBrowser>>,
    /// Current directory path display
    current_path: PathBuf,
    /// Cached directory entries
    entries: Vec<DirEntry>,
    /// Selected entry index
    selected: Option<usize>,
    /// Active transfers
    transfers: Vec<TransferProgress>,
    /// Focus handle
    focus_handle: FocusHandle,
    /// Loading state
    loading: bool,
    /// Error message
    error: Option<String>,
}

impl SftpPanel {
    pub fn new(browser: Arc<TokioMutex<SftpBrowser>>, cx: &mut Context<Self>) -> Self {
        Self {
            browser,
            current_path: PathBuf::from("/"),
            entries: Vec::new(),
            selected: None,
            transfers: Vec::new(),
            focus_handle: cx.focus_handle(),
            loading: false,
            error: None,
        }
    }

    /// Set initial path and load entries
    pub fn set_path(&mut self, path: PathBuf) {
        self.current_path = path;
    }

    /// Set entries from async load
    pub fn set_entries(&mut self, entries: Vec<DirEntry>, cx: &mut Context<Self>) {
        self.entries = entries;
        self.loading = false;
        self.error = None;
        self.selected = if self.entries.is_empty() { None } else { Some(0) };
        cx.notify();
    }

    /// Set error state
    pub fn set_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.error = Some(error);
        self.loading = false;
        cx.notify();
    }

    /// Navigate to a directory
    fn navigate_to(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.current_path = path.clone();
        self.loading = true;
        self.error = None;
        cx.notify();

        // Spawn async task to load directory
        let browser = self.browser.clone();
        cx.spawn(async move |entity, cx| {
            let result: Result<Vec<DirEntry>, SftpError> = {
                let mut browser: tokio::sync::MutexGuard<'_, SftpBrowser> = browser.lock().await;
                browser.list_dir(&path).await
            };

            entity.update(cx, |this, cx| {
                match result {
                    Ok(entries) => this.set_entries(entries, cx),
                    Err(e) => this.set_error(e.to_string(), cx),
                }
            }).ok();
        }).detach();
    }

    /// Go to parent directory
    fn go_up(&mut self, cx: &mut Context<Self>) {
        if let Some(parent) = self.current_path.parent() {
            let parent = parent.to_path_buf();
            self.navigate_to(parent, cx);
        }
    }

    /// Refresh current directory
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let path = self.current_path.clone();
        self.navigate_to(path, cx);
    }

    /// Open selected item (navigate if directory)
    fn open_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(idx) = self.selected {
            if let Some(entry) = self.entries.get(idx) {
                if entry.entry_type == EntryType::Directory {
                    let new_path = self.current_path.join(&entry.name);
                    self.navigate_to(new_path, cx);
                }
            }
        }
    }

    /// Select next item
    fn select_next(&mut self, cx: &mut Context<Self>) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = Some(match self.selected {
            Some(idx) => (idx + 1).min(self.entries.len() - 1),
            None => 0,
        });
        cx.notify();
    }

    /// Select previous item
    fn select_prev(&mut self, cx: &mut Context<Self>) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = Some(match self.selected {
            Some(idx) => idx.saturating_sub(1),
            None => 0,
        });
        cx.notify();
    }

    /// Handle keyboard input
    fn handle_key_input(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;

        match keystroke.key.as_str() {
            "escape" => {
                cx.emit(SftpPanelEvent::Close);
            }
            "enter" => {
                self.open_selected(cx);
            }
            "backspace" => {
                self.go_up(cx);
            }
            "up" => {
                self.select_prev(cx);
            }
            "down" => {
                self.select_next(cx);
            }
            "r" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                self.refresh(cx);
            }
            _ => {}
        }
    }
}

impl Focusable for SftpPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SftpPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let path_str = self.current_path.to_string_lossy().to_string();
        let selected = self.selected;
        let loading = self.loading;
        let has_error = self.error.is_some();
        let error_msg = self.error.clone();
        let is_empty = self.entries.is_empty();
        let entries = self.entries.clone();
        let transfers = self.transfers.clone();

        div()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1e2e))
            .border_l_1()
            .border_color(rgb(0x313244))
            .on_key_down(cx.listener(Self::handle_key_input))
            // Header with breadcrumb
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .bg(rgb(0x313244))
                    .border_b_1()
                    .border_color(rgb(0x45475a))
                    // Up button
                    .child(
                        div()
                            .id("sftp-up")
                            .px_2()
                            .py_1()
                            .cursor_pointer()
                            .text_sm()
                            .text_color(rgb(0x9399b2))
                            .hover(|s| s.text_color(rgb(0xcdd6f4)).bg(rgb(0x45475a)))
                            .rounded_sm()
                            .on_click(cx.listener(|this, _, _, cx| this.go_up(cx)))
                            .child("\u{2191}") // Up arrow
                    )
                    // Refresh button
                    .child(
                        div()
                            .id("sftp-refresh")
                            .px_2()
                            .py_1()
                            .cursor_pointer()
                            .text_sm()
                            .text_color(rgb(0x9399b2))
                            .hover(|s| s.text_color(rgb(0xcdd6f4)).bg(rgb(0x45475a)))
                            .rounded_sm()
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx)))
                            .child("\u{21BB}") // Refresh symbol
                    )
                    // Path
                    .child(
                        div()
                            .flex_1()
                            .px_2()
                            .text_sm()
                            .text_color(rgb(0xcdd6f4))
                            .overflow_hidden()
                            .child(path_str)
                    )
                    // Close button
                    .child(
                        div()
                            .id("sftp-close")
                            .px_2()
                            .py_1()
                            .cursor_pointer()
                            .text_sm()
                            .text_color(rgb(0x9399b2))
                            .hover(|s| s.text_color(rgb(0xf38ba8)))
                            .rounded_sm()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(SftpPanelEvent::Close)))
                            .child("\u{2715}") // X mark
                    )
            )
            // File list
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(
                        // Loading state
                        if loading {
                            div()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0x9399b2))
                                        .italic()
                                        .child("Loading...")
                                )
                                .into_any_element()
                        }
                        // Error state
                        else if has_error {
                            div()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .p_4()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0xf38ba8))
                                        .child(error_msg.unwrap_or_default())
                                )
                                .into_any_element()
                        }
                        // Empty state
                        else if is_empty {
                            div()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0x6c7086))
                                        .child("Empty directory")
                                )
                                .into_any_element()
                        }
                        // File entries
                        else {
                            div()
                                .flex()
                                .flex_col()
                                .children(entries.iter().enumerate().map(|(idx, entry)| {
                                    let is_selected = selected == Some(idx);
                                    let icon = match entry.entry_type {
                                        EntryType::Directory => "\u{1F4C1}", // Folder icon
                                        EntryType::File => "\u{1F4C4}",      // File icon
                                        EntryType::Symlink => "\u{1F517}",   // Link icon
                                        EntryType::Unknown => "\u{2753}",    // Question mark
                                    };

                                    let size_str = if entry.entry_type == EntryType::Directory {
                                        "-".to_string()
                                    } else {
                                        format_size(entry.size)
                                    };

                                    div()
                                        .id(ElementId::Name(format!("sftp-entry-{}", idx).into()))
                                        .flex()
                                        .items_center()
                                        .px_2()
                                        .py_1()
                                        .cursor_pointer()
                                        .when(is_selected, |s| s.bg(rgb(0x45475a)))
                                        .when(!is_selected, |s| s.hover(|h| h.bg(rgb(0x313244))))
                                        .on_click({
                                            let idx = idx;
                                            cx.listener(move |this, _, _, cx| {
                                                this.selected = Some(idx);
                                                cx.notify();
                                            })
                                        })
                                        // Icon
                                        .child(
                                            div()
                                                .w(px(24.0))
                                                .text_sm()
                                                .child(icon)
                                        )
                                        // Name
                                        .child(
                                            div()
                                                .flex_1()
                                                .text_sm()
                                                .text_color(rgb(0xcdd6f4))
                                                .overflow_hidden()
                                                .child(entry.name.clone())
                                        )
                                        // Size
                                        .child(
                                            div()
                                                .w(px(80.0))
                                                .text_xs()
                                                .text_color(rgb(0x9399b2))
                                                .text_right()
                                                .child(size_str)
                                        )
                                        // Permissions
                                        .child(
                                            div()
                                                .w(px(90.0))
                                                .text_xs()
                                                .text_color(rgb(0x6c7086))
                                                .child(entry.permissions.clone())
                                        )
                                }))
                                .into_any_element()
                        }
                    )
            )
            // Transfers section
            .when(!transfers.is_empty(), |el| {
                el.child(
                    div()
                        .border_t_1()
                        .border_color(rgb(0x45475a))
                        .p_2()
                        .children(transfers.iter().map(|t: &TransferProgress| {
                            let percent = t.progress_percent();
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_xs()
                                .child(
                                    div()
                                        .flex_1()
                                        .text_color(rgb(0xcdd6f4))
                                        .child(t.name.clone())
                                )
                                .child(
                                    div()
                                        .w(px(100.0))
                                        .h(px(4.0))
                                        .bg(rgb(0x313244))
                                        .rounded_full()
                                        .child(
                                            div()
                                                .h_full()
                                                .w(px(percent))
                                                .bg(rgb(0x89b4fa))
                                                .rounded_full()
                                        )
                                )
                                .child(
                                    div()
                                        .w(px(40.0))
                                        .text_right()
                                        .text_color(rgb(0x9399b2))
                                        .child(format!("{:.0}%", percent))
                                )
                        }))
                )
            })
    }
}
