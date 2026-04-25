use std::fs;
use std::path::PathBuf;

use adapter_quantum::quantum_state_event;
use plugin_registry::load_plugin_manifest_json;
use runtime_core::EventEnvelope;
use serde_json::json;
use station_replay::JsonlReplayLog;
use station_run::{
    load_manifest_json, load_run_profile_json, write_run_profile_json, RunProfile, RunStatus,
    RuntimePermissions,
};
use station_supervisor::{PluginRuntimeState, StationSupervisor, SupervisorError};

use station_kernel::KernelRuntime;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace crates dir")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn default_profile_path() -> PathBuf {
    repo_root().join("profiles/default.profile.json")
}

fn temp_profile_path(test_name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!("tnsr-kernel-{test_name}-{}.json", nanos))
}

fn write_profile(
    profile_name: &str,
    plugin_manifests: Vec<PathBuf>,
    schema_files: Vec<PathBuf>,
    permissions: RuntimePermissions,
) -> PathBuf {
    let profile = RunProfile {
        name: profile_name.to_string(),
        description: Some("test profile".to_string()),
        plugin_manifests: plugin_manifests
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        schema_files: schema_files
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        permissions,
    };
    let path = temp_profile_path(profile_name);
    write_run_profile_json(&profile, &path).expect("write profile");
    path
}

fn complete_default_run() -> (PathBuf, PathBuf, PathBuf) {
    let mut runtime = KernelRuntime::from_profile_path(default_profile_path()).expect("runtime");
    runtime.register_plugins().expect("register plugins");
    runtime.register_schemas().expect("register schemas");

    let event = quantum_state_event(runtime.run_id());
    let admitted = runtime
        .admit(event)
        .expect("admission should return")
        .expect("event should be admitted");
    runtime.publish_admitted(admitted).expect("publish");

    let run_dir = runtime.run_dir().to_path_buf();
    runtime
        .seal_run(RunStatus::Completed)
        .expect("seal completed run");
    (
        run_dir.clone(),
        run_dir.join("events.jsonl"),
        run_dir.join("manifest.json"),
    )
}

#[test]
fn valid_default_profile_completes_and_writes_manifest() {
    let (_run_dir, _events_path, manifest_path) = complete_default_run();
    let manifest = load_manifest_json(&manifest_path).expect("load manifest");

    assert_eq!(manifest.status, RunStatus::Completed);
    assert!(manifest.replay_valid);
    assert!(manifest.plugin_count >= 2);
    assert!(manifest
        .plugins
        .iter()
        .any(|plugin| !plugin.capability_claims.is_empty()));
}

#[test]
fn unknown_plugin_event_seals_failed_manifest_with_policy_denial() {
    let mut runtime = KernelRuntime::from_profile_path(default_profile_path()).expect("runtime");
    runtime.register_plugins().expect("register plugins");
    runtime.register_schemas().expect("register schemas");

    let event = EventEnvelope::new(
        runtime.run_id(),
        "quantum.state",
        "unknown_plugin",
        json!({
            "state_dim": 16,
            "collapse_ratio": 0.42,
            "euler_characteristic": 8
        }),
    );
    let denied = runtime
        .admit(event)
        .expect("admission should return")
        .expect_err("unknown plugin should be denied");

    let run_dir = runtime.run_dir().to_path_buf();
    runtime
        .seal_run_with_reason(RunStatus::Failed, denied.reason.clone())
        .expect("seal denied run");
    let manifest = load_manifest_json(run_dir.join("manifest.json")).expect("load manifest");

    assert_eq!(manifest.status, RunStatus::Failed);
    assert!(manifest
        .failure_reason
        .unwrap_or_default()
        .contains("not registered"));
}

#[test]
fn unauthorized_topic_event_seals_failed_manifest_with_policy_denial() {
    let mut runtime = KernelRuntime::from_profile_path(default_profile_path()).expect("runtime");
    runtime.register_plugins().expect("register plugins");
    runtime.register_schemas().expect("register schemas");

    let event = EventEnvelope::new(
        runtime.run_id(),
        "rag.result",
        "adapter_quantum",
        json!({
            "message": "not allowed topic for quantum plugin"
        }),
    );
    let denied = runtime
        .admit(event)
        .expect("admission should return")
        .expect_err("topic should be denied");

    let run_dir = runtime.run_dir().to_path_buf();
    runtime
        .seal_run_with_reason(RunStatus::Failed, denied.reason.clone())
        .expect("seal denied run");
    let manifest = load_manifest_json(run_dir.join("manifest.json")).expect("load manifest");

    assert_eq!(manifest.status, RunStatus::Failed);
    assert!(manifest
        .failure_reason
        .unwrap_or_default()
        .contains("cannot publish topic"));
}

