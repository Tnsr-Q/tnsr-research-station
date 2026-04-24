use adapter_quantum::quantum_state_event;
use artifact_ledger::ArtifactLedger;
use bridge_browser::to_browser_frame;
use bridge_gpui::to_gpui_overlay_line;
use plugin_registry::{PluginManifest, PluginRegistry};
use runtime_core::EventBus;

fn main() {
    let mut registry = PluginRegistry::default();
    registry
        .register(PluginManifest {
            id: "adapter_quantum".into(),
            kind: "compute".into(),
            transport: "local".into(),
            version: "0.1.0".into(),
            artifact_hash: "sha256:dev".into(),
            subscribes: vec!["quantum.analyze".into()],
            publishes: vec!["quantum.state".into()],
            capabilities: vec!["collapse_ratio".into(), "euler_characteristic".into()],
        })
        .expect("plugin registration");

    let mut bus = EventBus::new();
    let rx = bus.subscribe();

    let mut event = quantum_state_event();
    assert!(registry.topic_allowed(&event.topic));

    let mut ledger = ArtifactLedger::default();
    let artifact_hash = ledger.record(
        "quantum_state_payload",
        "adapter_quantum",
        event.payload.as_bytes(),
    );
    event.artifact_hash = Some(artifact_hash);

    bus.publish(event);
    let received = rx.recv().expect("receive");

    println!("browser frame: {}", to_browser_frame(&received));
    println!("gpui line: {}", to_gpui_overlay_line(&received));
    println!("ledger entries: {}", ledger.records().len());
}
