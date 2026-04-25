use plugin_registry::PluginManifest;
use runtime_core::{EventEnvelope, SessionState};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginRuntimeState {
    Registered,
    Admitted,
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    Quarantined,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRuntimeRecord {
    pub plugin_id: String,
    pub state: PluginRuntimeState,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RunProfile {
    pub name: String,
    pub enabled_plugins: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("plugin already tracked: {0}")]
    DuplicatePlugin(String),

    #[error("unknown plugin: {0}")]
    UnknownPlugin(String),

    #[error("invalid transition for {plugin_id}: {from:?} -> {to:?}")]
    InvalidTransition {
        plugin_id: String,
        from: PluginRuntimeState,
        to: PluginRuntimeState,
    },
}

pub struct StationSupervisor {
    pub session: SessionState,
    plugins: HashMap<String, PluginRuntimeRecord>,
}

impl StationSupervisor {
    pub fn new(profile_name: impl Into<String>) -> Self {
        Self {
            session: SessionState::new(profile_name),
            plugins: HashMap::new(),
        }
    }

    pub fn register_plugin(
        &mut self,
        manifest: &PluginManifest,
    ) -> Result<EventEnvelope, SupervisorError> {
        if self.plugins.contains_key(&manifest.id) {
            return Err(SupervisorError::DuplicatePlugin(manifest.id.clone()));
        }

        self.plugins.insert(
            manifest.id.clone(),
            PluginRuntimeRecord {
                plugin_id: manifest.id.clone(),
                state: PluginRuntimeState::Registered,
                last_error: None,
            },
        );

        Ok(EventEnvelope::new(
            self.session.run_id.clone(),
            "supervisor.plugin.registered",
            "station_supervisor",
            json!({
                "plugin_id": manifest.id,
                "state": "registered"
            }),
        ))
    }

    pub fn transition(
        &mut self,
        plugin_id: &str,
        to: PluginRuntimeState,
    ) -> Result<EventEnvelope, SupervisorError> {
        let record = self
            .plugins
            .get_mut(plugin_id)
            .ok_or_else(|| SupervisorError::UnknownPlugin(plugin_id.to_string()))?;

        let from = record.state;

        if !is_valid_transition(from, to) {
            return Err(SupervisorError::InvalidTransition {
                plugin_id: plugin_id.to_string(),
                from,
                to,
            });
        }

        record.state = to;

        Ok(EventEnvelope::new(
            self.session.run_id.clone(),
            "supervisor.plugin.transition",
            "station_supervisor",
            json!({
                "plugin_id": plugin_id,
                "from": format!("{from:?}"),
                "to": format!("{to:?}")
            }),
        ))
    }

    pub fn state_of(&self, plugin_id: &str) -> Option<PluginRuntimeState> {
        self.plugins.get(plugin_id).map(|r| r.state)
    }
}

fn is_valid_transition(from: PluginRuntimeState, to: PluginRuntimeState) -> bool {
    use PluginRuntimeState::*;

    matches!(
        (from, to),
        (Registered, Admitted)
            | (Admitted, Starting)
            | (Starting, Running)
            | (Running, Stopping)
            | (Stopping, Stopped)
            | (Starting, Failed)
            | (Running, Failed)
            | (Failed, Quarantined)
            | (Failed, Starting)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_registry::{PluginKind, PluginManifest, TransportKind};

    fn manifest() -> PluginManifest {
        PluginManifest {
            id: "adapter_quantum".to_string(),
            kind: PluginKind::Compute,
            transport: TransportKind::Local,
            version: "0.1.0".to_string(),
            artifact_hash:
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
            subscribes: vec!["quantum.analyze".to_string()],
            publishes: vec!["quantum.state".to_string()],
            capabilities: vec!["collapse_ratio".to_string()],
            capability_claims: vec![],
        }
    }

    #[test]
    fn test_register_and_transition_plugin() {
        let mut supervisor = StationSupervisor::new("default");
        let manifest = manifest();

        let event = supervisor
            .register_plugin(&manifest)
            .expect("register plugin");

        assert_eq!(event.topic, "supervisor.plugin.registered");
        assert_eq!(
            supervisor.state_of("adapter_quantum"),
            Some(PluginRuntimeState::Registered)
        );

        supervisor
            .transition("adapter_quantum", PluginRuntimeState::Admitted)
            .expect("admit plugin");

        assert_eq!(
            supervisor.state_of("adapter_quantum"),
            Some(PluginRuntimeState::Admitted)
        );
    }

    #[test]
    fn test_invalid_transition_rejected() {
        let mut supervisor = StationSupervisor::new("default");
        let manifest = manifest();

        supervisor
            .register_plugin(&manifest)
            .expect("register plugin");

        let result = supervisor.transition("adapter_quantum", PluginRuntimeState::Running);

        assert!(result.is_err());
    }
}
