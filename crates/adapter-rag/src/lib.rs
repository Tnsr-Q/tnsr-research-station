use runtime_core::EventEnvelope;

pub fn rag_log_event(trace_id: String, message: &str) -> EventEnvelope {
    let mut event = EventEnvelope::new(
        "rag.result",
        "adapter_rag",
        format!("{{\"message\":\"{}\"}}", message),
    );
    event.trace_id = trace_id;
    event
}
