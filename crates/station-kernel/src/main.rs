use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use adapter_quantum::quantum_state_event;
use artifact_ledger::{ArtifactLedger, ArtifactRecordRequest};
use bridge_browser::to_browser_frame;
use bridge_gpui::to_gpui_overlay_line;
use plugin_registry::{load_plugin_manifest_json, PluginKind, PluginRegistry, TransportKind};
use runtime_core::EventBus;
use station_policy::PolicyEngine;
use station_replay::JsonlReplayLog;
use station_run::{
    load_run_profile_json, write_manifest_json, ArtifactSummary, PluginSummary, RunManifest,
    RunStatus, SchemaSummary,
};
use station_schema::{load_schema_json, FieldType, SchemaRegistry};
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
        PluginKind::Semantic => "semantic",
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

fn build_plugin_summaries(registry: &PluginRegistry) -> Vec<PluginSummary> {
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
    plugins
}

fn build_schema_summaries(schemas: &SchemaRegistry) -> Vec<SchemaSummary> {
    let mut schema_summaries: Vec<SchemaSummary> = schemas
        .schemas()
        .into_iter()
        .map(|s| SchemaSummary {
            id: s.id.clone(),
            topic: s.topic.clone(),
            version: s.version.clone(),
            schema_hash: s.schema_hash.clone(),
            required_fields: s
                .required_fields
                .iter()
                .map(|(name, field_type)| {
                    let type_str = match field_type {
                        FieldType::Integer => "Integer",
                        FieldType::Number => "Number",
                        FieldType::String => "String",
                        FieldType::Boolean => "Boolean",
                        FieldType::Object => "Object",
                        FieldType::Array => "Array",
                    };
                    (name.clone(), type_str.to_string())
                })
                .collect(),
        })
        .collect();
    schema_summaries.sort_by(|a, b| a.topic.cmp(&b.topic));
    schema_summaries
}

fn build_artifact_summaries(ledger: &ArtifactLedger) -> Vec<ArtifactSummary> {
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
    artifacts
}

fn build_run_manifest(
    run_id: String,
    profile_name: String,
    status: RunStatus,
    started_at_ms: u128,
    events_path: &std::path::Path,
    registry: &PluginRegistry,
    schemas: &SchemaRegistry,
    ledger: &ArtifactLedger,
    failure_reason: Option<String>,
) -> Result<RunManifest, Box<dyn std::error::Error>> {
    let (records_verified, last_hash, verification_error) =
        match JsonlReplayLog::verify_chain(events_path) {
            Ok(report) => (report.records_verified, report.last_record_hash, None),
            Err(err) => (0, None, Some(err.to_string())),
        };

    let plugins = build_plugin_summaries(registry);
    let schema_summaries = build_schema_summaries(schemas);
    let artifacts = build_artifact_summaries(ledger);

    let replay_valid = verification_error.is_none();

    Ok(RunManifest {
        run_id,
        profile_name,
        status,
        event_log_path: "events.jsonl".into(),
        manifest_path: "manifest.json".into(),
        started_at_ms,
        completed_at_ms: Some(now_ms()?),
        records_verified,
        last_record_hash: last_hash,
        replay_valid,
        verification_error,
        failure_reason,
        plugin_count: plugins.len(),
        schema_count: schema_summaries.len(),
        artifact_count: artifacts.len(),
        plugins,
        schemas: schema_summaries,
        artifacts,
    })
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

    // 1. Load run profile
    let profile_path = std::env::var("TNSR_PROFILE")
        .unwrap_or_else(|_| "profiles/default.profile.json".to_string());
    let profile = load_run_profile_json(&profile_path)?;
    println!("Loaded profile: {}", profile.name);

    // 2. Initialize components
    let mut registry = PluginRegistry::default();
    let mut schemas = SchemaRegistry::default();
    let mut ledger = ArtifactLedger::default();
    let mut bus = EventBus::new();

    // 3. Setup Supervisor & Run Identity
    let mut supervisor = StationSupervisor::new(&profile.name);
    let run_id = supervisor.session.run_id.clone();
    let profile_name = supervisor.session.profile_name.clone();

    let run_dir = PathBuf::from("runs").join(&run_id);
    let events_path = run_dir.join("events.jsonl");
    let manifest_path = run_dir.join("manifest.json");

    // 4. Load and register plugins from profile
    let mut plugin_manifests = Vec::new();
    for plugin_path in &profile.plugin_manifests {
        let manifest = load_plugin_manifest_json(plugin_path)?;
        println!("Loading plugin: {} from {}", manifest.id, plugin_path);
        plugin_manifests.push(manifest);
    }

    // 5. Open Replay Log
    let mut replay = JsonlReplayLog::open(&events_path)?;

    // 6. Register plugins and write supervisor events
    for manifest in &plugin_manifests {
        registry
            .register(manifest.clone())
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

        let supervisor_event = supervisor.register_plugin(manifest)?;
        replay.append_record(&supervisor_event)?;

        let admitted_event = supervisor.transition(&manifest.id, PluginRuntimeState::Admitted)?;
        replay.append_record(&admitted_event)?;
    }

    // 7. Load and register schemas from profile
    for schema_path in &profile.schema_files {
        let schema = load_schema_json(schema_path)?;
        println!("Loading schema: {} from {}", schema.id, schema_path);
        schemas.register(schema)?;
    }

    // 8. Run Event
    let rx = bus.subscribe("quantum.state");
    let mut event = quantum_state_event(run_id.clone());

    let policy = PolicyEngine {
        plugins: &registry,
        schemas: &schemas,
        supervisor: &supervisor,
    };
    // Policy checks: existence, authorization, state, schema hash match, validation
    let admission = policy.admit_event(&mut event)?;

    // Append policy event to replay
    if let Some(policy_event) = admission.policy_event {
        replay.append_record(&policy_event)?;
    }

    // Only proceed if allowed
    if !admission.allowed {
        eprintln!(
            "Policy denial: {}",
            admission.reason.as_deref().unwrap_or("unknown reason")
        );
        // Write manifest and exit without panic
        let manifest = build_run_manifest(
            run_id,
            profile_name,
            RunStatus::Failed,
            started_at_ms,
            &events_path,
            &registry,
            &schemas,
            &ledger,
            admission.reason,
        )?;

        write_manifest_json(&manifest, &manifest_path)?;
        println!("Run sealed with policy denial: {}", manifest_path.display());
        return Ok(());
    }

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

    // 9. Bridges
    let browser_frame = to_browser_frame(&received)?;
    println!("browser frame: {browser_frame}");
    println!("gpui line: {}", to_gpui_overlay_line(&received));
    println!("ledger entries: {}", ledger.records().len());

    // 10. Verification & Manifest Generation
    let status = if JsonlReplayLog::verify_chain(&events_path).is_ok() {
        RunStatus::Completed
    } else {
        RunStatus::Failed
    };

    let manifest = build_run_manifest(
        run_id,
        profile_name,
        status,
        started_at_ms,
        &events_path,
        &registry,
        &schemas,
        &ledger,
        None,
    )?;

    write_manifest_json(&manifest, &manifest_path)?;
    println!("Run sealed: {}", manifest_path.display());

    Ok(())
}
