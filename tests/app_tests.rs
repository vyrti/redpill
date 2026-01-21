//! Comprehensive tests for RedPill application
//!
//! These tests cover the core functionality of the RedPillApp and related components.

mod test_utils;

use redpill::session::{SessionManager, SessionStorage, SshSession, LocalSession, SessionGroup};
use redpill::config::AppConfig;
use tempfile::TempDir;
use uuid::Uuid;

use test_utils::*;

// ============================================================================
// Session Manager State Tests
// ============================================================================

#[test]
fn test_session_manager_initial_state() {
    let ctx = TestContext::new();
    let manager = ctx.create_session_manager();

    assert!(manager.all_sessions().is_empty());
    assert!(manager.all_groups().is_empty());
    assert!(!manager.is_dirty());
}

#[test]
fn test_session_manager_top_level_groups_empty() {
    let ctx = TestContext::new();
    let manager = ctx.create_session_manager();

    assert!(manager.top_level_groups().is_empty());
}

#[test]
fn test_session_manager_ungrouped_sessions_empty() {
    let ctx = TestContext::new();
    let manager = ctx.create_session_manager();

    assert!(manager.ungrouped_sessions().is_empty());
}

// ============================================================================
// Session CRUD Tests
// ============================================================================

#[test]
fn test_add_ssh_session() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let session = create_test_ssh_session("Test Server", "192.168.1.1");
    let id = manager.add_ssh_session(session);

    assert!(manager.get_session(id).is_some());
    assert_eq!(manager.all_sessions().len(), 1);
    assert!(manager.is_dirty());
}

#[test]
fn test_add_local_session() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let session = create_test_local_session("My Terminal");
    let id = manager.add_local_session(session);

    assert!(manager.get_session(id).is_some());
    assert_eq!(manager.all_sessions().len(), 1);
}

#[test]
fn test_get_session_found() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let session = create_test_ssh_session("Test", "localhost");
    let id = manager.add_ssh_session(session);

    let retrieved = manager.get_session(id);
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().name(), "Test");
}

#[test]
fn test_get_session_not_found() {
    let ctx = TestContext::new();
    let manager = ctx.create_session_manager();

    let fake_id = Uuid::new_v4();
    assert!(manager.get_session(fake_id).is_none());
}

#[test]
fn test_delete_session_success() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let session = create_test_ssh_session("Delete Me", "localhost");
    let id = manager.add_ssh_session(session);

    assert!(manager.delete_session(id).is_ok());
    assert!(manager.get_session(id).is_none());
    assert_eq!(manager.all_sessions().len(), 0);
}

#[test]
fn test_delete_session_not_found() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let fake_id = Uuid::new_v4();
    assert!(manager.delete_session(fake_id).is_err());
}

#[test]
fn test_update_ssh_session() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let session = create_test_ssh_session("Original", "localhost");
    let id = manager.add_ssh_session(session);

    let mut updated = create_test_ssh_session("Updated", "newhost");
    updated.id = id;
    assert!(manager.update_ssh_session(id, updated).is_ok());

    let retrieved = manager.get_session(id).unwrap();
    assert_eq!(retrieved.name(), "Updated");
}

#[test]
fn test_sessions_in_group() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let group = create_test_group("Test Group");
    let group_id = manager.add_group(group);

    let session1 = create_test_ssh_session_in_group("Server 1", "10.0.0.1", group_id);
    let session2 = create_test_ssh_session_in_group("Server 2", "10.0.0.2", group_id);
    let session3 = create_test_ssh_session("Ungrouped", "10.0.0.3");

    manager.add_ssh_session(session1);
    manager.add_ssh_session(session2);
    manager.add_ssh_session(session3);

    let in_group = manager.sessions_in_group(group_id);
    assert_eq!(in_group.len(), 2);
}

#[test]
fn test_ungrouped_sessions() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let group = create_test_group("Test Group");
    let group_id = manager.add_group(group);

    let grouped = create_test_ssh_session_in_group("Grouped", "10.0.0.1", group_id);
    let ungrouped1 = create_test_ssh_session("Ungrouped 1", "10.0.0.2");
    let ungrouped2 = create_test_ssh_session("Ungrouped 2", "10.0.0.3");

    manager.add_ssh_session(grouped);
    manager.add_ssh_session(ungrouped1);
    manager.add_ssh_session(ungrouped2);

    let ungrouped = manager.ungrouped_sessions();
    assert_eq!(ungrouped.len(), 2);
}

// ============================================================================
// Group CRUD Tests
// ============================================================================

#[test]
fn test_add_group_top_level() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let group = create_test_group("Production");
    let id = manager.add_group(group);

    assert!(manager.get_group(id).is_some());
    assert_eq!(manager.top_level_groups().len(), 1);
}

