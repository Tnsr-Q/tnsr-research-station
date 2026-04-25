use std::path::Path;

use artifact_ledger::ArtifactRecordRequest;
use runtime_core::{EventEnvelope, PublishReport};
use serde_json::json;
use station_run::{RunManifest, RunStatus};
use station_supervisor::{PluginRuntimeState, SupervisorError};
use station_transport::{
    build_transport, TransportConfig, TransportError, TransportKind as FactoryTransportKind,
};

use plugin_registry::TransportKind as ManifestTransportKind;

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
        let starting_transition = match self
            .context
            .supervisor
            .transition(plugin_id, PluginRuntimeState::Starting)
        {
            Ok(transition_event) => transition_event,
            Err(denial) => {
                self.record_runtime_denied(
                    plugin_id,
                    "Starting",
                    supervisor_denial_kind(&denial),
                    denial.to_string(),
                )?;
                return Err(KernelError::Supervisor(denial));
            }
        };

        self.context.replay.append_record(&starting_transition)?;

        let manifest = self.context.registry.plugin(plugin_id).ok_or_else(|| {
            KernelError::PluginRegistration(format!("unknown plugin: {plugin_id}"))
        })?;

        let transport_config = TransportConfig {
            id: format!("plugin:{plugin_id}"),
            kind: factory_transport_kind(&manifest.transport),
            endpoint: manifest.transport_config.endpoint.clone(),
            command: manifest.transport_config.command.clone(),
            args: manifest.transport_config.args.clone(),
            working_dir: manifest.transport_config.working_dir.clone(),
            env_allowlist: manifest.transport_config.env_allowlist.clone(),
            timeout_ms: manifest.transport_config.timeout_ms,
        };

        let mut transport = match build_transport(&transport_config) {
            Ok(transport) => transport,
            Err(err) => {
                self.record_transport_failed(
                    plugin_id,
                    "factory",
                    transport_config.id.as_str(),
                    err.to_string(),
                )?;
                self.transition_plugin_failed(plugin_id)?;
                return Err(KernelError::Transport(err));
            }
        };

        if let Err(err) = transport.start() {
            self.record_transport_failed(
                plugin_id,
                "start",
                transport_config.id.as_str(),
                err.to_string(),
            )?;
            self.transition_plugin_failed(plugin_id)?;
            return Err(KernelError::Transport(err));
        }
        self.context
            .transports
            .insert(plugin_id.to_string(), transport);

        let running_transition = self
            .context
            .supervisor
            .transition(plugin_id, PluginRuntimeState::Running)?;
        self.context.replay.append_record(&running_transition)?;

        Ok(())
    }

    pub fn stop_plugin(&mut self, plugin_id: &str) -> Result<(), KernelError> {
        match self
            .context
            .supervisor
            .transition(plugin_id, PluginRuntimeState::Stopping)
        {
            Ok(stopping_transition) => {
                self.context.replay.append_record(&stopping_transition)?;
            }
            Err(denial) => {
                self.record_runtime_denied(
                    plugin_id,
                    "Stopping",
                    supervisor_denial_kind(&denial),
                    denial.to_string(),
                )?;
                return Err(KernelError::Supervisor(denial));
            }
        }

        if let Some(mut transport) = self.context.transports.remove(plugin_id) {
            let transport_id = transport.id().to_string();
            if let Err(err) = transport.stop() {
                self.record_transport_failed(plugin_id, "stop", &transport_id, err.to_string())?;
                return Err(KernelError::Transport(err));
            }
            let evidence_events = transport.drain_evidence_events();
            for evidence in evidence_events {
                self.context.replay.append_record(&evidence)?;
            }
        }

        let stopped_transition = self
            .context
            .supervisor
            .transition(plugin_id, PluginRuntimeState::Stopped)?;
        self.context.replay.append_record(&stopped_transition)?;

        Ok(())
    }

    pub fn send_to_plugin(
        &mut self,
        plugin_id: &str,
        admitted: &AdmittedEvent,
    ) -> Result<(), KernelError> {
        if !self
            .context
            .registry
            .can_subscribe(plugin_id, admitted.topic())
        {
            let reason = format!(
                "plugin {plugin_id} is not subscribed to topic {}",
                admitted.topic()
            );
            self.record_runtime_denied(
                plugin_id,
                "SendToPlugin",
                "SubscriptionDenied",
                reason.as_str(),
            )?;
            let transport_id = format!("plugin:{plugin_id}");
            self.record_transport_failed(plugin_id, "send", &transport_id, reason.as_str())?;
            return Err(KernelError::Transport(TransportError::SendFailed(reason)));
        }

        if !self.context.transports.contains_key(plugin_id) {
            let reason = "transport not active";
            let transport_id = format!("plugin:{plugin_id}");
            self.record_transport_failed(plugin_id, "send", &transport_id, reason)?;
            return Err(KernelError::Transport(TransportError::NotStarted));
        }
        let transport = self
            .context
            .transports
            .get_mut(plugin_id)
            .expect("transport existence checked");
        let transport_id = transport.id().to_string();
        let event = admitted.to_envelope_with_policy_event_id();
        if let Err(err) = transport.send(&event) {
            self.record_transport_failed(plugin_id, "send", &transport_id, err.to_string())?;
            return Err(KernelError::Transport(err));
        }

        Ok(())
    }

    pub fn drain_plugin_outputs(
        &mut self,
        plugin_id: &str,
    ) -> Result<Vec<AdmittedEvent>, KernelError> {
        let transport = self
            .context
            .transports
            .get_mut(plugin_id)
            .ok_or(KernelError::Transport(TransportError::NotStarted))?;

        let evidence_events = transport.drain_evidence_events();
        let candidate_events = transport.drain_candidate_events();

        for evidence in evidence_events {
            self.context.replay.append_record(&evidence)?;
        }

        let mut admitted_events = Vec::new();
        for candidate in candidate_events {
            match self.admit(candidate)? {
                Ok(admitted) => admitted_events.push(admitted),
                Err(_denied) => {}
            }
        }

        Ok(admitted_events)
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

    fn record_runtime_denied(
        &mut self,
        plugin_id: &str,
        target_state: &str,
        denial: &str,
        reason: impl Into<String>,
    ) -> Result<(), KernelError> {
        let evidence = EventEnvelope::new(
            self.run_id(),
            "policy.runtime.denied",
            "station_kernel",
            json!({
                "plugin_id": plugin_id,
                "target_state": target_state,
                "denial": denial,
                "reason": reason.into(),
            }),
        );
        self.context.replay.append_record(&evidence)?;
        Ok(())
    }

    fn transition_plugin_failed(&mut self, plugin_id: &str) -> Result<(), KernelError> {
        let failed_transition = self
            .context
            .supervisor
            .transition(plugin_id, PluginRuntimeState::Failed)?;
        self.context.replay.append_record(&failed_transition)?;
        Ok(())
    }

    fn record_transport_failed(
        &mut self,
        plugin_id: &str,
        stage: &str,
        transport_id: &str,
        reason: impl Into<String>,
    ) -> Result<(), KernelError> {
        let evidence = EventEnvelope::new(
            self.run_id(),
            "transport.runtime.failed",
            "station_kernel",
            json!({
                "plugin_id": plugin_id,
                "stage": stage,
                "transport_id": transport_id,
                "reason": reason.into(),
            }),
        );
        self.context.replay.append_record(&evidence)?;
        Ok(())
    }
}

