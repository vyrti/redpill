//! Terminal search bar component

use gpui::*;
use gpui::prelude::*;

/// Events emitted by SearchBar
pub enum SearchBarEvent {
    /// Close the search bar
    Close,
    /// Search query changed
    QueryChanged(String),
    /// Navigate to next match
    FindNext,
    /// Navigate to previous match
    FindPrev,
}

impl EventEmitter<SearchBarEvent> for SearchBar {}

/// Search bar state
pub struct SearchBar {
    /// Current search query
    query: String,
    /// Current match index (0-based)
    current_match: usize,
    /// Total number of matches
    total_matches: usize,
    /// Case sensitive search
    case_sensitive: bool,
    /// Focus handle for the input field
    focus_handle: FocusHandle,
    /// Cursor position in query
    cursor_pos: usize,
}

impl SearchBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            query: String::new(),
            current_match: 0,
            total_matches: 0,
            case_sensitive: false,
            focus_handle: cx.focus_handle(),
            cursor_pos: 0,
        }
    }

    /// Get the current search query
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Get whether search is case sensitive
    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// Update match count and reset to first match
    pub fn set_match_count(&mut self, count: usize, cx: &mut Context<Self>) {
        self.total_matches = count;
        if count == 0 {
            self.current_match = 0;
        } else if self.current_match >= count {
            self.current_match = count - 1;
        }
        cx.notify();
    }

    /// Get the current match index
    pub fn current_match_index(&self) -> usize {
        self.current_match
    }

    /// Focus the search bar
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus_handle, cx);
    }

    /// Handle character input
    fn handle_input(&mut self, text: &str, cx: &mut Context<Self>) {
        self.query.insert_str(self.cursor_pos, text);
        self.cursor_pos += text.len();
        self.current_match = 0;
        cx.emit(SearchBarEvent::QueryChanged(self.query.clone()));
        cx.notify();
    }

    /// Handle backspace
    fn handle_backspace(&mut self, cx: &mut Context<Self>) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.query.remove(self.cursor_pos);
            self.current_match = 0;
            cx.emit(SearchBarEvent::QueryChanged(self.query.clone()));
            cx.notify();
        }
    }

    /// Navigate to next match
    fn find_next(&mut self, cx: &mut Context<Self>) {
        if self.total_matches > 0 {
            self.current_match = (self.current_match + 1) % self.total_matches;
            cx.emit(SearchBarEvent::FindNext);
            cx.notify();
        }
    }

    /// Navigate to previous match
    fn find_prev(&mut self, cx: &mut Context<Self>) {
        if self.total_matches > 0 {
            if self.current_match == 0 {
                self.current_match = self.total_matches - 1;
            } else {
                self.current_match -= 1;
            }
            cx.emit(SearchBarEvent::FindPrev);
            cx.notify();
        }
    }

    /// Toggle case sensitivity
    fn toggle_case_sensitive(&mut self, cx: &mut Context<Self>) {
        self.case_sensitive = !self.case_sensitive;
        self.current_match = 0;
        cx.emit(SearchBarEvent::QueryChanged(self.query.clone()));
        cx.notify();
    }

    /// Close the search bar
    fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(SearchBarEvent::Close);
    }
}

