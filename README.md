# tnsr-research-station

Rust-first research-station kernel for replayable scientific runtimes.

`tnsr-research-station` is a typed Rust control plane for coordinating research adapters, sidecars, projections, schemas, artifacts, replay logs, and run manifests. The kernel is intentionally small: it does not try to become a monolithic research application. It provides the canonical runtime boundary that future quantum, RAG, RL, JAX, WASM, subprocess, browser, GPUI, and agent systems must pass through.

The central invariant is simple:

> No plugin-originating event becomes authoritative until it is admitted by policy, recorded into replay, and included in run closure.

---

## Current status

The current main branch supports:

- declarative run profiles
- plugin manifests with structured capability claims
- profile-level runtime permissions
- topic schemas and schema-hash attachment
- policy-admitted event publication
- replayable policy denials
- replayable supervisor lifecycle transitions
- content-addressed artifact provenance
- hash-chained JSONL replay logs
- sealed run manifests
- projection-only browser and GPUI bridges
- owned runtime transports
- local and null transports by default
- feature-gated subprocess sidecars
- subprocess stdin/stdout JSONL protocol
- subprocess stderr/evidence capture
- subprocess timeout/kill evidence
- subprocess output candidate admission through `PolicyEngine`
- bounded subprocess `working_dir` resolution
- default-deny subprocess environment inheritance

Subprocess execution is available only when explicitly enabled with the `subprocess` feature. Other remote transports are represented in the type system but remain disabled until their admission, replay, and security contracts are implemented.

---

## Repository layout

```text
tnsr-research-station/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── crates/
│   ├── runtime-core/          # EventEnvelope, EventBus, session identity, event lineage
│   ├── plugin-registry/       # Plugin manifests, topic auth, capability claims, transport config
│   ├── station-supervisor/    # Plugin lifecycle state machine and permission-gated startup
│   ├── station-schema/        # Topic payload schemas and schema-hash validation
│   ├── station-policy/        # Event admission gate
│   ├── artifact-ledger/       # SHA256 artifact provenance records
│   ├── station-replay/        # Hash-chained JSONL replay log
│   ├── station-run/           # RunProfile and RunManifest models
│   ├── station-transport/     # Transport trait, local/null, feature-gated subprocess
│   ├── station-telemetry/     # Tracing setup
│   ├── bridge-browser/        # Browser-safe projection frames
│   ├── bridge-gpui/           # Native/terminal projection formatting
│   ├── adapter-quantum/       # Proof adapter emitting quantum.state
│   ├── adapter-rag/           # Reserved semantic/RAG adapter surface
│   └── station-kernel/        # Runtime API and executable proof path
├── plugins/
│   ├── quantum-hybrid.plugin.json
│   └── rag.plugin.json
├── profiles/
│   └── default.profile.json
├── schemas/
│   ├── quantum-state.schema.json
│   ├── rag-result.schema.json
│   └── plugin-capability.schema.json
├── proto/
│   └── tnsr_event_v1.proto
└── runs/
    └── <run_id>/
        ├── events.jsonl
        └── manifest.json
```

---

## Quick start

### Run the default kernel proof path

```bash
cargo run -p station_kernel
```

The kernel uses `profiles/default.profile.json` unless `TNSR_PROFILE` is set:

```bash
TNSR_PROFILE=profiles/default.profile.json cargo run -p station_kernel
```

A successful run creates:

```text
runs/<run_id>/
├── events.jsonl
└── manifest.json
```

### Run checks

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### Run subprocess-enabled tests

```bash
cargo test -p station_transport --features subprocess
cargo test -p station_kernel --features subprocess
```

The default build does not enable subprocess execution.

---

## Runtime model

The kernel proof path is:

```text
RunProfile
  -> PluginManifest files
  -> PayloadSchema files
  -> PluginRegistry
  -> StationSupervisor
  -> PolicyEngine
  -> EventBus
  -> JsonlReplayLog
  -> ArtifactLedger
  -> RunManifest
```

For plugin-originating events:

```text
EventEnvelope
  -> policy admission
  -> schema hash attachment
  -> payload validation
  -> artifact hashing
  -> event publication
  -> replay append
  -> run manifest closure
```

For subprocess sidecars:

```text
admitted input event
  -> send_to_plugin(plugin_id, AdmittedEvent)
  -> subprocess stdin JSONL
  -> subprocess stdout candidate EventEnvelope
  -> drain_plugin_outputs(plugin_id)
  -> PolicyEngine admission
  -> AdmittedEvent returned only if allowed
  -> publish_admitted(...)
  -> replay + artifact + manifest closure
```

