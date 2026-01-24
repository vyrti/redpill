//! Kubernetes pod exec functionality
//!
//! Provides exec into pods using the kube crate's websocket support.

use futures::SinkExt;
use kube::{
    api::{Api, AttachParams, AttachedProcess, TerminalSize as KubeTerminalSize},
    Client,
};
use k8s_openapi::api::core::v1::Pod;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Error)]
pub enum ExecError {
    #[error("Kube error: {0}")]
    KubeError(#[from] kube::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Pod not found: {0}/{1}")]
    PodNotFound(String, String),
    #[error("Container not found: {0}")]
    ContainerNotFound(String),
    #[error("Exec not supported")]
    NotSupported,
    #[error("Remote command error: {0}")]
    RemoteCommandError(String),
}

/// Terminal size for exec
#[derive(Debug, Clone, Copy)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

impl From<TerminalSize> for KubeTerminalSize {
    fn from(size: TerminalSize) -> Self {
        KubeTerminalSize {
            width: size.cols,
            height: size.rows,
        }
    }
}

/// Pod exec session
pub struct PodExec {
    attached: AttachedProcess,
}

impl PodExec {
    /// Start an exec session into a pod
    pub async fn start(
        client: &Client,
        namespace: &str,
        pod_name: &str,
        container: Option<&str>,
        command: Vec<&str>,
        size: TerminalSize,
    ) -> Result<Self, ExecError> {
        let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);

        // Verify pod exists
        let _pod = pods.get(pod_name).await?;

        let mut attach_params = AttachParams::interactive_tty();
        if let Some(c) = container {
            attach_params = attach_params.container(c);
        }

        let cmd = if command.is_empty() {
            vec!["/bin/sh", "-c", "command -v bash >/dev/null && exec bash || exec sh"]
        } else {
            command
        };

        let mut attached = pods.exec(pod_name, cmd, &attach_params).await?;

        // Resize terminal
        if let Some(ref mut terminal) = attached.terminal_size() {
            let _ = terminal.send(size.into()).await;
        }

        Ok(Self { attached })
    }

    /// Start a shell exec session (convenience method)
    pub async fn shell(
        client: &Client,
        namespace: &str,
        pod_name: &str,
        container: Option<&str>,
        size: TerminalSize,
    ) -> Result<Self, ExecError> {
        Self::start(client, namespace, pod_name, container, vec![], size).await
    }

    /// Get stdin writer
    pub fn stdin(&mut self) -> Option<impl AsyncWriteExt + Unpin + '_> {
        self.attached.stdin()
    }

    /// Get stdout reader
    pub fn stdout(&mut self) -> Option<impl AsyncReadExt + Unpin + '_> {
        self.attached.stdout()
    }

    /// Get stderr reader (if separate from stdout)
    pub fn stderr(&mut self) -> Option<impl AsyncReadExt + Unpin + '_> {
        self.attached.stderr()
    }

    /// Resize the terminal
    pub async fn resize(&mut self, size: TerminalSize) -> Result<(), ExecError> {
        if let Some(ref mut terminal) = self.attached.terminal_size() {
            terminal.send(size.into()).await
                .map_err(|e| ExecError::RemoteCommandError(e.to_string()))?;
        }
        Ok(())
    }

    /// Wait for the exec to complete and get exit status
    pub async fn join(self) -> Result<(), ExecError> {
        self.attached.join().await
            .map_err(|e| ExecError::RemoteCommandError(e.to_string()))?;
        Ok(())
    }

    /// Take ownership of the attached process for manual handling
    pub fn into_inner(self) -> AttachedProcess {
        self.attached
    }
}
