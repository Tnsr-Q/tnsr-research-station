use plugin_registry::PluginRegistry;
use runtime_core::EventEnvelope;
use station_schema::{SchemaError, SchemaRegistry};
use station_supervisor::{PluginRuntimeState, StationSupervisor};

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("schema error: {0}")]
    Schema(#[from] SchemaError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventAdmission {
    pub allowed: bool,
    pub reason: Option<String>,
}

impl EventAdmission {
    pub fn allowed() -> Self {
        Self {
            allowed: true,
            reason: None,
        }
    }

    pub fn denied(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: Some(reason.into()),
        }
    }

    pub fn assert_allowed(&self) {
        assert!(
            self.allowed,
            "event denied: {}",
            self.reason.as_deref().unwrap_or("unknown policy reason")
        );
    }
}

pub struct PolicyEngine<'a> {
    pub plugins: &'a PluginRegistry,
    pub schemas: &'a SchemaRegistry,
    pub supervisor: &'a StationSupervisor,
}

impl<'a> PolicyEngine<'a> {
    pub fn admit_event(&self, event: &mut EventEnvelope) -> Result<EventAdmission, PolicyError> {
        // 1. Source plugin must be registered.
        if !self.plugins.has_plugin(&event.source) {
            return Ok(EventAdmission::denied(format!(
                "plugin not registered: {}",
                event.source
            )));
        }

        // 2. Source plugin must be allowed to publish event.topic.
        if !self.plugins.can_publish(&event.source, &event.topic) {
            return Ok(EventAdmission::denied(format!(
                "plugin {} cannot publish topic {}",
                event.source, event.topic
            )));
        }

        // 3. Source plugin must be Admitted or Running.
        let Some(state) = self.supervisor.state_of(&event.source) else {
            return Ok(EventAdmission::denied(format!(
                "plugin {} missing from supervisor runtime",
                event.source
            )));
        };

        if state != PluginRuntimeState::Admitted && state != PluginRuntimeState::Running {
            return Ok(EventAdmission::denied(format!(
                "plugin {} not in publishable state: {state:?}",
                event.source
            )));
        }

        // 4. Schema must exist.
        if self.schemas.schema_for_topic(&event.topic).is_none() {
            return Ok(EventAdmission::denied(format!(
                "missing schema for topic {}",
                event.topic
            )));
        }

        // 5. Attach schema hash if missing.
        if event.schema_hash.is_none() {
            self.schemas.attach_schema_hash(event)?;
        }

        // 6. Validate payload.
        self.schemas.validate(event)?;

        // 7. Return allowed.
        Ok(EventAdmission::allowed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_registry::{PluginKind, PluginManifest, TransportKind};
    use serde_json::json;
    use station_schema::PayloadSchema;

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
        }
    }

    fn schema() -> PayloadSchema {
        PayloadSchema::new(
            "tnsr.quantum.state.v1",
            "quantum.state",
            "1",
            vec![
                "state_dim".to_string(),
                "collapse_ratio".to_string(),
                "euler_characteristic".to_string(),
            ],
        )
    }

    #[test]
    fn admits_valid_event_and_attaches_schema_hash() {
        let manifest = manifest();

        let mut registry = PluginRegistry::default();
        registry
            .register(manifest.clone())
            .expect("register plugin");

        let mut supervisor = StationSupervisor::new("default");
        supervisor
            .register_plugin(&manifest)
            .expect("supervisor register plugin");
        supervisor
            .transition(&manifest.id, PluginRuntimeState::Admitted)
            .expect("admit plugin");

        let mut schemas = SchemaRegistry::default();
        schemas.register(schema()).expect("register schema");

        let mut event = EventEnvelope::new(
            supervisor.session.run_id.clone(),
            "quantum.state",
            "adapter_quantum",
            json!({
                "state_dim": 16,
                "collapse_ratio": 0.42,
                "euler_characteristic": 8
            }),
        );

        let admission = PolicyEngine {
            plugins: &registry,
            schemas: &schemas,
            supervisor: &supervisor,
        }
        .admit_event(&mut event)
        .expect("admission should not error");

        assert!(admission.allowed);
        assert!(event.schema_hash.is_some());
    }

    #[test]
    fn denies_unregistered_plugin() {
        let supervisor = StationSupervisor::new("default");
        let schemas = SchemaRegistry::default();
        let registry = PluginRegistry::default();
        let mut event = EventEnvelope::new(
            "run-test",
            "quantum.state",
            "unknown_plugin",
            json!({ "state_dim": 16 }),
        );

        let admission = PolicyEngine {
            plugins: &registry,
            schemas: &schemas,
            supervisor: &supervisor,
        }
        .admit_event(&mut event)
        .expect("admission should not error");

        assert!(!admission.allowed);
        assert!(admission
            .reason
            .expect("denial reason")
            .contains("not registered"));
    }
}
