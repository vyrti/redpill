//! Kubernetes pod exec backend
//!
//! Provides terminal I/O for Kubernetes pod exec sessions.

use futures::SinkExt;
use kube::{
    api::{Api, AttachParams},
    Client, Config,
};
use k8s_openapi::api::core::v1::Pod;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::session::K8sSession;

/// Errors that can occur during K8s exec operations
#[derive(Debug, Error)]
pub enum K8sError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Kube error: {0}")]
    KubeError(#[from] kube::Error),

    #[error("Config error: {0}")]
    ConfigError(#[from] kube::config::KubeconfigError),

    #[error("Infer config error: {0}")]
    InferConfigError(#[from] kube::config::InferConfigError),

    #[error("Pod not found: {0}/{1}")]
    PodNotFound(String, String),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Not connected")]
    NotConnected,
}

/// Result type for K8s operations
pub type K8sResult<T> = Result<T, K8sError>;

/// Terminal size for K8s PTY
#[derive(Debug, Clone, Copy, Default)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

impl TerminalSize {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self { cols, rows }
    }
}

impl From<TerminalSize> for kube::api::TerminalSize {
    fn from(size: TerminalSize) -> Self {
        kube::api::TerminalSize {
            width: size.cols,
            height: size.rows,
        }
    }
}

/// Connection state of the K8s backend
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Failed,
}

/// K8s pod exec backend
pub struct K8sBackend {
    session: K8sSession,
    state: ConnectionState,
    size: TerminalSize,
}

impl K8sBackend {
    /// Create a new K8s backend (not connected)
    pub fn new(session: K8sSession) -> Self {
        Self {
            session,
            state: ConnectionState::Disconnected,
            size: TerminalSize::default(),
        }
    }

    /// Get the session configuration
    pub fn session(&self) -> &K8sSession {
        &self.session
    }

    /// Get the current connection state
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Set the terminal size before connecting
    pub fn set_size(&mut self, size: TerminalSize) {
        self.size = size;
    }

    /// Connect to the pod and return I/O channels
    pub async fn connect(
        &mut self,
    ) -> K8sResult<(
        mpsc::Sender<Vec<u8>>,
        mpsc::Receiver<Vec<u8>>,
        mpsc::Sender<TerminalSize>,
    )> {
        self.state = ConnectionState::Connecting;

        // Create K8s client for the specific context
        let options = kube::config::KubeConfigOptions {
            context: Some(self.session.context.clone()),
            ..Default::default()
        };
        let config = Config::from_kubeconfig(&options).await?;
        let client = Client::try_from(config)?;

        let pods: Api<Pod> = Api::namespaced(client.clone(), &self.session.namespace);

        // Verify pod exists
        let _pod = pods.get(&self.session.pod).await.map_err(|_| {
            K8sError::PodNotFound(self.session.namespace.clone(), self.session.pod.clone())
        })?;

        // Set up attach parameters
        let mut attach_params = AttachParams::interactive_tty();
        if let Some(ref container) = self.session.container {
            attach_params = attach_params.container(container);
        }

        // Command to exec - prefer bash over sh
        let cmd = vec![
            "/bin/sh",
            "-c",
            "command -v bash >/dev/null && exec bash || exec sh",
        ];

        // Start exec
        let mut attached = pods.exec(&self.session.pod, cmd, &attach_params).await?;

        // Create channels for I/O
        let (write_tx, mut write_rx) = mpsc::channel::<Vec<u8>>(256);
        let (read_tx, read_rx) = mpsc::channel::<Vec<u8>>(256);
        let (resize_tx, mut resize_rx) = mpsc::channel::<TerminalSize>(16);

        // Initial resize
        if let Some(ref mut terminal_size) = attached.terminal_size() {
            let _ = terminal_size.send(self.size.into()).await;
        }

        self.state = ConnectionState::Connected;

        // Spawn I/O task
        tokio::spawn(async move {
            let mut stdin = attached.stdin().unwrap();
            let mut stdout = attached.stdout().unwrap();

            let mut stdout_buf = vec![0u8; 4096];

            loop {
                tokio::select! {
                    // Write data to pod
                    Some(data) = write_rx.recv() => {
                        if stdin.write_all(&data).await.is_err() {
                            break;
                        }
                        let _ = stdin.flush().await;
                    }

                    // Read data from pod
                    result = stdout.read(&mut stdout_buf) => {
                        match result {
                            Ok(0) => break, // EOF
                            Ok(n) => {
                                if read_tx.send(stdout_buf[..n].to_vec()).await.is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }

                    // Handle resize
                    Some(size) = resize_rx.recv() => {
                        if let Some(ref mut terminal_size) = attached.terminal_size() {
                            let _ = terminal_size.send(size.into()).await;
                        }
                    }
                }
            }

            tracing::info!("K8s exec I/O loop ended");
        });

        Ok((write_tx, read_rx, resize_tx))
    }
}
