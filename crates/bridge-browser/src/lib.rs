use runtime_core::EventEnvelope;

pub fn to_browser_frame(event: &EventEnvelope) -> String {
    format!(
        "{{\"topic\":\"{}\",\"trace_id\":\"{}\",\"source\":\"{}\",\"payload\":{}}}",
        event.topic, event.trace_id, event.source, event.payload
    )
}
