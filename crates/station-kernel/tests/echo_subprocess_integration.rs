#![cfg(all(feature = "subprocess", unix))]

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use runtime_core::EventEnvelope;
use serde_json::json;
use station_kernel::KernelRuntime;
use station_replay::JsonlReplayLog;
use station_run::RunStatus;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace crates directory")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn deterministic_echo_subprocess_round_trip_is_replayable() {
    let profile_path = repo_root().join("profile/echo-subprocess.profile.json");
    let mut runtime = KernelRuntime::from_profile_path(&profile_path).expect("runtime should init");

    runtime
        .register_plugins()
        .expect("plugin manifest should register");
    runtime.register_schemas().expect("schemas should register");
    runtime
        .start_plugin("echo_subprocess")
        .expect("subprocess plugin should start");

    let input_event = EventEnvelope::new(
        runtime.run_id(),
        "echo.input",
        "echo_subprocess",
        json!({"message": "deterministic-sidecar"}),
    );
    let admitted_input = runtime
        .admit(input_event)
        .expect("policy decision should be recorded")
        .expect("input event should be policy admitted");

    runtime
        .send_to_plugin("echo_subprocess", &admitted_input)
        .expect("send to sidecar should succeed");

    let mut admitted_outputs = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && admitted_outputs.is_empty() {
        let drained = runtime
            .drain_plugin_outputs("echo_subprocess")
            .expect("drain should succeed");
        admitted_outputs.extend(drained);
        if admitted_outputs.is_empty() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    assert_eq!(admitted_outputs.len(), 1, "expected one echo output event");
    let echoed = admitted_outputs
        .pop()
        .expect("expected an admitted subprocess output");
    assert_eq!(echoed.topic(), "echo.output");
    assert_eq!(echoed.source(), "echo_subprocess");

    runtime
        .publish_admitted(echoed)
        .expect("admitted sidecar event should publish");
    runtime
        .stop_plugin("echo_subprocess")
        .expect("subprocess plugin should stop");

    let run_dir = runtime.run_dir().to_path_buf();
    let events_path = run_dir.join("events.jsonl");
    let manifest = runtime
        .seal_run(RunStatus::Completed)
        .expect("run should seal successfully");

    let verification = JsonlReplayLog::verify_chain(&events_path).expect("replay should verify");
    assert!(verification.records_verified > 0);
    assert!(verification.last_record_hash.is_some());

    assert_eq!(manifest.status, RunStatus::Completed);
    assert!(
        manifest.replay_valid,
        "sealed manifest should mark replay valid"
    );
    assert!(manifest.last_record_hash.is_some());

    let events = JsonlReplayLog::read_all_events(events_path).expect("events should be readable");
    assert!(events.iter().any(|event| {
        event.topic == "policy.event.admitted"
            && event.payload["topic"] == "echo.output"
            && event.payload["source"] == "echo_subprocess"
    }));
    assert!(events.iter().any(|event| {
        event.topic == "echo.output"
            && event.source == "echo_subprocess"
            && event.payload["message"] == "deterministic-sidecar"
    }));

    let _ = fs::remove_dir_all(run_dir);
}
