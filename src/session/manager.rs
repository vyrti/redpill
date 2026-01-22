use thiserror::Error;
use uuid::Uuid;

use super::models::{Session, SessionData, SessionGroup, SshSession, LocalSession};
use super::storage::{SessionStorage, StorageError};

/// Errors that can occur during session management
#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Session not found: {0}")]
    SessionNotFound(Uuid),

    #[error("Group not found: {0}")]
    GroupNotFound(Uuid),

    #[error("Cannot delete group with children")]
    GroupHasChildren,

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

/// Manages sessions and groups, providing CRUD operations and persistence
pub struct SessionManager {
    /// The current session data
    data: SessionData,
    /// Storage backend for persistence
    storage: SessionStorage,
    /// Whether there are unsaved changes
    dirty: bool,
}

impl SessionManager {
    /// Create a new SessionManager, loading existing data from storage
    pub fn new() -> Result<Self, ManagerError> {
        let storage = SessionStorage::new()?;
        let mut data = storage.load()?;

        // Load credentials from keychain for SSH sessions
        Self::load_all_credentials(&mut data);

        Ok(Self {
            data,
            storage,
            dirty: false,
        })
    }

    /// Create a SessionManager with a custom storage backend
    pub fn with_storage(storage: SessionStorage) -> Result<Self, ManagerError> {
        let mut data = storage.load()?;

        // Load credentials from keychain for SSH sessions
        Self::load_all_credentials(&mut data);

        Ok(Self {
            data,
            storage,
            dirty: false,
        })
    }

    /// Load credentials from keychain for all SSH sessions
    fn load_all_credentials(data: &mut SessionData) {
        for session in &mut data.sessions {
            if let Session::Ssh(ssh_session) = session {
                ssh_session.load_credentials_from_keychain();
            }
        }
    }

    /// Get a reference to the current session data
    pub fn data(&self) -> &SessionData {
        &self.data
    }

    /// Check if there are unsaved changes
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    // === Session CRUD Operations ===

    /// Add a new SSH session
    pub fn add_ssh_session(&mut self, session: SshSession) -> Uuid {
        let id = session.id;
        self.data.sessions.push(Session::Ssh(session));
        self.dirty = true;
        tracing::info!("Added SSH session: {}", id);
        id
    }

    /// Add a new local session
    pub fn add_local_session(&mut self, session: LocalSession) -> Uuid {
        let id = session.id;
        self.data.sessions.push(Session::Local(session));
        self.dirty = true;
        tracing::info!("Added local session: {}", id);
        id
    }

    /// Get a session by ID
    pub fn get_session(&self, id: Uuid) -> Option<&Session> {
        self.data.find_session(id)
    }

    /// Get a mutable session by ID
    pub fn get_session_mut(&mut self, id: Uuid) -> Option<&mut Session> {
        self.dirty = true;
        self.data.find_session_mut(id)
    }

    /// Update an SSH session
    pub fn update_ssh_session(&mut self, id: Uuid, session: SshSession) -> Result<(), ManagerError> {
        let existing = self.data.sessions.iter_mut().find(|s| s.id() == id);
        match existing {
            Some(s) => {
                *s = Session::Ssh(session);
                self.dirty = true;
                Ok(())
            }
            None => Err(ManagerError::SessionNotFound(id)),
        }
    }

    /// Update a local session
    pub fn update_local_session(&mut self, id: Uuid, session: LocalSession) -> Result<(), ManagerError> {
        let existing = self.data.sessions.iter_mut().find(|s| s.id() == id);
        match existing {
            Some(s) => {
                *s = Session::Local(session);
                self.dirty = true;
                Ok(())
            }
            None => Err(ManagerError::SessionNotFound(id)),
        }
    }

    /// Delete a session
    pub fn delete_session(&mut self, id: Uuid) -> Result<Session, ManagerError> {
        let pos = self.data.sessions.iter().position(|s| s.id() == id);
        match pos {
            Some(index) => {
                let session = self.data.sessions.remove(index);

                // Delete credentials from keychain if this was an SSH session
                if let Session::Ssh(ref ssh_session) = session {
                    ssh_session.delete_credentials_from_keychain();
                }

                self.dirty = true;
                tracing::info!("Deleted session: {}", id);
                Ok(session)
            }
            None => Err(ManagerError::SessionNotFound(id)),
        }
    }

    /// Get all sessions
    pub fn all_sessions(&self) -> &[Session] {
        &self.data.sessions
    }

    /// Get sessions in a specific group
    pub fn sessions_in_group(&self, group_id: Uuid) -> Vec<&Session> {
        self.data.sessions_in_group(group_id)
    }

