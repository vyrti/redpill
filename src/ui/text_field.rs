use gpui::*;
use gpui::prelude::*;

/// Events emitted by TextField
pub enum TextFieldEvent {
    /// Content changed
    Changed(String),
    /// Enter key pressed (submit)
    Submit,
}

impl EventEmitter<TextFieldEvent> for TextField {}

/// A text input component with focus handling and keyboard events
pub struct TextField {
    focus_handle: FocusHandle,
    content: String,
    cursor_pos: usize,
    placeholder: SharedString,
    on_change: Option<Box<dyn Fn(&str, &mut Context<Self>) + 'static>>,
    is_password: bool,
}

impl TextField {
    /// Create a new text field with a placeholder
    pub fn new(cx: &mut Context<Self>, placeholder: impl Into<SharedString>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: String::new(),
            cursor_pos: 0,
            placeholder: placeholder.into(),
            on_change: None,
            is_password: false,
        }
    }

    /// Create a new text field with initial content
    pub fn with_content(cx: &mut Context<Self>, placeholder: impl Into<SharedString>, content: String) -> Self {
        let cursor_pos = content.len();
        Self {
            focus_handle: cx.focus_handle(),
            content,
            cursor_pos,
            placeholder: placeholder.into(),
            on_change: None,
            is_password: false,
        }
    }

    /// Set whether this is a password field (hides text)
    pub fn set_password(&mut self, is_password: bool) {
        self.is_password = is_password;
    }

    /// Set the change callback
    pub fn on_change(mut self, callback: impl Fn(&str, &mut Context<Self>) + 'static) -> Self {
        self.on_change = Some(Box::new(callback));
        self
    }

    /// Get the current content
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Set the content programmatically
    pub fn set_content(&mut self, text: impl Into<String>) {
        self.content = text.into();
        self.cursor_pos = self.content.len();
    }

    /// Get the focus handle
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// Focus the text field
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus_handle, cx);
    }

    /// Handle character input
    fn handle_input(&mut self, text: &str, cx: &mut Context<Self>) {
        // Insert text at cursor position
        self.content.insert_str(self.cursor_pos, text);
        self.cursor_pos += text.len();

        if let Some(ref callback) = self.on_change {
            callback(&self.content, cx);
        }
        cx.emit(TextFieldEvent::Changed(self.content.clone()));
        cx.notify();
    }

    /// Handle backspace
    fn handle_backspace(&mut self, cx: &mut Context<Self>) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.content.remove(self.cursor_pos);

            if let Some(ref callback) = self.on_change {
                callback(&self.content, cx);
            }
            cx.emit(TextFieldEvent::Changed(self.content.clone()));
            cx.notify();
        }
    }

    /// Handle delete
    fn handle_delete(&mut self, cx: &mut Context<Self>) {
        if self.cursor_pos < self.content.len() {
            self.content.remove(self.cursor_pos);

            if let Some(ref callback) = self.on_change {
                callback(&self.content, cx);
            }
            cx.emit(TextFieldEvent::Changed(self.content.clone()));
            cx.notify();
        }
    }

    /// Move cursor left
    fn move_left(&mut self, cx: &mut Context<Self>) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            cx.notify();
        }
    }

    /// Move cursor right
    fn move_right(&mut self, cx: &mut Context<Self>) {
        if self.cursor_pos < self.content.len() {
            self.cursor_pos += 1;
            cx.notify();
        }
    }

    /// Move cursor to start
    fn move_to_start(&mut self, cx: &mut Context<Self>) {
        self.cursor_pos = 0;
        cx.notify();
    }

    /// Move cursor to end
    fn move_to_end(&mut self, cx: &mut Context<Self>) {
        self.cursor_pos = self.content.len();
        cx.notify();
    }

    /// Get the display text (masked if password)
    fn display_text(&self) -> String {
        if self.is_password {
            "*".repeat(self.content.len())
        } else {
            self.content.clone()
        }
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_focused = self.focus_handle.is_focused(window);
        let has_content = !self.content.is_empty();
        let display_text = self.display_text();
        let cursor_pos = self.cursor_pos;
        let placeholder = self.placeholder.clone();

        // Collect chars for character-by-character rendering (enables wrapping)
        let chars: Vec<char> = display_text.chars().collect();

        div()
            .id("text-field")
            .track_focus(&self.focus_handle)
            .w_full()
            .min_h(px(32.0))
            .max_h(px(80.0))  // Max ~3 lines
            .px_2()
            .py_1()
            .bg(rgb(0x313244))
            .rounded_md()
            .border_1()
            .overflow_y_scroll()
            .when(is_focused, |this| {
                this.border_color(rgb(0x89b4fa))
            })
            .when(!is_focused, |this| {
                this.border_color(rgb(0x45475a))
            })
            .cursor_text()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                let keystroke = &event.keystroke;
                let is_cmd_or_ctrl = keystroke.modifiers.platform || keystroke.modifiers.control;

                match keystroke.key.as_str() {
                    "enter" if !keystroke.modifiers.shift => {
                        cx.emit(TextFieldEvent::Submit);
                    }
                    "backspace" => this.handle_backspace(cx),
                    "delete" => this.handle_delete(cx),
                    "left" => this.move_left(cx),
                    "right" => this.move_right(cx),
                    "home" => this.move_to_start(cx),
                    "end" => this.move_to_end(cx),
                    "a" if is_cmd_or_ctrl => {
                        // Select all - for now just move to end
                        this.move_to_end(cx);
                    }
                    "v" if is_cmd_or_ctrl => {
                        // Paste
                        if let Some(item) = cx.read_from_clipboard() {
                            if let Some(text) = item.text() {
                                this.handle_input(&text, cx);
                            }
                        }
                    }
                    "c" if is_cmd_or_ctrl => {
                        // Copy all content
                        if !this.content.is_empty() {
                            cx.write_to_clipboard(ClipboardItem::new_string(this.content.clone()));
                        }
                    }
                    "x" if is_cmd_or_ctrl => {
                        // Cut all content
                        if !this.content.is_empty() {
                            cx.write_to_clipboard(ClipboardItem::new_string(this.content.clone()));
                            this.content.clear();
                            this.cursor_pos = 0;
                            if let Some(ref callback) = this.on_change {
                                callback(&this.content, cx);
                            }
                            cx.emit(TextFieldEvent::Changed(this.content.clone()));
                            cx.notify();
                        }
                    }
                    "space" => {
                        this.handle_input(" ", cx);
                    }
                    key if key.len() == 1 && !keystroke.modifiers.control && !keystroke.modifiers.platform && !keystroke.modifiers.alt => {
                        this.handle_input(key, cx);
                    }
                    _ => {}
                }
            }))
            .on_click(cx.listener(|this, _event, window, cx| {
                window.focus(&this.focus_handle, cx);
            }))
            .child(
                div()
                    .w_full()
                    .text_sm()
                    .line_height(px(18.0))
                    .when(!has_content, |this| {
                        this.flex()
                            .items_center()
                            .text_color(rgb(0x585b70))
                            .italic()
                            .child(placeholder)
                            .when(is_focused, |el| {
                                // Show cursor in empty field when focused
                                el.child(
                                    div()
                                        .w(px(1.0))
                                        .h(px(14.0))
                                        .bg(rgb(0xcdd6f4))
                                )
                            })
                    })
                    .when(has_content, |this| {
                        // Render each character as inline span for proper wrapping
                        this.text_color(rgb(0xcdd6f4))
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .children(chars.iter().enumerate().flat_map(|(idx, ch)| {
                                        let mut elements: Vec<Div> = Vec::new();

                                        // Insert cursor before this character if position matches
                                        if idx == cursor_pos && is_focused {
                                            elements.push(
                                                div()
                                                    .w(px(1.0))
                                                    .h(px(14.0))
                                                    .bg(rgb(0xcdd6f4))
                                            );
                                        }

                                        // Character itself - use whitespace-pre to preserve spaces
                                        let ch_str = if *ch == ' ' { "\u{00A0}".to_string() } else { ch.to_string() };
                                        elements.push(div().child(ch_str));

                                        elements
                                    }))
                                    // Cursor at end if cursor_pos == chars.len()
                                    .when(cursor_pos == chars.len() && is_focused, |el| {
                                        el.child(
                                            div()
                                                .w(px(1.0))
                                                .h(px(14.0))
                                                .bg(rgb(0xcdd6f4))
                                        )
                                    })
                            )
                    })
            )
    }
}

/// Create a text field entity
pub fn text_field(placeholder: impl Into<SharedString>, cx: &mut App) -> Entity<TextField> {
    cx.new(|cx| TextField::new(cx, placeholder))
}

/// Create a text field entity with initial content
pub fn text_field_with_content(
    placeholder: impl Into<SharedString>,
    content: String,
    cx: &mut App,
) -> Entity<TextField> {
    cx.new(|cx| TextField::with_content(cx, placeholder, content))
}
