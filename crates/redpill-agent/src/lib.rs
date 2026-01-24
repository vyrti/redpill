//! Claude Code CLI Integration
//!
//! This crate provides communication with the Claude Code CLI via stream-json format.

pub mod connection;
pub mod protocol;

pub use connection::{ClaudeConnection, ConnectionError, SessionInfo};
pub use protocol::{SessionUpdate, ToolCall, ToolCallStatus, ToolKind};
