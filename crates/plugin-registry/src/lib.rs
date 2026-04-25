use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    Compute,
    Renderer,
    Bridge,
    Memory,
    Agent,
    Verifier,
    Semantic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    Local,
    Wasm,
    WebSocket,
    Grpc,
    ConnectRpc,
    Pyro5,
    Subprocess,
    Ffi,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid plugin kind: {0}")]
    InvalidKind(String),

    #[error("Invalid transport: {0}")]
    InvalidTransport(String),
}

impl TransportKind {
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "local" => Some(Self::Local),
            "wasm" => Some(Self::Wasm),
            "websocket" => Some(Self::WebSocket),
            "grpc" => Some(Self::Grpc),
            "connectrpc" => Some(Self::ConnectRpc),
            "pyro5" => Some(Self::Pyro5),
            "subprocess" => Some(Self::Subprocess),
            "ffi" => Some(Self::Ffi),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityClaim {
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub deterministic: bool,
    #[serde(default)]
    pub replay_safe: bool,
    #[serde(default)]
    pub projection_only: bool,
    #[serde(default)]
    pub emits_artifacts: bool,
    #[serde(default)]
    pub requires_network: bool,
    #[serde(default)]
    pub requires_filesystem: bool,
    #[serde(default)]
    pub requires_gpu: bool,
    #[serde(default)]
    pub max_runtime_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub kind: PluginKind,
    pub transport: TransportKind,
    pub version: String,
    pub artifact_hash: String,
    #[serde(default)]
    pub subscribes: Vec<String>,
    #[serde(default)]
    pub publishes: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub capability_claims: Vec<CapabilityClaim>,
}

#[derive(Default)]
pub struct PluginRegistry {
    plugins: HashMap<String, PluginManifest>,
}

fn has_duplicates(values: &[String]) -> bool {
    let unique: HashSet<_> = values.iter().collect();
    unique.len() != values.len()
}

pub fn is_sha256_urn(value: &str) -> bool {
    value.len() == "sha256:".len() + 64
        && value.starts_with("sha256:")
        && value["sha256:".len()..]
            .chars()
            .all(|c| c.is_ascii_hexdigit())
}

impl PluginRegistry {
    pub fn register(&mut self, plugin: PluginManifest) -> Result<(), String> {
        if plugin.id.trim().is_empty() {
            return Err("plugin id cannot be empty".into());
        }

        if self.plugins.contains_key(&plugin.id) {
            return Err(format!("duplicate plugin: {}", plugin.id));
        }

        if has_duplicates(&plugin.publishes) {
            return Err(format!("duplicate publish topics for {}", plugin.id));
        }

        if has_duplicates(&plugin.subscribes) {
            return Err(format!("duplicate subscribe topics for {}", plugin.id));
        }

        for claim in &plugin.capability_claims {
            if claim.name.trim().is_empty() {
                return Err(format!(
                    "capability claim name cannot be empty for plugin {}",
                    plugin.id
                ));
            }
        }

        let claim_names: Vec<String> = plugin
            .capability_claims
            .iter()
            .map(|c| c.name.clone())
            .collect();
        if has_duplicates(&claim_names) {
            return Err(format!(
                "duplicate capability claim names for {}",
                plugin.id
            ));
        }

        if !is_sha256_urn(&plugin.artifact_hash) {
            return Err(format!(
                "invalid artifact hash for {}: {}",
                plugin.id, plugin.artifact_hash
            ));
        }

        self.plugins.insert(plugin.id.clone(), plugin);
        Ok(())
    }

    pub fn has_plugin(&self, plugin_id: &str) -> bool {
        self.plugins.contains_key(plugin_id)
    }

    pub fn can_publish(&self, plugin_id: &str, topic: &str) -> bool {
        self.plugins
            .get(plugin_id)
            .map(|plugin| plugin.publishes.iter().any(|t| t == topic))
            .unwrap_or(false)
    }