impl Render for SearchBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_focused = self.focus_handle.is_focused(window);
        let query = self.query.clone();
        let cursor_pos = self.cursor_pos;
        let current = self.current_match;
        let total = self.total_matches;
        let case_sensitive = self.case_sensitive;

        div()
            .id("search-bar")
            .absolute()
            .top_2()
            .right_2()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .bg(rgb(0x313244))
            .border_1()
            .border_color(rgb(0x45475a))
            .rounded_md()
            .shadow_lg()
            // Input field
            .child(
                div()
                    .id("search-input")
                    .track_focus(&self.focus_handle)
                    .w(px(200.0))
                    .px_2()
                    .py_1()
                    .bg(rgb(0x1e1e2e))
                    .rounded_sm()
                    .border_1()
                    .when(is_focused, |s| s.border_color(rgb(0x89b4fa)))
                    .when(!is_focused, |s| s.border_color(rgb(0x45475a)))
                    .cursor_text()
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                        let keystroke = &event.keystroke;
                        match keystroke.key.as_str() {
                            "escape" => this.close(cx),
                            "enter" => {
                                if keystroke.modifiers.shift {
                                    this.find_prev(cx);
                                } else {
                                    this.find_next(cx);
                                }
                            }
                            "backspace" => this.handle_backspace(cx),
                            "f" if keystroke.modifiers.platform || keystroke.modifiers.control => {
                                // Ignore Cmd+F/Ctrl+F when already in search
                            }
                            key if key.len() == 1 && !keystroke.modifiers.control && !keystroke.modifiers.platform && !keystroke.modifiers.alt => {
                                this.handle_input(key, cx);
                            }
                            "space" => {
                                this.handle_input(" ", cx);
                            }
                            _ => {}
                        }
                    }))
                    .on_click(cx.listener(|this, _event, window, cx| {
                        window.focus(&this.focus_handle, cx);
                    }))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xcdd6f4))
                            .flex()
                            .items_center()
                            .when(query.is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_color(rgb(0x585b70))
                                        .italic()
                                        .child("Search...")
                                )
                                .when(is_focused, |el| {
                                    el.child(
                                        div()
                                            .w(px(1.0))
                                            .h(px(14.0))
                                            .bg(rgb(0xcdd6f4))
                                    )
                                })
                            })
                            .when(!query.is_empty(), |el| {
                                let chars: Vec<char> = query.chars().collect();
                                el.children(chars.iter().enumerate().flat_map(|(idx, ch)| {
                                    let mut elements: Vec<Div> = Vec::new();
                                    if idx == cursor_pos && is_focused {
                                        elements.push(
                                            div()
                                                .w(px(1.0))
                                                .h(px(14.0))
                                                .bg(rgb(0xcdd6f4))
                                        );
                                    }
                                    let ch_str = if *ch == ' ' { "\u{00A0}".to_string() } else { ch.to_string() };
                                    elements.push(div().child(ch_str));
                                    elements
                                }))
                                .when(cursor_pos == chars.len() && is_focused, |el| {
                                    el.child(
                                        div()
                                            .w(px(1.0))
                                            .h(px(14.0))
                                            .bg(rgb(0xcdd6f4))
                                    )
                                })
                            })
                    )
            )
            // Match counter
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x9399b2))
                    .min_w(px(50.0))
                    .text_right()
                    .when(total > 0, |el| {
                        el.child(format!("{}/{}", current + 1, total))
                    })
                    .when(total == 0 && !query.is_empty(), |el| {
                        el.text_color(rgb(0xf38ba8))
                            .child("0/0")
                    })
            )
            // Navigation buttons
            .child(
                div()
                    .id("search-prev")
                    .px_1()
                    .cursor_pointer()
                    .text_sm()
                    .text_color(rgb(0x9399b2))
                    .hover(|s| s.text_color(rgb(0xcdd6f4)))
                    .on_click(cx.listener(|this, _, _, cx| this.find_prev(cx)))
                    .child("\u{25B2}") // Up arrow
            )
            .child(
                div()
                    .id("search-next")
                    .px_1()
                    .cursor_pointer()
                    .text_sm()
                    .text_color(rgb(0x9399b2))
                    .hover(|s| s.text_color(rgb(0xcdd6f4)))
                    .on_click(cx.listener(|this, _, _, cx| this.find_next(cx)))
                    .child("\u{25BC}") // Down arrow
            )
            // Case sensitivity toggle
            .child(
                div()
                    .id("case-toggle")
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .text_xs()
                    .rounded_sm()
                    .when(case_sensitive, |s| {
                        s.bg(rgb(0x89b4fa))
                            .text_color(rgb(0x1e1e2e))
                    })
                    .when(!case_sensitive, |s| {
                        s.bg(rgb(0x45475a))
                            .text_color(rgb(0x9399b2))
                            .hover(|h| h.bg(rgb(0x585b70)))
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_case_sensitive(cx)))
                    .child("Aa")
            )
            // Close button
            .child(
                div()
                    .id("search-close")
                    .px_1()
                    .cursor_pointer()
                    .text_sm()
                    .text_color(rgb(0x9399b2))
                    .hover(|s| s.text_color(rgb(0xf38ba8)))
                    .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
                    .child("\u{2715}") // X mark
            )
    }
}

impl Focusable for SearchBar {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
