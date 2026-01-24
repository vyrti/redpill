//! Integration tests for redpill-agent
//!
//! These tests verify the full protocol flow with realistic Claude CLI output.
//! They test the public API and realistic message sequences.

use redpill_agent::protocol::{
    parse_output_message, ContentBlock, OutputMessage, SessionUpdate, ToolCall, ToolCallStatus,
    ToolKind, UserInput,
};
use redpill_agent::connection::SessionInfo;

// ============================================================================
// Full Conversation Flow Tests
// ============================================================================

/// Test a realistic init -> assistant message -> result flow
#[test]
fn test_full_conversation_flow() {
    // Step 1: Init message
    let init_json = r#"{
        "type": "system",
        "subtype": "init",
        "session_id": "session-test-123",
        "model": "claude-sonnet-4-20250514",
        "tools": ["Read", "Write", "Edit", "Bash"],
        "cwd": "/Users/test/project"
    }"#;

    let init_msg: OutputMessage = serde_json::from_str(init_json).unwrap();
    let init_updates = parse_output_message(&init_msg);

    assert_eq!(init_updates.len(), 1);
    let session_info = match &init_updates[0] {
        SessionUpdate::SessionInit {
            session_id,
            model,
            tools,
        } => {
            assert_eq!(session_id, "session-test-123");
            assert_eq!(model, "claude-sonnet-4-20250514");
            assert_eq!(tools.len(), 4);
            SessionInfo {
                session_id: session_id.clone(),
                model: model.clone(),
                tools: tools.clone(),
            }
        }
        _ => panic!("Expected SessionInit"),
    };

    // Step 2: Send user message (just verify serialization)
    let user_input = UserInput::new("Hello, Claude!");
    let user_json = serde_json::to_string(&user_input).unwrap();
    assert!(user_json.contains("Hello, Claude!"));

    // Step 3: Receive assistant response
    let assistant_json = r#"{
        "type": "assistant",
        "message": {
            "id": "msg_01ABC",
            "model": "claude-sonnet-4-20250514",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello! I'm Claude. How can I help you today?"}
            ],
            "stop_reason": "end_turn"
        }
    }"#;

    let assistant_msg: OutputMessage = serde_json::from_str(assistant_json).unwrap();
    let assistant_updates = parse_output_message(&assistant_msg);

    assert_eq!(assistant_updates.len(), 1);
    match &assistant_updates[0] {
        SessionUpdate::AssistantText { text } => {
            assert!(text.contains("Hello"));
            assert!(text.contains("Claude"));
        }
        _ => panic!("Expected AssistantText"),
    }

    // Step 4: Result message
    let result_json = r#"{
        "type": "result",
        "subtype": "success",
        "is_error": false,
        "result": "Task completed successfully",
        "total_cost_usd": 0.0042
    }"#;

    let result_msg: OutputMessage = serde_json::from_str(result_json).unwrap();
    let result_updates = parse_output_message(&result_msg);

    assert_eq!(result_updates.len(), 1);
    match &result_updates[0] {
        SessionUpdate::MessageComplete { result } => {
            assert!(result.contains("completed"));
        }
        _ => panic!("Expected MessageComplete"),
    }

    // Verify session info was captured correctly
    assert_eq!(session_info.session_id, "session-test-123");
    assert!(session_info.tools.contains(&"Bash".to_string()));
}

