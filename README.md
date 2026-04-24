# tnsr-research-station

Rust-first research-station kernel where Rust is the canonical manifold for control-plane truth, event routing, lifecycle supervision, schema authority hooks, and artifact provenance.

## Implemented now

- `runtime-core`
  - `EventEnvelope` contract version is `tnsr.event.v1`.
  - Payloads are `serde_json::Value`.
  - In-process `EventBus` supports delivery reporting and lineage helpers (`child_of`).
- `plugin-registry`
  - Per-plugin publish/subscribe authorization checks.
  - Typed plugin metadata for kind and transport, with manifest validation.
- `artifact-ledger`
  - Real SHA256 hashes (`sha256:<64-hex>`).
  - Provenance metadata recorded alongside each artifact.
- `bridge-browser` and `bridge-gpui`
  - Browser frames are serde-serialized event envelopes.
  - Native shell bridge line formatting.
- `station-telemetry`
  - JSON tracing initialization with idempotent `try_init` behavior.
- `adapter-quantum` + `adapter-rag`
  - Starter compute/semantic sidecar adapters emitting kernel events.

## Planned next

- Kernel lifecycle supervision beyond the in-process demo path.
- Schema authority hooks and stronger schema evolution policy.
- Extended replay tooling over artifact/event lineage.

## Not implemented yet

- Multi-process transport runtime (gRPC, websocket, subprocess orchestration).
- Production plugin packaging/signing pipeline.
- Centralized policy engine for plugin admission and runtime quotas.

## Run example

```bash
cargo run -p station_kernel
```
