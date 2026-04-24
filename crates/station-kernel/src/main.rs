use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use adapter_quantum::quantum_state_event;
use artifact_ledger::{ArtifactLedger, ArtifactRecordRequest};
use bridge_browser::to_browser_frame;
use bridge_gpui::to_gpui_overlay_line;
use plugin_registry::{PluginKind, PluginManifest, PluginRegistry, TransportKind};
use runtime_core::EventBus;
use station_policy::PolicyEngine;
use station_replay::JsonlReplayLog;
use station_run::{
    write_manifest_json, ArtifactSummary, PluginSummary, RunManifest, RunStatus, SchemaSummary,
};
use station_schema::{PayloadSchema, SchemaRegistry};
use station_supervisor::{PluginRuntimeState, StationSupervisor};

fn now_ms() -> Result<u128, std::time::SystemTimeError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
}

fn plugin_kind_wire(kind: &PluginKind) -> &'static str {
    match kind {
        PluginKind::Compute => "compute",
        PluginKind::Renderer => "renderer",
        PluginKind::Bridge => "bridge",
        PluginKind::Memory => "memory",
        PluginKind::Agent => "agent",
        PluginKind::Verifier => "verifier",
    }
}

fn transport_kind_wire(kind: &TransportKind) -> &'static str {
    match kind {
        TransportKind::Local => "local",
        TransportKind::Wasm => "wasm",
        TransportKind::WebSocket => "websocket",
        TransportKind::Grpc => "grpc",
        TransportKind::ConnectRpc => "connectrpc",
        TransportKind::Pyro5 => "pyro5",
        TransportKind::Subprocess => "subprocess",
        TransportKind::Ffi => "ffi",
    }
}

