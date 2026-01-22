use async_trait::async_trait;
use russh::client::{self, Handle, Msg};
use russh::keys::key::PublicKey;
use russh::{Channel, ChannelMsg, Disconnect};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

use crate::session::models::{AuthMethod, SshSession};

/// SSH connection configuration constants
const CONNECTION_TIMEOUT_SECS: u64 = 5;
const INACTIVITY_TIMEOUT_SECS: u64 = 300;
const KEEPALIVE_INTERVAL_SECS: u64 = 30;
const KEEPALIVE_MAX: usize = 3;

/// Reconnection configuration
const MAX_RECONNECT_ATTEMPTS: u32 = 3;
const INITIAL_RECONNECT_DELAY_SECS: u64 = 1;

/// Errors that can occur during SSH operations
#[derive(Debug, Error)]
pub enum SshError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Connection timed out after {0} seconds")]
    ConnectionTimeout(u64),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Host key verification failed: {0}")]
    HostKeyVerificationFailed(String),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Resize failed: {0}")]
    ResizeFailed(String),

    #[error("Not connected")]
    NotConnected,

    #[error("SSH error: {0}")]
    SshError(String),
}

/// Result type for SSH operations
pub type SshResult<T> = Result<T, SshError>;

/// Terminal size for SSH PTY
#[derive(Debug, Clone, Copy, Default)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl TerminalSize {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols,
            rows,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

/// Connection state of the SSH backend
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
    Failed,
}

/// Result of host key verification
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyStatus {
    /// Key matches known_hosts entry
    Verified,
    /// Host not in known_hosts, key was added (TOFU)
    TrustOnFirstUse,
    /// Key mismatch - potential MITM attack
    Mismatch,
    /// Could not verify (e.g., file error)
    Error(String),
}

/// SSH client handler for russh
struct SshClientHandler {
    /// Server hostname for host key verification
    hostname: String,
    /// Server host key verification callback result
    verified: bool,
    /// Host key verification status
    host_key_status: Option<HostKeyStatus>,
}

impl SshClientHandler {
    fn new(hostname: &str) -> Self {
        Self {
            hostname: hostname.to_string(),
            verified: false,
            host_key_status: None,
        }
    }
}

#[async_trait]
impl client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, server_public_key: &PublicKey) -> Result<bool, Self::Error> {
        let status = verify_host_key(&self.hostname, server_public_key);

        match &status {
            HostKeyStatus::Verified => {
                tracing::info!("Host key verified for {}", self.hostname);
                self.verified = true;
                self.host_key_status = Some(status);
                Ok(true)
            }
            HostKeyStatus::TrustOnFirstUse => {
                tracing::info!("New host key accepted for {} (TOFU)", self.hostname);
                self.verified = true;
                self.host_key_status = Some(status);
                Ok(true)
            }
            HostKeyStatus::Mismatch => {
                tracing::error!(
                    "HOST KEY VERIFICATION FAILED for {}! Potential MITM attack!",
                    self.hostname
                );
                self.verified = false;
                self.host_key_status = Some(status);
                Ok(false)
            }
            HostKeyStatus::Error(e) => {
                tracing::warn!("Host key verification error for {}: {}", self.hostname, e);
                // On error, we still accept (degrade gracefully) but log the issue
                self.verified = true;
                self.host_key_status = Some(status);
                Ok(true)
            }
        }
    }
}

/// Path to the known_hosts file
fn known_hosts_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".ssh").join("known_hosts"))
}

/// Verify a server's host key against known_hosts
fn verify_host_key(hostname: &str, server_key: &PublicKey) -> HostKeyStatus {
    let known_hosts_path = match known_hosts_path() {
        Some(p) => p,
        None => return HostKeyStatus::Error("Could not determine home directory".to_string()),
    };

    // Read known_hosts file
    let contents = match std::fs::read_to_string(&known_hosts_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // File doesn't exist, use TOFU
            return add_host_key_to_known_hosts(hostname, server_key);
        }
        Err(e) => return HostKeyStatus::Error(format!("Failed to read known_hosts: {}", e)),
    };

    // Convert russh key to base64 for comparison
    let server_key_type = key_type_string(server_key);
    let server_key_base64 = match encode_public_key_base64(server_key) {
        Ok(k) => k,
        Err(e) => return HostKeyStatus::Error(format!("Failed to encode server key: {}", e)),
    };

    // Parse known_hosts and look for matching host
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse line: hostname key-type key-data [comment]
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        let hosts = parts[0];
        let key_type = parts[1];
        let key_data = parts[2];

        // Check if this line matches our hostname
        if !host_matches(hosts, hostname) {
            continue;
        }

        // Found a matching host - check if the key matches
        if key_type == server_key_type && key_data == server_key_base64 {
            return HostKeyStatus::Verified;
        } else if key_type == server_key_type {
            // Same key type but different key - this is a mismatch!
            return HostKeyStatus::Mismatch;
        }
        // Different key type - continue looking (host might have multiple keys)
    }

    // Host not found, use TOFU
    add_host_key_to_known_hosts(hostname, server_key)
}

