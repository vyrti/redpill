use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during config operations
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    ReadError(#[from] std::io::Error),

    #[error("Failed to parse config file: {0}")]
    ParseError(#[from] serde_json::Error),

    #[error("Config directory not found")]
    ConfigDirNotFound,
}

/// Window state for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    /// Window width in pixels
    pub width: u32,
    /// Window height in pixels
    pub height: u32,
    /// Window X position
    pub x: Option<i32>,
    /// Window Y position
    pub y: Option<i32>,
    /// Whether the window is maximized
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1200,
            height: 800,
            x: None,
            y: None,
            maximized: false,
        }
    }
}

/// Terminal appearance settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalAppearance {
    /// Font family
    pub font_family: String,
    /// Font size in points
    pub font_size: f32,
    /// Minimum font size for zoom
    #[serde(default = "default_min_font_size")]
    pub min_font_size: f32,
    /// Maximum font size for zoom
    #[serde(default = "default_max_font_size")]
    pub max_font_size: f32,
    /// Line height multiplier
    pub line_height: f32,
    /// Theme name
    pub theme: String,
}

fn default_min_font_size() -> f32 {
    8.0
}

fn default_max_font_size() -> f32 {
    32.0
}

impl Default for TerminalAppearance {
    fn default() -> Self {
        Self {
            font_family: "JetBrains Mono".to_string(),
            font_size: 13.0,
            min_font_size: 8.0,
            max_font_size: 32.0,
            line_height: 1.2,
            theme: "dark".to_string(),
        }
    }
}

impl TerminalAppearance {
    /// Zoom in (increase font size)
    pub fn zoom_in(&mut self) {
        self.font_size = (self.font_size + 1.0).min(self.max_font_size);
    }

    /// Zoom out (decrease font size)
    pub fn zoom_out(&mut self) {
        self.font_size = (self.font_size - 1.0).max(self.min_font_size);
    }

    /// Reset zoom to default
    pub fn zoom_reset(&mut self) {
        self.font_size = 13.0;
    }
}

/// Session tree panel settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTreeSettings {
    /// Panel width in pixels
    pub width: u32,
    /// Whether the panel is visible
    pub visible: bool,
}

impl Default for SessionTreeSettings {
    fn default() -> Self {
        Self {
            width: 250,
            visible: true,
        }
    }
}

/// Keyboard shortcut definitions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBindings {
    /// New tab shortcut
    pub new_tab: String,
    /// Close tab shortcut
    pub close_tab: String,
    /// Next tab shortcut
    pub next_tab: String,
    /// Previous tab shortcut
    pub prev_tab: String,
    /// Toggle session tree shortcut
    pub toggle_session_tree: String,
    /// Copy shortcut
    pub copy: String,
    /// Paste shortcut
    pub paste: String,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            new_tab: "ctrl+shift+t".to_string(),
            close_tab: "ctrl+shift+w".to_string(),
            next_tab: "ctrl+tab".to_string(),
            prev_tab: "ctrl+shift+tab".to_string(),
            toggle_session_tree: "ctrl+b".to_string(),
            copy: "ctrl+shift+c".to_string(),
            paste: "ctrl+shift+v".to_string(),
        }
    }
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Window state
    #[serde(default)]
    pub window: WindowState,

    /// Terminal appearance
    #[serde(default)]
    pub appearance: TerminalAppearance,

    /// Session tree settings
    #[serde(default)]
    pub session_tree: SessionTreeSettings,

    /// Key bindings
    #[serde(default)]
    pub keybindings: KeyBindings,

    /// Number of scrollback lines
    #[serde(default = "default_scrollback_lines")]
    pub scrollback_lines: usize,

    /// Whether to confirm before closing tabs
    #[serde(default = "default_true")]
    pub confirm_close: bool,

    /// Whether to restore sessions on startup
    #[serde(default)]
    pub restore_sessions: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            window: WindowState::default(),
            appearance: TerminalAppearance::default(),
            session_tree: SessionTreeSettings::default(),
            keybindings: KeyBindings::default(),
            scrollback_lines: 10000,
            confirm_close: true,
            restore_sessions: false,
        }
    }
}

fn default_scrollback_lines() -> usize {
    10000
}

fn default_true() -> bool {
    true
}

impl AppConfig {
    /// Get the configuration directory path
    pub fn config_dir() -> Result<PathBuf, ConfigError> {
        let config_dir = dirs::config_dir()
            .ok_or(ConfigError::ConfigDirNotFound)?
            .join("redpill");

        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)?;
        }

        Ok(config_dir)
    }

    /// Get the configuration file path
    pub fn config_path() -> Result<PathBuf, ConfigError> {
        Ok(Self::config_dir()?.join("config.json"))
    }

    /// Load configuration from disk
    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::config_path()?;

        if !path.exists() {
            tracing::info!("Config file not found, using defaults");
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&path)?;
        let config: Self = serde_json::from_str(&contents)?;

        tracing::info!("Loaded configuration from {:?}", path);
        Ok(config)
    }

    /// Save configuration to disk
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::config_path()?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        let contents = serde_json::to_string_pretty(self)?;
        fs::write(&path, contents)?;

        tracing::info!("Saved configuration to {:?}", path);
        Ok(())
    }

    /// Reset to defaults and save
    pub fn reset(&mut self) -> Result<(), ConfigError> {
        *self = Self::default();
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_config_defaults() {
        let config = AppConfig::default();
        assert_eq!(config.window.width, 1200);
        assert_eq!(config.appearance.font_size, 13.0);
        assert_eq!(config.scrollback_lines, 10000);
    }

    #[test]
    fn test_config_serialization() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.window.width, parsed.window.width);
        assert_eq!(config.appearance.font_family, parsed.appearance.font_family);
    }
}
