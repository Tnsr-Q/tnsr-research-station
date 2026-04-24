# tnsr-research-station

Rust-first research-station kernel where Rust is the canonical control-plane spine for event contracts, plugin admission, schema authority, artifact provenance, lifecycle supervision, replay verification, and projection bridges.

The goal is not to build a monolithic research app. The goal is to build a small, typed, replayable runtime backbone that can safely coordinate future quantum, PPF, ONNX, RAG, RL, swarm, browser, GPUI, JAX, Julia, WASM, subprocess, and agent sidecars through explicit plugin and event contracts.

## Project Structure

```
tnsr-research-station/
├── Cargo.toml                      # Workspace manifest
├── Cargo.lock                      # Dependency lock file
├── README.md                       # This file
├── .gitignore
├── crates/                         # All Rust crates
│   ├── runtime-core/               # Core event runtime (EventEnvelope, EventBus, SessionState)
│   ├── plugin-registry/            # Plugin identity and authorization
│   ├── station-supervisor/         # Plugin lifecycle state machine
│   ├── station-schema/             # Topic-level payload contracts
│   ├── station-policy/             # Event admission gate
│   ├── artifact-ledger/            # Content-addressed artifact provenance
│   ├── station-replay/             # Tamper-evident event history (JSONL + hash chain)
│   ├── station-run/                # Run closure artifacts and manifests
│   ├── station-telemetry/          # Tracing/logging setup
│   ├── station-transport/          # Transport trait and implementations
│   ├── bridge-browser/             # Browser projection bridge
│   ├── bridge-gpui/                # Native shell projection bridge
│   ├── adapter-quantum/            # Quantum compute adapter
│   ├── adapter-rag/                # RAG/semantic adapter
│   └── station-kernel/             # Executable proof-of-life runtime
├── plugins/                        # Plugin manifest files
│   ├── quantum-hybrid.plugin.json
│   └── rag.plugin.json
├── profiles/                       # Run profile configurations
│   └── default.profile.json
├── schemas/                        # Payload schema definitions
│   └── quantum-state.schema.json
├── proto/                          # Protocol buffer definitions
│   └── tnsr_event_v1.proto
└── runs/                           # Run output directories
    └── run-<ulid>/
        ├── events.jsonl            # Append-only event log
        └── manifest.json           # Run closure summary
```

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
```

A successful kernel run creates a closed run folder:

```text
runs/<run_id>/
  events.jsonl
  manifest.json