    /// Get ungrouped sessions
    pub fn ungrouped_sessions(&self) -> Vec<&Session> {
        self.data.ungrouped_sessions()
    }

    /// Move a session to a different group
    pub fn move_session_to_group(&mut self, session_id: Uuid, group_id: Option<Uuid>) -> Result<(), ManagerError> {
        // Verify group exists if specified
        if let Some(gid) = group_id {
            if self.data.find_group(gid).is_none() {
                return Err(ManagerError::GroupNotFound(gid));
            }
        }

        let session = self.data.find_session_mut(session_id)
            .ok_or(ManagerError::SessionNotFound(session_id))?;

        session.set_group_id(group_id);
        self.dirty = true;
        Ok(())
    }

    // === Group CRUD Operations ===

    /// Add a new group
    pub fn add_group(&mut self, group: SessionGroup) -> Uuid {
        let id = group.id;
        self.data.groups.push(group);
        self.dirty = true;
        tracing::info!("Added group: {}", id);
        id
    }

    /// Get a group by ID
    pub fn get_group(&self, id: Uuid) -> Option<&SessionGroup> {
        self.data.find_group(id)
    }

    /// Get a mutable group by ID
    pub fn get_group_mut(&mut self, id: Uuid) -> Option<&mut SessionGroup> {
        self.dirty = true;
        self.data.find_group_mut(id)
    }

    /// Update a group
    pub fn update_group(&mut self, id: Uuid, group: SessionGroup) -> Result<(), ManagerError> {
        let existing = self.data.groups.iter_mut().find(|g| g.id == id);
        match existing {
            Some(g) => {
                *g = group;
                self.dirty = true;
                Ok(())
            }
            None => Err(ManagerError::GroupNotFound(id)),
        }
    }

    /// Delete a group (fails if it has children)
    pub fn delete_group(&mut self, id: Uuid) -> Result<SessionGroup, ManagerError> {
        // Check for child groups
        if !self.data.child_groups(Some(id)).is_empty() {
            return Err(ManagerError::GroupHasChildren);
        }

        // Check for sessions in this group
        if !self.data.sessions_in_group(id).is_empty() {
            return Err(ManagerError::GroupHasChildren);
        }

        let pos = self.data.groups.iter().position(|g| g.id == id);
        match pos {
            Some(index) => {
                let group = self.data.groups.remove(index);
                self.dirty = true;
                tracing::info!("Deleted group: {}", id);
                Ok(group)
            }
            None => Err(ManagerError::GroupNotFound(id)),
        }
    }

    /// Delete a group and all its contents recursively
    pub fn delete_group_recursive(&mut self, id: Uuid) -> Result<(), ManagerError> {
        // First, recursively delete child groups
        let child_ids: Vec<Uuid> = self.data.child_groups(Some(id))
            .iter()
            .map(|g| g.id)
            .collect();

        for child_id in child_ids {
            self.delete_group_recursive(child_id)?;
        }

        // Delete sessions in this group
        self.data.sessions.retain(|s| s.group_id() != Some(id));

        // Delete the group itself
        self.data.groups.retain(|g| g.id != id);
        self.dirty = true;

        Ok(())
    }

    /// Get all groups
    pub fn all_groups(&self) -> &[SessionGroup] {
        &self.data.groups
    }

    /// Get top-level groups (no parent)
    pub fn top_level_groups(&self) -> Vec<&SessionGroup> {
        self.data.child_groups(None)
    }

    /// Get child groups of a parent
    pub fn child_groups(&self, parent_id: Uuid) -> Vec<&SessionGroup> {
        self.data.child_groups(Some(parent_id))
    }

    /// Move a group to a different parent
    pub fn move_group(&mut self, group_id: Uuid, new_parent_id: Option<Uuid>) -> Result<(), ManagerError> {
        // Verify new parent exists if specified
        if let Some(pid) = new_parent_id {
            if self.data.find_group(pid).is_none() {
                return Err(ManagerError::GroupNotFound(pid));
            }

            // Prevent circular references
            if pid == group_id {
                return Err(ManagerError::InvalidOperation(
                    "Cannot make a group its own parent".to_string()
                ));
            }

            // Check if new_parent_id is a descendant of group_id
            if self.is_descendant(pid, group_id) {
                return Err(ManagerError::InvalidOperation(
                    "Cannot move a group to one of its descendants".to_string()
                ));
            }
        }

        let group = self.data.find_group_mut(group_id)
            .ok_or(ManagerError::GroupNotFound(group_id))?;

        group.parent_id = new_parent_id;
        self.dirty = true;
        Ok(())
    }

