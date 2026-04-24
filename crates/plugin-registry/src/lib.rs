use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PluginManifest {
    pub id: String,
    pub kind: String,
    pub transport: String,
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

impl PluginRegistry {
    pub fn register(&mut self, plugin: PluginManifest) -> Result<(), String> {
        if self.plugins.contains_key(&plugin.id) {
            return Err(format!("duplicate plugin: {}", plugin.id));
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
