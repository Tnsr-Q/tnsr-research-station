use adapter_quantum::quantum_state_event;
use artifact_ledger::{ArtifactLedger, ArtifactRecordRequest};
use bridge_browser::to_browser_frame;
use bridge_gpui::to_gpui_overlay_line;
use plugin_registry::{PluginKind, PluginManifest, PluginRegistry, TransportKind};
use runtime_core::EventBus;
use station_replay::JsonlReplayLog;
use station_supervisor::{PluginRuntimeState, StationSupervisor};

fn main() {
    station_telemetry::init();

    let mut registry = PluginRegistry::default();
    let quantum_manifest = PluginManifest {
        id: "adapter_quantum".into(),
        kind: PluginKind::Compute,
        transport: TransportKind::Local,
        version: "0.1.0".into(),
        artifact_hash: "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            .into(),
        subscribes: vec!["quantum.analyze".into()],
        publishes: vec!["quantum.state".into()],
        capabilities: vec!["collapse_ratio".into(), "euler_characteristic".into()],
    };

    registry
        .register(quantum_manifest.clone())
        .expect("plugin registration");

    let mut supervisor = StationSupervisor::new("default");
    let mut replay =
        JsonlReplayLog::open(format!("runs/{}/events.jsonl", supervisor.session.run_id))
            .expect("open replay log");

    let supervisor_event = supervisor
        .register_plugin(&quantum_manifest)
        .expect("supervisor register plugin");
    replay
        .append_record(&supervisor_event)
        .expect("append supervisor event");

    let admitted_event = supervisor
        .transition("adapter_quantum", PluginRuntimeState::Admitted)
        .expect("admit plugin");
    replay
        .append_record(&admitted_event)
        .expect("append admitted event");

    let mut bus = EventBus::new();
    let rx = bus.subscribe();

    let mut event = quantum_state_event(supervisor.session.run_id.clone());
    assert!(registry.can_publish(&event.source, &event.topic));

    let mut ledger = ArtifactLedger::default();
    let payload_bytes = serde_json::to_vec(&event.payload).expect("serialize payload");
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

    tracing::info!(
        topic = %event.topic,
        source = %event.source,
        trace_id = %event.trace_id,
        "publishing event"
    );

    let report = bus.publish(event);
    assert_eq!(report.failed, 0);
    let received = rx.recv().expect("receive");

    replay
        .append_record(&received)
        .expect("append received event");

    let browser_frame = to_browser_frame(&received).expect("serialize browser frame");
    println!("browser frame: {browser_frame}");
    println!("gpui line: {}", to_gpui_overlay_line(&received));
    println!("ledger entries: {}", ledger.records().len());
}