Stderr, parse failures, timeouts, non-zero exits, forced kills, and runtime denials are emitted as replayable evidence events.

---

## Core crates

### `runtime-core`

Canonical runtime substrate.

Provides:

- `EventEnvelope`
- event IDs and trace IDs
- parent/child lineage through `EventEnvelope::child_of`
- `EventBus`
- `PublishReport`
- session identity
- payloads as `serde_json::Value`

`runtime-core` intentionally does not own plugin registration, policy, schema validation, replay persistence, or artifact provenance.

---

### `plugin-registry`

Plugin identity and authorization layer.

A `PluginManifest` defines:

- `id`
- `kind`
- `transport`
- `version`
- `artifact_hash`
- `subscribes`
- `publishes`
- `capabilities`
- `capability_claims`
- `transport_config`

The registry enforces:

- non-empty plugin IDs
- duplicate plugin rejection
- duplicate publish-topic rejection
- duplicate subscribe-topic rejection
- SHA256 artifact-hash shape
- non-empty capability claim names
- duplicate capability claim rejection
- per-plugin publish authorization
- per-plugin subscribe authorization

#### Capability claims

Capability claims give the supervisor and manifest concrete runtime metadata:

```json
{
  "name": "quantum.state.emit",
  "enabled": true,
  "required": true,
  "deterministic": true,
  "replay_safe": true,
  "projection_only": false,
  "emits_artifacts": true,
  "requires_network": false,
  "requires_filesystem": false,
  "requires_gpu": false,
  "max_runtime_ms": 1000
}
```

#### Transport config

Subprocess-capable plugin manifests may include:

```json
{
  "transport_config": {
    "command": "python3",
    "args": ["fixtures/sidecars/echo_jsonl.py"],
    "sidecar_executable_hash": "sha256:<command-bytes>",
    "sidecar_args_hash": "sha256:<canonical-args-payload>",
    "working_dir": ".",
    "env_allowlist": [],
    "inherit_env": false,
    "timeout_ms": 1000
  }
}
```

`inherit_env` defaults to `false`.

---

### `station-run`

Run profiles and run manifests.

`RunProfile` controls:

- profile name
- description
- plugin manifest paths
- schema file paths
- runtime permissions

The default profile is conservative:

```json
{
  "allow_network": false,
  "allow_filesystem": false,
  "allow_gpu": false,
  "allow_projection_only": true,
  "allow_nondeterministic": false
}
```

`RunManifest` records:

- run ID
- profile name
- status
- failure reason, if any
- event log path
- manifest path
- start/completion timestamps
- replay validity
- final replay hash
- plugin summaries
- capability claim summaries
- schema summaries
- artifact summaries

Manifest paths are relative so a run folder can be moved as a sealed artifact.

---

### `station-supervisor`

Plugin lifecycle state machine.

States:

```text
Registered
Admitted
Starting
Running
Stopping
Stopped
Failed
Quarantined
```

The supervisor records lifecycle transitions as replayable events and enforces profile permissions during startup/running transitions. Runtime denials are evidence, not panics.

Examples of denied startup conditions:

- plugin requires network but profile forbids network
- plugin requires filesystem but profile forbids filesystem
- plugin requires GPU but profile forbids GPU
- plugin is projection-only but profile forbids projection-only plugins
- plugin is nondeterministic but profile forbids nondeterminism

---

### `station-schema`

Topic-level payload validation.

Supported field types:

```text
Integer
Number
String
Boolean
Object
Array
```

The schema registry:

- registers topic schemas
- computes schema hashes
- attaches schema hashes to events
- validates required fields
- validates required field types
- provides schema summaries for run manifests

Every plugin-originating event admitted by policy should have a registered topic schema.

---

### `station-policy`

Central event admission gate.

For plugin-originating events, policy checks:

1. source plugin exists
2. plugin may publish the event topic
3. plugin runtime state is publishable
4. topic schema exists
5. schema hash is attached
6. payload validates against schema

All plugins must be `Running` before publishing.

Policy emits:

```text
policy.event.admitted
policy.event.denied
```

Denials preserve trace lineage and are appended to replay.

---

### `station-replay`

Tamper-evident JSONL replay log.

Each replay record includes:

- index
- event
- event hash
- previous record hash
- record hash

Replay verification checks:

- contiguous indices
- event hash correctness
- previous-record hash linkage
- record hash correctness

Replay is the canonical record of runtime decisions.

---

### `artifact-ledger`

Content-addressed artifact provenance.

Artifacts are recorded with:

