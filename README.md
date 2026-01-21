# RedPill - SSH / Kubernetes Terminal Manager

A cross-platform GUI terminal application with SSH session management, session groups, and mass connect functionality built with Rust.

## Features

- **SSH Session Management**: Save and organize SSH connections
- **Session Groups**: Organize sessions into hierarchical groups
- **Mass Connect**: Connect to all sessions in a group with one click
- **Local Terminals**: Run local shell sessions
- **Tab Interface**: Multiple terminals in tabs
- **Persistent Configuration**: Sessions and settings saved to JSON

## Tech Stack

- **UI Framework**: gpui
- **Terminal Emulation**: alacritty_terminal
- **SSH Client**: russh
- **Async Runtime**: tokio

## Project Structure

```
redpill/
├── Cargo.toml
├── src/
│   ├── main.rs                 # App entry, gpui initialization
│   ├── app.rs                  # Main application state
│   ├── config.rs               # App configuration
│   ├── ui/
│   │   ├── mod.rs
│   │   ├── main_window.rs      # Main window with dock layout
│   │   ├── session_tree.rs     # Left panel: session/group tree
│   │   ├── terminal_tabs.rs    # Tab bar for open terminals
│   │   ├── terminal_view.rs    # Terminal rendering element
│   │   ├── session_dialog.rs   # Add/edit session dialog
│   │   └── group_dialog.rs     # Add/edit group dialog
│   ├── terminal/
│   │   ├── mod.rs
│   │   ├── terminal.rs         # Terminal wrapper (alacritty_terminal)
│   │   ├── backend.rs          # Backend trait definition
│   │   ├── ssh_backend.rs      # SSH connection backend (russh)
│   │   └── local_backend.rs    # Local PTY backend (portable-pty)
│   └── session/
│       ├── mod.rs
│       ├── models.rs           # Session, Group, Config structs
│       ├── manager.rs          # Session CRUD, persistence
│       └── storage.rs          # JSON file storage
```

## Configuration

Configuration is stored in:
- macOS: `~/.config/redpill/`
- Linux: `~/.config/redpill/`
- Windows: `%APPDATA%\redpill\`

### Sessions File (`sessions.json`)

```json
{
  "groups": [
    {
      "id": "uuid",
      "name": "Production",
      "parent_id": null,
      "color": "#ff5555"
    }
  ],
  "sessions": [
    {
      "session_type": "Ssh",
      "id": "uuid",
      "name": "web-server-1",
      "host": "192.168.1.100",
      "port": 22,
      "username": "admin",
      "auth": { "type": "PrivateKey", "path": "~/.ssh/id_rsa" },
      "group_id": "uuid"
    }
  ]
}
```

## Usage

### Keyboard Shortcuts

- `Ctrl+Shift+T`: New local terminal
- `Ctrl+Shift+W`: Close current tab
- `Ctrl+Tab`: Next tab
- `Ctrl+Shift+Tab`: Previous tab
- `Ctrl+B`: Toggle session tree
- `Ctrl+Shift+C`: Copy
- `Ctrl+Shift+V`: Paste

### Session Tree

- Double-click a session to connect
- Double-click a group to mass connect to all sessions
- Right-click for context menu options

## License

Dual-licensed under either:
- [MIT License](LICENSE-MIT)
or
- [Apache License 2.0](LICENSE-APACHE)
at your choise.
