use std::path::Path;

use artifact_ledger::ArtifactRecordRequest;
use runtime_core::{EventEnvelope, PublishReport};
use serde_json::json;
use station_run::{RunManifest, RunStatus};
use station_supervisor::{PluginRuntimeState, SupervisorError};

use crate::{
    admission::{self, AdmittedEvent},
    closure,
    context::{load_profile_plugin_manifest, load_profile_schema, KernelContext},
    errors::KernelError,
};

pub struct KernelRuntime {
    context: KernelContext,
}

impl KernelRuntime {
    pub fn from_profile_path(path: impl AsRef<Path>) -> Result<Self, KernelError> {
        Ok(Self {
            context: KernelContext::from_profile_path(path)?,
        })
    }

    pub fn register_plugins(&mut self) -> Result<(), KernelError> {
        let plugin_paths = self.context.profile.plugin_manifests.clone();
        for plugin_path in plugin_paths {
            let manifest = load_profile_plugin_manifest(&self.context, &plugin_path)?;

            self.context
                .registry
                .register(manifest.clone())
                .map_err(KernelError::PluginRegistration)?;

            let supervisor_event = self.context.supervisor.register_plugin(&manifest)?;
            self.context.replay.append_record(&supervisor_event)?;

            let admitted_event = self
                .context
                .supervisor
                .transition(&manifest.id, PluginRuntimeState::Admitted)?;
            self.context.replay.append_record(&admitted_event)?;
        }

        Ok(())
    }

    pub fn register_schemas(&mut self) -> Result<(), KernelError> {
        let schema_paths = self.context.profile.schema_files.clone();
        for schema_path in schema_paths {
            let schema = load_profile_schema(&self.context, &schema_path)?;
            self.context.schemas.register(schema)?;
        }

        Ok(())
    }

    pub fn admit(
        &mut self,
        event: EventEnvelope,
    ) -> Result<Result<AdmittedEvent, station_policy::EventAdmission>, KernelError> {
        admission::admit_and_record(&mut self.context, event)
    }

    pub fn publish_admitted(
        &mut self,
        admitted: AdmittedEvent,
    ) -> Result<PublishReport, KernelError> {
        let (mut event, policy_event_id) = admitted.into_parts();
        event.policy_event_id = Some(policy_event_id);

        let payload_bytes = serde_json::to_vec(&event.payload)?;
        let artifact = self.context.ledger.record_bytes(ArtifactRecordRequest {
            artifact_id: format!("{}_payload", event.topic.replace('.', "_")),
            source: event.source.clone(),
            data: payload_bytes,
            content_type: Some("application/json".into()),
            trace_id: Some(event.trace_id.clone()),
            parent_hash: event.input_hash.clone(),
            schema_hash: event.schema_hash.clone(),
        });
        event.artifact_hash = Some(artifact.hash);

        let rx = self.context.bus.subscribe(event.topic.clone());
        let report = self.context.bus.publish(event);
        if report.failed > 0 {
            return Err(KernelError::PublicationFailed {
                failed: report.failed,
            });
        }

        let received = rx.recv()?;
        self.context.replay.append_record(&received)?;

        Ok(report)
    }

    pub fn start_plugin(&mut self, plugin_id: &str) -> Result<(), KernelError> {
        match self
            .context
            .supervisor
            .transition(plugin_id, PluginRuntimeState::Starting)
        {
            Ok(transition_event) => {
                self.context.replay.append_record(&transition_event)?;
                Ok(())
            }
            Err(denial) => {
                let evidence = EventEnvelope::new(
                    self.run_id(),
                    "policy.runtime.denied",
                    "station_kernel",
                    json!({
                        "plugin_id": plugin_id,
                        "target_state": "Starting",
                        "denial": supervisor_denial_kind(&denial),
                        "reason": denial.to_string(),
                    }),
                );
                self.context.replay.append_record(&evidence)?;
                Err(KernelError::Supervisor(denial))
            }
        }
    }

    pub fn seal_run(self, status: RunStatus) -> Result<RunManifest, KernelError> {
        closure::seal_run(self.context, status, None)
    }

    pub fn seal_run_with_reason(
        self,
        status: RunStatus,
        failure_reason: Option<String>,
    ) -> Result<RunManifest, KernelError> {
        closure::seal_run(self.context, status, failure_reason)
    }

    pub fn run_dir(&self) -> &Path {
        &self.context.run_dir
    }

