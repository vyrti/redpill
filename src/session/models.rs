use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

/// Authentication method for SSH connections
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AuthMethod {
    /// Password authentication
    Password {
        /// The password (None = prompt at connect time, or stored in keychain if use_keychain is true)
        #[serde(skip_serializing_if = "Option::is_none")]
        password: Option<String>,
        /// Whether to save the password to the OS keychain (not plaintext)
        #[serde(default)]
        use_keychain: bool,
    },
    /// Private key authentication
    PrivateKey {
        /// Path to the private key file
        path: PathBuf,
        /// Passphrase for encrypted keys (None = prompt if needed, or stored in keychain)
        #[serde(skip_serializing_if = "Option::is_none")]
        passphrase: Option<String>,
        /// Whether to save the passphrase to the OS keychain (not plaintext)
        #[serde(default)]
        use_keychain: bool,
    },
    /// SSH agent authentication
    Agent,
}

impl Default for AuthMethod {
    fn default() -> Self {
        AuthMethod::Agent
    }
}

/// An SSH session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshSession {
    /// Unique identifier
    pub id: Uuid,
    /// Display name for the session
    pub name: String,
    /// Hostname or IP address
    pub host: String,
    /// SSH port (default: 22)
    #[serde(default = "default_port")]
    pub port: u16,
    /// Username for authentication
    pub username: String,
    /// Authentication method
    pub auth: AuthMethod,
    /// Optional group membership
    pub group_id: Option<Uuid>,
    /// Optional color tag for visual identification
    pub color_tag: Option<String>,
    /// Optional color scheme override for this session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_scheme: Option<String>,
}

fn default_port() -> u16 {
    22
}

impl SshSession {
    /// Create a new SSH session with default values
    pub fn new(name: impl Into<String>, host: impl Into<String>, username: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            host: host.into(),
            port: 22,
            username: username.into(),
            auth: AuthMethod::default(),
            group_id: None,
            color_tag: None,
            color_scheme: None,
        }
    }

    /// Get the connection address string
    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Store credentials to the OS keychain if use_keychain is enabled.
    /// After storing, clears the in-memory password/passphrase to prevent
    /// it from being serialized to JSON.
    pub fn store_credentials_to_keychain(&mut self) {
        use super::credentials::{CredentialManager, CredentialType};

        match &mut self.auth {
            AuthMethod::Password { password, use_keychain } => {
                if *use_keychain {
                    if let Some(pwd) = password.take() {
                        if let Err(e) = CredentialManager::store(
                            self.id,
                            CredentialType::Password,
                            &pwd,
                        ) {
                            tracing::warn!("Failed to store password in keychain: {}", e);
                            // Restore the password so it can be saved in JSON as fallback
                            *password = Some(pwd);
                        }
                    }
                }
            }
            AuthMethod::PrivateKey { passphrase, use_keychain, .. } => {
                if *use_keychain {
                    if let Some(pp) = passphrase.take() {
                        if let Err(e) = CredentialManager::store(
                            self.id,
                            CredentialType::Passphrase,
                            &pp,
                        ) {
                            tracing::warn!("Failed to store passphrase in keychain: {}", e);
                            // Restore the passphrase so it can be saved in JSON as fallback
                            *passphrase = Some(pp);
                        }
                    }
                }
            }
            AuthMethod::Agent => {}
        }
    }

    /// Load credentials from the OS keychain if use_keychain is enabled
    /// and the credential is not already present in memory.
    pub fn load_credentials_from_keychain(&mut self) {
        use super::credentials::{CredentialManager, CredentialType};

        match &mut self.auth {
            AuthMethod::Password { password, use_keychain } => {
                if *use_keychain && password.is_none() {
                    match CredentialManager::retrieve(self.id, CredentialType::Password) {
                        Ok(pwd) => {
                            *password = Some(pwd);
                        }
                        Err(e) => {
                            tracing::debug!(
                                "No password found in keychain for session {}: {}",
                                self.id,
                                e
                            );
                        }
                    }
                }
            }
            AuthMethod::PrivateKey { passphrase, use_keychain, .. } => {
                if *use_keychain && passphrase.is_none() {
                    match CredentialManager::retrieve(self.id, CredentialType::Passphrase) {
                        Ok(pp) => {
                            *passphrase = Some(pp);
                        }
                        Err(e) => {
                            tracing::debug!(
                                "No passphrase found in keychain for session {}: {}",
                                self.id,
                                e
                            );
                        }
                    }
                }
            }
            AuthMethod::Agent => {}
        }
    }

    /// Delete credentials from the OS keychain for this session
    pub fn delete_credentials_from_keychain(&self) {
        use super::credentials::CredentialManager;
        CredentialManager::delete_all(self.id);
    }
}

/// A local terminal session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalSession {
    /// Unique identifier
    pub id: Uuid,
    /// Display name for the session
    pub name: String,
    /// Shell to use (None = default shell)
    pub shell: Option<String>,
    /// Working directory (None = home directory)
    pub working_dir: Option<PathBuf>,
    /// Additional environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Optional group membership
    pub group_id: Option<Uuid>,
}

impl Default for LocalSession {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "Local Terminal".to_string(),
            shell: None,
            working_dir: None,
            env: HashMap::new(),
            group_id: None,
        }
    }
}