#[test]
fn test_add_group_nested() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let parent = create_test_group("Parent");
    let parent_id = manager.add_group(parent);

    let child = create_test_nested_group("Child", parent_id);
    let child_id = manager.add_group(child);

    assert!(manager.get_group(child_id).is_some());
    assert_eq!(manager.child_groups(parent_id).len(), 1);
}

#[test]
fn test_top_level_groups_populated() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    manager.add_group(create_test_group("Group 1"));
    manager.add_group(create_test_group("Group 2"));
    manager.add_group(create_test_group("Group 3"));

    assert_eq!(manager.top_level_groups().len(), 3);
}

#[test]
fn test_child_groups_empty() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let group = create_test_group("Parent");
    let group_id = manager.add_group(group);

    assert!(manager.child_groups(group_id).is_empty());
}

#[test]
fn test_child_groups_populated() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let parent = create_test_group("Parent");
    let parent_id = manager.add_group(parent);

    manager.add_group(create_test_nested_group("Child 1", parent_id));
    manager.add_group(create_test_nested_group("Child 2", parent_id));

    assert_eq!(manager.child_groups(parent_id).len(), 2);
}

#[test]
fn test_delete_group_empty() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let group = create_test_group("Empty Group");
    let id = manager.add_group(group);

    assert!(manager.delete_group(id).is_ok());
    assert!(manager.get_group(id).is_none());
}

#[test]
fn test_delete_group_with_children_fails() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let parent = create_test_group("Parent");
    let parent_id = manager.add_group(parent);

    manager.add_group(create_test_nested_group("Child", parent_id));

    // Should fail because group has children
    assert!(manager.delete_group(parent_id).is_err());
}

#[test]
fn test_delete_group_with_sessions_fails() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let group = create_test_group("Group with Sessions");
    let group_id = manager.add_group(group);

    let session = create_test_ssh_session_in_group("Server", "localhost", group_id);
    manager.add_ssh_session(session);

    // Should fail because group has sessions
    assert!(manager.delete_group(group_id).is_err());
}

#[test]
fn test_delete_group_recursive() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let parent = create_test_group("Parent");
    let parent_id = manager.add_group(parent);

    let child = create_test_nested_group("Child", parent_id);
    let child_id = manager.add_group(child);

    let session = create_test_ssh_session_in_group("Server", "localhost", child_id);
    manager.add_ssh_session(session);

    // Recursive delete should work
    assert!(manager.delete_group_recursive(parent_id).is_ok());
    assert!(manager.get_group(parent_id).is_none());
    assert!(manager.get_group(child_id).is_none());
    assert!(manager.all_sessions().is_empty());
}

// ============================================================================
// Session Movement Tests
// ============================================================================

#[test]
fn test_move_session_to_group() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let group = create_test_group("Target Group");
    let group_id = manager.add_group(group);

    let session = create_test_ssh_session("Server", "localhost");
    let session_id = manager.add_ssh_session(session);

    assert!(manager.move_session_to_group(session_id, Some(group_id)).is_ok());

    let session = manager.get_session(session_id).unwrap();
    assert_eq!(session.group_id(), Some(group_id));
}

#[test]
fn test_move_session_to_ungrouped() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let group = create_test_group("Group");
    let group_id = manager.add_group(group);

    let session = create_test_ssh_session_in_group("Server", "localhost", group_id);
    let session_id = manager.add_ssh_session(session);

    assert!(manager.move_session_to_group(session_id, None).is_ok());

    let session = manager.get_session(session_id).unwrap();
    assert!(session.group_id().is_none());
}

// ============================================================================
// Mass Connect Tests
// ============================================================================

#[test]
fn test_mass_connect_sessions_in_group() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let group = create_test_group("Servers");
    let group_id = manager.add_group(group);

    for i in 0..5 {
        let session = create_test_ssh_session_in_group(
            &format!("Server {}", i),
            &format!("10.0.0.{}", i),
            group_id,
        );
        manager.add_ssh_session(session);
    }

    let sessions = manager.get_all_sessions_in_group_recursive(group_id);
    assert_eq!(sessions.len(), 5);
}

#[test]
fn test_mass_connect_includes_nested_groups() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let parent = create_test_group("Parent");
    let parent_id = manager.add_group(parent);

    let child = create_test_nested_group("Child", parent_id);
    let child_id = manager.add_group(child);

    // Add sessions to both groups
    manager.add_ssh_session(create_test_ssh_session_in_group("Parent Server", "10.0.0.1", parent_id));
    manager.add_ssh_session(create_test_ssh_session_in_group("Child Server 1", "10.0.0.2", child_id));
    manager.add_ssh_session(create_test_ssh_session_in_group("Child Server 2", "10.0.0.3", child_id));

    let sessions = manager.get_all_sessions_in_group_recursive(parent_id);
    assert_eq!(sessions.len(), 3);
}

