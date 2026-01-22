//! AWS SSM Session Manager backend implementation
//!
//! This module implements the SSM Session Manager protocol for connecting
//! to EC2 instances and on-premises managed instances via AWS SSM.
//!
//! Protocol reference: AWS Session Manager Plugin source code

use aws_config::BehaviorVersion;
use aws_sdk_ssm::Client as SsmClient;
use futures::{SinkExt, StreamExt};
use sha2::{Digest, Sha256};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::{
    connect_async,
    tungstenite::Message as WsMessage,
    MaybeTlsStream, WebSocketStream,
};
use uuid::Uuid;

use crate::session::SsmSession;
use super::TerminalSize;

/// SSM WebSocket message types
mod message_type {
    pub const INPUT_STREAM_DATA: &str = "input_stream_data";
    pub const OUTPUT_STREAM_DATA: &str = "output_stream_data";
    pub const ACK: &str = "acknowledge";
    pub const START_PUBLICATION: &str = "start_publication";
    pub const PAUSE_PUBLICATION: &str = "pause_publication";
    pub const CHANNEL_CLOSED: &str = "channel_closed";
}

/// SSM payload types
mod payload_type {
    pub const OUTPUT: u32 = 1;
    pub const ERROR: u32 = 2;
    pub const SIZE_DATA: u32 = 3;
    pub const HANDSHAKE_REQUEST: u32 = 5;
    pub const HANDSHAKE_COMPLETE: u32 = 6;
}

/// Errors that can occur during SSM operations
#[derive(Debug, Error)]
pub enum SsmError {
    #[error("AWS configuration error: {0}")]
    AwsConfig(String),

    #[error("SSM API error: {0}")]
    SsmApi(String),

    #[error("WebSocket connection failed: {0}")]
    WebSocketConnection(String),

    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Session closed: {0}")]
    SessionClosed(String),

    #[error("Not connected")]
    NotConnected,

    #[error("Instance not found or not SSM-enabled: {0}")]
    InstanceNotFound(String),

    #[error("Timeout: {0}")]
    Timeout(String),
}

/// Result type for SSM operations
pub type SsmResult<T> = Result<T, SsmError>;

/// Connection state of the SSM backend
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Authenticating,
    Handshaking,
    Connected,
    Disconnecting,
    Failed,
}

/// Parsed SSM binary message header (120 bytes)
#[derive(Debug)]
struct SsmMessageHeader {
    /// Length of the header (should be 116)
    header_length: u32,
    /// Message type string
    message_type: String,
    /// Schema version (should be 1)
    schema_version: u32,
    /// Creation timestamp (milliseconds since epoch)
    created_date: u64,
    /// Sequence number for ordering
    sequence_number: i64,
    /// Message flags
    flags: u64,
    /// Unique message ID
    message_id: Uuid,
    /// SHA-256 digest of payload
    payload_digest: [u8; 32],
    /// Type of payload data
    payload_type: u32,
    /// Length of payload data
    payload_length: u32,
}

impl SsmMessageHeader {
    /// Parse a header from bytes
    fn parse(data: &[u8]) -> SsmResult<(Self, &[u8])> {
        if data.len() < 4 {
            return Err(SsmError::Protocol("Message too short for header length".into()));
        }

        let header_length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);

        // AWS SSM uses a header of 116 bytes (header content) + 4 bytes (header length field)
        // Total header: 120 bytes minimum for messages with payloads
        let total_header_size = (header_length + 4) as usize;
        if data.len() < total_header_size {
            return Err(SsmError::Protocol(format!(
                "Message too short: {} < {}",
                data.len(),
                total_header_size
            )));
        }

        // Extract message type (32 bytes, null-padded string)
        let message_type_bytes = &data[4..36];
        let message_type = String::from_utf8_lossy(message_type_bytes)
            .trim_end_matches('\0')
            .to_string();