impl LocalSession {
    /// Create a new local session with a custom name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            ..Default::default()
        }
    }
}

/// An AWS SSM Session Manager session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsmSession {
    /// Unique identifier
    pub id: Uuid,
    /// Display name for the session
    pub name: String,
    /// EC2 instance ID (i-xxx) or on-prem managed instance ID (mi-xxx)
    pub instance_id: String,
    /// AWS region (defaults to environment/config if None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// AWS profile name (defaults to "default" if None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Optional group membership
    pub group_id: Option<Uuid>,
    /// Optional color scheme override for this session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_scheme: Option<String>,
}

impl SsmSession {
    /// Create a new SSM session with default values
    pub fn new(name: impl Into<String>, instance_id: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            instance_id: instance_id.into(),
            region: None,
            profile: None,
            group_id: None,
            color_scheme: None,
        }
    }

    /// Create a new SSM session with region and profile
    pub fn with_config(
        name: impl Into<String>,
        instance_id: impl Into<String>,
        region: Option<String>,
        profile: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            instance_id: instance_id.into(),
            region,
            profile,
            group_id: None,
            color_scheme: None,
        }
    }
}

/// A session group for organizing sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionGroup {
    /// Unique identifier
    pub id: Uuid,
    /// Display name for the group
    pub name: String,
    /// Parent group ID for nested groups (None = top-level)
    pub parent_id: Option<Uuid>,
    /// Optional color for visual identification
    pub color: Option<String>,
}

impl SessionGroup {
    /// Create a new top-level group
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            parent_id: None,
            color: None,
        }
    }

    /// Create a new nested group
    pub fn new_nested(name: impl Into<String>, parent_id: Uuid) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            parent_id: Some(parent_id),
            color: None,
        }
    }
}

/// Union type for different session types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "session_type")]
pub enum Session {
    Ssh(SshSession),
    Local(LocalSession),
    Ssm(SsmSession),
}

impl Session {
    /// Get the session's unique ID
    pub fn id(&self) -> Uuid {
        match self {
            Session::Ssh(s) => s.id,
            Session::Local(s) => s.id,
            Session::Ssm(s) => s.id,
        }
    }

    /// Get the session's display name
    pub fn name(&self) -> &str {
        match self {
            Session::Ssh(s) => &s.name,
            Session::Local(s) => &s.name,
            Session::Ssm(s) => &s.name,
        }
    }

    /// Get the session's group ID
    pub fn group_id(&self) -> Option<Uuid> {
        match self {
            Session::Ssh(s) => s.group_id,
            Session::Local(s) => s.group_id,
            Session::Ssm(s) => s.group_id,
        }
    }

    /// Set the session's group ID
    pub fn set_group_id(&mut self, group_id: Option<Uuid>) {
        match self {
            Session::Ssh(s) => s.group_id = group_id,
            Session::Local(s) => s.group_id = group_id,
            Session::Ssm(s) => s.group_id = group_id,
        }
    }
}

/// The complete session data structure for persistence
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionData {
    /// All session groups
    #[serde(default)]
    pub groups: Vec<SessionGroup>,
    /// All sessions (SSH and local)
    #[serde(default)]
    pub sessions: Vec<Session>,
}

impl SessionData {
    /// Create empty session data
    pub fn new() -> Self {
        Self::default()
    }

    /// Find a session by ID
    pub fn find_session(&self, id: Uuid) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id() == id)
    }

    /// Find a session by ID (mutable)
    pub fn find_session_mut(&mut self, id: Uuid) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.id() == id)
    }

    /// Find a group by ID
    pub fn find_group(&self, id: Uuid) -> Option<&SessionGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Find a group by ID (mutable)
    pub fn find_group_mut(&mut self, id: Uuid) -> Option<&mut SessionGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// Get all sessions in a group
    pub fn sessions_in_group(&self, group_id: Uuid) -> Vec<&Session> {
        self.sessions
            .iter()
            .filter(|s| s.group_id() == Some(group_id))
            .collect()
    }

    /// Get all ungrouped sessions
    pub fn ungrouped_sessions(&self) -> Vec<&Session> {
        self.sessions
            .iter()
            .filter(|s| s.group_id().is_none())
            .collect()
    }

    /// Get child groups of a parent group
    pub fn child_groups(&self, parent_id: Option<Uuid>) -> Vec<&SessionGroup> {
        self.groups
            .iter()
            .filter(|g| g.parent_id == parent_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_session_creation() {
        let session = SshSession::new(
            "Test Server".to_string(),
            "192.168.1.1".to_string(),
            "admin".to_string(),
        );
        assert_eq!(session.name, "Test Server");
        assert_eq!(session.port, 22);
        assert_eq!(session.address(), "192.168.1.1:22");
    }

    #[test]
    fn test_session_data_operations() {
        let mut data = SessionData::new();

        let group = SessionGroup::new("Production".to_string());
        let group_id = group.id;
        data.groups.push(group);

        let mut session = SshSession::new(
            "web-server".to_string(),
            "10.0.0.1".to_string(),
            "root".to_string(),
        );
        session.group_id = Some(group_id);
        data.sessions.push(Session::Ssh(session));

        assert_eq!(data.sessions_in_group(group_id).len(), 1);
        assert_eq!(data.ungrouped_sessions().len(), 0);
    }
}
