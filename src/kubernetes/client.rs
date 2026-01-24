//! Kubernetes API client
//!
//! Wraps the kube crate to provide namespace and pod listing functionality.

use std::collections::HashMap;
use std::sync::OnceLock;
use kube::{
    api::{Api, ListParams},
    Client, Config,
    runtime::watcher::{self, Event as WatchEvent},
};
use k8s_openapi::api::core::v1::{Namespace, Pod};
use thiserror::Error;
use tokio::sync::RwLock;
use futures::StreamExt;

/// Global client cache - avoids recreating clients (expensive TLS handshake) for each request
static CLIENT_CACHE: OnceLock<RwLock<HashMap<String, Client>>> = OnceLock::new();

fn get_client_cache() -> &'static RwLock<HashMap<String, Client>> {
    CLIENT_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

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

    /// Create a client for a specific context (cached for performance)
    pub async fn for_context(context_name: &str) -> Result<Self, KubeClientError> {
        let cache = get_client_cache();

        // Try to get from cache first (fast path)
        {
            let read_guard = cache.read().await;
            if let Some(client) = read_guard.get(context_name) {
                tracing::debug!("K8s client cache HIT for {}", context_name);
                return Ok(Self {
                    client: client.clone(),
                    context_name: context_name.to_string(),
                });
            }
        }

        tracing::info!("K8s client cache MISS for {} - creating new client", context_name);
        let start = std::time::Instant::now();

        // Not in cache, create new client
        let options = kube::config::KubeConfigOptions {
            context: Some(context_name.to_string()),
            ..Default::default()
        };
        let config = Config::from_kubeconfig(&options).await?;
        tracing::debug!("Config loaded in {:?}", start.elapsed());

        let client = Client::try_from(config)?;
        tracing::debug!("Client created in {:?}", start.elapsed());

        // Store in cache
        {
            let mut write_guard = cache.write().await;
            write_guard.insert(context_name.to_string(), client.clone());
        }

        tracing::info!("K8s client for {} created in {:?}", context_name, start.elapsed());

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
        let start = std::time::Instant::now();
        let namespaces: Api<Namespace> = Api::all(self.client.clone());
        let list = namespaces.list(&ListParams::default()).await?;
        tracing::debug!("list_namespaces API call took {:?}", start.elapsed());

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
        let start = std::time::Instant::now();
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
        let list = pods.list(&ListParams::default()).await?;
        tracing::debug!("list_pods({}) API call took {:?}", namespace, start.elapsed());

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

    /// Watch namespaces for changes and send updates via the channel
    pub async fn watch_namespaces<F>(&self, mut on_event: F) -> Result<(), KubeClientError>
    where
        F: FnMut(NamespaceWatchEvent) + Send,
    {
        let namespaces: Api<Namespace> = Api::all(self.client.clone());
        let watcher_config = watcher::Config::default();
        let mut stream = watcher::watcher(namespaces, watcher_config).boxed();

        while let Some(event) = stream.next().await {
            match event {
                Ok(WatchEvent::Apply(ns)) => {
                    let name = ns.metadata.name.unwrap_or_default();
                    let status = ns.status
                        .and_then(|s| s.phase)
                        .unwrap_or_else(|| "Unknown".to_string());
                    on_event(NamespaceWatchEvent::Added(KubeNamespace { name, status }));
                }
                Ok(WatchEvent::Delete(ns)) => {
                    let name = ns.metadata.name.unwrap_or_default();
                    on_event(NamespaceWatchEvent::Deleted(name));
                }
                Ok(WatchEvent::Init) => {
                    // Initial list started
                }
                Ok(WatchEvent::InitApply(ns)) => {
                    // Initial list item
                    let name = ns.metadata.name.unwrap_or_default();
                    let status = ns.status
                        .and_then(|s| s.phase)
                        .unwrap_or_else(|| "Unknown".to_string());
                    on_event(NamespaceWatchEvent::Added(KubeNamespace { name, status }));
                }
                Ok(WatchEvent::InitDone) => {
                    // Initial list complete
                    on_event(NamespaceWatchEvent::InitDone);
                }
                Err(e) => {
                    tracing::warn!("Namespace watch error: {}", e);
                    // Continue watching after transient errors
                }
            }
        }

        Ok(())
    }

    /// Watch pods in a namespace for changes
    pub async fn watch_pods<F>(&self, namespace: &str, mut on_event: F) -> Result<(), KubeClientError>
    where
        F: FnMut(PodWatchEvent) + Send,
    {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
        let watcher_config = watcher::Config::default();
        let mut stream = watcher::watcher(pods, watcher_config).boxed();

        while let Some(event) = stream.next().await {
            match event {
                Ok(WatchEvent::Apply(pod)) | Ok(WatchEvent::InitApply(pod)) => {
                    let kube_pod = Self::convert_pod(pod);
                    on_event(PodWatchEvent::AddedOrModified(kube_pod));
                }
                Ok(WatchEvent::Delete(pod)) => {
                    let name = pod.metadata.name.unwrap_or_default();
                    on_event(PodWatchEvent::Deleted(name));
                }
                Ok(WatchEvent::Init) => {
                    // Initial list started
                }
                Ok(WatchEvent::InitDone) => {
                    on_event(PodWatchEvent::InitDone);
                }
                Err(e) => {
                    tracing::warn!("Pod watch error: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Convert a k8s Pod to our KubePod type
    fn convert_pod(pod: Pod) -> KubePod {
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
    }
}

/// Event from namespace watcher
#[derive(Debug, Clone)]
pub enum NamespaceWatchEvent {
    Added(KubeNamespace),
    Deleted(String),
    InitDone,
}

/// Event from pod watcher
#[derive(Debug, Clone)]
pub enum PodWatchEvent {
    AddedOrModified(KubePod),
    Deleted(String),
    InitDone,
}
