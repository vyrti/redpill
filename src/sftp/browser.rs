//! SFTP browser operations wrapper

use russh_sftp::client::SftpSession;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

/// SFTP error types
#[derive(Debug, Error)]
pub enum SftpError {
    #[error("SFTP session not connected")]
    NotConnected,

    #[error("SFTP error: {0}")]
    Sftp(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Path not found: {0}")]
    NotFound(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

/// Entry type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EntryType {
    File,
    Directory,
    Symlink,
    Unknown,
}

/// A directory entry
#[derive(Debug, Clone)]
pub struct DirEntry {
    /// Entry name
    pub name: String,
    /// Entry type
    pub entry_type: EntryType,
    /// File size in bytes
    pub size: u64,
    /// Last modified timestamp (Unix epoch)
    pub modified: u64,
    /// Permissions string (e.g., "rwxr-xr-x")
    pub permissions: String,
}

/// Transfer progress for file operations
#[derive(Debug, Clone)]
pub struct TransferProgress {
    /// Operation name (filename)
    pub name: String,
    /// Total bytes
    pub total: u64,
    /// Bytes transferred
    pub transferred: Arc<AtomicU64>,
    /// Whether complete
    pub complete: bool,
    /// Error message if failed
    pub error: Option<String>,
}

impl TransferProgress {
    pub fn new(name: String, total: u64) -> Self {
        Self {
            name,
            total,
            transferred: Arc::new(AtomicU64::new(0)),
            complete: false,
            error: None,
        }
    }

    pub fn progress_percent(&self) -> f32 {
        if self.total == 0 {
            return 100.0;
        }
        let transferred = self.transferred.load(Ordering::Relaxed);
        (transferred as f64 / self.total as f64 * 100.0) as f32
    }
}

/// SFTP browser wrapper
pub struct SftpBrowser {
    /// SFTP session
    session: Option<SftpSession>,
    /// Current directory
    current_path: PathBuf,
    /// Cached directory entries
    entries: Vec<DirEntry>,
}

impl SftpBrowser {
    /// Create a new SFTP browser (not yet connected)
    pub fn new() -> Self {
        Self {
            session: None,
            current_path: PathBuf::from("/"),
            entries: Vec::new(),
        }
    }

    /// Set the SFTP session
    pub fn set_session(&mut self, session: SftpSession) {
        self.session = Some(session);
        self.current_path = PathBuf::from("/");
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.session.is_some()
    }

    /// Get current path
    pub fn current_path(&self) -> &Path {
        &self.current_path
    }

    /// Get cached entries
    pub fn entries(&self) -> &[DirEntry] {
        &self.entries
    }

    /// List directory contents
    pub async fn list_dir(&mut self, path: &Path) -> Result<Vec<DirEntry>, SftpError> {
        let session = self.session.as_ref().ok_or(SftpError::NotConnected)?;

        let path_str = path.to_string_lossy().to_string();

        // Read directory using the russh_sftp API
        let items = session
            .read_dir(path_str)
            .await
            .map_err(|e| SftpError::Sftp(e.to_string()))?;

        let mut entries = Vec::new();

        for item in items {
            // Skip . and ..
            let filename = item.file_name();
            if filename == "." || filename == ".." {
                continue;
            }

            let file_type = item.file_type();
            let entry_type = if file_type.is_dir() {
                EntryType::Directory
            } else if file_type.is_symlink() {
                EntryType::Symlink
            } else if file_type.is_file() {
                EntryType::File
            } else {
                EntryType::Unknown
            };

            let metadata = item.metadata();
            let size = metadata.size.unwrap_or(0);
            let modified = metadata.mtime.map(|t| t as u64).unwrap_or(0);
            let permissions = format_permissions(metadata.permissions.unwrap_or(0));

            entries.push(DirEntry {
                name: filename.to_string(),
                entry_type,
                size,
                modified,
                permissions,
            });
        }

        // Sort: directories first, then by name
        entries.sort_by(|a, b| {
            match (a.entry_type, b.entry_type) {
                (EntryType::Directory, EntryType::Directory) => a.name.cmp(&b.name),
                (EntryType::Directory, _) => std::cmp::Ordering::Less,
                (_, EntryType::Directory) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });

        self.entries = entries.clone();
        self.current_path = path.to_path_buf();

        Ok(entries)
    }

    /// Change to directory
    pub async fn change_dir(&mut self, path: &Path) -> Result<(), SftpError> {
        self.list_dir(path).await?;
        Ok(())
    }

    /// Go to parent directory
    pub async fn go_up(&mut self) -> Result<(), SftpError> {
        if let Some(parent) = self.current_path.parent() {
            let parent = parent.to_path_buf();
            self.change_dir(&parent).await?;
        }
        Ok(())
    }

    /// Download a file
    pub async fn download(
        &self,
        remote_path: &Path,
        local_path: &Path,
        progress: &TransferProgress,
    ) -> Result<(), SftpError> {
        let session = self.session.as_ref().ok_or(SftpError::NotConnected)?;

        let remote_str = remote_path.to_string_lossy().to_string();

        // Open remote file
        let mut remote_file = session
            .open(remote_str)
            .await
            .map_err(|e| SftpError::Sftp(e.to_string()))?;

        let mut local_file = tokio::fs::File::create(local_path).await?;

        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut buf = vec![0u8; 32768];
        let mut total_read = 0u64;

        loop {
            let n = remote_file.read(&mut buf).await.map_err(|e| SftpError::Sftp(e.to_string()))?;
            if n == 0 {
                break;
            }
            local_file.write_all(&buf[..n]).await?;
            total_read += n as u64;
            progress.transferred.store(total_read, Ordering::Relaxed);
        }

        Ok(())
    }

    /// Upload a file
    pub async fn upload(
        &self,
        local_path: &Path,
        remote_path: &Path,
        progress: &TransferProgress,
    ) -> Result<(), SftpError> {
        let session = self.session.as_ref().ok_or(SftpError::NotConnected)?;

        let remote_str = remote_path.to_string_lossy().to_string();

        // Create remote file
        let mut remote_file = session
            .create(remote_str)
            .await
            .map_err(|e| SftpError::Sftp(e.to_string()))?;

        let mut local_file = tokio::fs::File::open(local_path).await?;

        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut buf = vec![0u8; 32768];
        let mut total_written = 0u64;

        loop {
            let n = local_file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            remote_file.write_all(&buf[..n]).await.map_err(|e| SftpError::Sftp(e.to_string()))?;
            total_written += n as u64;
            progress.transferred.store(total_written, Ordering::Relaxed);
        }

        Ok(())
    }

    /// Delete a file
    pub async fn remove_file(&self, path: &Path) -> Result<(), SftpError> {
        let session = self.session.as_ref().ok_or(SftpError::NotConnected)?;
        let path_str = path.to_string_lossy().to_string();
        session
            .remove_file(path_str)
            .await
            .map_err(|e| SftpError::Sftp(e.to_string()))?;
        Ok(())
    }

    /// Delete a directory
    pub async fn remove_dir(&self, path: &Path) -> Result<(), SftpError> {
        let session = self.session.as_ref().ok_or(SftpError::NotConnected)?;
        let path_str = path.to_string_lossy().to_string();
        session
            .remove_dir(path_str)
            .await
            .map_err(|e| SftpError::Sftp(e.to_string()))?;
        Ok(())
    }

    /// Rename a file or directory
    pub async fn rename(&self, old_path: &Path, new_path: &Path) -> Result<(), SftpError> {
        let session = self.session.as_ref().ok_or(SftpError::NotConnected)?;
        let old_str = old_path.to_string_lossy().to_string();
        let new_str = new_path.to_string_lossy().to_string();
        session
            .rename(old_str, new_str)
            .await
            .map_err(|e| SftpError::Sftp(e.to_string()))?;
        Ok(())
    }

    /// Create a directory
    pub async fn create_dir(&self, path: &Path) -> Result<(), SftpError> {
        let session = self.session.as_ref().ok_or(SftpError::NotConnected)?;
        let path_str = path.to_string_lossy().to_string();
        session
            .create_dir(path_str)
            .await
            .map_err(|e| SftpError::Sftp(e.to_string()))?;
        Ok(())
    }
}

impl Default for SftpBrowser {
    fn default() -> Self {
        Self::new()
    }
}

/// Format Unix permissions to human-readable string
fn format_permissions(mode: u32) -> String {
    let mut s = String::with_capacity(9);

    // Owner
    s.push(if mode & 0o400 != 0 { 'r' } else { '-' });
    s.push(if mode & 0o200 != 0 { 'w' } else { '-' });
    s.push(if mode & 0o100 != 0 { 'x' } else { '-' });

    // Group
    s.push(if mode & 0o040 != 0 { 'r' } else { '-' });
    s.push(if mode & 0o020 != 0 { 'w' } else { '-' });
    s.push(if mode & 0o010 != 0 { 'x' } else { '-' });

    // Others
    s.push(if mode & 0o004 != 0 { 'r' } else { '-' });
    s.push(if mode & 0o002 != 0 { 'w' } else { '-' });
    s.push(if mode & 0o001 != 0 { 'x' } else { '-' });

    s
}

/// Format file size to human-readable string
pub fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size < KB {
        format!("{} B", size)
    } else if size < MB {
        format!("{:.1} KB", size as f64 / KB as f64)
    } else if size < GB {
        format!("{:.1} MB", size as f64 / MB as f64)
    } else {
        format!("{:.1} GB", size as f64 / GB as f64)
    }
}