    pub fn can_subscribe(&self, plugin_id: &str, topic: &str) -> bool {
        self.plugins
            .get(plugin_id)
            .map(|plugin| plugin.subscribes.iter().any(|t| t == topic))
            .unwrap_or(false)
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    pub fn plugins(&self) -> Vec<&PluginManifest> {
        self.plugins.values().collect()
    }
}

pub fn load_plugin_manifest_json(path: impl AsRef<Path>) -> Result<PluginManifest, PluginError> {
    let data = fs::read_to_string(path)?;
    let manifest = serde_json::from_str(&data)?;
    Ok(manifest)
}

pub fn write_plugin_manifest_json(
    manifest: &PluginManifest,
    path: impl AsRef<Path>,
) -> Result<(), PluginError> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(manifest)?;
    fs::write(path, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str, publishes: Vec<&str>, subscribes: Vec<&str>) -> PluginManifest {
        PluginManifest {
            id: id.to_string(),
            kind: PluginKind::Compute,
            transport: TransportKind::Local,
            version: "0.1.0".to_string(),
            artifact_hash:
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
            subscribes: subscribes.into_iter().map(ToString::to_string).collect(),
            publishes: publishes.into_iter().map(ToString::to_string).collect(),
            capabilities: vec!["capability".to_string()],
            capability_claims: vec![],
        }
    }

    #[test]
    fn test_duplicate_plugin_rejected() {
        let mut registry = PluginRegistry::default();
        registry
            .register(manifest(
                "adapter_quantum",
                vec!["quantum.state"],
                vec!["quantum.analyze"],
            ))
            .expect("first registration should succeed");

        let result = registry.register(manifest(
            "adapter_quantum",
            vec!["quantum.state"],
            vec!["quantum.analyze"],
        ));

        assert!(result.is_err());
    }

    #[test]
    fn test_can_publish_is_plugin_specific() {
        let mut registry = PluginRegistry::default();
        registry
            .register(manifest("adapter_quantum", vec!["quantum.state"], vec![]))
            .expect("registration should succeed");
        registry
            .register(manifest("adapter_rag", vec!["rag.result"], vec![]))
            .expect("registration should succeed");

        assert!(registry.can_publish("adapter_quantum", "quantum.state"));
        assert!(!registry.can_publish("adapter_rag", "quantum.state"));
    }

    #[test]
    fn test_can_subscribe_is_plugin_specific() {
        let mut registry = PluginRegistry::default();
        registry
            .register(manifest("adapter_quantum", vec![], vec!["quantum.analyze"]))
            .expect("registration should succeed");
        registry
            .register(manifest("adapter_rag", vec![], vec!["rag.query"]))
            .expect("registration should succeed");

        assert!(registry.can_subscribe("adapter_quantum", "quantum.analyze"));
        assert!(!registry.can_subscribe("adapter_rag", "quantum.analyze"));
    }