/// Test conversation with tool use
#[test]
fn test_conversation_with_tool_use() {
    // User asks to read a file
    let user_input = UserInput::new("Can you read the Cargo.toml file?");
    let _ = serde_json::to_string(&user_input).unwrap();

    // Claude responds with tool use
    let response_json = r#"{
        "type": "assistant",
        "message": {
            "content": [
                {"type": "text", "text": "I'll read the Cargo.toml file for you."},
                {
                    "type": "tool_use",
                    "id": "toolu_01XYZ",
                    "name": "Read",
                    "input": {"file_path": "Cargo.toml"}
                }
            ],
            "stop_reason": "tool_use"
        }
    }"#;

    let msg: OutputMessage = serde_json::from_str(response_json).unwrap();
    let updates = parse_output_message(&msg);

    assert_eq!(updates.len(), 2);

    // First: text explanation
    match &updates[0] {
        SessionUpdate::AssistantText { text } => {
            assert!(text.contains("read"));
            assert!(text.contains("Cargo.toml"));
        }
        _ => panic!("Expected AssistantText"),
    }

    // Second: tool use
    match &updates[1] {
        SessionUpdate::ToolUse {
            tool_id,
            tool_name,
            input,
        } => {
            assert_eq!(tool_id, "toolu_01XYZ");
            assert_eq!(tool_name, "Read");
            assert_eq!(input["file_path"], "Cargo.toml");
        }
        _ => panic!("Expected ToolUse"),
    }
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_error_response_handling() {
    let error_json = r#"{
        "type": "result",
        "subtype": "error",
        "is_error": true,
        "result": "API rate limit exceeded. Please try again in 60 seconds."
    }"#;

    let msg: OutputMessage = serde_json::from_str(error_json).unwrap();
    let updates = parse_output_message(&msg);

    assert_eq!(updates.len(), 1);
    match &updates[0] {
        SessionUpdate::Error { message } => {
            assert!(message.contains("rate limit"));
        }
        _ => panic!("Expected Error"),
    }
}

#[test]
fn test_malformed_json_handling() {
    // This simulates what happens when we receive non-JSON output
    let result = serde_json::from_str::<OutputMessage>("not valid json");
    assert!(result.is_err());
}

#[test]
fn test_partial_json_handling() {
    // Incomplete JSON (as might happen during streaming)
    let result = serde_json::from_str::<OutputMessage>(r#"{"type": "assistant""#);
    assert!(result.is_err());
}

// ============================================================================
// Session Info Extraction Tests
// ============================================================================

#[test]
fn test_session_info_extraction_from_init() {
    let init_json = r#"{
        "type": "system",
        "subtype": "init",
        "session_id": "sess_abc123",
        "model": "claude-opus-4-20250514",
        "tools": ["Read", "Write", "Edit", "Bash", "Glob", "Grep", "WebFetch", "WebSearch", "Task"],
        "cwd": "/home/user/project"
    }"#;

    let msg: OutputMessage = serde_json::from_str(init_json).unwrap();

    // Extract directly from message
    assert_eq!(msg.session_id.as_deref(), Some("sess_abc123"));
    assert_eq!(msg.model.as_deref(), Some("claude-opus-4-20250514"));
    assert_eq!(msg.cwd.as_deref(), Some("/home/user/project"));

    let tools = msg.tools.as_ref().unwrap();
    assert_eq!(tools.len(), 9);
    assert!(tools.contains(&"Task".to_string()));

    // Also verify via parse_output_message
    let updates = parse_output_message(&msg);
    match &updates[0] {
        SessionUpdate::SessionInit {
            session_id,
            model,
            tools,
        } => {
            // Create SessionInfo
            let info = SessionInfo {
                session_id: session_id.clone(),
                model: model.clone(),
                tools: tools.clone(),
            };
            assert_eq!(info.session_id, "sess_abc123");
            assert_eq!(info.model, "claude-opus-4-20250514");
        }
        _ => panic!("Expected SessionInit"),
    }
}

// ============================================================================
// Streaming Simulation Tests
// ============================================================================

#[test]
fn test_streaming_assistant_chunks() {
    // Simulate streaming response chunks
    let chunks = vec![
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Here's "}]}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"a simple "}]}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Rust function:\n\n```rust\nfn hello() {\n    println!(\"Hello!\");\n}\n```"}]}}"#,
    ];

    let mut full_response = String::new();

    for chunk_json in chunks {
        let msg: OutputMessage = serde_json::from_str(chunk_json).unwrap();
        let updates = parse_output_message(&msg);

        for update in updates {
            if let SessionUpdate::AssistantText { text } = update {
                full_response.push_str(&text);
            }
        }
    }

    assert!(full_response.contains("Here's a simple Rust function"));
    assert!(full_response.contains("```rust"));
    assert!(full_response.contains("println!"));
}

// ============================================================================
// Tool Kind and Status Tests
// ============================================================================

