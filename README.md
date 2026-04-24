# tnsr-research-station

Rust-first research-station kernel where Rust is the canonical manifold for control-plane truth, event routing, lifecycle supervision, schema authority hooks, and artifact provenance.

## Initial milestone implemented

- `runtime-core`: typed event envelope + in-process event bus + session state.
- `plugin-registry`: capability/topic registration with allowlist checks.
- `artifact-ledger`: SHA256 artifact records.
- `bridge-browser` and `bridge-gpui`: synchronization adapters for browser and native shells.
- `adapter-quantum` + `adapter-rag`: first compute/semantic sidecar adapters.

## Run example

```bash
cargo run -p station_kernel
```
