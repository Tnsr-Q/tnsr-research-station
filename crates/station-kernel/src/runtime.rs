use std::path::Path;

use artifact_ledger::ArtifactRecordRequest;
use runtime_core::{EventEnvelope, PublishReport};
use station_policy::EventAdmission;
use station_run::{RunManifest, RunStatus};
use station_supervisor::PluginRuntimeState;

use crate::{
    admission, closure,
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

    pub fn admit_and_record(
        &mut self,
        event: &mut EventEnvelope,
    ) -> Result<EventAdmission, KernelError> {
        admission::admit_and_record(&mut self.context, event)
    }

    pub fn publish_admitted(
        &mut self,
        mut event: EventEnvelope,
    ) -> Result<PublishReport, KernelError> {
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

    pub fn seal_run(self, status: RunStatus) -> Result<RunManifest, KernelError> {
        closure::seal_run(self.context, status, None)
    }

    pub(crate) fn seal_run_with_reason(
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

#[cfg(test)]
mod tests {
    use adapter_quantum::quantum_state_event;
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

        let mut event = quantum_state_event("test-run".to_string());
        let admission = runtime
            .admit_and_record(&mut event)
            .expect("admission should return result");

        assert!(!admission.allowed);

        let manifest = runtime
            .seal_run_with_reason(RunStatus::Failed, admission.reason.clone())
            .expect("failed run should still seal");

        assert_eq!(manifest.status, RunStatus::Failed);
        assert!(manifest.failure_reason.is_some());
    }
}