#[test]
fn test_tool_call_lifecycle() {
    // Create a pending tool call
    let mut tool_call = ToolCall {
        tool_call_id: "toolu_test".into(),
        title: "Reading file...".into(),
        kind: ToolKind::Read,
        status: ToolCallStatus::Pending,
        content: None,
    };

    assert_eq!(tool_call.status, ToolCallStatus::Pending);

    // Simulate state transitions
    tool_call.status = ToolCallStatus::InProgress;
    assert_eq!(tool_call.status, ToolCallStatus::InProgress);

    tool_call.status = ToolCallStatus::Completed;
    tool_call.content = Some("File contents here...".into());
    assert_eq!(tool_call.status, ToolCallStatus::Completed);
    assert!(tool_call.content.is_some());
}

#[test]
fn test_tool_kind_for_common_tools() {
    // Verify all commonly used Claude tools map correctly
    assert_eq!(ToolKind::from("Read"), ToolKind::Read);
    assert_eq!(ToolKind::from("Write"), ToolKind::Create);
    assert_eq!(ToolKind::from("Edit"), ToolKind::Edit);
    assert_eq!(ToolKind::from("Bash"), ToolKind::Bash);
    assert_eq!(ToolKind::from("Glob"), ToolKind::Search);
    assert_eq!(ToolKind::from("Grep"), ToolKind::Search);
    assert_eq!(ToolKind::from("WebFetch"), ToolKind::Web);
    assert_eq!(ToolKind::from("WebSearch"), ToolKind::Web);
    assert_eq!(ToolKind::from("Task"), ToolKind::Execute);
}

// ============================================================================
// Content Block Parsing Tests
// ============================================================================

#[test]
fn test_all_content_block_types() {
    let json = r#"{
        "type": "assistant",
        "message": {
            "content": [
                {"type": "text", "text": "Explanation text"},
                {"type": "tool_use", "id": "tool_1", "name": "Read", "input": {}},
                {"type": "tool_result", "tool_use_id": "tool_1", "content": "result"},
                {"type": "unknown_future_type", "data": "ignored"}
            ]
        }
    }"#;

    let msg: OutputMessage = serde_json::from_str(json).unwrap();
    let content = &msg.message.as_ref().unwrap().content;

    assert_eq!(content.len(), 4);
    assert!(matches!(content[0], ContentBlock::Text { .. }));
    assert!(matches!(content[1], ContentBlock::ToolUse { .. }));
    assert!(matches!(content[2], ContentBlock::ToolResult { .. }));
    assert!(matches!(content[3], ContentBlock::Unknown));
}

#[test]
fn test_content_block_text_extraction() {
    let json = r#"{
        "type": "assistant",
        "message": {
            "content": [
                {"type": "text", "text": "Line 1\nLine 2\nLine 3"}
            ]
        }
    }"#;

    let msg: OutputMessage = serde_json::from_str(json).unwrap();
    let content = &msg.message.as_ref().unwrap().content;

    if let ContentBlock::Text { text } = &content[0] {
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "Line 1");
    } else {
        panic!("Expected Text block");
    }
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[test]
fn test_empty_session_init() {
    let json = r#"{"type":"system","subtype":"init"}"#;
    let msg: OutputMessage = serde_json::from_str(json).unwrap();
    let updates = parse_output_message(&msg);

    // Should still produce a SessionInit with empty/default values
    assert_eq!(updates.len(), 1);
    match &updates[0] {
        SessionUpdate::SessionInit {
            session_id,
            model,
            tools,
        } => {
            assert!(session_id.is_empty());
            assert!(model.is_empty());
            assert!(tools.is_empty());
        }
        _ => panic!("Expected SessionInit"),
    }
}

#[test]
fn test_unicode_in_messages() {
    let json = r#"{
        "type": "assistant",
        "message": {
            "content": [
                {"type": "text", "text": "こんにちは! 🎉 مرحبا Привет 你好"}
            ]
        }
    }"#;

    let msg: OutputMessage = serde_json::from_str(json).unwrap();
    let updates = parse_output_message(&msg);

    match &updates[0] {
        SessionUpdate::AssistantText { text } => {
            assert!(text.contains("こんにちは"));
            assert!(text.contains("🎉"));
            assert!(text.contains("مرحبا"));
            assert!(text.contains("Привет"));
            assert!(text.contains("你好"));
        }
        _ => panic!("Expected AssistantText"),
    }
}