#[test]
fn test_mass_connect_empty_group() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let group = create_test_group("Empty");
    let group_id = manager.add_group(group);

    let sessions = manager.get_all_sessions_in_group_recursive(group_id);
    assert!(sessions.is_empty());
}

// ============================================================================
// Persistence Tests
// ============================================================================

#[test]
fn test_save_and_reload() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    // Add some data
    let group = create_test_group("Persistent Group");
    let group_id = manager.add_group(group);

    let session = create_test_ssh_session_in_group("Persistent Server", "localhost", group_id);
    let session_id = manager.add_ssh_session(session);

    // Save
    assert!(manager.save().is_ok());
    assert!(!manager.is_dirty());

    // Create new manager from same storage
    let manager2 = ctx.create_session_manager();

    assert!(manager2.get_group(group_id).is_some());
    assert!(manager2.get_session(session_id).is_some());
}

#[test]
fn test_reload_discards_changes() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    // Add and save
    let session1 = create_test_ssh_session("Original", "localhost");
    let id1 = manager.add_ssh_session(session1);
    manager.save().unwrap();

    // Add more (unsaved)
    let session2 = create_test_ssh_session("Unsaved", "localhost");
    let id2 = manager.add_ssh_session(session2);

    assert!(manager.is_dirty());

    // Reload should discard unsaved changes
    manager.reload().unwrap();

    assert!(manager.get_session(id1).is_some());
    assert!(manager.get_session(id2).is_none());
    assert!(!manager.is_dirty());
}

// ============================================================================
// Config Tests
// ============================================================================

#[test]
fn test_config_defaults() {
    let config = AppConfig::default();

    assert_eq!(config.window.width, 1200);
    assert_eq!(config.window.height, 800);
    assert_eq!(config.appearance.font_size, 13.0);
    assert_eq!(config.scrollback_lines, 10000);
    assert!(config.confirm_close);
}

#[test]
fn test_config_zoom_in() {
    let mut config = AppConfig::default();
    let initial = config.appearance.font_size;

    config.appearance.zoom_in();

    assert_eq!(config.appearance.font_size, initial + 1.0);
}

#[test]
fn test_config_zoom_out() {
    let mut config = AppConfig::default();
    let initial = config.appearance.font_size;

    config.appearance.zoom_out();

    assert_eq!(config.appearance.font_size, initial - 1.0);
}

#[test]
fn test_config_zoom_in_max() {
    let mut config = AppConfig::default();
    config.appearance.font_size = config.appearance.max_font_size;

    config.appearance.zoom_in();

    assert_eq!(config.appearance.font_size, config.appearance.max_font_size);
}

#[test]
fn test_config_zoom_out_min() {
    let mut config = AppConfig::default();
    config.appearance.font_size = config.appearance.min_font_size;

    config.appearance.zoom_out();

    assert_eq!(config.appearance.font_size, config.appearance.min_font_size);
}

#[test]
fn test_config_zoom_reset() {
    let mut config = AppConfig::default();
    config.appearance.font_size = 20.0;

    config.appearance.zoom_reset();

    assert_eq!(config.appearance.font_size, 13.0);
}

#[test]
fn test_config_serialization() {
    let config = AppConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let parsed: AppConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(config.window.width, parsed.window.width);
    assert_eq!(config.appearance.font_family, parsed.appearance.font_family);
}

// ============================================================================
// Integration Tests with Test Data
// ============================================================================

#[test]
fn test_populated_session_manager() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let data = populate_test_data(&mut manager);

    // Verify groups
    assert_eq!(manager.top_level_groups().len(), 3);
    assert_eq!(manager.child_groups(data.prod_group_id).len(), 1);

    // Verify sessions
    assert_eq!(manager.sessions_in_group(data.prod_group_id).len(), 2);
    assert_eq!(manager.sessions_in_group(data.prod_db_group_id).len(), 1);
    assert_eq!(manager.ungrouped_sessions().len(), 2); // ungrouped ssh + local
}

#[test]
fn test_group_hierarchy() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let data = populate_test_data(&mut manager);

    // Verify nested structure
    let prod_db = manager.get_group(data.prod_db_group_id).unwrap();
    assert_eq!(prod_db.parent_id, Some(data.prod_group_id));

    // Production should be top-level
    let prod = manager.get_group(data.prod_group_id).unwrap();
    assert!(prod.parent_id.is_none());
}

#[test]
fn test_mass_connect_production() {
    let ctx = TestContext::new();
    let mut manager = ctx.create_session_manager();

    let data = populate_test_data(&mut manager);

    // Mass connect should get all production sessions including nested
    let sessions = manager.get_all_sessions_in_group_recursive(data.prod_group_id);

    // Should include: web1, web2, db1
    assert_eq!(sessions.len(), 3);
}