/// Check if a hostname pattern matches a hostname
fn host_matches(pattern: &str, hostname: &str) -> bool {
    // Handle comma-separated host list
    for host_pattern in pattern.split(',') {
        let host_pattern = host_pattern.trim();

        // Handle hashed hosts (start with |)
        if host_pattern.starts_with('|') {
            // Hashed hosts are more complex to verify - skip for now
            continue;
        }

        // Handle [hostname]:port format
        let host_pattern = if host_pattern.starts_with('[') {
            if let Some(end) = host_pattern.find(']') {
                &host_pattern[1..end]
            } else {
                host_pattern
            }
        } else {
            host_pattern
        };

        // Simple exact match or wildcard
        if host_pattern == hostname {
            return true;
        }

        // Handle wildcard patterns
        if host_pattern.contains('*') {
            let pattern = host_pattern.replace("*", ".*");
            if let Ok(re) = regex_lite::Regex::new(&format!("^{}$", pattern)) {
                if re.is_match(hostname) {
                    return true;
                }
            }
        }
    }
    false
}

/// Get the SSH key type string for a public key
fn key_type_string(key: &PublicKey) -> &str {
    // Use the key's name method which returns the algorithm identifier
    key.name()
}

/// Encode a public key to base64 (SSH wire format)
fn encode_public_key_base64(key: &PublicKey) -> Result<String, String> {
    use russh_keys::PublicKeyBase64;
    Ok(key.public_key_base64())
}

/// Add a new host key to known_hosts (TOFU)
fn add_host_key_to_known_hosts(hostname: &str, key: &PublicKey) -> HostKeyStatus {
    let known_hosts_path = match known_hosts_path() {
        Some(p) => p,
        None => return HostKeyStatus::Error("Could not determine home directory".to_string()),
    };

    // Ensure .ssh directory exists
    if let Some(ssh_dir) = known_hosts_path.parent() {
        if let Err(e) = std::fs::create_dir_all(ssh_dir) {
            return HostKeyStatus::Error(format!("Failed to create .ssh directory: {}", e));
        }
    }

    let key_type = key_type_string(key);
    let key_base64 = match encode_public_key_base64(key) {
        Ok(k) => k,
        Err(e) => return HostKeyStatus::Error(e),
    };

    let entry = format!("{} {} {}\n", hostname, key_type, key_base64);

    // Append to known_hosts
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&known_hosts_path)
    {
        Ok(mut file) => {
            if let Err(e) = file.write_all(entry.as_bytes()) {
                return HostKeyStatus::Error(format!("Failed to write to known_hosts: {}", e));
            }
            tracing::info!("Added host key for {} to known_hosts", hostname);
            HostKeyStatus::TrustOnFirstUse
        }
        Err(e) => HostKeyStatus::Error(format!("Failed to open known_hosts: {}", e)),
    }
}

/// SSH backend implementation using russh
pub struct SshBackend {
    /// SSH session handle
    session: Option<Handle<SshClientHandler>>,
    /// SSH channel for PTY
    channel: Option<Channel<Msg>>,
    /// Current connection state
    state: ConnectionState,
    /// Session configuration
    config: SshSession,
    /// Current terminal size
    size: TerminalSize,
    /// Read buffer for accumulated data
    read_buffer: Vec<u8>,
    /// Channel for sending write requests (decoupled from read loop)
    write_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
}

impl SshBackend {
    /// Create a new SSH backend (not yet connected)
    pub fn new(config: SshSession) -> Self {
        Self {
            session: None,
            channel: None,
            state: ConnectionState::Disconnected,
            config,
            size: TerminalSize::new(80, 24),
            read_buffer: Vec::new(),
            write_tx: None,
        }
    }

