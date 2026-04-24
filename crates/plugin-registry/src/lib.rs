use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginKind {
    Compute,
    Renderer,
    Bridge,
    Memory,
    Agent,
    Verifier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
pub struct PluginManifest {
    pub id: String,
    pub kind: PluginKind,
    pub transport: TransportKind,
    pub version: String,
    pub artifact_hash: String,
    pub subscribes: Vec<String>,
    pub publishes: Vec<String>,
    pub capabilities: Vec<String>,
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

        if !is_sha256_urn(&plugin.artifact_hash) {
            return Err(format!(
                "invalid artifact hash for {}: {}",
                plugin.id, plugin.artifact_hash
            ));
        }

        self.plugins.insert(plugin.id.clone(), plugin);
        Ok(())
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
}