    pub fn run_id(&self) -> String {
        self.context.supervisor.session.run_id.clone()
    }
}

fn supervisor_denial_kind(denial: &SupervisorError) -> &'static str {
    match denial {
        SupervisorError::DuplicatePlugin(_) => "DuplicatePlugin",
        SupervisorError::UnknownPlugin(_) => "UnknownPlugin",
        SupervisorError::InvalidTransition { .. } => "InvalidTransition",
        SupervisorError::NetworkDenied { .. } => "NetworkDenied",
        SupervisorError::FilesystemDenied { .. } => "FilesystemDenied",
        SupervisorError::GpuDenied { .. } => "GpuDenied",
        SupervisorError::ProjectionDenied { .. } => "ProjectionDenied",
        SupervisorError::NondeterministicDenied { .. } => "NondeterministicDenied",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use adapter_quantum::quantum_state_event;
    use station_replay::JsonlReplayLog;
    use station_run::RunStatus;

    use super::*;

    fn root_profile_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace crates dir")
            .parent()
            .expect("workspace root")
            .join("profiles/default.profile.json")
    }

    #[test]
    fn runtime_can_register_plugins_and_schemas() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        runtime
            .register_plugins()
            .expect("plugins should register successfully");
        runtime
            .register_schemas()
            .expect("schemas should register successfully");
    }

    #[test]
    fn runtime_denial_can_be_sealed_without_panicking() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        runtime
            .register_schemas()
            .expect("schemas should register successfully");

        let event = quantum_state_event("test-run".to_string());
        let admission = runtime
            .admit(event)
            .expect("admission should return result");

        let denied = admission.expect_err("event should be denied without admitted plugin");

        let manifest = runtime
            .seal_run_with_reason(RunStatus::Failed, denied.reason.clone())
            .expect("failed run should still seal");

        assert_eq!(manifest.status, RunStatus::Failed);
        assert!(manifest.failure_reason.is_some());
    }

    #[test]
    fn denied_startup_transition_is_recorded_as_replay_evidence() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        runtime
            .register_plugins()
            .expect("plugins should register successfully");

        let result = runtime.start_plugin("adapter_rag");
        assert!(matches!(
            result,
            Err(KernelError::Supervisor(
                SupervisorError::NetworkDenied { .. }
            ))
        ));

        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");
        let denial_event = events
            .iter()
            .find(|event| event.topic == "policy.runtime.denied")
            .expect("denial evidence should be present");
        assert_eq!(denial_event.source, "station_kernel");
        assert_eq!(denial_event.payload["plugin_id"], "adapter_rag");
        assert_eq!(denial_event.payload["target_state"], "Starting");
        assert_eq!(denial_event.payload["denial"], "NetworkDenied");

        let _ = fs::remove_dir_all(runtime.run_dir());
    }

    #[test]
    fn admitted_event_exposes_metadata_but_not_inner_constructor() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        runtime
            .register_plugins()
            .expect("plugins should register successfully");
        runtime
            .register_schemas()
            .expect("schemas should register successfully");

        let event = quantum_state_event(runtime.run_id());
        let admitted = runtime
            .admit(event)
            .expect("admission should return result")
            .expect("event should be admitted");

        assert_eq!(admitted.topic(), "quantum.state");
        assert_eq!(admitted.source(), "adapter_quantum");
        assert!(!admitted.policy_event_id().is_empty());
        assert!(!admitted.event_id().is_empty());

        let _ = fs::remove_dir_all(runtime.run_dir());
    }

    #[test]
    fn published_event_records_policy_event_id() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        runtime
            .register_plugins()
            .expect("plugins should register successfully");
        runtime
            .register_schemas()
            .expect("schemas should register successfully");

        let event = quantum_state_event(runtime.run_id());
        let admitted = runtime
            .admit(event)
            .expect("admission should return result")
            .expect("event should be admitted");
        let expected_policy_event_id = admitted.policy_event_id().to_string();

        runtime
            .publish_admitted(admitted)
            .expect("publish should succeed");

        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");
        let published_event = events
            .iter()
            .find(|event| event.topic == "quantum.state")
            .expect("published event should be present");
        assert_eq!(
            published_event.policy_event_id.as_deref(),
            Some(expected_policy_event_id.as_str())
        );

        let _ = fs::remove_dir_all(runtime.run_dir());
    }
}
