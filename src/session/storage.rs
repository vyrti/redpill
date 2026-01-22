use std::fs;
use std::path::PathBuf;
use thiserror::Error;

use super::models::SessionData;

/// Errors that can occur during session storage operations
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Failed to read sessions file: {0}")]
    ReadError(#[from] std::io::Error),

    #[error("Failed to parse sessions file: {0}")]
    ParseError(#[from] serde_json::Error),

    #[error("Config directory not found")]
    ConfigDirNotFound,
}

/// Handles persistence of session data to JSON files
pub struct SessionStorage {
    /// Path to the sessions.json file
    file_path: PathBuf,
}

impl SessionStorage {
    /// Create a new SessionStorage with the default path
    /// (~/.config/redpill/sessions.json on Unix, %APPDATA%\redpill\sessions.json on Windows)
    pub fn new() -> Result<Self, StorageError> {
        let config_dir = Self::config_dir()?;
        Ok(Self {
            file_path: config_dir.join("sessions.json"),
        })
    }

    /// Create a SessionStorage with a custom file path
    pub fn with_path(file_path: PathBuf) -> Self {
        Self { file_path }
    }

    /// Get the configuration directory path
    pub fn config_dir() -> Result<PathBuf, StorageError> {
        let config_dir = dirs::config_dir()
            .ok_or(StorageError::ConfigDirNotFound)?
            .join("redpill");

        // Create the directory if it doesn't exist
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)?;
        }

        Ok(config_dir)
    }

    /// Load session data from disk
    pub fn load(&self) -> Result<SessionData, StorageError> {
        if !self.file_path.exists() {
            tracing::info!("Sessions file not found, returning empty data");
            return Ok(SessionData::new());
        }

        let contents = fs::read_to_string(&self.file_path)?;
        let data: SessionData = serde_json::from_str(&contents)?;

        tracing::info!(
            "Loaded {} sessions and {} groups from {:?}",
            data.sessions.len(),
            data.groups.len(),
            self.file_path
        );

        Ok(data)
    }

    /// Save session data to disk
    pub fn save(&self, data: &SessionData) -> Result<(), StorageError> {
        // Ensure parent directory exists
        if let Some(parent) = self.file_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        let contents = serde_json::to_string_pretty(data)?;
        fs::write(&self.file_path, contents)?;

        tracing::info!(
            "Saved {} sessions and {} groups to {:?}",
            data.sessions.len(),
            data.groups.len(),
            self.file_path
        );

        Ok(())
    }

    /// Get the path to the sessions file
    pub fn file_path(&self) -> &PathBuf {
        &self.file_path
    }

    /// Check if the sessions file exists
    #[must_use]
    pub fn exists(&self) -> bool {
        self.file_path.exists()
    }

    /// Create a backup of the current sessions file
    pub fn backup(&self) -> Result<PathBuf, StorageError> {
        if !self.file_path.exists() {
            return Ok(self.file_path.clone());
        }

        let backup_path = self.file_path.with_extension("json.backup");
        fs::copy(&self.file_path, &backup_path)?;

        tracing::info!("Created backup at {:?}", backup_path);
        Ok(backup_path)
    }
}

impl Default for SessionStorage {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| Self::with_path(PathBuf::from("sessions.json")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::models::{Session, SessionGroup, SshSession};
    use std::env;
    use tempfile::tempdir;

    #[test]
    fn test_storage_roundtrip() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_sessions.json");
        let storage = SessionStorage::with_path(file_path);

        // Create test data
        let mut data = SessionData::new();
        let group = SessionGroup::new("Test Group".to_string());
        data.groups.push(group);

        let session = SshSession::new(
            "Test Server".to_string(),
            "localhost".to_string(),
            "user".to_string(),
        );
        data.sessions.push(Session::Ssh(session));

        // Save and reload
        storage.save(&data).unwrap();
        let loaded = storage.load().unwrap();

        assert_eq!(loaded.groups.len(), 1);
        assert_eq!(loaded.sessions.len(), 1);
        assert_eq!(loaded.groups[0].name, "Test Group");
    }

    #[test]
    fn test_load_nonexistent_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("nonexistent.json");
        let storage = SessionStorage::with_path(file_path);

        let data = storage.load().unwrap();
        assert!(data.sessions.is_empty());
        assert!(data.groups.is_empty());
    }
}
