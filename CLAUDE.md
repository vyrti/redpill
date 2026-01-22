# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build              # Build (never use --release flag)
cargo run                # Run
cargo test               # Run all tests
cargo test session::     # Run tests in session module
cargo test --lib         # Library tests only
```

## Code Guidelines

**Zero-copy**: Prefer `&str` over `String`, `&[u8]` over `Vec<u8>`, use `Cow<'_, str>` when ownership is conditional. Avoid `.clone()` unless necessary.

**Atomics**: Use `AtomicBool`, `AtomicUsize` etc. for cross-thread flags/counters instead of `Mutex<bool>` or `Mutex<usize>`. Use appropriate `Ordering` (`Relaxed` for counters, `Acquire`/`Release` for synchronization, `SeqCst` only when necessary).

**Rust best practices**:
- Prefer `impl Trait` over `Box<dyn Trait>` when possible
- Use `?` for error propagation, avoid `.unwrap()` except in tests
- Prefer iterators over index loops
- Use `#[must_use]` on functions returning values that shouldn't be ignored
- Derive traits in standard order: `Debug, Clone, Copy, PartialEq, Eq, Hash, Default`

## Architecture

RedPill is a cross-platform GUI terminal with SSH session management, built with GPUI (Zed's UI framework).

### Module Structure

- **`src/app.rs`** - Core application state (`RedPillApp`), tab management, SSH I/O loop (`spawn_ssh_io_loop`)
- **`src/session/`** - Session/group data models, CRUD operations, JSON persistence, OS keychain credential storage
- **`src/terminal/`** - Terminal emulation wrapping `alacritty_terminal`, dual-mode (local PTY / SSH)
- **`src/ui/`** - GPUI components: main window, terminal rendering, session tree, dialogs

### Key Patterns

**Global State**: `AppState` wraps `Arc<Mutex<RedPillApp>>` and Tokio runtime, registered as GPUI Global.

**SSH I/O**: Uses `tokio::select!` in `spawn_ssh_io_loop()` to multiplex:
- `write_rx` - keyboard input → SSH
- `resize_rx` - terminal size changes
- `channel.wait()` - SSH data → terminal display

**Terminal Modes**: `TerminalMode2` enum with `Local` (PTY via alacritty event loop) and `Remote` (SSH via russh channel).

**Dirty Flag**: Atomic bool on Terminal signals new SSH content for lock-free UI polling.

**Credentials**: Passwords/passphrases stored in OS keychain via `keyring` crate, never in JSON files.

### Data Flow

1. User input → `terminal_view.rs::handle_key_input()` → `Terminal::write()` → `write_tx` channel
2. SSH data → I/O loop `channel.wait()` → `Terminal::write_to_pty()` → sets dirty flag
3. UI poll (2ms) → checks dirty flag → `cx.notify()` → re-render

### Config Paths

- macOS/Linux: `~/.config/redpill/{config.json, sessions.json}`
- Windows: `%APPDATA%\redpill/`

## Key Files for Common Tasks

| Task | Files |
|------|-------|
| SSH connection logic | `app.rs` (I/O loop), `terminal/ssh_backend.rs` |
| Terminal rendering | `ui/terminal_view.rs` |
| Keyboard handling | `terminal/keys.rs`, `ui/terminal_view.rs::handle_key_input` |
| Session persistence | `session/storage.rs`, `session/manager.rs` |
| Add new UI component | `ui/mod.rs`, create new file following `session_dialog.rs` pattern |

## Threading Model

- **Main thread**: GPUI event loop, UI rendering
- **Tokio runtime**: SSH I/O, async operations (spawned from `AppState::tokio_runtime`)
- **Alacritty event loop**: Local PTY I/O (separate thread per local terminal)

Use `Arc<Mutex<Terminal>>` for cross-thread terminal access. SSH terminals use channels instead of locks for I/O to avoid contention.