fn factory_transport_kind(kind: &ManifestTransportKind) -> FactoryTransportKind {
    match kind {
        ManifestTransportKind::Local => FactoryTransportKind::Local,
        ManifestTransportKind::Wasm => FactoryTransportKind::Wasm,
        ManifestTransportKind::WebSocket => FactoryTransportKind::WebSocket,
        ManifestTransportKind::Grpc => FactoryTransportKind::Grpc,
        ManifestTransportKind::ConnectRpc => FactoryTransportKind::ConnectRpc,
        ManifestTransportKind::Pyro5 => FactoryTransportKind::Pyro5,
        ManifestTransportKind::Subprocess => FactoryTransportKind::Subprocess,
        ManifestTransportKind::Ffi => FactoryTransportKind::Ffi,
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
    use std::path::PathBuf;

    use adapter_quantum::quantum_state_event;
    use plugin_registry::{PluginKind, PluginManifest, PluginTransportConfig, TransportKind};
    use station_replay::JsonlReplayLog;
    use station_run::RunStatus;

    use super::*;

    struct RunDirCleanup(PathBuf);

    impl RunDirCleanup {
        fn track(runtime: &KernelRuntime) -> Self {
            Self(runtime.run_dir().to_path_buf())
        }
    }

    impl Drop for RunDirCleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn root_profile_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace crates dir")
            .parent()
            .expect("workspace root")
            .join("profiles/default.profile.json")
    }

    fn register_admitted_plugin(
        runtime: &mut KernelRuntime,
        plugin_id: &str,
        transport: TransportKind,
    ) {
        register_admitted_plugin_with_topics(
            runtime,
            plugin_id,
            transport,
            vec![],
            vec!["quantum.state"],
        );
    }

    fn register_admitted_plugin_with_topics(
        runtime: &mut KernelRuntime,
        plugin_id: &str,
        transport: TransportKind,
        subscribes: Vec<&str>,
        publishes: Vec<&str>,
    ) {
        let manifest = PluginManifest {
            id: plugin_id.to_string(),
            kind: PluginKind::Compute,
            transport,
            version: "0.1.0".to_string(),
            artifact_hash:
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
            subscribes: subscribes.into_iter().map(ToString::to_string).collect(),
            publishes: publishes.into_iter().map(ToString::to_string).collect(),
            capabilities: vec![],
            capability_claims: vec![],
            transport_config: PluginTransportConfig::default(),
        };
        runtime
            .context
            .registry
            .register(manifest.clone())
            .expect("register plugin");
        let registered_event = runtime
            .context
            .supervisor
            .register_plugin(&manifest)
            .expect("register plugin with supervisor");
        runtime
            .context
            .replay
            .append_record(&registered_event)
            .expect("append registered event");
        let admitted_event = runtime
            .context
            .supervisor
            .transition(plugin_id, PluginRuntimeState::Admitted)
            .expect("admit plugin");
        runtime
            .context
            .replay
            .append_record(&admitted_event)
            .expect("append admitted event");
    }

    #[cfg(feature = "subprocess")]
    fn register_admitted_plugin_manifest(runtime: &mut KernelRuntime, manifest: PluginManifest) {
        runtime
            .context
            .registry
            .register(manifest.clone())
            .expect("register plugin");
        let registered_event = runtime
            .context
            .supervisor
            .register_plugin(&manifest)
            .expect("register plugin with supervisor");
        runtime
            .context
            .replay
            .append_record(&registered_event)
            .expect("append registered event");
        let admitted_event = runtime
            .context
            .supervisor
            .transition(&manifest.id, PluginRuntimeState::Admitted)
            .expect("admit plugin");
        runtime
            .context
            .replay
            .append_record(&admitted_event)
            .expect("append admitted event");
    }

    #[cfg(feature = "subprocess")]
    fn drain_plugin_outputs_until_activity(
        runtime: &mut KernelRuntime,
        plugin_id: &str,
        timeout: std::time::Duration,
    ) -> Vec<AdmittedEvent> {
        let deadline = std::time::Instant::now() + timeout;
        let mut admitted_events = Vec::new();

        loop {
            let mut drained = runtime
                .drain_plugin_outputs(plugin_id)
                .expect("drain should succeed");
            admitted_events.append(&mut drained);

            if !admitted_events.is_empty() {
                break;
            }

            let replay_events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
                .expect("events should be readable");
            if replay_events
                .iter()
                .any(|event| event.topic == "policy.event.denied")
            {
                break;
            }

            if std::time::Instant::now() >= deadline {
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        admitted_events
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
    fn start_plugin_stores_active_transport() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        register_admitted_plugin(&mut runtime, "test_local", TransportKind::Local);

        runtime
            .start_plugin("test_local")
            .expect("local plugin should start");
        assert!(runtime.context.transports.contains_key("test_local"));

        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");

        let starting_index = events
            .iter()
            .position(|event| {
                event.topic == "supervisor.plugin.transition"
                    && event.payload["plugin_id"] == "test_local"
                    && event.payload["to"] == "Starting"
            })
            .expect("starting transition should be present");

        let running_index = events
            .iter()
            .position(|event| {
                event.topic == "supervisor.plugin.transition"
                    && event.payload["plugin_id"] == "test_local"
                    && event.payload["to"] == "Running"
            })
            .expect("running transition should be present");

        assert!(starting_index < running_index);

        let _ = fs::remove_dir_all(runtime.run_dir());
    }

    #[test]
    fn stop_plugin_removes_active_transport() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        register_admitted_plugin(&mut runtime, "test_local_stop", TransportKind::Local);

        runtime
            .start_plugin("test_local_stop")
            .expect("local plugin should start");
        assert!(runtime.context.transports.contains_key("test_local_stop"));
        runtime
            .stop_plugin("test_local_stop")
            .expect("plugin should stop");
        assert!(!runtime.context.transports.contains_key("test_local_stop"));

        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");
        assert!(events.iter().any(|event| {
            event.topic == "supervisor.plugin.transition"
                && event.payload["plugin_id"] == "test_local_stop"
                && event.payload["to"] == "Stopping"
        }));
        assert!(events.iter().any(|event| {
            event.topic == "supervisor.plugin.transition"
                && event.payload["plugin_id"] == "test_local_stop"
                && event.payload["to"] == "Stopped"
        }));

        let _ = fs::remove_dir_all(runtime.run_dir());
    }

    #[test]
    fn stop_plugin_stops_transport_before_stopped_transition() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        register_admitted_plugin(&mut runtime, "test_local_order", TransportKind::Local);
        runtime
            .start_plugin("test_local_order")
            .expect("local plugin should start");
        runtime
            .stop_plugin("test_local_order")
            .expect("plugin should stop");

        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");
        let stopping_index = events
            .iter()
            .position(|event| {
                event.topic == "supervisor.plugin.transition"
                    && event.payload["plugin_id"] == "test_local_order"
                    && event.payload["to"] == "Stopping"
            })
            .expect("stopping transition should be present");
        let stopped_index = events
            .iter()
            .position(|event| {
                event.topic == "supervisor.plugin.transition"
                    && event.payload["plugin_id"] == "test_local_order"
                    && event.payload["to"] == "Stopped"
            })
            .expect("stopped transition should be present");
        assert!(stopping_index < stopped_index);
        assert!(!runtime.context.transports.contains_key("test_local_order"));

        let _ = fs::remove_dir_all(runtime.run_dir());
    }

    #[test]
    fn starting_non_enabled_transport_records_denial_or_transport_error_evidence() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        register_admitted_plugin(&mut runtime, "test_wasm", TransportKind::Wasm);

        let result = runtime.start_plugin("test_wasm");
        assert!(matches!(result, Err(KernelError::Transport(_))));

        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");
        assert!(events.iter().any(|event| {
            event.topic == "transport.runtime.failed"
                && event.payload["plugin_id"] == "test_wasm"
                && event.payload["reason"] == "transport error: transport kind not enabled"
        }));
        assert!(
            !events.iter().any(|event| {
                event.topic == "supervisor.plugin.transition"
                    && event.payload["plugin_id"] == "test_wasm"
                    && event.payload["to"] == "Running"
            }),
            "transport failures must not transition plugin to Running"
        );

        let _ = fs::remove_dir_all(runtime.run_dir());
    }

    #[test]
    fn cannot_send_to_plugin_before_start() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        register_admitted_plugin_with_topics(
            &mut runtime,
            "test_local_not_started",
            TransportKind::Local,
            vec!["quantum.state"],
            vec!["quantum.state"],
        );

        let event = EventEnvelope::new(
            runtime.run_id(),
            "quantum.state",
            "adapter_quantum",
            json!({"phase":"stable"}),
        );
        let admitted = AdmittedEvent::new(event, "policy-event-id".to_string());
        let send_result = runtime.send_to_plugin("test_local_not_started", &admitted);
        assert!(matches!(
            send_result,
            Err(KernelError::Transport(TransportError::NotStarted))
        ));

        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");
        assert!(events.iter().any(|event| {
            event.topic == "transport.runtime.failed"
                && event.payload["plugin_id"] == "test_local_not_started"
                && event.payload["stage"] == "send"
        }));

        let _ = fs::remove_dir_all(runtime.run_dir());
    }

    #[test]
    fn can_send_to_local_plugin_after_start() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        register_admitted_plugin_with_topics(
            &mut runtime,
            "test_local_send",
            TransportKind::Local,
            vec!["quantum.state"],
            vec!["quantum.state"],
        );
        runtime
            .start_plugin("test_local_send")
            .expect("local plugin should start");

        let event = EventEnvelope::new(
            runtime.run_id(),
            "quantum.state",
            "test_local_send",
            json!({"phase":"stable"}),
        );
        let admitted = AdmittedEvent::new(event, "policy-event-id".to_string());
        runtime
            .send_to_plugin("test_local_send", &admitted)
            .expect("send to local plugin should succeed");

        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");
        assert!(!events.iter().any(|event| {
            event.topic == "transport.runtime.failed"
                && event.payload["plugin_id"] == "test_local_send"
        }));

        let _ = fs::remove_dir_all(runtime.run_dir());
    }

    #[test]
    fn send_to_plugin_requires_target_subscription() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        register_admitted_plugin_with_topics(
            &mut runtime,
            "test_local_unsubscribed",
            TransportKind::Local,
            vec![],
            vec!["quantum.state"],
        );
        runtime
            .start_plugin("test_local_unsubscribed")
            .expect("local plugin should start");

        let event = EventEnvelope::new(
            runtime.run_id(),
            "quantum.state",
            "adapter_quantum",
            json!({"phase":"stable"}),
        );
        let admitted = AdmittedEvent::new(event, "policy-event-id".to_string());
        let send_result = runtime.send_to_plugin("test_local_unsubscribed", &admitted);
        assert!(matches!(
            send_result,
            Err(KernelError::Transport(TransportError::SendFailed(_)))
        ));

        let _ = fs::remove_dir_all(runtime.run_dir());
    }

    #[test]
    fn unsubscribed_target_send_records_runtime_denial() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        register_admitted_plugin_with_topics(
            &mut runtime,
            "test_local_unsubscribed_denial",
            TransportKind::Local,
            vec![],
            vec!["quantum.state"],
        );
        runtime
            .start_plugin("test_local_unsubscribed_denial")
            .expect("local plugin should start");

        let event = EventEnvelope::new(
            runtime.run_id(),
            "quantum.state",
            "adapter_quantum",
            json!({"phase":"stable"}),
        );
        let admitted = AdmittedEvent::new(event, "policy-event-id".to_string());
        let _ = runtime.send_to_plugin("test_local_unsubscribed_denial", &admitted);

        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");
        assert!(events.iter().any(|event| {
            event.topic == "policy.runtime.denied"
                && event.payload["plugin_id"] == "test_local_unsubscribed_denial"
                && event.payload["target_state"] == "SendToPlugin"
                && event.payload["denial"] == "SubscriptionDenied"
        }));

        let _ = fs::remove_dir_all(runtime.run_dir());
    }

    #[test]
    fn transport_start_failure_transitions_plugin_to_failed() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        register_admitted_plugin(
            &mut runtime,
            "test_wasm_failed_transition",
            TransportKind::Wasm,
        );

        let result = runtime.start_plugin("test_wasm_failed_transition");
        assert!(matches!(result, Err(KernelError::Transport(_))));

        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");
        let starting_index = events
            .iter()
            .position(|event| {
                event.topic == "supervisor.plugin.transition"
                    && event.payload["plugin_id"] == "test_wasm_failed_transition"
                    && event.payload["to"] == "Starting"
            })
            .expect("starting transition should be present");
        let transport_failed_index = events
            .iter()
            .position(|event| {
                event.topic == "transport.runtime.failed"
                    && event.payload["plugin_id"] == "test_wasm_failed_transition"
            })
            .expect("transport failure should be present");
        let failed_transition_index = events
            .iter()
            .position(|event| {
                event.topic == "supervisor.plugin.transition"
                    && event.payload["plugin_id"] == "test_wasm_failed_transition"
                    && event.payload["to"] == "Failed"
            })
            .expect("failed transition should be present");

        assert!(starting_index < transport_failed_index);
        assert!(transport_failed_index < failed_transition_index);

        let _ = fs::remove_dir_all(runtime.run_dir());
    }

    #[cfg(unix)]
    #[test]
    fn replay_append_failure_during_transport_failure_is_not_swallowed() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        let _run_dir_cleanup = RunDirCleanup::track(&runtime);
        register_admitted_plugin_with_topics(
            &mut runtime,
            "test_local_replay_failure",
            TransportKind::Local,
            vec!["quantum.state"],
            vec!["quantum.state"],
        );
        runtime
            .context
            .replay
            .debug_set_broken_writer()
            .expect("configure broken writer");

        let event = EventEnvelope::new(
            runtime.run_id(),
            "quantum.state",
            "adapter_quantum",
            json!({"phase":"stable"}),
        );
        let admitted = AdmittedEvent::new(event, "policy-event-id".to_string());
        let send_result = runtime.send_to_plugin("test_local_replay_failure", &admitted);
        assert!(matches!(send_result, Err(KernelError::Replay(_))));
    }

    #[test]
    fn transport_error_is_recorded_as_replay_evidence() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        register_admitted_plugin(&mut runtime, "test_bad_transport", TransportKind::Wasm);

        let result = runtime.start_plugin("test_bad_transport");
        assert!(matches!(result, Err(KernelError::Transport(_))));

        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");
        let failure_event = events
            .iter()
            .find(|event| event.topic == "transport.runtime.failed")
            .expect("transport failure evidence should be present");
        assert_eq!(failure_event.source, "station_kernel");
        assert_eq!(failure_event.payload["plugin_id"], "test_bad_transport");
        assert_eq!(
            failure_event.payload["transport_id"],
            "plugin:test_bad_transport"
        );

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

    #[cfg(feature = "subprocess")]
    #[cfg(unix)]
    #[test]
    fn subprocess_manifest_requires_command_when_feature_enabled() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        register_admitted_plugin_with_topics(
            &mut runtime,
            "test_subprocess_no_command",
            TransportKind::Subprocess,
            vec!["quantum.state"],
            vec!["quantum.state"],
        );

        let result = runtime.start_plugin("test_subprocess_no_command");
        assert!(matches!(result, Err(KernelError::Transport(_))));

        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");
        assert!(events.iter().any(|event| {
            event.topic == "transport.runtime.failed"
                && event.payload["plugin_id"] == "test_subprocess_no_command"
                && event.payload["reason"]
                    == "transport error: subprocess transport requires command"
        }));
    }

    #[cfg(feature = "subprocess")]
    #[cfg(unix)]
    #[test]
    fn kernel_starts_subprocess_from_manifest_transport_config() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        let manifest = PluginManifest {
            id: "test_subprocess_manifest".to_string(),
            kind: PluginKind::Compute,
            transport: TransportKind::Subprocess,
            version: "0.1.0".to_string(),
            artifact_hash:
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
            subscribes: vec!["quantum.state".to_string()],
            publishes: vec!["quantum.state".to_string()],
            capabilities: vec![],
            capability_claims: vec![],
            transport_config: PluginTransportConfig {
                command: Some("/bin/cat".to_string()),
                timeout_ms: Some(50),
                ..PluginTransportConfig::default()
            },
        };
        register_admitted_plugin_manifest(&mut runtime, manifest);

        runtime
            .start_plugin("test_subprocess_manifest")
            .expect("subprocess plugin should start");
        assert!(runtime
            .context
            .transports
            .contains_key("test_subprocess_manifest"));
    }

    #[cfg(feature = "subprocess")]
    #[cfg(unix)]
    #[test]
    fn kernel_drains_subprocess_stderr_as_replay_evidence() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        register_admitted_plugin_manifest(
            &mut runtime,
            PluginManifest {
                id: "test_subprocess_stderr".to_string(),
                kind: PluginKind::Compute,
                transport: TransportKind::Subprocess,
                version: "0.1.0".to_string(),
                artifact_hash:
                    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .to_string(),
                subscribes: vec!["quantum.state".to_string()],
                publishes: vec!["quantum.state".to_string()],
                capabilities: vec![],
                capability_claims: vec![],
                transport_config: PluginTransportConfig {
                    command: Some("/bin/sh".to_string()),
                    args: vec![
                        "-lc".to_string(),
                        "echo 'stderr-line' 1>&2; sleep 0.1".to_string(),
                    ],
                    ..PluginTransportConfig::default()
                },
            },
        );
        runtime
            .start_plugin("test_subprocess_stderr")
            .expect("subprocess should start");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let stderr_seen = loop {
            let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
                .expect("events should be readable");
            if events
                .iter()
                .any(|event| event.topic == "transport.runtime.stderr")
            {
                break true;
            }
            if std::time::Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };

        runtime
            .stop_plugin("test_subprocess_stderr")
            .expect("stop should succeed");
        assert!(
            stderr_seen,
            "expected transport.runtime.stderr evidence before timeout"
        );
    }

    #[cfg(feature = "subprocess")]
    #[cfg(unix)]
    #[test]
    fn kernel_rejects_unadmitted_subprocess_stdout_event() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        runtime
            .register_schemas()
            .expect("schemas should register successfully");
        register_admitted_plugin_manifest(
            &mut runtime,
            PluginManifest {
                id: "test_subprocess_denied".to_string(),
                kind: PluginKind::Compute,
                transport: TransportKind::Subprocess,
                version: "0.1.0".to_string(),
                artifact_hash:
                    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .to_string(),
                subscribes: vec!["quantum.state".to_string()],
                publishes: vec![],
                capabilities: vec![],
                capability_claims: vec![],
                transport_config: PluginTransportConfig {
                    command: Some("/bin/cat".to_string()),
                    ..PluginTransportConfig::default()
                },
            },
        );
        runtime
            .start_plugin("test_subprocess_denied")
            .expect("subprocess should start");
        let event = EventEnvelope::new(
            runtime.run_id(),
            "quantum.state",
            "test_subprocess_denied",
            json!({"state_dim": 1, "collapse_ratio": 0.5, "euler_characteristic": 1}),
        );
        let admitted = AdmittedEvent::new(event, "policy-event-id".to_string());
        runtime
            .send_to_plugin("test_subprocess_denied", &admitted)
            .expect("send should succeed");
        let admitted_events = drain_plugin_outputs_until_activity(
            &mut runtime,
            "test_subprocess_denied",
            std::time::Duration::from_secs(1),
        );
        assert!(admitted_events.is_empty());

        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");
        assert!(events
            .iter()
            .any(|event| event.topic == "policy.event.denied"));
    }

    #[cfg(feature = "subprocess")]
    #[cfg(unix)]
    #[test]
    fn kernel_admits_valid_subprocess_stdout_event_before_publish() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        runtime
            .register_schemas()
            .expect("schemas should register successfully");
        register_admitted_plugin_manifest(
            &mut runtime,
            PluginManifest {
                id: "test_subprocess_allowed".to_string(),
                kind: PluginKind::Compute,
                transport: TransportKind::Subprocess,
                version: "0.1.0".to_string(),
                artifact_hash:
                    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .to_string(),
                subscribes: vec!["quantum.state".to_string()],
                publishes: vec!["quantum.state".to_string()],
                capabilities: vec![],
                capability_claims: vec![],
                transport_config: PluginTransportConfig {
                    command: Some("/bin/cat".to_string()),
                    ..PluginTransportConfig::default()
                },
            },
        );
        runtime
            .start_plugin("test_subprocess_allowed")
            .expect("subprocess should start");
        let event = EventEnvelope::new(
            runtime.run_id(),
            "quantum.state",
            "test_subprocess_allowed",
            json!({"state_dim": 1, "collapse_ratio": 0.5, "euler_characteristic": 1}),
        );
        let admitted = AdmittedEvent::new(event, "policy-event-id".to_string());
        runtime
            .send_to_plugin("test_subprocess_allowed", &admitted)
            .expect("send should succeed");
        let admitted_events = drain_plugin_outputs_until_activity(
            &mut runtime,
            "test_subprocess_allowed",
            std::time::Duration::from_secs(1),
        );
        assert_eq!(admitted_events.len(), 1);
        assert_eq!(admitted_events[0].topic(), "quantum.state");
    }

    #[cfg(feature = "subprocess")]
    #[cfg(unix)]
    #[test]
    fn subprocess_candidate_event_cannot_bypass_policy_engine() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        runtime
            .register_schemas()
            .expect("schemas should register successfully");
        register_admitted_plugin_manifest(
            &mut runtime,
            PluginManifest {
                id: "test_subprocess_bypass".to_string(),
                kind: PluginKind::Compute,
                transport: TransportKind::Subprocess,
                version: "0.1.0".to_string(),
                artifact_hash:
                    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .to_string(),
                subscribes: vec!["quantum.state".to_string()],
                publishes: vec![],
                capabilities: vec![],
                capability_claims: vec![],
                transport_config: PluginTransportConfig {
                    command: Some("/bin/cat".to_string()),
                    ..PluginTransportConfig::default()
                },
            },
        );
        runtime
            .start_plugin("test_subprocess_bypass")
            .expect("subprocess should start");
        let event = EventEnvelope::new(
            runtime.run_id(),
            "quantum.state",
            "test_subprocess_bypass",
            json!({"state_dim": 1, "collapse_ratio": 0.5, "euler_characteristic": 1}),
        );
        let admitted = AdmittedEvent::new(event, "policy-event-id".to_string());
        runtime
            .send_to_plugin("test_subprocess_bypass", &admitted)
            .expect("send should succeed");

        let admitted_events = runtime
            .drain_plugin_outputs("test_subprocess_bypass")
            .expect("drain should succeed");
        assert!(admitted_events.is_empty());

        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");
        assert!(!events.iter().any(
            |event| event.source == "test_subprocess_bypass" && event.topic == "quantum.state"
        ));
    }

    #[cfg(feature = "subprocess")]
    #[cfg(unix)]
    #[test]
    fn subprocess_timeout_evidence_is_appended_to_replay() {
        let mut runtime =
            KernelRuntime::from_profile_path(root_profile_path()).expect("runtime should init");
        register_admitted_plugin_manifest(
            &mut runtime,
            PluginManifest {
                id: "test_subprocess_timeout".to_string(),
                kind: PluginKind::Compute,
                transport: TransportKind::Subprocess,
                version: "0.1.0".to_string(),
                artifact_hash:
                    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .to_string(),
                subscribes: vec!["quantum.state".to_string()],
                publishes: vec!["quantum.state".to_string()],
                capabilities: vec![],
                capability_claims: vec![],
                transport_config: PluginTransportConfig {
                    command: Some("/bin/sleep".to_string()),
                    args: vec!["5".to_string()],
                    timeout_ms: Some(10),
                    ..PluginTransportConfig::default()
                },
            },
        );
        runtime
            .start_plugin("test_subprocess_timeout")
            .expect("subprocess should start");
        runtime
            .stop_plugin("test_subprocess_timeout")
            .expect("stop should succeed");
        let events = JsonlReplayLog::read_all_events(runtime.context.events_path.clone())
            .expect("events should be readable");
        assert!(events
            .iter()
            .any(|event| event.topic == "transport.runtime.timeout"));
    }
}