    /// Check if `potential_descendant` is a descendant of `ancestor`
    fn is_descendant(&self, potential_descendant: Uuid, ancestor: Uuid) -> bool {
        let mut current = self.data.find_group(potential_descendant);
        while let Some(group) = current {
            if let Some(parent_id) = group.parent_id {
                if parent_id == ancestor {
                    return true;
                }
                current = self.data.find_group(parent_id);
            } else {
                break;
            }
        }
        false
    }

    // === Mass Connect ===

    /// Get all session IDs in a group (including nested groups)
    pub fn get_all_sessions_in_group_recursive(&self, group_id: Uuid) -> Vec<Uuid> {
        let mut session_ids = Vec::new();

        // Add sessions directly in this group
        for session in self.data.sessions_in_group(group_id) {
            session_ids.push(session.id());
        }

        // Recursively add sessions from child groups
        for child_group in self.data.child_groups(Some(group_id)) {
            session_ids.extend(self.get_all_sessions_in_group_recursive(child_group.id));
        }

        session_ids
    }

    // === Persistence ===

    /// Save changes to storage
    pub fn save(&mut self) -> Result<(), ManagerError> {
        // Store credentials to keychain before saving
        // This clears passwords from memory so they don't get serialized to JSON
        for session in &mut self.data.sessions {
            if let Session::Ssh(ssh_session) = session {
                ssh_session.store_credentials_to_keychain();
            }
        }

        self.storage.save(&self.data)?;
        self.dirty = false;

        // Reload credentials from keychain so they're available in memory
        Self::load_all_credentials(&mut self.data);

        Ok(())
    }

    /// Reload data from storage, discarding unsaved changes
    pub fn reload(&mut self) -> Result<(), ManagerError> {
        self.data = self.storage.load()?;
        self.dirty = false;
        Ok(())
    }

    /// Create a backup of the current sessions file
    pub fn backup(&self) -> Result<std::path::PathBuf, ManagerError> {
        Ok(self.storage.backup()?)
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new().expect("Failed to create SessionManager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_manager() -> SessionManager {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_sessions.json");
        let storage = SessionStorage::with_path(file_path);
        SessionManager::with_storage(storage).unwrap()
    }

    #[test]
    fn test_session_crud() {
        let mut manager = create_test_manager();

        // Create
        let session = SshSession::new(
            "Test".to_string(),
            "localhost".to_string(),
            "user".to_string(),
        );
        let id = manager.add_ssh_session(session);

        // Read
        let retrieved = manager.get_session(id).unwrap();
        assert_eq!(retrieved.name(), "Test");

        // Update
        let mut updated = SshSession::new(
            "Updated".to_string(),
            "localhost".to_string(),
            "user".to_string(),
        );
        updated.id = id;
        manager.update_ssh_session(id, updated).unwrap();

        let retrieved = manager.get_session(id).unwrap();
        assert_eq!(retrieved.name(), "Updated");

        // Delete
        manager.delete_session(id).unwrap();
        assert!(manager.get_session(id).is_none());
    }

    #[test]
    fn test_group_operations() {
        let mut manager = create_test_manager();

        // Create parent group
        let parent = SessionGroup::new("Parent".to_string());
        let parent_id = manager.add_group(parent);

        // Create child group
        let child = SessionGroup::new_nested("Child".to_string(), parent_id);
        let child_id = manager.add_group(child);

        // Verify hierarchy
        assert_eq!(manager.child_groups(parent_id).len(), 1);
        assert_eq!(manager.top_level_groups().len(), 1);

        // Cannot delete parent with children
        assert!(manager.delete_group(parent_id).is_err());

        // Can delete child
        manager.delete_group(child_id).unwrap();

        // Now can delete parent
        manager.delete_group(parent_id).unwrap();
    }

    #[test]
    fn test_mass_connect() {
        let mut manager = create_test_manager();

        // Create group
        let group = SessionGroup::new("Servers".to_string());
        let group_id = manager.add_group(group);

        // Add sessions to group
        for i in 0..3 {
            let mut session = SshSession::new(
                format!("Server{}", i),
                format!("10.0.0.{}", i),
                "admin".to_string(),
            );
            session.group_id = Some(group_id);
            manager.add_ssh_session(session);
        }

        // Get all sessions for mass connect
        let session_ids = manager.get_all_sessions_in_group_recursive(group_id);
        assert_eq!(session_ids.len(), 3);
    }
}
