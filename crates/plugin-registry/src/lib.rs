use std::collections::{HashMap, HashSet};

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
    allowed_topics: HashSet<String>,
}

impl PluginRegistry {
    pub fn register(&mut self, plugin: PluginManifest) -> Result<(), String> {
        if self.plugins.contains_key(&plugin.id) {
            return Err(format!("duplicate plugin: {}", plugin.id));
        }

        self.allowed_topics
            .extend(plugin.subscribes.iter().cloned());
        self.allowed_topics.extend(plugin.publishes.iter().cloned());
        self.plugins.insert(plugin.id.clone(), plugin);
        Ok(())
    }

    pub fn topic_allowed(&self, topic: &str) -> bool {
        self.allowed_topics.contains(topic)
    }
}
