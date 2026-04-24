use runtime_core::EventEnvelope;
use serde_json::json;

pub fn quantum_state_event() -> EventEnvelope {
    EventEnvelope::new(
        "quantum.state",
        "adapter_quantum",
        json!({
            "state_dim": 16,
            "collapse_ratio": 0.42,
            "euler_characteristic": 8,
        }),
    )
}