- artifact ID
- source
- SHA256 hash
- algorithm
- byte length
- content type
- trace ID
- parent hash
- schema hash

Current implementation is an in-memory ledger used during run closure. Persistent artifact ledgers are a future hardening step.

---

### `station-transport`

Transport abstraction and implementations.

The transport trait is:

```rust
pub trait Transport {
    fn id(&self) -> &str;
    fn start(&mut self) -> Result<(), TransportError>;
    fn stop(&mut self) -> Result<(), TransportError>;
    fn send(&mut self, event: &EventEnvelope) -> Result<(), TransportError>;

    fn drain_candidate_events(&mut self) -> Vec<EventEnvelope> {
        Vec::new()
    }

    fn drain_evidence_events(&mut self) -> Vec<EventEnvelope> {
        Vec::new()
    }
}
```

Implemented/default transports:

- `LocalTransport` — in-memory, test/local execution
- `NullTransport` — no-op, discards sent events

Feature-gated transport:

- `SubprocessTransport` — external process sidecar, JSONL stdin/stdout protocol

Reserved/disabled transport kinds:

- `wasm`
- `websocket`
- `grpc`
- `connectrpc`
- `pyro5`
- `ffi`

#### Subprocess behavior

With `--features subprocess`, subprocess transport:

- spawns configured command
- writes admitted input events to stdin as JSONL
- reads stdout/stderr on background line readers
- parses stdout lines as candidate `EventEnvelope`s
- buffers candidate events until kernel drains and admits them
- converts stderr lines into replayable evidence
- emits parse-failure evidence for invalid stdout JSON
- records non-zero child exits
- closes stdin on stop
- waits cooperatively
- kills and reaps child on timeout
- records timeout and killed evidence
- defaults to an empty environment
- supports explicit `inherit_env`
- supports explicit `env_allowlist`
- supports bounded `working_dir`
- validates `sidecar_executable_hash` against resolved command bytes
- validates `sidecar_args_hash` against canonical args payload and local file hashes
- computes canonical args payload from manifest-provided args (normalized separators + optional file bytes hash) so hashes are checkout-path independent for relative args

---

### `station-kernel`

Public runtime API and executable proof path.

Public API:

```rust
pub use admission::AdmittedEvent;
pub use errors::KernelError;
pub use runtime::KernelRuntime;
```

Internal modules such as admission, context, closure, and runtime internals are private. External crates interact with `KernelRuntime`, not with raw context internals.

Key methods:

```rust
KernelRuntime::from_profile_path(...)
KernelRuntime::register_plugins()
KernelRuntime::register_schemas()
KernelRuntime::admit(event)
KernelRuntime::publish_admitted(admitted)
KernelRuntime::start_plugin(plugin_id)
KernelRuntime::send_to_plugin(plugin_id, &admitted)
KernelRuntime::drain_plugin_outputs(plugin_id)
KernelRuntime::stop_plugin(plugin_id)
KernelRuntime::seal_run(status)
KernelRuntime::seal_run_with_reason(status, reason)
```

The type boundary prevents raw plugin events from being published unless they have passed policy and become an `AdmittedEvent`.

---

### Projection bridges

#### `bridge-browser`

Serializes canonical events into browser-safe JSON frames.

Browser output is a projection only. It is not authoritative state.

#### `bridge-gpui`

Formats canonical events for native/terminal projection surfaces.

GPUI output is also projection-only.

---

### Adapters

#### `adapter-quantum`

Proof adapter that emits `quantum.state` events.

Current payload shape includes:

- `state_dim`
- `collapse_ratio`
- `euler_characteristic`

#### `adapter-rag`

Semantic/RAG adapter surface reserved for future runtime expansion.

Current manifest declares `rag.result` publication and `quantum.state` subscription, but default runtime permissions deny network access. Starting a network-requiring RAG path should produce replayable denial evidence unless the profile explicitly allows network.

---

## Event model

Every runtime event is an `EventEnvelope`.

Conceptual fields include:

- `version`
- `event_id`
- `trace_id`
- `parent_id`
- `session_id`
- `topic`
- `source`
- `created_at_ms`
- `payload`
- `input_hash`
- `artifact_hash`
- `schema_hash`
- `plugin_hash`
- `policy_event_id`

Derived events should use `EventEnvelope::child_of` to preserve trace lineage.

---

## Security and replay invariants

### Plugin events

Plugin-originating events must not bypass:

- `PluginRegistry`
- `StationSupervisor`
- `SchemaRegistry`
- `PolicyEngine`
- `JsonlReplayLog`
- `ArtifactLedger`
- `RunManifest`