    #[test]
    fn test_plugin_manifest_round_trips_json() {
        let manifest = PluginManifest {
            id: "adapter_test".to_string(),
            kind: PluginKind::Compute,
            transport: TransportKind::Local,
            version: "0.1.0".to_string(),
            artifact_hash:
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
            subscribes: vec!["test.input".to_string()],
            publishes: vec!["test.output".to_string()],
            capabilities: vec!["test_capability".to_string()],
            capability_claims: vec![CapabilityClaim {
                name: "test_claim".to_string(),
                ..Default::default()
            }],
        };

        let id = ulid::Ulid::new().to_string();
        let path = std::env::temp_dir().join(format!("tnsr-plugin-manifest-{}.json", id));

        write_plugin_manifest_json(&manifest, &path).unwrap();
        let loaded = load_plugin_manifest_json(&path).unwrap();

        assert_eq!(loaded.id, "adapter_test");
        assert_eq!(loaded.kind, PluginKind::Compute);
        assert_eq!(loaded.transport, TransportKind::Local);
        assert_eq!(loaded.publishes, vec!["test.output"]);
        assert_eq!(loaded.subscribes, vec!["test.input"]);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_capability_claims_round_trip_json() {
        let manifest = PluginManifest {
            id: "adapter_claims_test".to_string(),
            kind: PluginKind::Compute,
            transport: TransportKind::Local,
            version: "0.1.0".to_string(),
            artifact_hash:
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
            subscribes: vec![],
            publishes: vec![],
            capabilities: vec!["simple_cap".to_string()],
            capability_claims: vec![
                CapabilityClaim {
                    name: "claim_a".to_string(),
                    enabled: true,
                    required: true,
                    deterministic: true,
                    replay_safe: true,
                    emits_artifacts: true,
                    requires_gpu: false,
                    max_runtime_ms: Some(1_000),
                    ..Default::default()
                },
                CapabilityClaim {
                    name: "claim_b".to_string(),
                    projection_only: true,
                    requires_network: true,
                    ..Default::default()
                },
            ],
        };

        let id = ulid::Ulid::new().to_string();
        let path = std::env::temp_dir().join(format!("tnsr-capability-claims-{}.json", id));

        write_plugin_manifest_json(&manifest, &path).unwrap();
        let loaded = load_plugin_manifest_json(&path).unwrap();

        assert_eq!(loaded.capabilities, vec!["simple_cap"]);
        assert_eq!(loaded.capability_claims.len(), 2);
        assert_eq!(loaded.capability_claims[0].name, "claim_a");
        assert!(loaded.capability_claims[0].enabled);
        assert!(loaded.capability_claims[0].required);
        assert!(loaded.capability_claims[0].deterministic);
        assert!(loaded.capability_claims[0].replay_safe);
        assert!(!loaded.capability_claims[0].projection_only);
        assert!(loaded.capability_claims[0].emits_artifacts);
        assert_eq!(loaded.capability_claims[0].max_runtime_ms, Some(1_000));
        assert_eq!(loaded.capability_claims[1].name, "claim_b");
        assert!(loaded.capability_claims[1].projection_only);
        assert!(loaded.capability_claims[1].requires_network);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_duplicate_capability_claims_rejected() {
        let mut registry = PluginRegistry::default();
        let manifest = PluginManifest {
            id: "adapter_dup_claims".to_string(),
            kind: PluginKind::Compute,
            transport: TransportKind::Local,
            version: "0.1.0".to_string(),
            artifact_hash:
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
            subscribes: vec![],
            publishes: vec![],
            capabilities: vec![],
            capability_claims: vec![
                CapabilityClaim {
                    name: "duplicate_name".to_string(),
                    ..Default::default()
                },
                CapabilityClaim {
                    name: "duplicate_name".to_string(),
                    projection_only: true,
                    ..Default::default()
                },
            ],
        };

        let result = registry.register(manifest);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("duplicate capability claim names"), "{}", err);
    }

    #[test]
    fn test_empty_capability_name_rejected() {
        let mut registry = PluginRegistry::default();
        let manifest = PluginManifest {
            id: "adapter_empty_claim".to_string(),
            kind: PluginKind::Compute,
            transport: TransportKind::Local,
            version: "0.1.0".to_string(),
            artifact_hash:
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
            subscribes: vec![],
            publishes: vec![],
            capabilities: vec![],
            capability_claims: vec![CapabilityClaim {
                name: "  ".to_string(),
                ..Default::default()
            }],
        };

        let result = registry.register(manifest);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("capability claim name cannot be empty"),
            "{}",
            err
        );
    }

    #[test]
    fn test_projection_only_capability_serializes() {
        let claim = CapabilityClaim {
            name: "projection_view".to_string(),
            projection_only: true,
            enabled: true,
            replay_safe: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&claim).unwrap();
        let loaded: CapabilityClaim = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.name, "projection_view");
        assert!(loaded.projection_only);
        assert!(loaded.enabled);
        assert!(loaded.replay_safe);
    }
}
