use runtime_core::EventEnvelope;
use serde_json::json;

pub fn rag_log_event(
    session_id: impl Into<String>,
    trace_id: String,
    message: &str,
) -> EventEnvelope {
    let mut event = EventEnvelope::new(
        session_id,
        "rag.result",
        "adapter_rag",
        json!({
            "message": message,
        }),
    );
    event.trace_id = trace_id;
    event
}