### Admission tokens

Only the kernel can create or destructure admitted event tokens. External code receives read-only metadata and can pass the token back into kernel APIs.

### Runtime denials

Expected denials do not panic. They become replay evidence:

```text
policy.event.denied
policy.runtime.denied
transport.runtime.failed
transport.runtime.timeout
transport.runtime.killed
transport.runtime.stderr
transport.runtime.stdout_parse_failed
```

### Sidecar outputs

Subprocess stdout is not trusted. It is only a candidate event stream.

```text
stdout line
  -> parse as EventEnvelope
  -> candidate buffer
  -> kernel drain
  -> PolicyEngine
  -> AdmittedEvent or denial evidence
```

### Projections

Browser and GPUI projections are read-only views. They are not authoritative control-plane inputs.

---

## Default profile

The default profile loads:

```json
{
  "plugin_manifests": [
    "plugins/quantum-hybrid.plugin.json",
    "plugins/rag.plugin.json"
  ],
  "schema_files": [
    "schemas/quantum-state.schema.json",
    "schemas/rag-result.schema.json",
    "schemas/plugin-capability.schema.json"
  ]
}
```

Default permissions are conservative:

```json
{
  "allow_network": false,
  "allow_filesystem": false,
  "allow_gpu": false,
  "allow_projection_only": true,
  "allow_nondeterministic": false
}
```

---

## Working with subprocess sidecars

Subprocess sidecars are disabled in the default feature set.

Enable tests with:

```bash
cargo test -p station_transport --features subprocess
cargo test -p station_kernel --features subprocess
```

A subprocess plugin manifest must provide a command when the subprocess feature is enabled:

```json
{
  "id": "adapter_echo",
  "kind": "compute",
  "transport": "subprocess",
  "version": "0.1.0",
  "artifact_hash": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "subscribes": ["echo.input"],
  "publishes": ["echo.output"],
  "capabilities": ["echo.emit", "replay.safe"],
  "capability_claims": [
    {
      "name": "echo.emit",
      "enabled": true,
      "required": true,
      "deterministic": true,
      "replay_safe": true,
      "requires_network": false,
      "requires_filesystem": false,
      "requires_gpu": false
    }
  ],
  "transport_config": {
    "command": "python3",
    "args": ["fixtures/sidecars/echo_jsonl.py"],
    "sidecar_executable_hash": "sha256:<command-bytes>",
    "sidecar_args_hash": "sha256:<canonical-args-payload>",
    "working_dir": ".",
    "env_allowlist": [],
    "inherit_env": false,
    "timeout_ms": 1000
  }
}
```

Expected sidecar protocol:

- stdin: JSONL `EventEnvelope` values from kernel to sidecar
- stdout: JSONL candidate `EventEnvelope` values from sidecar to kernel
- stderr: diagnostic lines captured as replay evidence

The sidecar should not assume its stdout event will be accepted. The kernel admits or denies it.

---

## Development rules

1. Do not add transport before policy/replay/run closure.
2. Do not publish plugin-originating events without `PolicyEngine` admission.
3. Do not write raw JSON strings where `serde_json::Value` is expected.
4. Do not manually format browser JSON.
5. Do not use non-cryptographic hashes for provenance.
6. Do not let `station-kernel` absorb reusable logic.
7. Do not make browser or GPUI projections authoritative.
8. Do not panic on expected runtime denials; emit evidence events.
9. Every new runtime decision should be replayable.
10. Every new crate needs unit tests.
11. Every new transport path must have feature gating, tests, denial evidence, and replay closure.

---

## Test matrix

Recommended before merging runtime changes:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Recommended before merging subprocess changes:

```bash
cargo test -p station_transport --features subprocess
cargo test -p station_kernel --features subprocess
```

Recommended sanity run:

```bash
cargo run -p station_kernel
```

---

## Roadmap

### Recently completed

Deterministic subprocess fixture coverage is now present in-repo:

```text
fixtures/sidecars/echo_jsonl.py
plugins/echo-subprocess.plugin.json
schemas/echo-input.schema.json
schemas/echo-output.schema.json
profile/echo-subprocess.profile.json
crates/station-kernel/tests/echo_subprocess_integration.rs
```

### Next

- persistent artifact ledger on disk
- first real scientific subprocess adapter
- explicit executable allowlist beyond hash pinning
- stronger system-event schema policy
- real WebSocket transport behind feature flag
- real gRPC/ConnectRPC transport behind feature flag
- WASM sidecar sandbox
- projection dashboards backed only by replay/projection frames

---

## License

MIT
