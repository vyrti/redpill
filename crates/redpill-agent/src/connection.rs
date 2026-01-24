//! Claude Code CLI Connection
//!
//! Manages the connection to Claude Code CLI via stream-json format.
//! Uses std::process with a dedicated reader thread for GPUI compatibility.

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use async_channel::{Receiver, Sender};

use crate::protocol::{OutputMessage, SessionUpdate, UserInput, parse_output_message};

/// Error type for connection operations
#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Connection closed")]
    ConnectionClosed,
    #[error("Spawn error: {0}")]
    SpawnError(String),
}

pub type Result<T> = std::result::Result<T, ConnectionError>;

/// Session info received from init message
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub model: String,
    pub tools: Vec<String>,
}

/// Connection to Claude Code CLI
pub struct ClaudeConnection {
    /// The child process
    #[allow(dead_code)]
    child: Mutex<Child>,
    /// Stdin writer (sync)
    stdin: Mutex<BufWriter<ChildStdin>>,
    /// Whether the connection is alive
    alive: Arc<AtomicBool>,
    /// Session info
    session_info: Mutex<Option<SessionInfo>>,
}

impl ClaudeConnection {
    /// Connect to Claude Code CLI
    ///
    /// Spawns the claude CLI with stream-json mode and returns a receiver for updates.
    /// Extra args can be used to pass permission flags like "--dangerously-skip-permissions".
    pub fn connect(cwd: &Path) -> Result<(Self, Receiver<SessionUpdate>)> {
        Self::connect_with_args(cwd, &[])
    }