```

`events.jsonl` is the append-only replay log.
`manifest.json` is the run closure artifact summarizing replay verification, plugins, schemas, and artifacts.

## Features and Components

### Core Runtime

#### runtime-core
**Canonical runtime substrate** for the event-driven architecture.

**Features:**
- `EventEnvelope` contract version: `tnsr.event.v1`
- `SessionState` with generated run/session identity (ULID-based)
- `EventBus` for in-process event publication
- `PublishReport` for tracking event delivery
- Event lineage helper: `EventEnvelope::child_of` for trace preservation
- Payloads use `serde_json::Value` for schema flexibility

**Design principle:** runtime-core stays minimal and should not own plugin logic, schema logic, replay persistence, or policy admission.

---

### Plugin System

#### plugin-registry
**Typed plugin identity and authorization** layer.

**Features:**
- `PluginManifest` - declarative plugin contracts
- `PluginKind` - plugin type classification (compute, semantic, bridge, etc.)
- `TransportKind` - transport layer identification
- SHA256 artifact hash validation for plugin provenance
- Duplicate publish/subscribe topic rejection
- Per-plugin publish/subscribe authorization
- Registry introspection for manifest generation

**Design principle:** Plugin manifests are runtime claims, not arbitrary strings.

#### station-supervisor
**Runtime lifecycle state machine** for registered plugins.

**States:**
- `Registered` - Plugin manifest accepted
- `Admitted` - Plugin passed admission checks
- `Starting` - Plugin initialization in progress
- `Running` - Plugin actively processing
- `Stopping` - Plugin shutdown in progress
- `Stopped` - Plugin cleanly terminated
- `Failed` - Plugin encountered error
- `Quarantined` - Plugin isolated due to policy violation

**Features:**
- Emits lifecycle events for observability
- Tracks whether a plugin is in a publishable runtime state
- State transitions are enforced and replayable

---

### Schema and Policy

#### station-schema
**Topic-level payload contracts** and validation.

**Features:**
- `PayloadSchema` - declarative field requirements
- `SchemaRegistry` - centralized schema management
- Schema hash generation (SHA256)
- Automatic schema hash attachment to events
- Required-field validation
- Schema introspection for manifest generation

**Design principle:** The schema layer is intentionally small. Full JSON Schema support should wait until the native contract layer becomes limiting.

#### station-policy
**Central event admission gate** - the security boundary.

**Admission checks:**
1. Source plugin exists in registry
2. Source plugin may publish the event topic
3. Source plugin is in an admitted/running state
4. Topic schema exists in registry
5. Schema hash is attached (auto-attached if missing)
6. Payload validates against registered topic schema

**Future work:** Expected runtime denials should become replayable policy events rather than kernel panics.

---

### Provenance and Replay

#### artifact-ledger
**Content-addressed artifact provenance** tracking.

**Features:**
- SHA256 content hashes for all artifacts
- `ArtifactRecord` - immutable artifact metadata
- `ArtifactRecordRequest` - artifact registration API
- Content type metadata (MIME types)
- Trace linkage (parent/child relationships)
- Parent hash linkage for dependency tracking
- Schema hash linkage for contract verification
- Artifact introspection for run manifests

**Current limitation:** Ledger is in-memory. Future work will persist artifact records and verify stored bytes.

#### station-replay
**Tamper-evident event history** using hash chains.

**Features:**
- JSONL replay log (append-only)
- Hash-chained `ReplayRecord` structures
- Canonical JSON event hashing (deterministic)
- Contiguous index verification
- Previous-record hash verification
- Event hash verification
- Final record hash reporting

**Design principle:** Replay is the canonical record of what happened. All runtime decisions must be replayable.

---

### Transport Layer

#### station-transport
**Transport trait and implementations** for plugin communication.

**Transport Trait:**
```rust
pub trait Transport {
    fn id(&self) -> &str;
    fn start(&mut self) -> Result<(), TransportError>;
    fn stop(&mut self) -> Result<(), TransportError>;
    fn send(&mut self, event: &EventEnvelope) -> Result<(), TransportError>;
}
```

**Implemented transport interfaces / stubs:**

1. **LocalTransport** - In-memory transport for testing and local execution
   - Collects events in a vector
   - No actual transmission
   - Useful for unit tests and demos

2. **NullTransport** - No-op transport that discards all events
   - Useful for benchmarking
   - Testing scenarios where delivery is not needed

3. **SubprocessTransport** - Stateful skeleton for subprocess transport
   - Command/config state is captured
   - Real process spawn/send wiring is not integrated yet

4. **WebSocketTransport** - Stateful skeleton for WebSocket transport
   - Endpoint/config state is captured
   - Real socket connect/send wiring is not integrated yet

5. **GrpcTransport** - Stateful skeleton for gRPC transport
   - Service/config state is captured
   - Real RPC connect/send wiring is not integrated yet

6. **ConnectRpcTransport** - Stateful skeleton for Connect RPC transport
   - Service/config state is captured
   - Real connect/send wiring is not integrated yet

7. **Pyro5Transport** - Stateful skeleton for Pyro5 transport
   - Target/config state is captured
   - Real remote object connection/send wiring is not integrated yet

8. **FfiTransport** - Stateful skeleton for FFI transport
   - Library/config state is captured
   - Real FFI invocation wiring is not integrated yet

**Status:** Skeleton implementations are present. Transport startup/routing is not yet integrated into `station-kernel`.

---

### Run Management

#### station-run
**Run closure artifacts** and manifest generation.

**Features:**
- `RunManifest` - comprehensive run summary
- `RunStatus` - run lifecycle state (active, completed, failed)
- `PluginSummary` - plugin participation records
- `SchemaSummary` - schema usage records
- `ArtifactSummary` - artifact generation records
- JSON manifest write/load helpers
- Snake-case status serialization
- Manifest round-trip tests

**A run manifest records:**
- Run ID (ULID)
- Profile name
- Status
- Relative event log path (`events.jsonl`)
- Relative manifest path (`manifest.json`)
- Start/completion timestamps
- Replay verification result
- Final replay hash
- Plugin summaries
- Schema summaries
- Artifact summaries

**Design principle:** Manifests store relative paths so run folders are portable sealed artifacts.

---

### Projection Bridges

#### bridge-browser
**Browser projection bridge** for web visualization.

**Features:**
- Serializes canonical `EventEnvelope` values into browser-safe JSON frames
- Non-authoritative projections (read-only views)
- Suitable for web dashboards and monitoring

**Design principle:** Browser frames are projections only. They are not authoritative state.

#### bridge-gpui
**Native shell projection bridge** for terminal output.

**Features:**
- Formats canonical event envelopes into GPUI-style overlay lines
- Terminal-friendly output
- Real-time event stream visualization

**Design principle:** GPUI output is a projection only. It should not become the control-plane source of truth.

---

### Adapters

#### adapter-quantum
**Quantum compute adapter** (proof-of-concept).

**Features:**
- Emits schema-validated `quantum.state` events
- Produces payload fields:
  - `state_dim` - state space dimensionality
  - `collapse_ratio` - quantum collapse metric
  - `euler_characteristic` - topological invariant

**Purpose:** Demonstrates compute adapter pattern for the kernel proof path.

#### adapter-rag
**RAG/semantic adapter** (reserved for future expansion).

**Intended topics:**
- `rag.query` - semantic search queries
- `rag.result` - search results with embeddings
- Verifier critique events
- Memory lookup metadata

---

### Observability

#### station-telemetry
**Tracing and logging setup** for observability.

**Features:**
- JSON tracing initialization
- Idempotent `try_init` behavior
- Structured logging compatible with log aggregation systems

---

### Runtime Kernel

#### station-kernel
**Executable proof-of-life** for the Rust backbone.

Current kernel behavior:
- Initializes telemetry.
- Creates a supervisor session and run ID.
- Creates `runs/<run_id>/`.
- Registers the quantum plugin.
- Emits supervisor lifecycle events.
- Opens `events.jsonl`.
- Registers the `quantum.state` schema.
- Builds and admits a quantum event through policy.
- Records payload provenance through ArtifactLedger.
- Publishes the event on EventBus.
- Appends the received event to replay.
- Verifies the replay chain.
- Writes `manifest.json`.
- Prints browser and GPUI projections.

Run example:

```bash
cargo run -p station_kernel
```

Expected output includes a run folder similar to:

```text
runs/run-<ulid>/
  events.jsonl
  manifest.json
