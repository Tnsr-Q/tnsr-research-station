use plugin_registry::PluginManifest;
use runtime_core::{EventEnvelope, SessionState};
use serde::{Deserialize, Serialize};
use serde_json::json;
use station_run::RuntimePermissions;
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

    #[error("plugin {plugin_id} capability claim {claim} requires network but profile forbids it")]
    NetworkDenied { plugin_id: String, claim: String },

    #[error(
        "plugin {plugin_id} capability claim {claim} requires filesystem but profile forbids it"
    )]
    FilesystemDenied { plugin_id: String, claim: String },

    #[error("plugin {plugin_id} capability claim {claim} requires GPU but profile forbids it")]
    GpuDenied { plugin_id: String, claim: String },

    #[error("plugin {plugin_id} capability claim {claim} is projection-only but projection permissions are disabled")]
    ProjectionDenied { plugin_id: String, claim: String },

    #[error("plugin {plugin_id} capability claim {claim} is nondeterministic but profile forbids nondeterminism")]
    NondeterministicDenied { plugin_id: String, claim: String },
}

pub struct StationSupervisor {
    pub session: SessionState,
    plugins: HashMap<String, PluginRuntimeRecord>,
    manifests: HashMap<String, PluginManifest>,
    permissions: RuntimePermissions,
}

impl StationSupervisor {
    pub fn new(profile_name: impl Into<String>) -> Self {
        Self::new_with_permissions(profile_name, RuntimePermissions::default())
    }

    pub fn new_with_permissions(
        profile_name: impl Into<String>,
        permissions: RuntimePermissions,
    ) -> Self {
        Self {
            session: SessionState::new(profile_name),
            plugins: HashMap::new(),
            manifests: HashMap::new(),
            permissions,
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
        self.manifests.insert(manifest.id.clone(), manifest.clone());

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
        let from = self
            .plugins
            .get(plugin_id)
            .map(|record| record.state)
            .ok_or_else(|| SupervisorError::UnknownPlugin(plugin_id.to_string()))?;

        if !is_valid_transition(from, to) {
            return Err(SupervisorError::InvalidTransition {
                plugin_id: plugin_id.to_string(),
                from,
                to,
            });
        }

        if to == PluginRuntimeState::Starting || to == PluginRuntimeState::Running {
            let manifest = self
                .manifests
                .get(plugin_id)
                .ok_or_else(|| SupervisorError::UnknownPlugin(plugin_id.to_string()))?;
            self.ensure_manifest_permitted(manifest)?;
        }

        let record = self
            .plugins
            .get_mut(plugin_id)
            .ok_or_else(|| SupervisorError::UnknownPlugin(plugin_id.to_string()))?;
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

    fn ensure_manifest_permitted(&self, manifest: &PluginManifest) -> Result<(), SupervisorError> {
        for claim in &manifest.capability_claims {
            if !claim.enabled {
                continue;
            }

            if claim.requires_network && !self.permissions.allow_network {
                return Err(SupervisorError::NetworkDenied {
                    plugin_id: manifest.id.clone(),
                    claim: claim.name.clone(),
                });
            }

            if claim.requires_filesystem && !self.permissions.allow_filesystem {
                return Err(SupervisorError::FilesystemDenied {
                    plugin_id: manifest.id.clone(),
                    claim: claim.name.clone(),
                });
            }

            if claim.requires_gpu && !self.permissions.allow_gpu {
                return Err(SupervisorError::GpuDenied {
                    plugin_id: manifest.id.clone(),
                    claim: claim.name.clone(),
                });
            }

            if claim.projection_only && !self.permissions.allow_projection_only {
                return Err(SupervisorError::ProjectionDenied {
                    plugin_id: manifest.id.clone(),
                    claim: claim.name.clone(),
                });
            }

            if !claim.deterministic && !self.permissions.allow_nondeterministic {
                return Err(SupervisorError::NondeterministicDenied {
                    plugin_id: manifest.id.clone(),
                    claim: claim.name.clone(),
                });
            }
        }

        Ok(())
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
    use plugin_registry::{PluginKind, PluginManifest, PluginTransportConfig, TransportKind};

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
            transport_config: PluginTransportConfig::default(),
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

    #[test]
    fn rejects_manifest_when_gpu_permission_is_denied() {
        let mut supervisor = StationSupervisor::new_with_permissions(
            "default",
            RuntimePermissions {
                allow_network: false,
                allow_filesystem: false,
                allow_gpu: false,
                allow_projection_only: true,
                allow_nondeterministic: false,
            },
        );
        let mut manifest = manifest();
        manifest.capability_claims = vec![plugin_registry::CapabilityClaim {
            name: "simulation.hypergrid".to_string(),
            enabled: true,
            required: true,
            deterministic: true,
            replay_safe: true,
            projection_only: false,
            emits_artifacts: false,
            requires_network: false,
            requires_filesystem: false,
            requires_gpu: true,
            max_runtime_ms: None,
        }];
        supervisor
            .register_plugin(&manifest)
            .expect("register plugin");
        supervisor
            .transition(&manifest.id, PluginRuntimeState::Admitted)
            .expect("admit plugin");

        let denied = supervisor.transition(&manifest.id, PluginRuntimeState::Starting);

        assert!(matches!(denied, Err(SupervisorError::GpuDenied { .. })));
    }
}