#[test]
fn wrong_schema_type_seals_failed_manifest_with_policy_denial() {
    let mut runtime = KernelRuntime::from_profile_path(default_profile_path()).expect("runtime");
    runtime.register_plugins().expect("register plugins");
    runtime.register_schemas().expect("register schemas");

    let event = EventEnvelope::new(
        runtime.run_id(),
        "quantum.state",
        "adapter_quantum",
        json!({
            "state_dim": "not an integer",
            "collapse_ratio": 0.42,
            "euler_characteristic": 8
        }),
    );
    let denied = runtime
        .admit(event)
        .expect("admission should return")
        .expect_err("schema type mismatch should be denied");

    let run_dir = runtime.run_dir().to_path_buf();
    runtime
        .seal_run_with_reason(RunStatus::Failed, denied.reason.clone())
        .expect("seal denied run");
    let manifest = load_manifest_json(run_dir.join("manifest.json")).expect("load manifest");

    assert_eq!(manifest.status, RunStatus::Failed);
    assert!(manifest
        .failure_reason
        .unwrap_or_default()
        .contains("schema validation failed"));
}

#[test]
fn manifest_counts_match_registered_runtime_state() {
    let (_run_dir, _events_path, manifest_path) = complete_default_run();
    let manifest = load_manifest_json(&manifest_path).expect("load manifest");

    assert_eq!(manifest.plugin_count, manifest.plugins.len());
    assert_eq!(manifest.schema_count, manifest.schemas.len());
    assert_eq!(manifest.artifact_count, manifest.artifacts.len());
    assert!(manifest.plugins.iter().all(|p| !p.id.is_empty()));
}

#[test]
fn replay_chain_verifies_after_completed_run() {
    let (_run_dir, events_path, _manifest_path) = complete_default_run();
    let report = JsonlReplayLog::verify_chain(&events_path).expect("verify replay");
    assert!(report.records_verified > 0);
    assert!(report.last_record_hash.is_some());
}

#[test]
fn tampered_replay_chain_fails_verification() {
    let (_run_dir, events_path, _manifest_path) = complete_default_run();
    fs::write(&events_path, "{\"tampered\":true}\n").expect("tamper replay file");
    let verification = JsonlReplayLog::verify_chain(&events_path);
    assert!(verification.is_err());
}

#[test]
fn plugin_requiring_network_is_rejected_when_profile_forbids_network() {
    let rag_manifest = load_plugin_manifest_json(repo_root().join("plugins/rag.plugin.json"))
        .expect("load rag manifest");
    let mut supervisor = StationSupervisor::new_with_permissions(
        "network-denied",
        RuntimePermissions {
            allow_network: false,
            allow_filesystem: false,
            allow_gpu: false,
            allow_projection_only: true,
            allow_nondeterministic: true,
        },
    );
    supervisor
        .register_plugin(&rag_manifest)
        .expect("register rag plugin");
    supervisor
        .transition(&rag_manifest.id, PluginRuntimeState::Admitted)
        .expect("admit plugin");

    let denied = supervisor.transition(&rag_manifest.id, PluginRuntimeState::Starting);
    assert!(matches!(denied, Err(SupervisorError::NetworkDenied { .. })));
}

#[test]
fn plugin_requiring_gpu_is_rejected_when_profile_forbids_gpu() {
    let quantum_manifest =
        load_plugin_manifest_json(repo_root().join("plugins/quantum-hybrid.plugin.json"))
            .expect("load quantum manifest");
    let mut supervisor = StationSupervisor::new_with_permissions(
        "gpu-denied",
        RuntimePermissions {
            allow_network: false,
            allow_filesystem: false,
            allow_gpu: false,
            allow_projection_only: true,
            allow_nondeterministic: false,
        },
    );
    supervisor
        .register_plugin(&quantum_manifest)
        .expect("register quantum plugin");
    supervisor
        .transition(&quantum_manifest.id, PluginRuntimeState::Admitted)
        .expect("admit plugin");

    let denied = supervisor.transition(&quantum_manifest.id, PluginRuntimeState::Starting);
    assert!(matches!(denied, Err(SupervisorError::GpuDenied { .. })));
}

#[test]
fn projection_only_plugins_require_projection_permission() {
    let profile_path = write_profile(
        "projection-denied",
        vec![repo_root().join("plugins/rag.plugin.json")],
        vec![repo_root().join("schemas/rag-result.schema.json")],
        RuntimePermissions {
            allow_network: true,
            allow_filesystem: false,
            allow_gpu: false,
            allow_projection_only: false,
            allow_nondeterministic: true,
        },
    );

    let profile = load_run_profile_json(&profile_path).expect("load profile");
    let rag_manifest = load_plugin_manifest_json(&profile.plugin_manifests[0]).expect("manifest");
    let mut supervisor =
        StationSupervisor::new_with_permissions(profile.name, profile.permissions.clone());
    supervisor
        .register_plugin(&rag_manifest)
        .expect("register rag plugin");
    supervisor
        .transition(&rag_manifest.id, PluginRuntimeState::Admitted)
        .expect("admit plugin");

    let denied = supervisor.transition(&rag_manifest.id, PluginRuntimeState::Starting);
    assert!(matches!(
        denied,
        Err(SupervisorError::ProjectionDenied { .. })
    ));

    let _ = fs::remove_file(profile_path);
}