    /// Connect to Claude Code CLI with additional arguments
    ///
    /// Spawns the claude CLI with stream-json mode and extra args.
    pub fn connect_with_args(cwd: &Path, extra_args: &[&str]) -> Result<(Self, Receiver<SessionUpdate>)> {
        tracing::info!("Claude: spawning claude CLI in {:?} with extra args: {:?}", cwd, extra_args);

        let mut args = vec![
            "--print",
            "--input-format", "stream-json",
            "--output-format", "stream-json",
            "--verbose",
        ];
        args.extend(extra_args);

        // Spawn with stream-json format for bidirectional communication
        let mut child = Command::new("claude")
            .args(&args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ConnectionError::SpawnError(format!("Failed to spawn claude: {}", e)))?;

        tracing::info!("Claude: process spawned successfully");

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ConnectionError::SpawnError("Failed to get stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ConnectionError::SpawnError("Failed to get stdout".into()))?;

        let (update_tx, update_rx) = async_channel::bounded(100);
        let alive = Arc::new(AtomicBool::new(true));

        // Spawn reader thread
        let alive_clone = alive.clone();
        thread::spawn(move || {
            Self::reader_thread(stdout, update_tx, alive_clone);
        });

        let conn = Self {
            child: Mutex::new(child),
            stdin: Mutex::new(BufWriter::new(stdin)),
            alive,
            session_info: Mutex::new(None),
        };

        // Send a minimal "ping" message to trigger the init output
        // The CLI only outputs init after receiving the first user input
        conn.send_message(".")?;

        Ok((conn, update_rx))
    }

    /// Send a user message to Claude
    pub fn send_message(&self, content: &str) -> Result<()> {
        let input = UserInput::new(content);
        let line = serde_json::to_string(&input)? + "\n";

        tracing::debug!("Claude: sending message: {}", line.trim());

        let mut stdin = self.stdin.lock().unwrap();
        stdin.write_all(line.as_bytes())?;
        stdin.flush()?;

        Ok(())
    }

    /// Get session info (if received)
    pub fn session_info(&self) -> Option<SessionInfo> {
        self.session_info.lock().unwrap().clone()
    }

    /// Set session info (called by panel when init message received)
    pub fn set_session_info(&self, info: SessionInfo) {
        *self.session_info.lock().unwrap() = Some(info);
    }

    /// Reader thread that processes stdout from Claude CLI
    fn reader_thread(
        stdout: ChildStdout,
        update_tx: Sender<SessionUpdate>,
        alive: Arc<AtomicBool>,
    ) {
        tracing::info!("Claude: reader thread started");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    tracing::info!("Claude: EOF, connection closed");
                    alive.store(false, Ordering::SeqCst);
                    break;
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    tracing::debug!("Claude: received: {}", trimmed);

                    // Parse as output message
                    match serde_json::from_str::<OutputMessage>(trimmed) {
                        Ok(msg) => {
                            let updates = parse_output_message(&msg);
                            for update in updates {
                                if update_tx.send_blocking(update).is_err() {
                                    tracing::warn!("Claude: receiver dropped");
                                    alive.store(false, Ordering::SeqCst);
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            tracing::debug!("Claude: non-JSON output ({}): {}", e, trimmed);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Claude: read error: {}", e);
                    alive.store(false, Ordering::SeqCst);
                    break;
                }
            }
        }
        tracing::info!("Claude: reader thread exiting");
    }

    /// Check if connection is alive
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Disconnect and kill the process
    pub fn disconnect(&self) {
        tracing::info!("Claude: disconnecting");
        self.alive.store(false, Ordering::SeqCst);
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

impl Drop for ClaudeConnection {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // SessionInfo Tests
    // ============================================================================

    #[test]
    fn test_session_info_construction() {
        let info = SessionInfo {
            session_id: "session_123".into(),
            model: "claude-sonnet-4-20250514".into(),
            tools: vec!["Read".into(), "Write".into(), "Bash".into()],
        };

        assert_eq!(info.session_id, "session_123");
        assert_eq!(info.model, "claude-sonnet-4-20250514");
        assert_eq!(info.tools.len(), 3);
    }

    #[test]
    fn test_session_info_clone() {
        let info = SessionInfo {
            session_id: "test".into(),
            model: "claude".into(),
            tools: vec!["Read".into()],
        };

        let cloned = info.clone();
        assert_eq!(cloned.session_id, info.session_id);
        assert_eq!(cloned.model, info.model);
        assert_eq!(cloned.tools, info.tools);
    }

    #[test]
    fn test_session_info_debug() {
        let info = SessionInfo {
            session_id: "debug_test".into(),
            model: "test_model".into(),
            tools: vec![],
        };

        let debug = format!("{:?}", info);
        assert!(debug.contains("debug_test"));
        assert!(debug.contains("test_model"));
    }

    #[test]
    fn test_session_info_with_empty_fields() {
        let info = SessionInfo {
            session_id: String::new(),
            model: String::new(),
            tools: vec![],
        };

        assert!(info.session_id.is_empty());
        assert!(info.model.is_empty());
        assert!(info.tools.is_empty());
    }

    #[test]
    fn test_session_info_with_many_tools() {
        let tools: Vec<String> = vec![
            "Read", "Write", "Edit", "Bash", "Glob", "Grep",
            "WebFetch", "WebSearch", "Task", "NotebookEdit",
        ].into_iter().map(String::from).collect();

        let info = SessionInfo {
            session_id: "full_session".into(),
            model: "claude-opus-4-20250514".into(),
            tools,
        };

        assert_eq!(info.tools.len(), 10);
        assert!(info.tools.contains(&"Bash".to_string()));
        assert!(info.tools.contains(&"NotebookEdit".to_string()));
    }

    // ============================================================================
    // ConnectionError Tests
    // ============================================================================

    #[test]
    fn test_connection_error_display_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = ConnectionError::Io(io_err);
        let display = format!("{}", err);
        assert!(display.contains("IO error"));
    }

    #[test]
    fn test_connection_error_display_json() {
        let json_str = "invalid json {";
        let json_err = serde_json::from_str::<serde_json::Value>(json_str).unwrap_err();
        let err = ConnectionError::Json(json_err);
        let display = format!("{}", err);
        assert!(display.contains("JSON error"));
    }

    #[test]
    fn test_connection_error_display_closed() {
        let err = ConnectionError::ConnectionClosed;
        let display = format!("{}", err);
        assert_eq!(display, "Connection closed");
    }

    #[test]
    fn test_connection_error_display_spawn() {
        let err = ConnectionError::SpawnError("Failed to spawn claude".into());
        let display = format!("{}", err);
        assert!(display.contains("Spawn error"));
        assert!(display.contains("Failed to spawn claude"));
    }

    #[test]
    fn test_connection_error_debug() {
        let err = ConnectionError::ConnectionClosed;
        let debug = format!("{:?}", err);
        assert!(debug.contains("ConnectionClosed"));
    }

    #[test]
    fn test_connection_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broken");
        let err: ConnectionError = io_err.into();
        assert!(matches!(err, ConnectionError::Io(_)));
    }

    #[test]
    fn test_connection_error_from_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err: ConnectionError = json_err.into();
        assert!(matches!(err, ConnectionError::Json(_)));
    }

    // ============================================================================
    // UserInput JSON Format Tests
    // ============================================================================

    #[test]
    fn test_user_input_json_structure() {
        let input = UserInput::new("Test message");
        let json = serde_json::to_string(&input).unwrap();

        // Verify exact JSON structure expected by Claude CLI
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Top level must have "type" and "message"
        assert!(parsed.get("type").is_some());
        assert!(parsed.get("message").is_some());

        // "type" must be "user"
        assert_eq!(parsed["type"], "user");

        // "message" must have "role" and "content"
        let message = &parsed["message"];
        assert!(message.get("role").is_some());
        assert!(message.get("content").is_some());
        assert_eq!(message["role"], "user");
        assert_eq!(message["content"], "Test message");
    }

    #[test]
    fn test_user_input_newline_appended() {
        // The send_message method appends newline, verify serialization allows it
        let input = UserInput::new("message");
        let json = serde_json::to_string(&input).unwrap();
        let with_newline = json + "\n";

        // Should still be parseable before the newline
        let trimmed = with_newline.trim();
        let _: serde_json::Value = serde_json::from_str(trimmed).unwrap();
    }

    #[test]
    fn test_user_input_json_compact() {
        let input = UserInput::new("test");
        let json = serde_json::to_string(&input).unwrap();

        // Should not contain unnecessary whitespace (compact format)
        assert!(!json.contains('\n'));
        assert!(!json.contains("  ")); // No double spaces
    }

    // ============================================================================
    // Result Type Tests
    // ============================================================================

    #[test]
    fn test_result_type_alias() {
        fn example_function() -> Result<String> {
            Ok("success".into())
        }

        let result = example_function();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
    }

    #[test]
    fn test_result_type_with_error() {
        fn failing_function() -> Result<String> {
            Err(ConnectionError::ConnectionClosed)
        }

        let result = failing_function();
        assert!(result.is_err());
    }
}