```

Inspect the generated manifest:

```bash
cat runs/<run_id>/manifest.json
```

The manifest should include:

```json
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
```

Exact hashes, timestamps, and run IDs will differ between runs.

Test:

```bash
cargo test --workspace
```
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

## Current Status

### Completed Features ✓

All initially planned PRs have been implemented:

- ✓ **PR 13** — Transport trait skeleton with `LocalTransport` and `NullTransport`
- ✓ **PR 14** — Six additional transport skeletons:
  - SubprocessTransport
  - WebSocketTransport
  - GrpcTransport
  - ConnectRpcTransport
  - Pyro5Transport
  - FfiTransport

### Current Limitations

- EventBus is still broadcast-to-all, not topic-aware
- Policy denial is not yet replayed as evidence
- Schema validation is required-fields only, not typed fields
- `schemas/quantum-state.schema.json` currently reflects forward-looking typed fields; runtime validation is still Rust-side required-field presence only
- Artifact ledger records are in-memory during the demo path
- Plugin manifests are constructed in code, not loaded from files
- No real transport runtime exists yet (skeletons only), and transport startup/routing is not yet integrated into `station-kernel`
- Browser and GPUI bridges are projection-only
- station-kernel is still a proof-of-life executable, not a full runtime scheduler

### Next Development Priorities

**PR 9 — Make policy events replayable**

Goal:
- `policy.event.admitted`
- `policy.event.denied`

Acceptance criteria:
- Valid admission emits `policy.event.admitted`
- Denial emits `policy.event.denied`
- Denials are appended to replay
- Kernel does not panic on expected policy denial
- Denial includes source, topic, reason, and trace ID

**PR 10 — Topic-aware EventBus**

Goal:
```rust
bus.subscribe("quantum.state");
bus.subscribe_prefix("supervisor.");
bus.publish(event);
```

Acceptance criteria:
- Exact topic subscribers receive matching events
- Prefix subscribers receive matching topic families
- Unmatched subscribers receive nothing
- Dead subscribers are pruned
- `PublishReport` includes attempted, delivered, failed, and skipped counts

**PR 11 — Schema field types**

Goal:
```rust
PayloadSchema::new(...)
    .required("state_dim", FieldType::Integer)
    .required("collapse_ratio", FieldType::Number)
    .required("euler_characteristic", FieldType::Integer);
```

Acceptance criteria:
- Missing field fails
- Wrong type fails
- Valid payload passes
- Schema hash changes when field types change

**PR 12 — Manifest-backed run loading**

Goal:
- Load run profile files
- Load plugin manifest files
- Load schema files
- Create a closed run folder from declarative inputs

## Developer Rules

1. Do not add transport before policy/replay/run closure
2. Do not publish plugin-originating events without PolicyEngine admission
3. Do not write raw JSON strings where `serde_json::Value` is expected
4. Do not manually format browser JSON
5. Do not use non-cryptographic hashes for provenance
6. Do not let station-kernel absorb reusable logic
7. Do not make browser or GPUI projections authoritative
8. Do not panic on expected runtime denials; emit evidence events
9. Every new runtime decision should be replayable
10. Every new crate needs unit tests

## Target Architecture

```
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
```

### Future Sidecar Integration

Future sidecars may attach through:
- `local` - in-process
- `wasm` - WebAssembly modules
- `subprocess` - external processes
- `websocket` - WebSocket connections
- `grpc` - gRPC services
- `connectrpc` - Connect RPC services
- `pyro5` - Python remote objects
- `ffi` - Foreign function interface

**Security boundary:** All sidecars must never bypass:
- PluginRegistry
- StationSupervisor
- SchemaRegistry
- PolicyEngine
- JsonlReplayLog
- ArtifactLedger
- RunManifest
