//! Claude Code Agent Panel

use gpui::*;
use gpui::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use redpill_agent::{
    ClaudeConnection, SessionInfo, SessionUpdate,
    ToolCall, ToolCallStatus, ToolKind,
};
use crate::app::AppState;
use super::text_field::{TextField, TextFieldEvent};

#[derive(Clone, Debug)]
pub struct AgentMessage {
    pub id: usize,
    pub role: MessageRole,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AgentConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

/// Permission mode for Claude CLI
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum PermissionMode {
    /// Default mode - asks for permission
    #[default]
    Default,
    /// Bypass all permission prompts
    BypassPermissions,
    /// Plan mode - requires approval before executing
    PlanMode,
}

pub enum AgentPanelEvent {
    MessageReceived(String),
    ToggleVisibility,
}

impl EventEmitter<AgentPanelEvent> for AgentPanel {}

pub struct AgentPanel {
    connection: Option<Arc<ClaudeConnection>>,
    session_info: Option<SessionInfo>,
    connection_state: AgentConnectionState,
    permission_mode: PermissionMode,
    messages: Vec<AgentMessage>,
    pending_tool_calls: Vec<ToolCall>,
    input_field: Entity<TextField>,
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
    update_rx: Option<async_channel::Receiver<SessionUpdate>>,
    skip_first_response: bool,
    next_message_id: usize,
    _subscriptions: Vec<Subscription>,
}

impl AgentPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let input_field = cx.new(|cx| TextField::new(cx, ""));

        let input_sub = cx.subscribe(&input_field, |this, _field, event, cx| {
            if let TextFieldEvent::Submit = event {
                this.send_message(cx);
            }
        });

        let mut panel = Self {
            connection: None,
            session_info: None,
            connection_state: AgentConnectionState::Disconnected,
            permission_mode: PermissionMode::BypassPermissions, // Default to bypass for convenience
            messages: Vec::new(),
            pending_tool_calls: Vec::new(),
            input_field,
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
            update_rx: None,
            skip_first_response: false,
            next_message_id: 0,
            _subscriptions: vec![input_sub],
        };

