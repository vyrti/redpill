//! Test utilities for RedPill
//!
//! This module provides helper functions for creating test fixtures
//! and managing test environments.

use redpill::session::{
    AuthMethod, LocalSession, Session, SessionGroup, SessionManager, SessionStorage, SshSession,
};
use std::path::PathBuf;
use tempfile::TempDir;
use uuid::Uuid;

/// Test context that manages temporary directories and cleanup
pub struct TestContext {
    pub temp_dir: TempDir,
}

impl TestContext {
    /// Create a new test context with a temporary directory
    pub fn new() -> Self {
        Self {
            temp_dir: TempDir::new().expect("Failed to create temp directory"),
        }
    }

    /// Get the path to the sessions file
    pub fn sessions_path(&self) -> PathBuf {
        self.temp_dir.path().join("sessions.json")
    }

    /// Create a session manager using the temp directory
    pub fn create_session_manager(&self) -> SessionManager {
        let storage = SessionStorage::with_path(self.sessions_path());
        SessionManager::with_storage(storage).expect("Failed to create session manager")
    }
}

/// Create a test SSH session with the given name and host
pub fn create_test_ssh_session(name: &str, host: &str) -> SshSession {
    SshSession::new(name.to_string(), host.to_string(), "testuser".to_string())
}

/// Create a test SSH session with a specific group
pub fn create_test_ssh_session_in_group(name: &str, host: &str, group_id: Uuid) -> SshSession {
    let mut session = create_test_ssh_session(name, host);
    session.group_id = Some(group_id);
    session
}

/// Create a test SSH session with password auth
pub fn create_test_ssh_session_with_password(name: &str, host: &str, password: &str) -> SshSession {
    let mut session = create_test_ssh_session(name, host);
    session.auth = AuthMethod::Password {
        password: Some(password.to_string()),
        save_password: false,
    };
    session
}

/// Create a test SSH session with key auth
pub fn create_test_ssh_session_with_key(name: &str, host: &str, key_path: &str) -> SshSession {
    let mut session = create_test_ssh_session(name, host);
    session.auth = AuthMethod::PrivateKey {
        path: PathBuf::from(key_path),
        passphrase: None,
        save_passphrase: false,
    };
    session
}

/// Create a test local session
pub fn create_test_local_session(name: &str) -> LocalSession {
    LocalSession::new(name.to_string())
}

/// Create a test local session in a group
pub fn create_test_local_session_in_group(name: &str, group_id: Uuid) -> LocalSession {
    let mut session = LocalSession::new(name.to_string());
    session.group_id = Some(group_id);
    session
}

/// Create a test session group
pub fn create_test_group(name: &str) -> SessionGroup {
    SessionGroup::new(name.to_string())
}

/// Create a test session group with a color
pub fn create_test_group_with_color(name: &str, color: &str) -> SessionGroup {
    let mut group = SessionGroup::new(name.to_string());
    group.color = Some(color.to_string());
    group
}

/// Create a nested test session group
pub fn create_test_nested_group(name: &str, parent_id: Uuid) -> SessionGroup {
    SessionGroup::new_nested(name.to_string(), parent_id)
}

/// Populate a session manager with test data
pub fn populate_test_data(manager: &mut SessionManager) -> TestData {
    // Create groups
    let prod_group = create_test_group_with_color("Production", "#f38ba8");
    let prod_id = manager.add_group(prod_group);

    let staging_group = create_test_group_with_color("Staging", "#f9e2af");
    let staging_id = manager.add_group(staging_group);

    let dev_group = create_test_group_with_color("Development", "#a6e3a1");
    let dev_id = manager.add_group(dev_group);

    // Create nested group under production
    let prod_db = create_test_nested_group("Databases", prod_id);
    let prod_db_id = manager.add_group(prod_db);

    // Create SSH sessions
    let web1 = create_test_ssh_session_in_group("web-server-1", "10.0.1.1", prod_id);
    let web1_id = manager.add_ssh_session(web1);

    let web2 = create_test_ssh_session_in_group("web-server-2", "10.0.1.2", prod_id);
    let web2_id = manager.add_ssh_session(web2);

    let db1 = create_test_ssh_session_in_group("mysql-primary", "10.0.2.1", prod_db_id);
    let db1_id = manager.add_ssh_session(db1);

    let staging_web = create_test_ssh_session_in_group("staging-web", "10.0.10.1", staging_id);
    let staging_web_id = manager.add_ssh_session(staging_web);

    let dev_local = create_test_ssh_session_in_group("dev-server", "192.168.1.100", dev_id);
    let dev_local_id = manager.add_ssh_session(dev_local);

    // Create an ungrouped session
    let ungrouped = create_test_ssh_session("personal-server", "example.com");
    let ungrouped_id = manager.add_ssh_session(ungrouped);

    // Create a local session
    let local = create_test_local_session("Local Terminal");
    let local_id = manager.add_local_session(local);

    TestData {
        prod_group_id: prod_id,
        staging_group_id: staging_id,
        dev_group_id: dev_id,
        prod_db_group_id: prod_db_id,
        web1_id,
        web2_id,
        db1_id,
        staging_web_id,
        dev_local_id,
        ungrouped_id,
        local_id,
    }
}

/// Test data IDs for reference
pub struct TestData {
    pub prod_group_id: Uuid,
    pub staging_group_id: Uuid,
    pub dev_group_id: Uuid,
    pub prod_db_group_id: Uuid,
    pub web1_id: Uuid,
    pub web2_id: Uuid,
    pub db1_id: Uuid,
    pub staging_web_id: Uuid,
    pub dev_local_id: Uuid,
    pub ungrouped_id: Uuid,
    pub local_id: Uuid,
}
