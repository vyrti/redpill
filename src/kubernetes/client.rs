//! Kubernetes API client
//!
//! Wraps the kube crate to provide namespace and pod listing functionality.

use kube::{
    api::{Api, ListParams},
    Client, Config,
};
use k8s_openapi::api::core::v1::{Namespace, Pod};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KubeClientError {
    #[error("Failed to create client: {0}")]
    ClientError(#[from] kube::Error),
    #[error("Failed to load config: {0}")]
    ConfigError(#[from] kube::config::KubeconfigError),
    #[error("Failed to infer config: {0}")]
    InferError(#[from] kube::config::InferConfigError),
    #[error("No context available")]
    NoContext,
}

/// A Kubernetes namespace
#[derive(Debug, Clone)]
pub struct KubeNamespace {
    pub name: String,
    pub status: String,
}

/// A Kubernetes pod
#[derive(Debug, Clone)]
pub struct KubePod {
    pub name: String,
    pub namespace: String,
    pub status: String,
    pub ready: String,
    pub containers: Vec<String>,
}

/// Kubernetes API client
pub struct KubeClient {
    client: Client,
    context_name: String,
}

impl KubeClient {
    /// Create a new client using the default kubeconfig and current context
    pub async fn new() -> Result<Self, KubeClientError> {
        let config = Config::infer().await?;
        let context_name = config.default_namespace.clone();
        let client = Client::try_from(config)?;
        Ok(Self { client, context_name })
    }

    /// Create a client for a specific context
    pub async fn for_context(context_name: &str) -> Result<Self, KubeClientError> {
        let options = kube::config::KubeConfigOptions {
            context: Some(context_name.to_string()),
            ..Default::default()
        };
        let config = Config::from_kubeconfig(&options).await?;
        let client = Client::try_from(config)?;
        Ok(Self {
            client,
            context_name: context_name.to_string(),
        })
    }

    /// Get the context name this client is connected to
    pub fn context_name(&self) -> &str {
        &self.context_name
    }

    /// Get the raw kube client for exec operations
    pub fn inner(&self) -> &Client {
        &self.client
    }

    /// List all namespaces
    pub async fn list_namespaces(&self) -> Result<Vec<KubeNamespace>, KubeClientError> {
        let namespaces: Api<Namespace> = Api::all(self.client.clone());
        let list = namespaces.list(&ListParams::default()).await?;

        Ok(list.items.into_iter().map(|ns| {
            let name = ns.metadata.name.unwrap_or_default();
            let status = ns.status
                .and_then(|s| s.phase)
                .unwrap_or_else(|| "Unknown".to_string());
            KubeNamespace { name, status }
        }).collect())
    }

    /// List pods in a namespace
    pub async fn list_pods(&self, namespace: &str) -> Result<Vec<KubePod>, KubeClientError> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
        let list = pods.list(&ListParams::default()).await?;

        Ok(list.items.into_iter().map(|pod| {
            let name = pod.metadata.name.unwrap_or_default();
            let namespace = pod.metadata.namespace.unwrap_or_default();

            let (status, ready, containers) = if let Some(status) = pod.status {
                let phase = status.phase.unwrap_or_else(|| "Unknown".to_string());

                let container_statuses = status.container_statuses.unwrap_or_default();
                let total = container_statuses.len();
                let ready_count = container_statuses.iter()
                    .filter(|c| c.ready)
                    .count();
                let ready_str = format!("{}/{}", ready_count, total);

                let container_names: Vec<String> = container_statuses.iter()
                    .map(|c| c.name.clone())
                    .collect();

                (phase, ready_str, container_names)
            } else {
                ("Unknown".to_string(), "0/0".to_string(), vec![])
            };

            KubePod {
                name,
                namespace,
                status,
                ready,
                containers,
            }
        }).collect())
    }

    /// Get a specific pod
    pub async fn get_pod(&self, namespace: &str, name: &str) -> Result<KubePod, KubeClientError> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
        let pod = pods.get(name).await?;

        let pod_name = pod.metadata.name.unwrap_or_default();
        let pod_namespace = pod.metadata.namespace.unwrap_or_default();

        let (status, ready, containers) = if let Some(status) = pod.status {
            let phase = status.phase.unwrap_or_else(|| "Unknown".to_string());

            let container_statuses = status.container_statuses.unwrap_or_default();
            let total = container_statuses.len();
            let ready_count = container_statuses.iter()
                .filter(|c| c.ready)
                .count();
            let ready_str = format!("{}/{}", ready_count, total);

            let container_names: Vec<String> = container_statuses.iter()
                .map(|c| c.name.clone())
                .collect();

            (phase, ready_str, container_names)
        } else {
            ("Unknown".to_string(), "0/0".to_string(), vec![])
        };

        Ok(KubePod {
            name: pod_name,
            namespace: pod_namespace,
            status,
            ready,
            containers,
        })
    }
}
