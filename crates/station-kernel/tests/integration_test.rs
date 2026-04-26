use std::fs;
use std::path::PathBuf;

use adapter_quantum::quantum_state_event;
use artifact_ledger::{ArtifactLedger, ArtifactRecordRequest};
use plugin_registry::{load_plugin_manifest_json, PluginRegistry};
use runtime_core::EventBus;
use station_policy::PolicyEngine;
use station_replay::JsonlReplayLog;
use station_run::{load_manifest_json, load_run_profile_json, RunStatus, RuntimePermissions};
use station_schema::{load_schema_json, SchemaRegistry};
use station_supervisor::{PluginRuntimeState, StationSupervisor};

/// Get the repository root directory
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Create a temporary test run directory
fn create_test_run_dir(test_name: &str) -> PathBuf {
    let test_dir = std::env::temp_dir().join(format!("tnsr-test-{}", test_name));
    if test_dir.exists() {
        fs::remove_dir_all(&test_dir).unwrap();
    }
    fs::create_dir_all(&test_dir).unwrap();
    test_dir
}

#[test]
fn end_to_end_kernel_run_produces_valid_manifest() {
    // Setup
    let root = repo_root();
    let test_dir = create_test_run_dir("e2e-kernel");

    // 1. Load profile
    let profile_path = root.join("profiles/default.profile.json");
    let profile = load_run_profile_json(&profile_path).expect("Failed to load profile");

    // 2. Initialize components
    let mut registry = PluginRegistry::default();
    let mut schemas = SchemaRegistry::default();
    let mut ledger = ArtifactLedger::default();
    let mut bus = EventBus::new();

    // 3. Setup Supervisor
    let mut supervisor = StationSupervisor::new_with_permissions(
        &profile.name,
        RuntimePermissions {
            allow_network: true,
            allow_filesystem: false,
            allow_gpu: true,
            allow_projection_only: true,
            allow_nondeterministic: true,
        },
    );
    let run_id = supervisor.session.run_id.clone();

    let events_path = test_dir.join("events.jsonl");
    let manifest_path = test_dir.join("manifest.json");

    // 4. Load and register plugins
    for plugin_path in &profile.plugin_manifests {
        let full_path = root.join(plugin_path);
        let manifest = load_plugin_manifest_json(&full_path).expect("Failed to load plugin");
        registry
            .register(manifest.clone())
            .expect("Failed to register plugin");
    }

    // 5. Open Replay Log
    let mut replay = JsonlReplayLog::open(&events_path).expect("Failed to open replay log");

    // 6. Register plugins with supervisor
    for plugin_path in &profile.plugin_manifests {
        let full_path = root.join(plugin_path);
        let manifest = load_plugin_manifest_json(&full_path).expect("Failed to load plugin");

        let supervisor_event = supervisor
            .register_plugin(&manifest)
            .expect("Failed to register plugin");
        replay
            .append_record(&supervisor_event)
            .expect("Failed to append supervisor event");

        let admitted_event = supervisor
            .transition(&manifest.id, PluginRuntimeState::Admitted)
            .expect("Failed to transition plugin");
        replay
            .append_record(&admitted_event)
            .expect("Failed to append admitted event");

        let starting_event = supervisor
            .transition(&manifest.id, PluginRuntimeState::Starting)
            .expect("Failed to transition plugin to starting");
        replay
            .append_record(&starting_event)
            .expect("Failed to append starting event");

        let running_event = supervisor
            .transition(&manifest.id, PluginRuntimeState::Running)
            .expect("Failed to transition plugin to running");
        replay
            .append_record(&running_event)
            .expect("Failed to append running event");
    }

    // 7. Load and register schemas
    for schema_path in &profile.schema_files {
        let full_path = root.join(schema_path);
        let schema = load_schema_json(&full_path).expect("Failed to load schema");
        schemas.register(schema).expect("Failed to register schema");
    }

    // 8. Create and admit an event
    let rx = bus.subscribe("quantum.state");
    let mut event = quantum_state_event(run_id.clone());

    let policy = PolicyEngine {
        plugins: &registry,
        schemas: &schemas,
        supervisor: &supervisor,
    };

    let admission = policy
        .admit_event(&mut event)
        .expect("Policy admission failed");

    // Append policy event
    if let Some(policy_event) = admission.policy_event {
        replay
            .append_record(&policy_event)
            .expect("Failed to append policy event");
    }

    assert!(admission.allowed, "Event should be admitted");

    // 9. Record artifact
    let payload_bytes = serde_json::to_vec(&event.payload).expect("Failed to serialize payload");
    let artifact = ledger.record_bytes(ArtifactRecordRequest {
        artifact_id: "test_payload".into(),
        source: "adapter_quantum".into(),
        data: payload_bytes,
        content_type: Some("application/json".into()),
        trace_id: Some(event.trace_id.clone()),
        parent_hash: event.input_hash.clone(),
        schema_hash: event.schema_hash.clone(),
    });
    event.artifact_hash = Some(artifact.hash);

    // 10. Publish and receive event
    let report = bus.publish(event);
    assert_eq!(report.failed, 0, "Event publication should not fail");

    let received = rx.recv().expect("Should receive event");
    replay
        .append_record(&received)
        .expect("Failed to append event to replay");

    // 11. Verify replay chain
    let verification = JsonlReplayLog::verify_chain(&events_path);
    assert!(verification.is_ok(), "Replay chain should be valid");

    let verify_report = verification.unwrap();
    assert!(
        verify_report.records_verified > 0,
        "Should have verified records"
    );
    assert!(
        verify_report.last_record_hash.is_some(),
        "Should have last record hash"
    );

    // 12. Write manifest (simulating the kernel's manifest generation)
    let manifest = station_run::RunManifest {
        run_id: run_id.clone(),
        profile_name: profile.name.clone(),
        status: RunStatus::Completed,
        event_log_path: "events.jsonl".into(),
        manifest_path: "manifest.json".into(),
        started_at_ms: 0,
        completed_at_ms: Some(1),
        records_verified: verify_report.records_verified,
        last_record_hash: verify_report.last_record_hash,
        replay_valid: true,
        verification_error: None,
        failure_reason: None,
        plugin_count: registry.plugins().len(),
        schema_count: schemas.schema_count(),
        artifact_count: ledger.records().len(),
        plugins: vec![],
        schemas: vec![],
        artifacts: vec![],
    };

    station_run::write_manifest_json(&manifest, &manifest_path).expect("Failed to write manifest");

    // 13. Verify manifest can be loaded
    assert!(manifest_path.exists(), "Manifest file should exist");
    let loaded_manifest = load_manifest_json(&manifest_path).expect("Failed to load manifest");

    assert_eq!(loaded_manifest.run_id, run_id);
    assert_eq!(loaded_manifest.status, RunStatus::Completed);
    assert!(loaded_manifest.replay_valid, "Replay should be valid");
    assert!(loaded_manifest.records_verified > 0);
    assert!(
        loaded_manifest.plugin_count >= 2,
        "Should have at least 2 plugins"
    );
    assert!(
        loaded_manifest.schema_count >= 2,
        "Should have at least 2 schemas"
    );

    // Cleanup
    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn policy_denial_creates_failed_manifest() {
    // Setup
    let root = repo_root();
    let test_dir = create_test_run_dir("policy-denial");

    // 1. Load profile
    let profile_path = root.join("profiles/default.profile.json");
    let profile = load_run_profile_json(&profile_path).expect("Failed to load profile");

    // 2. Initialize components
    let registry = PluginRegistry::default(); // Empty registry - no plugins registered
    let mut schemas = SchemaRegistry::default();
    let _ledger = ArtifactLedger::default();

    // 3. Setup Supervisor
    let supervisor = StationSupervisor::new(&profile.name);
    let run_id = supervisor.session.run_id.clone();

    let events_path = test_dir.join("events.jsonl");
    let manifest_path = test_dir.join("manifest.json");

    // 4. Open Replay Log
    let mut replay = JsonlReplayLog::open(&events_path).expect("Failed to open replay log");

    // 5. Load and register schemas (but not plugins)
    for schema_path in &profile.schema_files {
        let full_path = root.join(schema_path);
        let schema = load_schema_json(&full_path).expect("Failed to load schema");
        schemas.register(schema).expect("Failed to register schema");
    }

    // 6. Create an event from an unregistered plugin
    let mut event = quantum_state_event(run_id.clone());

    let policy = PolicyEngine {
        plugins: &registry, // Empty registry
        schemas: &schemas,
        supervisor: &supervisor,
    };

    // 7. Attempt to admit event - should be denied because plugin is not registered
    let admission = policy
        .admit_event(&mut event)
        .expect("Policy should return admission result");

    // Policy should deny the event
    assert!(
        !admission.allowed,
        "Policy should reject event from unregistered plugin"
    );
    assert!(admission.reason.is_some(), "Denial should have a reason");

    // Append policy event to replay
    if let Some(policy_event) = admission.policy_event {
        replay
            .append_record(&policy_event)
            .expect("Failed to append policy event");
    }

    // 8. Verify replay chain (should still be valid)
    let verification = JsonlReplayLog::verify_chain(&events_path);
    assert!(
        verification.is_ok(),
        "Replay chain should be valid even with denial"
    );

    // 9. Write manifest with failed status
    let verify_report = verification.unwrap();
    let manifest = station_run::RunManifest {
        run_id: run_id.clone(),
        profile_name: profile.name.clone(),
        status: RunStatus::Failed,
        event_log_path: "events.jsonl".into(),
        manifest_path: "manifest.json".into(),
        started_at_ms: 0,
        completed_at_ms: Some(1),
        records_verified: verify_report.records_verified,
        last_record_hash: verify_report.last_record_hash,
        replay_valid: true,
        verification_error: None,
        failure_reason: Some("Plugin not registered".into()),
        plugin_count: 0,
        schema_count: schemas.schema_count(),
        artifact_count: 0,
        plugins: vec![],
        schemas: vec![],
        artifacts: vec![],
    };

    station_run::write_manifest_json(&manifest, &manifest_path).expect("Failed to write manifest");

    // 10. Verify manifest
    assert!(manifest_path.exists(), "Manifest file should exist");
    let loaded_manifest = load_manifest_json(&manifest_path).expect("Failed to load manifest");

    assert_eq!(loaded_manifest.status, RunStatus::Failed);
    assert!(
        loaded_manifest.failure_reason.is_some(),
        "Should have failure reason"
    );
    assert_eq!(
        loaded_manifest.plugin_count, 0,
        "No plugins should be registered"
    );
    assert!(loaded_manifest.replay_valid, "Replay should still be valid");

    // Cleanup
    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn supervisor_lifecycle_events_are_replayable() {
    // Setup
    let root = repo_root();
    let test_dir = create_test_run_dir("supervisor-lifecycle");

    let profile_path = root.join("profiles/default.profile.json");
    let profile = load_run_profile_json(&profile_path).expect("Failed to load profile");

    let mut supervisor = StationSupervisor::new_with_permissions(
        &profile.name,
        RuntimePermissions {
            allow_network: true,
            allow_filesystem: false,
            allow_gpu: true,
            allow_projection_only: true,
            allow_nondeterministic: true,
        },
    );

    let events_path = test_dir.join("events.jsonl");
    let mut replay = JsonlReplayLog::open(&events_path).expect("Failed to open replay log");

    // Load first plugin
    let plugin_path = root.join("plugins/quantum-hybrid.plugin.json");
    let manifest = load_plugin_manifest_json(&plugin_path).expect("Failed to load plugin");

    // Register and transition through states
    let register_event = supervisor
        .register_plugin(&manifest)
        .expect("Failed to register");
    replay
        .append_record(&register_event)
        .expect("Failed to append");

    let admitted_event = supervisor
        .transition(&manifest.id, PluginRuntimeState::Admitted)
        .expect("Failed to transition to Admitted");
    replay
        .append_record(&admitted_event)
        .expect("Failed to append");

    let starting_event = supervisor
        .transition(&manifest.id, PluginRuntimeState::Starting)
        .expect("Failed to transition to Starting");
    replay
        .append_record(&starting_event)
        .expect("Failed to append");

    let running_event = supervisor
        .transition(&manifest.id, PluginRuntimeState::Running)
        .expect("Failed to transition to Running");
    replay
        .append_record(&running_event)
        .expect("Failed to append");

    // Verify all events are in replay log
    let verification = JsonlReplayLog::verify_chain(&events_path);
    assert!(verification.is_ok(), "Replay chain should be valid");

    let report = verification.unwrap();
    assert_eq!(report.records_verified, 4, "Should have 4 lifecycle events");

    // Cleanup
    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn artifact_ledger_records_are_tracked() {
    let mut ledger = ArtifactLedger::default();

    // Record multiple artifacts
    let artifact1 = ledger.record_bytes(ArtifactRecordRequest {
        artifact_id: "artifact_1".into(),
        source: "test_source".into(),
        data: b"test data 1".to_vec(),
        content_type: Some("text/plain".into()),
        trace_id: Some("trace-1".into()),
        parent_hash: None,
        schema_hash: None,
    });

    let _artifact2 = ledger.record_bytes(ArtifactRecordRequest {
        artifact_id: "artifact_2".into(),
        source: "test_source".into(),
        data: b"test data 2".to_vec(),
        content_type: Some("text/plain".into()),
        trace_id: Some("trace-1".into()),
        parent_hash: Some(artifact1.hash.clone()),
        schema_hash: None,
    });

    // Verify records
    assert_eq!(ledger.records().len(), 2, "Should have 2 artifact records");

    let records = ledger.records();
    assert_eq!(records[0].artifact_id, "artifact_1");
    assert_eq!(records[1].artifact_id, "artifact_2");
    assert_eq!(records[1].parent_hash.as_ref(), Some(&artifact1.hash));
}