#[test]
fn test_very_long_text_content() {
    // Test with a large text block (simulating code output)
    let long_text = "x".repeat(100_000);
    let json = format!(
        r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{}"}}]}}}}"#,
        long_text
    );

    let msg: OutputMessage = serde_json::from_str(&json).unwrap();
    let updates = parse_output_message(&msg);

    match &updates[0] {
        SessionUpdate::AssistantText { text } => {
            assert_eq!(text.len(), 100_000);
        }
        _ => panic!("Expected AssistantText"),
    }
}

#[test]
fn test_special_characters_in_tool_input() {
    let json = r#"{
        "type": "assistant",
        "message": {
            "content": [
                {
                    "type": "tool_use",
                    "id": "tool_special",
                    "name": "Bash",
                    "input": {
                        "command": "echo 'Hello \"World\"' | grep -E '\\w+'"
                    }
                }
            ]
        }
    }"#;

    let msg: OutputMessage = serde_json::from_str(json).unwrap();
    let updates = parse_output_message(&msg);

    match &updates[0] {
        SessionUpdate::ToolUse { input, .. } => {
            let cmd = input["command"].as_str().unwrap();
            assert!(cmd.contains("Hello"));
            assert!(cmd.contains("grep"));
        }
        _ => panic!("Expected ToolUse"),
    }
}

// ============================================================================
// Real-World Message Samples
// ============================================================================

#[test]
fn test_realistic_code_generation_response() {
    let json = r#"{
        "type": "assistant",
        "message": {
            "id": "msg_realistic",
            "model": "claude-sonnet-4-20250514",
            "role": "assistant",
            "content": [
                {
                    "type": "text",
                    "text": "I'll create a simple Rust function for you:\n\n```rust\n/// Checks if a number is prime\npub fn is_prime(n: u64) -> bool {\n    if n < 2 {\n        return false;\n    }\n    for i in 2..=(n as f64).sqrt() as u64 {\n        if n % i == 0 {\n            return false;\n        }\n    }\n    true\n}\n```"
                }
            ],
            "stop_reason": "end_turn"
        }
    }"#;

    let msg: OutputMessage = serde_json::from_str(json).unwrap();
    let updates = parse_output_message(&msg);

    match &updates[0] {
        SessionUpdate::AssistantText { text } => {
            assert!(text.contains("```rust"));
            assert!(text.contains("is_prime"));
            assert!(text.contains("pub fn"));
        }
        _ => panic!("Expected AssistantText"),
    }
}

#[test]
fn test_realistic_multi_tool_response() {
    let json = r#"{
        "type": "assistant",
        "message": {
            "content": [
                {"type": "text", "text": "Let me check the project structure and then update the code."},
                {
                    "type": "tool_use",
                    "id": "toolu_glob",
                    "name": "Glob",
                    "input": {"pattern": "src/**/*.rs"}
                },
                {
                    "type": "tool_use",
                    "id": "toolu_read",
                    "name": "Read",
                    "input": {"file_path": "src/main.rs"}
                }
            ],
            "stop_reason": "tool_use"
        }
    }"#;

    let msg: OutputMessage = serde_json::from_str(json).unwrap();
    let updates = parse_output_message(&msg);

    // Should have 1 text + 2 tool uses = 3 updates
    assert_eq!(updates.len(), 3);

    // Verify order is preserved
    assert!(matches!(updates[0], SessionUpdate::AssistantText { .. }));
    assert!(matches!(updates[1], SessionUpdate::ToolUse { .. }));
    assert!(matches!(updates[2], SessionUpdate::ToolUse { .. }));

    // Verify tool details
    if let SessionUpdate::ToolUse { tool_name, .. } = &updates[1] {
        assert_eq!(tool_name, "Glob");
    }
    if let SessionUpdate::ToolUse { tool_name, .. } = &updates[2] {
        assert_eq!(tool_name, "Read");
    }
}