    /// Connect to the SSH server
    pub async fn connect(&mut self) -> SshResult<()> {
        self.state = ConnectionState::Connecting;

        // Create russh client config with timeouts and keepalive
        let ssh_config = client::Config {
            inactivity_timeout: Some(Duration::from_secs(INACTIVITY_TIMEOUT_SECS)),
            keepalive_interval: Some(Duration::from_secs(KEEPALIVE_INTERVAL_SECS)),
            keepalive_max: KEEPALIVE_MAX,
            ..Default::default()
        };
        let ssh_config = Arc::new(ssh_config);

        // Connect to the server with timeout
        let addr = format!("{}:{}", self.config.host, self.config.port);
        tracing::info!("Connecting to SSH server: {}", addr);

        let handler = SshClientHandler::new(&self.config.host);
        let connect_future = client::connect(ssh_config, &addr, handler);

        let mut session = match tokio::time::timeout(
            Duration::from_secs(CONNECTION_TIMEOUT_SECS),
            connect_future,
        )
        .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                self.state = ConnectionState::Failed;
                return Err(SshError::ConnectionFailed(e.to_string()));
            }
            Err(_) => {
                self.state = ConnectionState::Failed;
                return Err(SshError::ConnectionTimeout(CONNECTION_TIMEOUT_SECS));
            }
        };

        // Authenticate
        let authenticated = self.authenticate(&mut session).await?;
        if !authenticated {
            self.state = ConnectionState::Failed;
            return Err(SshError::AuthenticationFailed(
                "Authentication failed".to_string(),
            ));
        }

        // Open a session channel
        let channel = match session.channel_open_session().await {
            Ok(c) => c,
            Err(e) => {
                self.state = ConnectionState::Failed;
                return Err(SshError::SshError(format!(
                    "Failed to open channel: {}",
                    e
                )));
            }
        };

        // Request a PTY (want_reply=true to wait for server confirmation)
        tracing::info!("Requesting PTY...");
        if let Err(e) = channel
            .request_pty(
                true,
                "xterm-256color",
                self.size.cols as u32,
                self.size.rows as u32,
                self.size.pixel_width as u32,
                self.size.pixel_height as u32,
                &[], // Terminal modes
            )
            .await
        {
            self.state = ConnectionState::Failed;
            return Err(SshError::SshError(format!("Failed to request PTY: {}", e)));
        }
        tracing::info!("PTY granted");

        // Request a shell (want_reply=true to wait for server confirmation)
        tracing::info!("Requesting shell...");
        if let Err(e) = channel.request_shell(true).await {
            self.state = ConnectionState::Failed;
            return Err(SshError::SshError(format!(
                "Failed to request shell: {}",
                e
            )));
        }
        tracing::info!("Shell started");

        self.session = Some(session);
        self.channel = Some(channel);
        self.state = ConnectionState::Connected;

        tracing::info!("SSH connection established to {}", addr);
        Ok(())
    }

    /// Authenticate with the server using the configured method
    async fn authenticate(&self, session: &mut Handle<SshClientHandler>) -> SshResult<bool> {
        let username = &self.config.username;
        tracing::info!("Authenticating as user: {}", username);

        match &self.config.auth {
            AuthMethod::Password { password, .. } => {
                tracing::info!("Using password authentication");
                let password = password.as_ref().ok_or_else(|| {
                    SshError::AuthenticationFailed("Password not provided".to_string())
                })?;

                match session.authenticate_password(username, password).await {
                    Ok(result) => {
                        tracing::info!("Password auth result: {}", result);
                        Ok(result)
                    }
                    Err(e) => {
                        tracing::error!("Password auth error: {}", e);
                        Err(SshError::AuthenticationFailed(e.to_string()))
                    }
                }
            }

            AuthMethod::PrivateKey {
                path, passphrase, ..
            } => {
                tracing::info!("Using private key authentication from: {:?}", path);
                let key = load_private_key(path, passphrase.as_deref())?;
                match session.authenticate_publickey(username, Arc::new(key)).await {
                    Ok(result) => {
                        tracing::info!("Key auth result: {}", result);
                        Ok(result)
                    }
                    Err(e) => {
                        tracing::error!("Key auth error: {}", e);
                        Err(SshError::AuthenticationFailed(e.to_string()))
                    }
                }
            }

            AuthMethod::Agent => {
                tracing::info!("Using SSH agent authentication");
                // Try to connect to SSH agent
                match self.authenticate_with_agent(session, username).await {
                    Ok(result) => {
                        tracing::info!("Agent auth result: {}", result);
                        Ok(result)
                    }
                    Err(e) => {
                        tracing::error!("Agent auth error: {}", e);
                        Err(SshError::AuthenticationFailed(format!(
                            "Agent authentication failed: {}",
                            e
                        )))
                    }
                }
            }
        }
    }

    /// Authenticate using SSH agent
    async fn authenticate_with_agent(
        &self,
        session: &mut Handle<SshClientHandler>,
        username: &str,
    ) -> SshResult<bool> {
        // Get the SSH_AUTH_SOCK environment variable
        let socket_path = std::env::var("SSH_AUTH_SOCK").map_err(|_| {
            SshError::AuthenticationFailed("SSH_AUTH_SOCK not set".to_string())
        })?;

        // Connect to the agent
        let mut agent = russh_keys::agent::client::AgentClient::connect_uds(&socket_path)
            .await
            .map_err(|e| SshError::AuthenticationFailed(format!("Failed to connect to agent: {}", e)))?;

        // Get identities from agent
        let identities = agent
            .request_identities()
            .await
            .map_err(|e| SshError::AuthenticationFailed(format!("Failed to get identities: {}", e)))?;

        // Try each identity using authenticate_future with the agent as signer
        for identity in identities {
            let (returned_agent, result) = session
                .authenticate_future(username, identity, agent)
                .await;
            agent = returned_agent;

            match result {
                Ok(true) => return Ok(true),
                Ok(false) => continue,
                Err(_) => continue,
            }
        }

        Ok(false)
    }

    /// Set up the write channel sender
    ///
    /// Returns a sender that can be used to send write data without holding the backend lock.
    /// The receiver should be used by the read/write loop to actually send data.
    pub fn setup_write_channel(&mut self) -> tokio::sync::mpsc::UnboundedReceiver<Vec<u8>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.write_tx = Some(tx);
        rx
    }

    /// Get the write sender for sending data without locking
    pub fn get_write_sender(&self) -> Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>> {
        self.write_tx.clone()
    }

    /// Write data to the SSH channel (requires holding the lock)
    pub async fn write(&mut self, data: &[u8]) -> SshResult<()> {
        let channel = self.channel.as_ref().ok_or(SshError::NotConnected)?;

        channel
            .data(data)
            .await
            .map_err(|e| SshError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )))?;

        Ok(())
    }

    /// Send data through the write channel (doesn't require the lock)
    pub fn send_write(&self, data: Vec<u8>) -> SshResult<()> {
        if let Some(tx) = &self.write_tx {
            tx.send(data).map_err(|_| SshError::ChannelClosed)?;
            Ok(())
        } else {
            Err(SshError::NotConnected)
        }
    }

    /// Take the channel out of the backend for direct I/O
    ///
    /// This allows the channel to be used directly in a select! loop
    /// without needing to lock the backend. Returns the channel and
    /// write receiver for the I/O task.
    pub fn take_channel_for_io(&mut self) -> Option<(Channel<Msg>, tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>)> {
        let channel = self.channel.take()?;
        // Create a new write channel since we're taking ownership
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.write_tx = Some(tx);
        Some((channel, rx))
    }

    /// Read data from the SSH channel
    ///
    /// Returns:
    /// - Ok(n) where n > 0: data was read
    /// - Ok(0): no data available yet (timeout or non-data protocol message) OR connection closed
    ///          Check is_alive() to distinguish between these cases
    /// - Err: connection error
    ///
    /// Uses a timeout to periodically release the lock, allowing concurrent writes.
    pub async fn read(&mut self, buf: &mut [u8]) -> SshResult<usize> {
        // First, check if we have buffered data
        if !self.read_buffer.is_empty() {
            let len = std::cmp::min(buf.len(), self.read_buffer.len());
            buf[..len].copy_from_slice(&self.read_buffer[..len]);
            self.read_buffer.drain(..len);
            return Ok(len);
        }

        let channel = self.channel.as_mut().ok_or(SshError::NotConnected)?;

        // Use a timeout to periodically release the lock, allowing writes to proceed
        let wait_result = tokio::time::timeout(
            Duration::from_millis(50),
            channel.wait()
        ).await;

        match wait_result {
            Ok(Some(ChannelMsg::Data { data })) => {
                let len = std::cmp::min(buf.len(), data.len());
                buf[..len].copy_from_slice(&data[..len]);

                // Buffer any remaining data
                if data.len() > len {
                    self.read_buffer.extend_from_slice(&data[len..]);
                }

                Ok(len)
            }
            Ok(Some(ChannelMsg::ExtendedData { data, .. })) => {
                // Handle stderr data the same way
                let len = std::cmp::min(buf.len(), data.len());
                buf[..len].copy_from_slice(&data[..len]);

                if data.len() > len {
                    self.read_buffer.extend_from_slice(&data[len..]);
                }

                Ok(len)
            }
            Ok(Some(ChannelMsg::Eof)) | Ok(Some(ChannelMsg::Close)) => {
                self.state = ConnectionState::Disconnected;
                Ok(0) // True EOF - connection closed
            }
            Ok(Some(ChannelMsg::ExitStatus { exit_status })) => {
                tracing::info!("Remote process exited with status: {}", exit_status);
                self.state = ConnectionState::Disconnected;
                Ok(0) // Process ended - connection closed
            }
            Ok(Some(_)) => {
                // Other protocol messages (WindowAdjust, Success, etc.)
                // Return 0 but keep state as Connected - caller should retry
                Ok(0)
            }
            Ok(None) => {
                self.state = ConnectionState::Disconnected;
                Err(SshError::ChannelClosed)
            }
            Err(_) => {
                // Timeout - no data available yet, release lock and let caller retry
                Ok(0)
            }
        }
    }

    /// Resize the SSH PTY
    pub async fn resize(&mut self, size: TerminalSize) -> SshResult<()> {
        self.size = size;

        if let Some(channel) = &self.channel {
            channel
                .window_change(
                    size.cols as u32,
                    size.rows as u32,
                    size.pixel_width as u32,
                    size.pixel_height as u32,
                )
                .await
                .map_err(|e| SshError::ResizeFailed(e.to_string()))?;
        }

        Ok(())
    }

    /// Close the SSH connection
    pub async fn close(&mut self) -> SshResult<()> {
        self.state = ConnectionState::Disconnecting;

        if let Some(channel) = self.channel.take() {
            let _ = channel.eof().await;
        }

        if let Some(session) = self.session.take() {
            let _ = session
                .disconnect(Disconnect::ByApplication, "User disconnected", "en")
                .await;
        }

        self.state = ConnectionState::Disconnected;
        Ok(())
    }

    /// Check if the SSH connection is alive
    pub fn is_alive(&self) -> bool {
        self.state == ConnectionState::Connected
    }

    /// Get the current connection state
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Get a description of the connection
    pub fn description(&self) -> String {
        format!(
            "{}@{}:{}",
            self.config.username, self.config.host, self.config.port
        )
    }

    /// Attempt to reconnect with exponential backoff
    ///
    /// Returns Ok(()) if reconnection succeeds, Err if all attempts fail.
    pub async fn reconnect(&mut self) -> SshResult<()> {
        let mut delay_secs = INITIAL_RECONNECT_DELAY_SECS;

        for attempt in 1..=MAX_RECONNECT_ATTEMPTS {
            tracing::info!(
                "Reconnection attempt {}/{} to {} (waiting {}s)",
                attempt,
                MAX_RECONNECT_ATTEMPTS,
                self.description(),
                delay_secs
            );

            // Wait before attempting reconnection
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;

            // Clean up any existing connection state
            self.session = None;
            self.channel = None;
            self.read_buffer.clear();
            self.state = ConnectionState::Disconnected;

            // Attempt to connect
            match self.connect().await {
                Ok(()) => {
                    tracing::info!("Reconnection successful on attempt {}", attempt);
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!(
                        "Reconnection attempt {} failed: {}",
                        attempt,
                        e
                    );

                    if attempt < MAX_RECONNECT_ATTEMPTS {
                        // Exponential backoff
                        delay_secs *= 2;
                    }
                }
            }
        }

        self.state = ConnectionState::Failed;
        Err(SshError::ConnectionFailed(format!(
            "Failed to reconnect after {} attempts",
            MAX_RECONNECT_ATTEMPTS
        )))
    }

    /// Get access to session config for reconnection
    pub fn config(&self) -> &SshSession {
        &self.config
    }
}

/// Load a private key from a file
fn load_private_key(
    path: &Path,
    passphrase: Option<&str>,
) -> SshResult<russh_keys::key::KeyPair> {
    // Expand ~ in path
    let path = if path.starts_with("~") {
        if let Some(home) = dirs::home_dir() {
            home.join(path.strip_prefix("~").unwrap())
        } else {
            path.to_path_buf()
        }
    } else {
        path.to_path_buf()
    };

    russh_keys::load_secret_key(&path, passphrase).map_err(|e| {
        SshError::AuthenticationFailed(format!("Failed to load private key: {}", e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_backend_creation() {
        let session = SshSession::new(
            "test".to_string(),
            "localhost".to_string(),
            "user".to_string(),
        );
        let backend = SshBackend::new(session);
        assert_eq!(backend.state(), ConnectionState::Disconnected);
        assert!(!backend.is_alive());
    }
}
