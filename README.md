# tnsr-research-station

Rust-first research-station kernel where Rust is the canonical control-plane spine for event contracts, plugin admission, schema authority, artifact provenance, lifecycle supervision, replay verification, and projection bridges.

The goal is not to build a monolithic research app. The goal is to build a small, typed, replayable runtime backbone that can safely coordinate future quantum, PPF, ONNX, RAG, RL, swarm, browser, GPUI, JAX, Julia, WASM, subprocess, and agent sidecars through explicit plugin and event contracts.

## Current kernel model

The current proof path is:

```text
PluginManifest
  -> PluginRegistry registration
  -> StationSupervisor runtime registration/admission
  -> PayloadSchema registration
  -> adapter emits EventEnvelope
  -> PolicyEngine admits event
  -> ArtifactLedger records payload provenance
  -> EventBus publishes event
  -> JsonlReplayLog seals event into hash chain
  -> browser / GPUI bridges render projections
  -> station_run writes run manifest

A successful kernel run creates a closed run folder:

runs/<run_id>/
  events.jsonl
  manifest.json

events.jsonl is the append-only replay log.
manifest.json is the run closure artifact summarizing replay verification, plugins, schemas, and artifacts.

Implemented now
runtime-core

Canonical runtime substrate.

EventEnvelope contract version: tnsr.event.v1
SessionState with generated run/session identity
EventBus in-process publication
PublishReport
Event lineage helper: EventEnvelope::child_of
Payloads use serde_json::Value

runtime-core should stay minimal. It should not own plugin logic, schema logic, replay persistence, or policy admission.

plugin-registry

Typed plugin identity and authorization.

PluginManifest
PluginKind
TransportKind
SHA256 artifact hash validation
Duplicate publish/subscribe topic rejection
Per-plugin publish/subscribe authorization
Registry introspection for manifest generation

Plugin manifests are runtime claims. They are not arbitrary strings.

station-supervisor

Runtime lifecycle state for registered plugins.

Current states:

Registered
Admitted
Starting
Running
Stopping
Stopped
Failed
Quarantined

The supervisor emits lifecycle events and tracks whether a plugin is in a publishable runtime state.

station-schema

Topic-level payload contracts.

PayloadSchema
SchemaRegistry
Schema hash generation
Schema hash attachment
Required-field validation
Schema introspection for manifest generation

The schema layer is intentionally small. Full JSON Schema support should wait until the native contract layer becomes limiting.

station-policy

Central event admission gate.

Current admission checks:

Source plugin exists
Source plugin may publish the event topic
Source plugin is in an admitted/running state
Topic schema exists
Schema hash is attached if missing
Payload validates against the registered topic schema

Expected runtime denials should eventually become replayable policy events. The current kernel proof path still asserts successful admission for the happy-path demo.

artifact-ledger

Content-addressed artifact provenance.

SHA256 content hashes
ArtifactRecord
ArtifactRecordRequest
Content type metadata
Trace linkage
Parent hash linkage
Schema hash linkage
Artifact introspection for run manifests

The current ledger is in-memory during the demo path. Future work should persist artifact records and verify stored bytes.

station-replay

Tamper-evident event history.

JSONL replay log
Hash-chained ReplayRecord
Canonical JSON event hashing
Contiguous index verification
Previous-record hash verification
Event hash verification
Final record hash reporting

Replay is the canonical record of what happened.

station-run

Run closure artifacts.

RunManifest
RunStatus
PluginSummary
SchemaSummary
ArtifactSummary
JSON manifest write/load helpers
Snake-case status serialization
Manifest round-trip tests

A run manifest records:

run ID
profile name
status
relative event log path
relative manifest path
start/completion timestamps
replay verification result
final replay hash
plugin summaries
schema summaries
artifact summaries
bridge-browser

Browser projection bridge.

Serializes canonical EventEnvelope values into browser-safe JSON frames

Browser frames are projections only. They are not authoritative state.

bridge-gpui

Native shell projection bridge.

Formats canonical event envelopes into GPUI-style overlay lines

GPUI output is a projection only. It should not become the control-plane source of truth.

station-telemetry

Tracing/logging setup.

JSON tracing initialization
Idempotent try_init behavior
adapter-quantum

Starter compute adapter.

Emits a schema-validated quantum.state event
Produces payload fields used by the current kernel proof path:
state_dim
collapse_ratio
euler_characteristic
adapter-rag

Starter semantic adapter.

Reserved for RAG/query/result event expansion
Intended future topics include:
rag.query
rag.result
verifier critique events
memory lookup metadata
station-kernel

Executable proof-of-life for the Rust backbone.

Current kernel behavior:

Initializes telemetry.
Creates a supervisor session and run ID.
Creates runs/<run_id>/.
Registers the quantum plugin.
Emits supervisor lifecycle events.
Opens events.jsonl.
Registers the quantum.state schema.
Builds and admits a quantum event through policy.
Records payload provenance through ArtifactLedger.
Publishes the event on EventBus.
Appends the received event to replay.
Verifies the replay chain.
Writes manifest.json.
Prints browser and GPUI projections.
Run example
cargo run -p station_kernel

Expected output includes a run folder similar to:

runs/run-<ulid>/
  events.jsonl
  manifest.json

Inspect the generated manifest:

cat runs/<run_id>/manifest.json

The manifest should include:

{
  "status": "completed",
  "event_log_path": "events.jsonl",
  "manifest_path": "manifest.json",
  "replay_valid": true,
  "plugins": [
    {
      "id": "adapter_quantum",
      "kind": "compute",
      "transport": "local"
    }
  ],
  "schemas": [
    {
      "id": "tnsr.quantum.state.v1",
      "topic": "quantum.state"
    }
  ],
  "artifacts": [
    {
      "artifact_id": "quantum_state_payload",
      "source": "adapter_quantum",
      "algorithm": "sha256"
    }
  ]
}

Exact hashes, timestamps, and run IDs will differ between runs.

Test
cargo test --workspace
Core invariants
Event invariants

Every runtime event must be a runtime_core::EventEnvelope.

Required conceptual fields:

version
event_id
trace_id
parent_id
session_id
topic
source
created_at_ms
payload
input_hash
artifact_hash
schema_hash
plugin_hash

Derived events should use EventEnvelope::child_of so trace lineage is preserved.

Policy invariants

No plugin-originating event should be published or replayed without policy admission.

Policy currently checks:

plugin exists
plugin may publish topic
plugin is in allowed runtime state
schema exists for topic
schema_hash is attached
payload validates

Future policy denials should be emitted as replayable policy.event.denied events rather than causing kernel panics.

Replay invariants

Replay records are append-only and hash chained.

Each record contains:

index
event
event_hash
previous_record_hash
record_hash

Replay verification checks:

contiguous indices
event hash correctness
previous-record hash linkage
record hash correctness
Schema invariants

Every policy-admitted event topic should have a registered schema.

The current schema layer validates required fields and schema hashes. Field-type validation is the next natural schema hardening step.

Plugin invariants

Plugins are typed runtime entities.

A plugin manifest defines:

id
kind
transport
version
artifact_hash
publishes
subscribes
capabilities

Plugins may only publish topics declared in their manifests.

Manifest invariants

A completed run should produce:

runs/<run_id>/events.jsonl
runs/<run_id>/manifest.json

The manifest should summarize:

run identity
profile name
status
event log path
replay verification result
last replay hash
registered plugins
registered schemas
recorded artifacts

The manifest stores relative paths so the run folder can be moved as a sealed artifact.

Current limitations
EventBus is still broadcast-to-all, not topic-aware.
Policy denial is not yet replayed as evidence.
Schema validation is required-fields only, not typed fields.
Artifact ledger records are in-memory during the demo path.
Plugin manifests are constructed in code, not loaded from files.
No real transport runtime exists yet.
Browser and GPUI bridges are projection-only.
station-kernel is still a proof-of-life executable, not a full runtime scheduler.
Recommended next PRs
PR 9 — Make policy events replayable

Goal:

policy.event.admitted
policy.event.denied

Acceptance criteria:

Valid admission emits policy.event.admitted.
Denial emits policy.event.denied.
Denials are appended to replay.
Kernel does not panic on expected policy denial.
Denial includes source, topic, reason, and trace ID.
PR 10 — Topic-aware EventBus

Goal:

bus.subscribe("quantum.state");
bus.subscribe_prefix("supervisor.");
bus.publish(event);

Acceptance criteria:

Exact topic subscribers receive matching events.
Prefix subscribers receive matching topic families.
Unmatched subscribers receive nothing.
Dead subscribers are pruned.
PublishReport includes attempted, delivered, failed, and skipped counts.
PR 11 — Schema field types

Goal:

PayloadSchema::new(...)
    .required("state_dim", FieldType::Integer)
    .required("collapse_ratio", FieldType::Number)
    .required("euler_characteristic", FieldType::Integer);

Acceptance criteria:

Missing field fails.
Wrong type fails.
Valid payload passes.
Schema hash changes when field types change.
PR 12 — Manifest-backed run loading

Goal:

Load run profile files.
Load plugin manifest files.
Load schema files.
Create a closed run folder from declarative inputs.
PR 13 — Transport trait skeleton

Only after policy/replay/run closure stabilizes:

pub trait Transport {
    fn id(&self) -> &str;
    fn start(&mut self) -> Result<(), TransportError>;
    fn stop(&mut self) -> Result<(), TransportError>;
    fn send(&mut self, event: &EventEnvelope) -> Result<(), TransportError>;
}

Start with:

LocalTransport
NullTransport

Then add:

SubprocessTransport
WebSocketTransport
GrpcTransport
ConnectRpcTransport
Pyro5Transport
FfiTransport
Developer rules
Do not add transport before policy/replay/run closure.
Do not publish plugin-originating events without PolicyEngine admission.
Do not write raw JSON strings where serde_json::Value is expected.
Do not manually format browser JSON.
Do not use non-cryptographic hashes for provenance.
Do not let station-kernel absorb reusable logic.
Do not make browser or GPUI projections authoritative.
Do not panic on expected runtime denials; emit evidence events.
Every new runtime decision should be replayable.
Every new crate needs unit tests.
Target architecture
station-kernel
  ├── loads RunProfile
  ├── loads PluginManifest files
  ├── loads PayloadSchema definitions
  ├── initializes StationSupervisor
  ├── initializes PolicyEngine
  ├── initializes EventBus
  ├── initializes JsonlReplayLog
  ├── initializes ArtifactLedger
  ├── starts admitted transports
  ├── routes policy-admitted events
  ├── seals replay records
  ├── records artifact provenance
  ├── emits telemetry
  └── writes RunManifest

Future sidecars may attach through:

local
wasm
subprocess
websocket
grpc
connectrpc
pyro5
ffi

But sidecars must never bypass:

PluginRegistry
StationSupervisor
SchemaRegistry
PolicyEngine
JsonlReplayLog
ArtifactLedger
RunManifest