fn main() {
    station_telemetry::init();
    if let Err(err) = run() {
        eprintln!("station kernel failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let started_at_ms = now_ms()?;

    // 1. Initialize components with correct constructors
    let mut registry = PluginRegistry::default();
    let mut schemas = SchemaRegistry::default();
    let mut ledger = ArtifactLedger::default();
    let mut bus = EventBus::new();

    // 2. Setup Supervisor & Run Identity
    let mut supervisor = StationSupervisor::new("default");
    let run_id = supervisor.session.run_id.clone();
    let profile_name = supervisor.session.profile_name.clone();

    let run_dir = PathBuf::from("runs").join(&run_id);
    let events_path = run_dir.join("events.jsonl");
    let manifest_path = run_dir.join("manifest.json");

    // 3. Register Plugin & Supervisor Events
    let quantum_manifest = PluginManifest {
        id: "adapter_quantum".into(),
        kind: PluginKind::Compute,
        transport: TransportKind::Local,
        version: "0.1.0".into(),
        artifact_hash: "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        subscribes: vec!["quantum.analyze".into()],
        publishes: vec!["quantum.state".into()],
        capabilities: vec!["collapse_ratio".into(), "euler_characteristic".into()],
    };

    registry
        .register(quantum_manifest.clone())
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

    let supervisor_event = supervisor.register_plugin(&quantum_manifest)?;
    let admitted_event = supervisor.transition("adapter_quantum", PluginRuntimeState::Admitted)?;

    // 4. Open Replay Log & Write Supervisor Events
    let mut replay = JsonlReplayLog::open(&events_path)?;
    replay.append_record(&supervisor_event)?;
    replay.append_record(&admitted_event)?;

    // 5. Register Schema
    let quantum_state_schema = PayloadSchema::new(
        "tnsr.quantum.state.v1",
        "quantum.state",
        "1",
        vec![
            "state_dim".into(),
            "collapse_ratio".into(),
            "euler_characteristic".into(),
        ],
    );
    schemas.register(quantum_state_schema)?;

    // 6. Run Event
    let rx = bus.subscribe();
    let mut event = quantum_state_event(run_id.clone());

    let policy = PolicyEngine {
        plugins: &registry,
        schemas: &schemas,
        supervisor: &supervisor,
    };
    // Policy checks: existence, authorization, state, schema hash match, validation
    policy.admit_event(&mut event)?.assert_allowed();

    let payload_bytes = serde_json::to_vec(&event.payload)?;
    let artifact = ledger.record_bytes(ArtifactRecordRequest {
        artifact_id: "quantum_state_payload".into(),
        source: "adapter_quantum".into(),
        data: payload_bytes,
        content_type: Some("application/json".into()),
        trace_id: Some(event.trace_id.clone()),
        parent_hash: event.input_hash.clone(),
        schema_hash: event.schema_hash.clone(),
    });
    event.artifact_hash = Some(artifact.hash);

    let report = bus.publish(event);
    assert_eq!(report.failed, 0);

    let received = rx.recv()?;
    replay.append_record(&received)?;

    // 7. Bridges
    let browser_frame = to_browser_frame(&received)?;
    println!("browser frame: {browser_frame}");
    println!("gpui line: {}", to_gpui_overlay_line(&received));
    println!("ledger entries: {}", ledger.records().len());

    // 8. Verification & Manifest Generation
    let (records_verified, last_hash, verification_error) =
        match JsonlReplayLog::verify_chain(&events_path) {
            Ok(report) => (report.records_verified, report.last_record_hash, None),
            Err(err) => (0, None, Some(err.to_string())),
        };

    // 8a. Sort Summaries for Determinism
    let mut plugins: Vec<PluginSummary> = registry
        .plugins()
        .into_iter()
        .map(|m| PluginSummary {
            id: m.id.clone(),
            kind: plugin_kind_wire(&m.kind).into(),
            transport: transport_kind_wire(&m.transport).into(),
            version: m.version.clone(),
            artifact_hash: m.artifact_hash.clone(),
            publishes: m.publishes.clone(),
            subscribes: m.subscribes.clone(),
            capabilities: m.capabilities.clone(),
        })
        .collect();
    plugins.sort_by(|a, b| a.id.cmp(&b.id));

    let mut schema_summaries: Vec<SchemaSummary> = schemas
        .schemas()
        .into_iter()
        .map(|s| SchemaSummary {
            id: s.id.clone(),
            topic: s.topic.clone(),
            version: s.version.clone(),
            schema_hash: s.schema_hash.clone(),
            required_fields: s.required_fields.clone(),
        })
        .collect();
    schema_summaries.sort_by(|a, b| a.topic.cmp(&b.topic));

    let mut artifacts: Vec<ArtifactSummary> = ledger
        .records()
        .iter()
        .map(|r| ArtifactSummary {
            artifact_id: r.artifact_id.clone(),
            source: r.source.clone(),
            hash: r.hash.clone(),
            algorithm: r.algorithm.clone(),
            byte_len: r.byte_len,
            content_type: r.content_type.clone(),
            trace_id: r.trace_id.clone(),
            parent_hash: r.parent_hash.clone(),
            schema_hash: r.schema_hash.clone(),
        })
        .collect();
    artifacts.sort_by(|a, b| a.artifact_id.cmp(&b.artifact_id));

    let replay_valid = verification_error.is_none();

    let manifest = RunManifest {
        run_id,
        profile_name,
        status: if replay_valid {
            RunStatus::Completed
        } else {
            RunStatus::Failed
        },
        event_log_path: "events.jsonl".into(),
        manifest_path: "manifest.json".into(),
        started_at_ms,
        completed_at_ms: Some(now_ms()?),
        records_verified,
        last_record_hash: last_hash,
        replay_valid,
        verification_error,
        plugin_count: plugins.len(),
        schema_count: schema_summaries.len(),
        artifact_count: artifacts.len(),
        plugins,
        schemas: schema_summaries,
        artifacts,
    };

    write_manifest_json(&manifest, &manifest_path)?;
    println!("Run sealed: {}", manifest_path.display());

    Ok(())
}
