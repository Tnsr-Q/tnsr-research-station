use runtime_core::EventEnvelope;
use serde_json::json;

pub fn rag_log_event(trace_id: String, message: &str) -> EventEnvelope {
    let mut event = EventEnvelope::new(
        "rag.result",
        "adapter_rag",
        json!({
            "message": message,
        }),
    );
    event.trace_id = trace_id;
    event
}
