//! Claude Code CLI Streaming JSON Protocol
//!
//! Protocol types for communicating with Claude Code CLI via stream-json format.
//! Run with: claude --print --input-format stream-json --output-format stream-json --verbose

use serde::{Deserialize, Serialize};

// ============================================================================
// Input Messages (sent to Claude CLI via stdin)
// ============================================================================

/// User message input
#[derive(Debug, Clone, Serialize)]
pub struct UserInput {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub message: UserMessage,
}

impl UserInput {
    pub fn new(content: &str) -> Self {
        Self {
            msg_type: "user".into(),
            message: UserMessage {
                role: "user".into(),
                content: content.into(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UserMessage {
    pub role: String,
    pub content: String,
}

// ============================================================================
// Output Messages (received from Claude CLI via stdout)
// ============================================================================

/// Generic output message from Claude CLI
#[derive(Debug, Clone, Deserialize)]
pub struct OutputMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub message: Option<AssistantMessage>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub is_error: Option<bool>,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
}

/// Assistant message from Claude
#[derive(Debug, Clone, Deserialize)]
pub struct AssistantMessage {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub stop_reason: Option<String>,
}

/// Content block in an assistant message
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
    },
    #[serde(other)]
    Unknown,
}

// ============================================================================
// Session Updates (for UI)
// ============================================================================

/// Session update events for the UI
#[derive(Debug, Clone)]
pub enum SessionUpdate {
    /// Initial session info received
    SessionInit {
        session_id: String,
        model: String,
        tools: Vec<String>,
    },
    /// Text chunk from assistant
    AssistantText { text: String },
    /// Tool use from assistant
    ToolUse {
        tool_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    /// Message complete with result
    MessageComplete { result: String },
    /// Error occurred
    Error { message: String },
}

/// Tool call for display in UI
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub tool_call_id: String,
    pub title: String,
    pub kind: ToolKind,
    pub status: ToolCallStatus,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Bash,
    Execute,
    Read,
    Edit,
    Create,
    Search,
    Web,
    Unknown,
}

impl From<&str> for ToolKind {
    fn from(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "bash" => ToolKind::Bash,
            "execute" | "task" => ToolKind::Execute,
            "read" => ToolKind::Read,
            "edit" => ToolKind::Edit,
            "write" => ToolKind::Create,
            "glob" | "grep" => ToolKind::Search,
            "webfetch" | "websearch" => ToolKind::Web,
            _ => ToolKind::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallStatus {
    Pending,
    WaitingForConfirmation,
    InProgress,
    Completed,
    Failed,
}

/// Parse an output message into session updates
pub fn parse_output_message(msg: &OutputMessage) -> Vec<SessionUpdate> {
    let mut updates = Vec::new();

    match msg.msg_type.as_str() {
        "system" if msg.subtype.as_deref() == Some("init") => {
            updates.push(SessionUpdate::SessionInit {
                session_id: msg.session_id.clone().unwrap_or_default(),
                model: msg.model.clone().unwrap_or_default(),
                tools: msg.tools.clone().unwrap_or_default(),
            });
        }
        "assistant" => {
            if let Some(message) = &msg.message {
                for block in &message.content {
                    match block {
                        ContentBlock::Text { text } => {
                            updates.push(SessionUpdate::AssistantText { text: text.clone() });
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            updates.push(SessionUpdate::ToolUse {
                                tool_id: id.clone(),
                                tool_name: name.clone(),
                                input: input.clone(),
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
        "result" => {
            if msg.is_error == Some(true) {
                updates.push(SessionUpdate::Error {
                    message: msg.result.clone().unwrap_or_else(|| "Unknown error".into()),
                });
            } else {
                updates.push(SessionUpdate::MessageComplete {
                    result: msg.result.clone().unwrap_or_default(),
                });
            }
        }
        _ => {}
    }

    updates
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // UserInput Serialization Tests
    // ============================================================================

    #[test]
    fn test_user_input_serialization() {
        let input = UserInput::new("Hello");
        let json = serde_json::to_string(&input).unwrap();
        assert!(json.contains("\"type\":\"user\""));
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"Hello\""));
    }

    #[test]
    fn test_user_input_exact_json_format() {
        let input = UserInput::new("test message");
        let json = serde_json::to_string(&input).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "user");
        assert_eq!(parsed["message"]["role"], "user");
        assert_eq!(parsed["message"]["content"], "test message");
    }

    #[test]
    fn test_user_input_with_special_characters() {
        let input = UserInput::new("Hello \"world\" with\nnewlines\tand tabs");
        let json = serde_json::to_string(&input).unwrap();
        // Should serialize without error and be valid JSON
        let _: serde_json::Value = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn test_user_input_with_unicode() {
        let input = UserInput::new("Hello 世界 🌍 émoji");
        let json = serde_json::to_string(&input).unwrap();
        assert!(json.contains("世界"));
        assert!(json.contains("🌍"));
    }

    #[test]
    fn test_user_input_empty_content() {
        let input = UserInput::new("");
        let json = serde_json::to_string(&input).unwrap();
        assert!(json.contains("\"content\":\"\""));
    }

    // ============================================================================
    // OutputMessage Init Tests
    // ============================================================================

    #[test]
    fn test_output_message_init() {
        let json = r#"{"type":"system","subtype":"init","session_id":"abc","model":"claude","tools":["Read","Write"]}"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.msg_type, "system");
        assert_eq!(msg.subtype, Some("init".into()));

        let updates = parse_output_message(&msg);
        assert_eq!(updates.len(), 1);
        match &updates[0] {
            SessionUpdate::SessionInit { session_id, model, tools } => {
                assert_eq!(session_id, "abc");
                assert_eq!(model, "claude");
                assert_eq!(tools.len(), 2);
            }
            _ => panic!("Expected SessionInit"),
        }
    }

    #[test]
    fn test_real_init_message() {
        // Realistic init message from Claude CLI
        let json = r#"{
            "type": "system",
            "subtype": "init",
            "session_id": "session-abc123def456",
            "model": "claude-sonnet-4-20250514",
            "tools": ["Read", "Write", "Edit", "Bash", "Glob", "Grep", "WebFetch", "WebSearch", "Task"],
            "cwd": "/Users/test/project"
        }"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        assert_eq!(msg.msg_type, "system");
        assert_eq!(msg.subtype.as_deref(), Some("init"));
        assert_eq!(msg.session_id.as_deref(), Some("session-abc123def456"));
        assert_eq!(msg.model.as_deref(), Some("claude-sonnet-4-20250514"));
        assert_eq!(msg.cwd.as_deref(), Some("/Users/test/project"));
        assert_eq!(msg.tools.as_ref().unwrap().len(), 9);

        let updates = parse_output_message(&msg);
        assert_eq!(updates.len(), 1);
        match &updates[0] {
            SessionUpdate::SessionInit { session_id, model, tools } => {
                assert_eq!(session_id, "session-abc123def456");
                assert_eq!(model, "claude-sonnet-4-20250514");
                assert!(tools.contains(&"Bash".to_string()));
            }
            _ => panic!("Expected SessionInit"),
        }
    }

    #[test]
    fn test_init_with_empty_tools() {
        let json = r#"{"type":"system","subtype":"init","session_id":"test","model":"claude","tools":[]}"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        let updates = parse_output_message(&msg);
        match &updates[0] {
            SessionUpdate::SessionInit { tools, .. } => {
                assert!(tools.is_empty());
            }
            _ => panic!("Expected SessionInit"),
        }
    }

    #[test]
    fn test_init_with_missing_optional_fields() {
        // Minimal init message - only required fields
        let json = r#"{"type":"system","subtype":"init"}"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        assert_eq!(msg.session_id, None);
        assert_eq!(msg.model, None);
        assert_eq!(msg.tools, None);

        let updates = parse_output_message(&msg);
        match &updates[0] {
            SessionUpdate::SessionInit { session_id, model, tools } => {
                assert_eq!(session_id, "");
                assert_eq!(model, "");
                assert!(tools.is_empty());
            }
            _ => panic!("Expected SessionInit"),
        }
    }

    // ============================================================================
    // OutputMessage Assistant Tests
    // ============================================================================

    #[test]
    fn test_output_message_assistant() {
        let json = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello!"}]}}"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.msg_type, "assistant");

        let updates = parse_output_message(&msg);
        assert_eq!(updates.len(), 1);
        match &updates[0] {
            SessionUpdate::AssistantText { text } => {
                assert_eq!(text, "Hello!");
            }
            _ => panic!("Expected AssistantText"),
        }
    }

    #[test]
    fn test_multiple_content_blocks() {
        // Message with text + tool_use + text
        let json = r#"{
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "text", "text": "Let me check that file."},
                    {"type": "tool_use", "id": "tool_123", "name": "Read", "input": {"file_path": "/test.txt"}},
                    {"type": "text", "text": "Done reading."}
                ]
            }
        }"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        let updates = parse_output_message(&msg);
        assert_eq!(updates.len(), 3);

        match &updates[0] {
            SessionUpdate::AssistantText { text } => {
                assert_eq!(text, "Let me check that file.");
            }
            _ => panic!("Expected AssistantText for first block"),
        }

        match &updates[1] {
            SessionUpdate::ToolUse { tool_id, tool_name, input } => {
                assert_eq!(tool_id, "tool_123");
                assert_eq!(tool_name, "Read");
                assert_eq!(input["file_path"], "/test.txt");
            }
            _ => panic!("Expected ToolUse for second block"),
        }

        match &updates[2] {
            SessionUpdate::AssistantText { text } => {
                assert_eq!(text, "Done reading.");
            }
            _ => panic!("Expected AssistantText for third block"),
        }
    }

    #[test]
    fn test_tool_use_with_complex_input() {
        let json = r#"{
            "type": "assistant",
            "message": {
                "content": [
                    {
                        "type": "tool_use",
                        "id": "tool_456",
                        "name": "Edit",
                        "input": {
                            "file_path": "/src/main.rs",
                            "old_string": "fn main() {}",
                            "new_string": "fn main() {\n    println!(\"Hello\");\n}",
                            "replace_all": false
                        }
                    }
                ]
            }
        }"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        let updates = parse_output_message(&msg);
        assert_eq!(updates.len(), 1);

        match &updates[0] {
            SessionUpdate::ToolUse { tool_name, input, .. } => {
                assert_eq!(tool_name, "Edit");
                assert_eq!(input["file_path"], "/src/main.rs");
                assert_eq!(input["replace_all"], false);
            }
            _ => panic!("Expected ToolUse"),
        }
    }

    #[test]
    fn test_tool_use_with_nested_json_input() {
        let json = r#"{
            "type": "assistant",
            "message": {
                "content": [
                    {
                        "type": "tool_use",
                        "id": "tool_789",
                        "name": "Bash",
                        "input": {
                            "command": "echo '{\"key\": \"value\"}'",
                            "timeout": 30000
                        }
                    }
                ]
            }
        }"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        let updates = parse_output_message(&msg);
        match &updates[0] {
            SessionUpdate::ToolUse { input, .. } => {
                assert!(input["command"].as_str().unwrap().contains("key"));
            }
            _ => panic!("Expected ToolUse"),
        }
    }

    #[test]
    fn test_real_assistant_message_with_tool_use() {
        // Realistic message from Claude CLI
        let json = r#"{
            "type": "assistant",
            "message": {
                "id": "msg_01XYZ",
                "model": "claude-sonnet-4-20250514",
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "I'll read the configuration file to understand the project structure."},
                    {
                        "type": "tool_use",
                        "id": "toolu_01ABC",
                        "name": "Read",
                        "input": {"file_path": "/Users/test/project/Cargo.toml"}
                    }
                ],
                "stop_reason": "tool_use"
            }
        }"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        let message = msg.message.as_ref().unwrap();
        assert_eq!(message.id.as_deref(), Some("msg_01XYZ"));
        assert_eq!(message.model.as_deref(), Some("claude-sonnet-4-20250514"));
        assert_eq!(message.stop_reason.as_deref(), Some("tool_use"));

        let updates = parse_output_message(&msg);
        assert_eq!(updates.len(), 2);
    }

    #[test]
    fn test_streaming_text_chunks() {
        // Simulate multiple streaming text updates
        let chunks = vec![
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":" world"}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"!"}]}}"#,
        ];

        let mut full_text = String::new();
        for chunk_json in chunks {
            let msg: OutputMessage = serde_json::from_str(chunk_json).unwrap();
            let updates = parse_output_message(&msg);
            for update in updates {
                if let SessionUpdate::AssistantText { text } = update {
                    full_text.push_str(&text);
                }
            }
        }

        assert_eq!(full_text, "Hello world!");
    }

    #[test]
    fn test_empty_content_array() {
        let json = r#"{"type":"assistant","message":{"content":[]}}"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        let updates = parse_output_message(&msg);
        assert!(updates.is_empty());
    }

    #[test]
    fn test_assistant_message_without_message_field() {
        let json = r#"{"type":"assistant"}"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        let updates = parse_output_message(&msg);
        assert!(updates.is_empty());
    }

    // ============================================================================
    // ContentBlock Parsing Tests
    // ============================================================================

    #[test]
    fn test_tool_result_parsing() {
        let json = r#"{
            "type": "assistant",
            "message": {
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01ABC",
                        "content": "File contents here..."
                    }
                ]
            }
        }"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        // tool_result should parse but not create an update (handled differently)
        let updates = parse_output_message(&msg);
        assert!(updates.is_empty()); // Our parser ignores tool_result blocks
    }

    #[test]
    fn test_tool_result_with_structured_content() {
        let json = r#"{
            "type": "assistant",
            "message": {
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01ABC",
                        "content": {"status": "success", "output": "done"}
                    }
                ]
            }
        }"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();
        let message = msg.message.as_ref().unwrap();

        match &message.content[0] {
            ContentBlock::ToolResult { tool_use_id, content } => {
                assert_eq!(tool_use_id, "toolu_01ABC");
                assert_eq!(content["status"], "success");
            }
            _ => panic!("Expected ToolResult"),
        }
    }

    #[test]
    fn test_unknown_content_block_type() {
        // Unknown type should deserialize to Unknown variant, not panic
        let json = r#"{
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "text", "text": "Hello"},
                    {"type": "future_new_type", "some_field": "value"},
                    {"type": "text", "text": "World"}
                ]
            }
        }"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();
        let message = msg.message.as_ref().unwrap();

        assert_eq!(message.content.len(), 3);
        assert!(matches!(message.content[0], ContentBlock::Text { .. }));
        assert!(matches!(message.content[1], ContentBlock::Unknown));
        assert!(matches!(message.content[2], ContentBlock::Text { .. }));

        // Unknown blocks should be skipped in updates
        let updates = parse_output_message(&msg);
        assert_eq!(updates.len(), 2); // Only the two text blocks
    }

    // ============================================================================
    // OutputMessage Result Tests
    // ============================================================================

    #[test]
    fn test_output_message_result() {
        let json = r#"{"type":"result","subtype":"success","is_error":false,"result":"Done!"}"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.msg_type, "result");

        let updates = parse_output_message(&msg);
        assert_eq!(updates.len(), 1);
        match &updates[0] {
            SessionUpdate::MessageComplete { result } => {
                assert_eq!(result, "Done!");
            }
            _ => panic!("Expected MessageComplete"),
        }
    }

    #[test]
    fn test_output_message_error() {
        let json = r#"{"type":"result","is_error":true,"result":"Something went wrong"}"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        let updates = parse_output_message(&msg);
        assert_eq!(updates.len(), 1);
        match &updates[0] {
            SessionUpdate::Error { message } => {
                assert_eq!(message, "Something went wrong");
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_error_without_result_field() {
        let json = r#"{"type":"result","is_error":true}"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        let updates = parse_output_message(&msg);
        match &updates[0] {
            SessionUpdate::Error { message } => {
                assert_eq!(message, "Unknown error");
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_result_with_cost_tracking() {
        let json = r#"{"type":"result","is_error":false,"result":"Complete","total_cost_usd":0.0123}"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        assert_eq!(msg.total_cost_usd, Some(0.0123));
    }

    #[test]
    fn test_result_with_empty_result_string() {
        let json = r#"{"type":"result","is_error":false,"result":""}"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        let updates = parse_output_message(&msg);
        match &updates[0] {
            SessionUpdate::MessageComplete { result } => {
                assert_eq!(result, "");
            }
            _ => panic!("Expected MessageComplete"),
        }
    }

    // ============================================================================
    // Unknown/Malformed Message Tests
    // ============================================================================

    #[test]
    fn test_unknown_message_type() {
        let json = r#"{"type":"future_type","data":"some value"}"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        // Should parse without error
        assert_eq!(msg.msg_type, "future_type");

        // Should produce no updates
        let updates = parse_output_message(&msg);
        assert!(updates.is_empty());
    }

    #[test]
    fn test_system_message_without_init_subtype() {
        let json = r#"{"type":"system","subtype":"status","data":"running"}"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        // Should not produce SessionInit
        let updates = parse_output_message(&msg);
        assert!(updates.is_empty());
    }

    #[test]
    fn test_minimal_message() {
        // Absolute minimum valid message
        let json = r#"{"type":"unknown"}"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.msg_type, "unknown");
        assert!(msg.subtype.is_none());
        assert!(msg.session_id.is_none());
        assert!(msg.message.is_none());
    }

    #[test]
    fn test_extra_fields_ignored() {
        // JSON with extra fields should still parse
        let json = r#"{
            "type": "assistant",
            "unknown_field": "value",
            "another_field": 123,
            "message": {
                "content": [{"type": "text", "text": "Hello"}],
                "extra_message_field": true
            }
        }"#;
        let msg: OutputMessage = serde_json::from_str(json).unwrap();

        let updates = parse_output_message(&msg);
        assert_eq!(updates.len(), 1);
    }

    // ============================================================================
    // ToolKind Tests
    // ============================================================================

    #[test]
    fn test_tool_kind_from_str() {
        assert_eq!(ToolKind::from("Bash"), ToolKind::Bash);
        assert_eq!(ToolKind::from("Read"), ToolKind::Read);
        assert_eq!(ToolKind::from("Edit"), ToolKind::Edit);
        assert_eq!(ToolKind::from("Write"), ToolKind::Create);
        assert_eq!(ToolKind::from("Glob"), ToolKind::Search);
        assert_eq!(ToolKind::from("WebFetch"), ToolKind::Web);
        assert_eq!(ToolKind::from("Unknown"), ToolKind::Unknown);
    }

    #[test]
    fn test_tool_kind_case_insensitive() {
        assert_eq!(ToolKind::from("bash"), ToolKind::Bash);
        assert_eq!(ToolKind::from("BASH"), ToolKind::Bash);
        assert_eq!(ToolKind::from("BaSh"), ToolKind::Bash);
    }

    #[test]
    fn test_tool_kind_all_mappings() {
        assert_eq!(ToolKind::from("bash"), ToolKind::Bash);
        assert_eq!(ToolKind::from("execute"), ToolKind::Execute);
        assert_eq!(ToolKind::from("task"), ToolKind::Execute);
        assert_eq!(ToolKind::from("read"), ToolKind::Read);
        assert_eq!(ToolKind::from("edit"), ToolKind::Edit);
        assert_eq!(ToolKind::from("write"), ToolKind::Create);
        assert_eq!(ToolKind::from("glob"), ToolKind::Search);
        assert_eq!(ToolKind::from("grep"), ToolKind::Search);
        assert_eq!(ToolKind::from("webfetch"), ToolKind::Web);
        assert_eq!(ToolKind::from("websearch"), ToolKind::Web);
    }

    // ============================================================================
    // ToolCallStatus Tests
    // ============================================================================

    #[test]
    fn test_tool_call_status_equality() {
        assert_eq!(ToolCallStatus::Pending, ToolCallStatus::Pending);
        assert_ne!(ToolCallStatus::Pending, ToolCallStatus::Completed);
    }

    #[test]
    fn test_tool_call_construction() {
        let tool_call = ToolCall {
            tool_call_id: "test_id".into(),
            title: "Read file".into(),
            kind: ToolKind::Read,
            status: ToolCallStatus::Pending,
            content: Some("file.txt".into()),
        };

        assert_eq!(tool_call.tool_call_id, "test_id");
        assert_eq!(tool_call.kind, ToolKind::Read);
        assert_eq!(tool_call.status, ToolCallStatus::Pending);
    }

    // ============================================================================
    // SessionUpdate Tests
    // ============================================================================

    #[test]
    fn test_session_update_debug_impl() {
        let update = SessionUpdate::AssistantText { text: "Hello".into() };
        let debug = format!("{:?}", update);
        assert!(debug.contains("AssistantText"));
        assert!(debug.contains("Hello"));
    }

    #[test]
    fn test_session_update_clone() {
        let update = SessionUpdate::ToolUse {
            tool_id: "id".into(),
            tool_name: "Read".into(),
            input: serde_json::json!({"path": "/test"}),
        };
        let cloned = update.clone();

        match cloned {
            SessionUpdate::ToolUse { tool_id, tool_name, input } => {
                assert_eq!(tool_id, "id");
                assert_eq!(tool_name, "Read");
                assert_eq!(input["path"], "/test");
            }
            _ => panic!("Clone should preserve variant"),
        }
    }
}
