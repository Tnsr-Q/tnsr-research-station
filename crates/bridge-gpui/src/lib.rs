use runtime_core::EventEnvelope;

pub fn to_gpui_overlay_line(event: &EventEnvelope) -> String {
    format!("[{}] {} <= {}", event.trace_id, event.topic, event.source)
}