        let schema_version = u32::from_be_bytes([data[36], data[37], data[38], data[39]]);
        let created_date = u64::from_be_bytes([
            data[40], data[41], data[42], data[43], data[44], data[45], data[46], data[47],
        ]);
        let sequence_number = i64::from_be_bytes([
            data[48], data[49], data[50], data[51], data[52], data[53], data[54], data[55],
        ]);
        let flags = u64::from_be_bytes([
            data[56], data[57], data[58], data[59], data[60], data[61], data[62], data[63],
        ]);

        // Message ID (16 bytes UUID)
        let mut message_id_bytes = [0u8; 16];
        message_id_bytes.copy_from_slice(&data[64..80]);
        let message_id = Uuid::from_bytes(message_id_bytes);

        // Payload digest (32 bytes SHA-256)
        let mut payload_digest = [0u8; 32];
        payload_digest.copy_from_slice(&data[80..112]);

        let payload_type = u32::from_be_bytes([data[112], data[113], data[114], data[115]]);
        let payload_length = u32::from_be_bytes([data[116], data[117], data[118], data[119]]);

        let payload_start = 120;
        let payload = &data[payload_start..];

        Ok((
            SsmMessageHeader {
                header_length,
                message_type,
                schema_version,
                created_date,
                sequence_number,
                flags,
                message_id,
                payload_digest,
                payload_type,
                payload_length,
            },
            payload,
        ))
    }
}

/// Build an SSM binary message
fn build_ssm_message(
    message_type: &str,
    sequence_number: i64,
    payload_type: u32,
    payload: &[u8],
) -> Vec<u8> {
    let mut message = Vec::with_capacity(120 + payload.len());

    // Header length (116 bytes, not including this field)
    message.extend_from_slice(&116u32.to_be_bytes());

    // Message type (32 bytes, null-padded)
    let mut msg_type_bytes = [0u8; 32];
    let msg_type_str = message_type.as_bytes();
    let copy_len = msg_type_str.len().min(32);
    msg_type_bytes[..copy_len].copy_from_slice(&msg_type_str[..copy_len]);
    message.extend_from_slice(&msg_type_bytes);

    // Schema version (1)
    message.extend_from_slice(&1u32.to_be_bytes());

    // Created date (milliseconds since epoch)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    message.extend_from_slice(&now.to_be_bytes());

    // Sequence number
    message.extend_from_slice(&sequence_number.to_be_bytes());

    // Flags (0)
    message.extend_from_slice(&0u64.to_be_bytes());

    // Message ID (random UUID)
    let msg_id = Uuid::new_v4();
    message.extend_from_slice(msg_id.as_bytes());

    // Payload digest (SHA-256)
    let mut hasher = Sha256::new();
    hasher.update(payload);
    let digest = hasher.finalize();
    message.extend_from_slice(&digest);

    // Payload type
    message.extend_from_slice(&payload_type.to_be_bytes());

    // Payload length
    message.extend_from_slice(&(payload.len() as u32).to_be_bytes());

    // Payload
    message.extend_from_slice(payload);

    message
}

/// Build an acknowledgement message
fn build_ack_message(message_id: Uuid, sequence_number: i64) -> Vec<u8> {
    // ACK payload is the message ID string
    let payload = message_id.to_string();
    build_ssm_message(message_type::ACK, sequence_number, 0, payload.as_bytes())
}