        panel.auto_connect(cx);
        panel
    }

    fn add_message(&mut self, role: MessageRole, content: String) {
        let id = self.next_message_id;
        self.next_message_id += 1;
        self.messages.push(AgentMessage { id, role, content });
    }

    fn scroll_to_bottom(&mut self, cx: &mut Context<Self>) {
        // Schedule scroll to bottom after layout
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(10)).await;
            this.update(cx, |this, cx| {
                this.scroll_handle.scroll_to_bottom();
                cx.notify();
            }).ok();
        }).detach();
    }

    fn auto_connect(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            this.update(cx, |this, cx| this.connect(cx)).ok();
        }).detach();
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        if matches!(self.connection_state, AgentConnectionState::Connected | AgentConnectionState::Connecting) {
            return;
        }

        self.connection_state = AgentConnectionState::Connecting;
        self.messages.clear();
        self.next_message_id = 0;
        self.add_message(MessageRole::System, "Connecting...".into());
        cx.notify();

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        // Build args based on permission mode
        let extra_args: Vec<&str> = match self.permission_mode {
            PermissionMode::Default => vec![],
            PermissionMode::BypassPermissions => vec!["--dangerously-skip-permissions"],
            PermissionMode::PlanMode => vec!["--plan"],
        };

        match ClaudeConnection::connect_with_args(&cwd, &extra_args) {
            Ok((conn, update_rx)) => {
                self.connection = Some(Arc::new(conn));
                self.update_rx = Some(update_rx);
                self.skip_first_response = true;
                self.start_update_polling(cx);
                cx.notify();
            }
            Err(e) => {
                self.connection_state = AgentConnectionState::Error(e.to_string());
                self.add_message(MessageRole::System, format!("Failed: {}", e));
                cx.notify();
            }
        }
    }

    fn cycle_permission_mode(&mut self, cx: &mut Context<Self>) {
        // Only allow changing mode when disconnected
        if self.connection_state != AgentConnectionState::Disconnected {
            return;
        }
        self.permission_mode = match self.permission_mode {
            PermissionMode::Default => PermissionMode::BypassPermissions,
            PermissionMode::BypassPermissions => PermissionMode::PlanMode,
            PermissionMode::PlanMode => PermissionMode::Default,
        };
        cx.notify();
    }

    fn permission_mode_label(&self) -> &'static str {
        match self.permission_mode {
            PermissionMode::Default => "Default",
            PermissionMode::BypassPermissions => "Bypass",
            PermissionMode::PlanMode => "Plan",
        }
    }

    fn disconnect(&mut self, cx: &mut Context<Self>) {
        if let Some(conn) = self.connection.take() {
            conn.disconnect();
        }
        self.session_info = None;
        self.update_rx = None;
        self.connection_state = AgentConnectionState::Disconnected;
        self.add_message(MessageRole::System, "Disconnected.".into());
        self.scroll_to_bottom(cx);
        cx.notify();
    }

    fn send_message(&mut self, cx: &mut Context<Self>) {
        let raw_content = self.input_field.read(cx).content().trim().to_string();
        if raw_content.is_empty() {
            return;
        }

        // Expand @terminal if present
        let content = if raw_content.contains("@terminal") {
            self.expand_terminal_context(&raw_content, cx)
        } else {
            raw_content.clone()
        };

        // Show user's original message (not expanded)
        self.add_message(MessageRole::User, raw_content);
        self.input_field.update(cx, |f, _| f.set_content(""));
        self.scroll_to_bottom(cx);
        cx.notify();

        // Send expanded content to Claude
        if let Some(conn) = self.connection.clone() {
            if let Err(e) = conn.send_message(&content) {
                self.add_message(MessageRole::System, format!("Error: {}", e));
                self.scroll_to_bottom(cx);
                cx.notify();
            }
        } else {
            self.add_message(MessageRole::System, "Not connected".into());
            self.scroll_to_bottom(cx);
            cx.notify();
        }
    }

    /// Expand @terminal mentions with actual terminal content
    fn expand_terminal_context(&self, content: &str, cx: &App) -> String {
        let terminal_content = cx.try_global::<AppState>()
            .and_then(|state| {
                let app = state.app.lock();
                app.active_tab().map(|tab| {
                    let terminal = tab.terminal.lock();
                    terminal.extract_last_lines(100)
                })
            });

        match terminal_content {
            Some(tc) => content.replace(
                "@terminal",
                &format!("<terminal_output>\n{}\n</terminal_output>", tc)
            ),
            None => content.replace("@terminal", "[No active terminal]"),
        }
    }

    fn start_update_polling(&mut self, cx: &mut Context<Self>) {
        let Some(update_rx) = self.update_rx.clone() else { return };

        cx.spawn(async move |this, cx| {
            loop {
                match update_rx.recv().await {
                    Ok(update) => {
                        let cont = this.update(cx, |this, cx| {
                            this.handle_update(update, cx);
                            this.connection.as_ref().map(|c| c.is_alive()).unwrap_or(false)
                        }).unwrap_or(false);
                        if !cont { break; }
                    }
                    Err(_) => {
                        this.update(cx, |this, cx| {
                            this.connection_state = AgentConnectionState::Disconnected;
                            this.add_message(MessageRole::System, "Connection closed.".into());
                            this.scroll_to_bottom(cx);
                            cx.notify();
                        }).ok();
                        break;
                    }
                }
            }
        }).detach();
    }

    fn handle_update(&mut self, update: SessionUpdate, cx: &mut Context<Self>) {
        match update {
            SessionUpdate::SessionInit { session_id, model, tools } => {
                self.session_info = Some(SessionInfo {
                    session_id: session_id.clone(),
                    model: model.clone(),
                    tools: tools.clone(),
                });
                if let Some(conn) = &self.connection {
                    conn.set_session_info(SessionInfo { session_id, model: model.clone(), tools });
                }
                self.connection_state = AgentConnectionState::Connected;
                // Replace "Connecting..." with "Connected" but keep any user messages
                if let Some(first) = self.messages.first_mut() {
                    if first.role == MessageRole::System && first.content == "Connecting..." {
                        first.content = format!("Connected ({})", model);
                    }
                } else {
                    self.add_message(MessageRole::System, format!("Connected ({})", model));
                }
                cx.notify();
            }
            SessionUpdate::AssistantText { text } => {
                if self.skip_first_response { return; }

                if let Some(last) = self.messages.last_mut() {
                    if last.role == MessageRole::Assistant {
                        last.content.push_str(&text);
                        self.scroll_to_bottom(cx);
                        cx.notify();
                        return;
                    }
                }
                self.add_message(MessageRole::Assistant, text);
                self.scroll_to_bottom(cx);
                cx.notify();
            }
            SessionUpdate::ToolUse { tool_id, tool_name, input } => {
                let title = format!("{}: {}",
                    tool_name,
                    input.get("command")
                        .or_else(|| input.get("file_path"))
                        .or_else(|| input.get("pattern"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("...")
                );
                self.pending_tool_calls.push(ToolCall {
                    tool_call_id: tool_id,
                    title,
                    kind: ToolKind::from(tool_name.as_str()),
                    status: ToolCallStatus::InProgress,
                    content: None,
                });
                self.scroll_to_bottom(cx);
                cx.notify();
            }
            SessionUpdate::MessageComplete { .. } => {
                if self.skip_first_response {
                    self.skip_first_response = false;
                    return;
                }
                self.pending_tool_calls.clear();
                self.scroll_to_bottom(cx);
                cx.notify();
            }
            SessionUpdate::Error { message } => {
                self.add_message(MessageRole::System, format!("Error: {}", message));
                self.scroll_to_bottom(cx);
                cx.notify();
            }
        }
    }
}

impl Render for AgentPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_connected = self.connection_state == AgentConnectionState::Connected;
        let messages = self.messages.clone();
        let tool_calls = self.pending_tool_calls.clone();

        div()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .h_full()
            .w(px(360.0))
            .bg(rgb(0x1e1e2e))
            .border_l_1()
            .border_color(rgb(0x313244))
            // Header
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(0x313244))
                    .child(
                        div().flex().items_center().gap_2()
                            .child(
                                div().id("collapse").px_1().cursor_pointer().text_xs()
                                    .text_color(rgb(0x6c7086))
                                    .hover(|s| s.text_color(rgb(0xcdd6f4)))
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(AgentPanelEvent::ToggleVisibility)))
                                    .child("\u{25B6}")
                            )
                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0xcdd6f4)).child("Claude"))
                            .child(
                                div().w(px(8.0)).h(px(8.0)).rounded_full()
                                    .bg(match &self.connection_state {
                                        AgentConnectionState::Connected => rgb(0xa6e3a1),
                                        AgentConnectionState::Connecting => rgb(0xf9e2af),
                                        AgentConnectionState::Disconnected => rgb(0x6c7086),
                                        AgentConnectionState::Error(_) => rgb(0xf38ba8),
                                    })
                            )
                    )
                    .child(
                        div().flex().items_center().gap_2()
                            // Permission mode selector (only clickable when disconnected)
                            .child(
                                div().id("mode").px_2().py_1().rounded_sm().text_xs()
                                    .when(!is_connected && self.connection_state != AgentConnectionState::Connecting, |el| {
                                        el.cursor_pointer()
                                            .bg(match self.permission_mode {
                                                PermissionMode::Default => rgb(0x6c7086),
                                                PermissionMode::BypassPermissions => rgb(0xf9e2af),
                                                PermissionMode::PlanMode => rgb(0x89b4fa),
                                            })
                                            .text_color(rgb(0x1e1e2e))
                                            .hover(|s| s.opacity(0.8))
                                            .on_click(cx.listener(|this, _, _, cx| this.cycle_permission_mode(cx)))
                                    })
                                    .when(is_connected || self.connection_state == AgentConnectionState::Connecting, |el| {
                                        el.bg(rgb(0x313244))
                                            .text_color(rgb(0x9399b2))
                                    })
                                    .child(self.permission_mode_label())
                            )
                            .when_some(self.session_info.as_ref(), |el, info| {
                                el.child(
                                    div().px_2().py_1().rounded_sm().bg(rgb(0x313244))
                                        .text_xs().text_color(rgb(0x9399b2))
                                        .child(info.model.split('-').last().unwrap_or(&info.model).to_string())
                                )
                            })
                            .child(
                                div().id("connect").px_2().py_1().rounded_sm().cursor_pointer().text_xs()
                                    .when(is_connected, |el| {
                                        el.bg(rgb(0xf38ba8)).text_color(rgb(0x1e1e2e))
                                            .hover(|s| s.bg(rgb(0xeba0ac)))
                                            .on_click(cx.listener(|this, _, _, cx| this.disconnect(cx)))
                                            .child("Disconnect")
                                    })
                                    .when(!is_connected, |el| {
                                        el.bg(rgb(0xa6e3a1)).text_color(rgb(0x1e1e2e))
                                            .hover(|s| s.bg(rgb(0x94e2d5)))
                                            .on_click(cx.listener(|this, _, _, cx| this.connect(cx)))
                                            .child("Connect")
                                    })
                            )
                    )
            )
            // Messages (scrollable)
            .child(
                div()
                    .id("messages-container")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_handle)
                    .p_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .children(messages.iter().map(|msg| {
                                let (bg, tc, right) = match msg.role {
                                    MessageRole::User => (rgb(0x45475a), rgb(0xcdd6f4), true),
                                    MessageRole::Assistant => (rgb(0x313244), rgb(0xcdd6f4), false),
                                    MessageRole::System => (rgb(0x1e1e2e), rgb(0x6c7086), false),
                                };
                                let label = match msg.role {
                                    MessageRole::User => "You",
                                    MessageRole::Assistant => "Claude",
                                    MessageRole::System => "",
                                };
                                let msg_id = ElementId::Name(format!("msg-{}", msg.id).into());
                                div()
                                    .id(msg_id)
                                    .w_full()
                                    .flex()
                                    .when(right, |e| e.flex_row_reverse())
                                    .child(
                                        div()
                                            .max_w(px(280.0))
                                            .bg(bg)
                                            .rounded_md()
                                            .px_3()
                                            .py_2()
                                            .when(!label.is_empty(), |e| {
                                                e.child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(0x6c7086))
                                                        .mb_1()
                                                        .child(label)
                                                )
                                            })
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(tc)
                                                    .child(msg.content.clone())
                                            )
                                    )
                            }))
                    )
            )
            // Tool calls
            .when(!tool_calls.is_empty(), |el| {
                el.child(
                    div().px_3().py_2().border_t_1().border_color(rgb(0x313244))
                        .children(tool_calls.iter().map(|tc| {
                            div().flex().items_center().gap_2().py_1()
                                .child(div().w(px(8.0)).h(px(8.0)).rounded_full().bg(rgb(0x89b4fa)))
                                .child(div().flex_1().text_xs().text_color(rgb(0xcdd6f4)).overflow_hidden().child(tc.title.clone()))
                        }))
                )
            })
            // Input
            .child(
                div().flex().items_center().gap_2().px_3().py_2().border_t_1().border_color(rgb(0x313244))
                    .when(is_connected, |el| {
                        el.child(div().flex_1().child(self.input_field.clone()))
                          .child(
                              div().id("send").px_3().py_1().rounded_md().cursor_pointer()
                                  .bg(rgb(0x89b4fa)).text_color(rgb(0x1e1e2e)).text_sm()
                                  .hover(|s| s.bg(rgb(0xb4befe)))
                                  .on_click(cx.listener(|this, _, _, cx| this.send_message(cx)))
                                  .child("Send")
                          )
                    })
                    .when(!is_connected, |el| {
                        el.child(div().flex_1().text_sm().text_color(rgb(0x6c7086)).child("Connect to chat"))
                    })
            )
    }
}

pub fn agent_panel(cx: &mut App) -> Entity<AgentPanel> {
    cx.new(|cx| AgentPanel::new(cx))
}
