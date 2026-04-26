use runtime_core::EventEnvelope;

pub fn to_gpui_overlay_line(event: &EventEnvelope) -> String {
    format!("[{}] {} <= {}", event.trace_id, event.topic, event.source)
}

pub fn to_gpui_detailed_line(event: &EventEnvelope) -> String {
    format!(
        "[{}] {} <= {} | event_id={} | payload={}",
        event.trace_id,
        event.topic,
        event.source,
        event.event_id,
        serde_json::to_string(&event.payload).unwrap_or_else(|_| "{}".to_string())
    )
}

pub fn to_gpui_compact(event: &EventEnvelope) -> String {
    format!("{} <= {}", event.topic, event.source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_overlay_line_format() {
        let event = EventEnvelope::new(
            "session-test",
            "quantum.state",
            "adapter_quantum",
            json!({ "state_dim": 16 }),
        );

        let line = to_gpui_overlay_line(&event);
        assert!(line.contains(&event.trace_id));
        assert!(line.contains("quantum.state"));
        assert!(line.contains("adapter_quantum"));
        assert!(line.starts_with('['));
    }

    #[test]
    fn test_detailed_line_includes_event_id_and_payload() {
        let event = EventEnvelope::new(
            "session-test",
            "quantum.state",
            "adapter_quantum",
            json!({ "state_dim": 16, "collapse_ratio": 0.42 }),
        );

        let line = to_gpui_detailed_line(&event);
        assert!(line.contains(&event.event_id));
        assert!(line.contains("state_dim"));
        assert!(line.contains("collapse_ratio"));
        assert!(line.contains("event_id="));
        assert!(line.contains("payload="));
    }

    #[test]
    fn test_compact_format_minimal() {
        let event = EventEnvelope::new(
            "session-test",
            "quantum.state",
            "adapter_quantum",
            json!({}),
        );

        let line = to_gpui_compact(&event);
        assert_eq!(line, "quantum.state <= adapter_quantum");
        assert!(!line.contains(&event.trace_id));
        assert!(!line.contains(&event.event_id));
    }

    #[test]
    fn test_overlay_line_handles_long_ids() {
        let mut event = EventEnvelope::new(
            "session-test",
            "quantum.state",
            "adapter_quantum",
            json!({}),
        );
        event.trace_id = "very-long-trace-id-12345678901234567890".to_string();

        let line = to_gpui_overlay_line(&event);
        assert!(line.contains("very-long-trace-id-12345678901234567890"));
    }

    #[test]
    fn test_detailed_line_handles_complex_payload() {
        let event = EventEnvelope::new(
            "session-test",
            "quantum.state",
            "adapter_quantum",
            json!({
                "state_dim": 16,
                "nested": {
                    "value": 42,
                    "array": [1, 2, 3]
                }
            }),
        );

        let line = to_gpui_detailed_line(&event);
        assert!(line.contains("nested"));
        assert!(line.contains("array"));
    }
}