/// Build a size (resize) message
fn build_size_message(sequence_number: i64, cols: u16, rows: u16) -> Vec<u8> {
    // Size payload is JSON: {"cols": N, "rows": M}
    let payload = format!(r#"{{"cols":{},"rows":{}}}"#, cols, rows);
    build_ssm_message(
        message_type::INPUT_STREAM_DATA,
        sequence_number,
        payload_type::SIZE_DATA,
        payload.as_bytes(),
    )
}

/// Build an input data message
fn build_input_message(sequence_number: i64, data: &[u8]) -> Vec<u8> {
    build_ssm_message(
        message_type::INPUT_STREAM_DATA,
        sequence_number,
        payload_type::OUTPUT,
        data,
    )
}

/// AWS SSM Session Manager backend
pub struct SsmBackend {
    /// Session configuration
    config: SsmSession,
    /// Current connection state
    state: ConnectionState,
    /// Current terminal size
    size: TerminalSize,
    /// Outgoing sequence number counter
    sequence_number: i64,
    /// Channel for sending write requests
    write_tx: Option<UnboundedSender<Vec<u8>>>,
    /// Channel for sending resize requests
    resize_tx: Option<UnboundedSender<TerminalSize>>,
    /// Stream URL from StartSession response
    stream_url: Option<String>,
    /// Token from StartSession response
    token: Option<String>,
    /// Session ID from StartSession response
    session_id: Option<String>,
}

impl SsmBackend {
    /// Create a new SSM backend (not yet connected)
    pub fn new(config: SsmSession) -> Self {
        Self {
            config,
            state: ConnectionState::Disconnected,
            size: TerminalSize::new(80, 24),
            sequence_number: 0,
            write_tx: None,
            resize_tx: None,
            stream_url: None,
            token: None,
            session_id: None,
        }
    }

    /// Get the next sequence number
    fn next_sequence(&mut self) -> i64 {
        let seq = self.sequence_number;
        self.sequence_number += 1;
        seq
    }

    /// Connect to the SSM session
    ///
    /// This performs the following steps:
    /// 1. Load AWS credentials and create SSM client
    /// 2. Call StartSession API to get WebSocket URL and token
    /// 3. Connect to WebSocket
    /// 4. Send authentication message
    /// 5. Complete handshake
    pub async fn connect(&mut self) -> SsmResult<()> {
        self.state = ConnectionState::Connecting;

        // Build AWS config
        let mut config_loader = aws_config::defaults(BehaviorVersion::latest());

        // Apply profile if specified
        if let Some(ref profile) = self.config.profile {
            config_loader = config_loader.profile_name(profile);
        }

        // Apply region if specified
        if let Some(ref region) = self.config.region {
            config_loader = config_loader.region(aws_sdk_ssm::config::Region::new(region.clone()));
        }

        let aws_config = config_loader.load().await;

        // Create SSM client
        let ssm_client = SsmClient::new(&aws_config);

        tracing::info!("Starting SSM session to instance: {}", self.config.instance_id);

        // Call StartSession API
        let start_session_result = tokio::time::timeout(
            Duration::from_secs(30),
            ssm_client
                .start_session()
                .target(&self.config.instance_id)
                .send(),
        )
        .await
        .map_err(|_| SsmError::Timeout("StartSession API call timed out".into()))?
        .map_err(|e| {
            // Check for common errors
            let err_msg = e.to_string();
            if err_msg.contains("TargetNotConnected") || err_msg.contains("InvalidInstanceId") {
                SsmError::InstanceNotFound(format!(
                    "Instance {} is not connected to SSM or does not exist",
                    self.config.instance_id
                ))
            } else if err_msg.contains("AccessDenied") || err_msg.contains("UnauthorizedAccess") {
                SsmError::Authentication(format!("Access denied: {}", err_msg))
            } else {
                SsmError::SsmApi(err_msg)
            }
        })?;

        let stream_url = start_session_result
            .stream_url()
            .ok_or_else(|| SsmError::SsmApi("No stream URL in response".into()))?
            .to_string();

        let token = start_session_result
            .token_value()
            .ok_or_else(|| SsmError::SsmApi("No token in response".into()))?
            .to_string();

        let session_id = start_session_result
            .session_id()
            .map(|s| s.to_string());

        self.stream_url = Some(stream_url.clone());
        self.token = Some(token.clone());
        self.session_id = session_id.clone();

        tracing::info!("SSM session started, session_id: {:?}", session_id);

        Ok(())
    }

    /// Set up the write and resize channels for I/O
    ///
    /// Returns receivers that should be used by the I/O loop.
    pub fn setup_channels(&mut self) -> (UnboundedReceiver<Vec<u8>>, UnboundedReceiver<TerminalSize>) {
        let (write_tx, write_rx) = tokio::sync::mpsc::unbounded_channel();
        let (resize_tx, resize_rx) = tokio::sync::mpsc::unbounded_channel();
        self.write_tx = Some(write_tx);
        self.resize_tx = Some(resize_tx);
        (write_rx, resize_rx)
    }

    /// Get the write sender for sending data without locking
    pub fn get_write_sender(&self) -> Option<UnboundedSender<Vec<u8>>> {
        self.write_tx.clone()
    }

    /// Get the resize sender for sending resize requests without locking
    pub fn get_resize_sender(&self) -> Option<UnboundedSender<TerminalSize>> {
        self.resize_tx.clone()
    }

    /// Get the stream URL for WebSocket connection
    pub fn stream_url(&self) -> Option<&str> {
        self.stream_url.as_deref()
    }

    /// Get the authentication token
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// Get the current connection state
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Set the connection state
    pub fn set_state(&mut self, state: ConnectionState) {
        self.state = state;
    }

    /// Check if the connection is alive
    pub fn is_alive(&self) -> bool {
        self.state == ConnectionState::Connected
    }

    /// Get a description of the connection
    pub fn description(&self) -> String {
        format!(
            "SSM:{}@{}",
            self.config.instance_id,
            self.config.region.as_deref().unwrap_or("default-region")
        )
    }

    /// Get the session configuration
    pub fn config(&self) -> &SsmSession {
        &self.config
    }

    /// Close the SSM session
    pub async fn close(&mut self) -> SsmResult<()> {
        self.state = ConnectionState::Disconnecting;
        // Note: The WebSocket will be closed by the I/O loop
        // We might want to call TerminateSession API here for cleanup
        self.state = ConnectionState::Disconnected;
        Ok(())
    }
}

/// Type alias for the WebSocket stream
pub type SsmWebSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Connect to the SSM WebSocket and perform authentication
pub async fn connect_websocket(backend: &mut SsmBackend) -> SsmResult<SsmWebSocket> {
    let stream_url = backend
        .stream_url()
        .ok_or(SsmError::NotConnected)?
        .to_string();

    let token = backend
        .token()
        .ok_or(SsmError::NotConnected)?
        .to_string();

    tracing::info!("Connecting to SSM WebSocket...");

    // Connect to WebSocket
    let (mut ws_stream, _response) = tokio::time::timeout(
        Duration::from_secs(30),
        connect_async(&stream_url),
    )
    .await
    .map_err(|_| SsmError::Timeout("WebSocket connection timed out".into()))?
    .map_err(|e| SsmError::WebSocketConnection(e.to_string()))?;

    tracing::info!("WebSocket connected, sending authentication...");

    backend.set_state(ConnectionState::Authenticating);

    // Send authentication message (JSON with TokenValue)
    let auth_message = serde_json::json!({
        "MessageSchemaVersion": "1.0",
        "RequestId": Uuid::new_v4().to_string(),
        "TokenValue": token
    });

    ws_stream
        .send(WsMessage::Text(auth_message.to_string().into()))
        .await
        .map_err(|e| SsmError::WebSocket(format!("Failed to send auth: {}", e)))?;

    // Wait for authentication response (text message)
    let auth_response = tokio::time::timeout(Duration::from_secs(10), ws_stream.next())
        .await
        .map_err(|_| SsmError::Timeout("Authentication response timed out".into()))?
        .ok_or_else(|| SsmError::SessionClosed("WebSocket closed during auth".into()))?
        .map_err(|e| SsmError::WebSocket(format!("Auth response error: {}", e)))?;

    tracing::debug!("Auth response: {:?}", auth_response);

    backend.set_state(ConnectionState::Handshaking);

    // Now we need to handle the handshake - send initial size
    // The server will send handshake request, we respond with handshake complete
    tracing::info!("SSM authentication complete, starting handshake...");

    // Send initial terminal size
    let size_msg = build_size_message(backend.next_sequence(), 80, 24);
    ws_stream
        .send(WsMessage::Binary(size_msg.into()))
        .await
        .map_err(|e| SsmError::WebSocket(format!("Failed to send size: {}", e)))?;

    backend.set_state(ConnectionState::Connected);
    tracing::info!("SSM session connected and ready");

    Ok(ws_stream)
}

/// Handle an incoming SSM message
///
/// Returns the terminal output data if this is an output message, None otherwise.
/// Also returns whether an ACK should be sent and the message ID for ACKing.
pub fn handle_ssm_message(data: &[u8]) -> SsmResult<(Option<Vec<u8>>, Option<(Uuid, i64)>)> {
    if data.len() < 120 {
        // Too short for a proper SSM message, might be a keepalive or control
        return Ok((None, None));
    }

    let (header, payload) = SsmMessageHeader::parse(data)?;

    tracing::trace!(
        "SSM message: type={}, seq={}, payload_type={}, payload_len={}",
        header.message_type,
        header.sequence_number,
        header.payload_type,
        header.payload_length
    );

    match header.message_type.as_str() {
        message_type::OUTPUT_STREAM_DATA => {
            // Output data - extract and return for terminal display
            let payload_data = &payload[..header.payload_length as usize];

            match header.payload_type {
                payload_type::OUTPUT | payload_type::ERROR => {
                    // Terminal output data
                    Ok((
                        Some(payload_data.to_vec()),
                        Some((header.message_id, header.sequence_number)),
                    ))
                }
                payload_type::HANDSHAKE_REQUEST => {
                    // Handshake request - we should respond but for now just ACK
                    tracing::debug!("Received handshake request");
                    Ok((None, Some((header.message_id, header.sequence_number))))
                }
                payload_type::HANDSHAKE_COMPLETE => {
                    tracing::debug!("Received handshake complete");
                    Ok((None, Some((header.message_id, header.sequence_number))))
                }
                _ => {
                    tracing::debug!("Unknown payload type: {}", header.payload_type);
                    Ok((None, Some((header.message_id, header.sequence_number))))
                }
            }
        }
        message_type::ACK => {
            // ACK from server - no need to do anything
            Ok((None, None))
        }
        message_type::START_PUBLICATION => {
            tracing::debug!("Received start_publication");
            Ok((None, None))
        }
        message_type::PAUSE_PUBLICATION => {
            tracing::debug!("Received pause_publication");
            Ok((None, None))
        }
        message_type::CHANNEL_CLOSED => {
            tracing::info!("SSM channel closed by server");
            Err(SsmError::SessionClosed("Channel closed by server".into()))
        }
        _ => {
            tracing::debug!("Unknown message type: {}", header.message_type);
            Ok((None, None))
        }
    }
}

/// Build messages for the I/O loop
pub struct SsmMessageBuilder {
    sequence_number: i64,
}

impl SsmMessageBuilder {
    pub fn new() -> Self {
        Self { sequence_number: 0 }
    }

    fn next_sequence(&mut self) -> i64 {
        let seq = self.sequence_number;
        self.sequence_number += 1;
        seq
    }

    /// Build an input message for keyboard data
    pub fn build_input(&mut self, data: &[u8]) -> Vec<u8> {
        build_input_message(self.next_sequence(), data)
    }

    /// Build a resize message
    pub fn build_resize(&mut self, cols: u16, rows: u16) -> Vec<u8> {
        build_size_message(self.next_sequence(), cols, rows)
    }

    /// Build an ACK message
    pub fn build_ack(&mut self, message_id: Uuid, sequence_number: i64) -> Vec<u8> {
        build_ack_message(message_id, sequence_number)
    }
}

impl Default for SsmMessageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssm_backend_creation() {
        let session = SsmSession::new("test", "i-1234567890abcdef0");
        let backend = SsmBackend::new(session);
        assert_eq!(backend.state(), ConnectionState::Disconnected);
        assert!(!backend.is_alive());
    }

    #[test]
    fn test_message_builder() {
        let mut builder = SsmMessageBuilder::new();
        let msg = builder.build_input(b"hello");
        assert!(msg.len() >= 120);

        let resize_msg = builder.build_resize(120, 40);
        assert!(resize_msg.len() >= 120);
    }
}
