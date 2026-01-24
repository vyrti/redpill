//! Kubernetes configuration parsing
//!
//! Parses kubeconfig files (typically ~/.kube/config) to extract
//! clusters, contexts, and authentication information.

use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KubeConfigError {
    #[error("Failed to read kubeconfig: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("Failed to parse kubeconfig: {0}")]
    ParseError(String),
    #[error("Context not found: {0}")]
    ContextNotFound(String),
    #[error("Cluster not found: {0}")]
    ClusterNotFound(String),
    #[error("No kubeconfig found")]
    NotFound,
}

/// A Kubernetes cluster from kubeconfig
#[derive(Debug, Clone)]
pub struct KubeCluster {
    pub name: String,
    pub server: String,
    pub certificate_authority: Option<String>,
    pub certificate_authority_data: Option<String>,
    pub insecure_skip_tls_verify: bool,
}

/// A Kubernetes context from kubeconfig
#[derive(Debug, Clone)]
pub struct KubeContext {
    pub name: String,
    pub cluster: String,
    pub user: String,
    pub namespace: Option<String>,
}

/// Parsed kubeconfig
#[derive(Debug, Clone)]
pub struct KubeConfig {
    pub path: PathBuf,
    pub current_context: Option<String>,
    pub contexts: Vec<KubeContext>,
    pub clusters: HashMap<String, KubeCluster>,
}

impl KubeConfig {
    /// Load kubeconfig from the default location (~/.kube/config)
    pub fn load_default() -> Result<Self, KubeConfigError> {
        let path = Self::default_path()?;
        Self::load_from(&path)
    }

    /// Get the default kubeconfig path
    pub fn default_path() -> Result<PathBuf, KubeConfigError> {
        // Check KUBECONFIG env var first
        if let Ok(kubeconfig) = std::env::var("KUBECONFIG") {
            let path = PathBuf::from(kubeconfig.split(':').next().unwrap_or(&kubeconfig));
            if path.exists() {
                return Ok(path);
            }
        }

        // Fall back to ~/.kube/config
        let home = dirs::home_dir().ok_or(KubeConfigError::NotFound)?;
        let path = home.join(".kube").join("config");
        if path.exists() {
            Ok(path)
        } else {
            Err(KubeConfigError::NotFound)
        }
    }

    /// Load kubeconfig from a specific path
    pub fn load_from(path: &PathBuf) -> Result<Self, KubeConfigError> {
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content, path.clone())
    }

    /// Parse kubeconfig YAML content
    fn parse(content: &str, path: PathBuf) -> Result<Self, KubeConfigError> {
        // Use serde_json to parse YAML (kube crate handles this internally,
        // but we do basic parsing for UI display)
        let yaml: serde_json::Value = serde_yaml_ng::from_str(content)
            .map_err(|e| KubeConfigError::ParseError(e.to_string()))?;

        let current_context = yaml.get("current-context")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Parse clusters
        let mut clusters = HashMap::new();
        if let Some(cluster_list) = yaml.get("clusters").and_then(|v| v.as_array()) {
            for cluster in cluster_list {
                if let (Some(name), Some(cluster_data)) = (
                    cluster.get("name").and_then(|v| v.as_str()),
                    cluster.get("cluster"),
                ) {
                    let server = cluster_data.get("server")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    clusters.insert(name.to_string(), KubeCluster {
                        name: name.to_string(),
                        server,
                        certificate_authority: cluster_data.get("certificate-authority")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        certificate_authority_data: cluster_data.get("certificate-authority-data")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        insecure_skip_tls_verify: cluster_data.get("insecure-skip-tls-verify")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                    });
                }
            }
        }

        // Parse contexts
        let mut contexts = Vec::new();
        if let Some(context_list) = yaml.get("contexts").and_then(|v| v.as_array()) {
            for context in context_list {
                if let (Some(name), Some(context_data)) = (
                    context.get("name").and_then(|v| v.as_str()),
                    context.get("context"),
                ) {
                    contexts.push(KubeContext {
                        name: name.to_string(),
                        cluster: context_data.get("cluster")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        user: context_data.get("user")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        namespace: context_data.get("namespace")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                    });
                }
            }
        }

        Ok(Self {
            path,
            current_context,
            contexts,
            clusters,
        })
    }

    /// Get the current context
    pub fn current_context(&self) -> Option<&KubeContext> {
        self.current_context.as_ref()
            .and_then(|name| self.contexts.iter().find(|c| &c.name == name))
    }

    /// Get a context by name
    pub fn get_context(&self, name: &str) -> Option<&KubeContext> {
        self.contexts.iter().find(|c| c.name == name)
    }

    /// Get a cluster by name
    pub fn get_cluster(&self, name: &str) -> Option<&KubeCluster> {
        self.clusters.get(name)
    }

    /// Check if kubeconfig exists at default location
    pub fn exists() -> bool {
        Self::default_path().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_kubeconfig() {
        let yaml = r#"
apiVersion: v1
kind: Config
current-context: minikube
clusters:
- name: minikube
  cluster:
    server: https://192.168.49.2:8443
    certificate-authority: /home/user/.minikube/ca.crt
- name: production
  cluster:
    server: https://k8s.example.com:6443
    insecure-skip-tls-verify: true
contexts:
- name: minikube
  context:
    cluster: minikube
    user: minikube
    namespace: default
- name: production
  context:
    cluster: production
    user: admin
users:
- name: minikube
  user:
    client-certificate: /home/user/.minikube/profiles/minikube/client.crt
    client-key: /home/user/.minikube/profiles/minikube/client.key
"#;

        let config = KubeConfig::parse(yaml, PathBuf::from("/test/config")).unwrap();

        assert_eq!(config.current_context, Some("minikube".to_string()));
        assert_eq!(config.contexts.len(), 2);
        assert_eq!(config.clusters.len(), 2);

        let ctx = config.current_context().unwrap();
        assert_eq!(ctx.name, "minikube");
        assert_eq!(ctx.cluster, "minikube");
        assert_eq!(ctx.namespace, Some("default".to_string()));

        let cluster = config.get_cluster("production").unwrap();
        assert!(cluster.insecure_skip_tls_verify);
    }
}
