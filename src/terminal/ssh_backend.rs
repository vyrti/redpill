use async_trait::async_trait;
use russh::client::{self, Handle, Msg};
use russh::keys::key::PublicKey;
use russh::{Channel, ChannelMsg, Disconnect};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

use crate::session::models::{AuthMethod, SshSession};

/// Errors that can occur during SSH operations
#[derive(Debug, Error)]
pub enum SshError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

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

/// SSH client handler for russh
struct SshClientHandler {
    /// Server host key verification callback result
    verified: bool,
}

impl SshClientHandler {
    fn new() -> Self {
        Self { verified: false }
    }
}

#[async_trait]
impl client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, _server_public_key: &PublicKey) -> Result<bool, Self::Error> {
        // TODO: Implement proper host key verification
        // For now, accept all keys (this should be improved for production)
        self.verified = true;
        Ok(true)
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
        }
    }

    /// Connect to the SSH server
    pub async fn connect(&mut self) -> SshResult<()> {
        self.state = ConnectionState::Connecting;

        // Create russh client config
        let ssh_config = client::Config::default();
        let ssh_config = Arc::new(ssh_config);

        // Connect to the server
        let addr = format!("{}:{}", self.config.host, self.config.port);
        tracing::info!("Connecting to SSH server: {}", addr);

        let handler = SshClientHandler::new();
        let mut session = match client::connect(ssh_config, &addr, handler).await {
            Ok(s) => s,
            Err(e) => {
                self.state = ConnectionState::Failed;
                return Err(SshError::ConnectionFailed(e.to_string()));
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

        // Request a PTY
        if let Err(e) = channel
            .request_pty(
                false,
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

        // Request a shell
        if let Err(e) = channel.request_shell(false).await {
            self.state = ConnectionState::Failed;
            return Err(SshError::SshError(format!(
                "Failed to request shell: {}",
                e
            )));
        }

        self.session = Some(session);
        self.channel = Some(channel);
        self.state = ConnectionState::Connected;

        tracing::info!("SSH connection established to {}", addr);
        Ok(())
    }

    /// Authenticate with the server using the configured method
    async fn authenticate(&self, session: &mut Handle<SshClientHandler>) -> SshResult<bool> {
        let username = &self.config.username;

        match &self.config.auth {
            AuthMethod::Password { password, .. } => {
                let password = password.as_ref().ok_or_else(|| {
                    SshError::AuthenticationFailed("Password not provided".to_string())
                })?;

                match session.authenticate_password(username, password).await {
                    Ok(result) => Ok(result),
                    Err(e) => Err(SshError::AuthenticationFailed(e.to_string())),
                }
            }

            AuthMethod::PrivateKey {
                path, passphrase, ..
            } => {
                let key = load_private_key(path, passphrase.as_deref())?;
                match session.authenticate_publickey(username, Arc::new(key)).await {
                    Ok(result) => Ok(result),
                    Err(e) => Err(SshError::AuthenticationFailed(e.to_string())),
                }
            }

            AuthMethod::Agent => {
                // Try to connect to SSH agent
                match self.authenticate_with_agent(session, username).await {
                    Ok(result) => Ok(result),
                    Err(e) => Err(SshError::AuthenticationFailed(format!(
                        "Agent authentication failed: {}",
                        e
                    ))),
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

    /// Write data to the SSH channel
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

    /// Read data from the SSH channel
    pub async fn read(&mut self, buf: &mut [u8]) -> SshResult<usize> {
        // First, check if we have buffered data
        if !self.read_buffer.is_empty() {
            let len = std::cmp::min(buf.len(), self.read_buffer.len());
            buf[..len].copy_from_slice(&self.read_buffer[..len]);
            self.read_buffer.drain(..len);
            return Ok(len);
        }

        let channel = self.channel.as_mut().ok_or(SshError::NotConnected)?;

        // Wait for data from the channel
        match channel.wait().await {
            Some(ChannelMsg::Data { data }) => {
                let len = std::cmp::min(buf.len(), data.len());
                buf[..len].copy_from_slice(&data[..len]);

                // Buffer any remaining data
                if data.len() > len {
                    self.read_buffer.extend_from_slice(&data[len..]);
                }

                Ok(len)
            }
            Some(ChannelMsg::ExtendedData { data, .. }) => {
                // Handle stderr data the same way
                let len = std::cmp::min(buf.len(), data.len());
                buf[..len].copy_from_slice(&data[..len]);

                if data.len() > len {
                    self.read_buffer.extend_from_slice(&data[len..]);
                }

                Ok(len)
            }
            Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => {
                self.state = ConnectionState::Disconnected;
                Ok(0)
            }
            Some(ChannelMsg::ExitStatus { exit_status }) => {
                tracing::info!("Remote process exited with status: {}", exit_status);
                self.state = ConnectionState::Disconnected;
                Ok(0)
            }
            Some(_) => {
                // Other message types, try again
                Ok(0)
            }
            None => {
                self.state = ConnectionState::Disconnected;
                Err(SshError::ChannelClosed)
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
