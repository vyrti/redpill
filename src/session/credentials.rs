use keyring::Entry;
use thiserror::Error;
use uuid::Uuid;

/// The service name used for keychain entries
const SERVICE_NAME: &str = "redpill-term";

/// Credential types stored in the keychain
#[derive(Debug, Clone, Copy)]
pub enum CredentialType {
    /// SSH password
    Password,
    /// SSH key passphrase
    Passphrase,
}

impl CredentialType {
    fn prefix(&self) -> &'static str {
        match self {
            CredentialType::Password => "password",
            CredentialType::Passphrase => "passphrase",
        }
    }
}

/// Errors that can occur during credential operations
#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("Keyring error: {0}")]
    KeyringError(String),

    #[error("Credential not found")]
    NotFound,
}

impl From<keyring::Error> for CredentialError {
    fn from(e: keyring::Error) -> Self {
        match e {
            keyring::Error::NoEntry => CredentialError::NotFound,
            other => CredentialError::KeyringError(other.to_string()),
        }
    }
}

/// Manages secure credential storage using the OS keychain
pub struct CredentialManager;

impl CredentialManager {
    /// Generate a keychain entry name for a session
    fn entry_name(session_id: Uuid, cred_type: CredentialType) -> String {
        format!("{}:{}", cred_type.prefix(), session_id)
    }

    /// Store a credential in the keychain
    pub fn store(
        session_id: Uuid,
        cred_type: CredentialType,
        secret: &str,
    ) -> Result<(), CredentialError> {
        let entry_name = Self::entry_name(session_id, cred_type);
        let entry = Entry::new(SERVICE_NAME, &entry_name)?;
        entry.set_password(secret)?;
        tracing::debug!("Stored credential for session {} ({:?})", session_id, cred_type);
        Ok(())
    }

    /// Retrieve a credential from the keychain
    pub fn retrieve(
        session_id: Uuid,
        cred_type: CredentialType,
    ) -> Result<String, CredentialError> {
        let entry_name = Self::entry_name(session_id, cred_type);
        let entry = Entry::new(SERVICE_NAME, &entry_name)?;
        let secret = entry.get_password()?;
        tracing::debug!("Retrieved credential for session {} ({:?})", session_id, cred_type);
        Ok(secret)
    }

    /// Delete a credential from the keychain
    pub fn delete(
        session_id: Uuid,
        cred_type: CredentialType,
    ) -> Result<(), CredentialError> {
        let entry_name = Self::entry_name(session_id, cred_type);
        let entry = Entry::new(SERVICE_NAME, &entry_name)?;
        entry.delete_credential()?;
        tracing::debug!("Deleted credential for session {} ({:?})", session_id, cred_type);
        Ok(())
    }

    /// Check if a credential exists in the keychain
    pub fn exists(session_id: Uuid, cred_type: CredentialType) -> bool {
        Self::retrieve(session_id, cred_type).is_ok()
    }

    /// Delete all credentials for a session (password and passphrase)
    pub fn delete_all(session_id: Uuid) {
        let _ = Self::delete(session_id, CredentialType::Password);
        let _ = Self::delete(session_id, CredentialType::Passphrase);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests interact with the real system keychain.
    // They are ignored by default to avoid polluting the keychain during CI.

    #[test]
    #[ignore]
    fn test_credential_roundtrip() {
        let session_id = Uuid::new_v4();
        let password = "test_password_12345";

        // Store
        CredentialManager::store(session_id, CredentialType::Password, password).unwrap();

        // Retrieve
        let retrieved = CredentialManager::retrieve(session_id, CredentialType::Password).unwrap();
        assert_eq!(retrieved, password);

        // Delete
        CredentialManager::delete(session_id, CredentialType::Password).unwrap();

        // Verify deletion
        assert!(!CredentialManager::exists(session_id, CredentialType::Password));
    }
}
