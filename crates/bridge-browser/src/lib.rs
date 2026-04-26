use runtime_core::EventEnvelope;
use serde_json::json;

pub fn to_browser_frame(event: &EventEnvelope) -> Result<String, serde_json::Error> {
    serde_json::to_string(event)
}

pub fn to_browser_frame_pretty(event: &EventEnvelope) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(event)
}

pub fn to_browser_projection(event: &EventEnvelope) -> Result<String, serde_json::Error> {
    let projection = json!({
        "event_id": event.event_id,
        "trace_id": event.trace_id,
        "topic": event.topic,
        "source": event.source,
        "created_at_ms": event.created_at_ms,
        "payload": event.payload,
    });
    serde_json::to_string(&projection)
}

pub fn to_browser_timeline_entry(event: &EventEnvelope) -> Result<String, serde_json::Error> {
    let entry = json!({
        "timestamp": event.created_at_ms,
        "topic": event.topic,
        "source": event.source,
        "trace_id": event.trace_id,
        "summary": format!("{} from {}", event.topic, event.source),
    });
    serde_json::to_string(&entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_browser_frame_is_valid_json() {
        let event = EventEnvelope::new(
            "session-browser",
            "quantum.state",
            "adapter_quantum",
            json!({ "collapse_ratio": 0.42 }),
        );

        let frame = to_browser_frame(&event).expect("frame serialization should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&frame).expect("browser frame should be valid json");

        assert_eq!(parsed["event_id"], event.event_id);
        assert_eq!(parsed["version"], "tnsr.event.v1");
    }

    #[test]
    fn test_browser_frame_pretty_is_formatted() {
        let event = EventEnvelope::new(
            "session-browser",
            "quantum.state",
            "adapter_quantum",
            json!({ "collapse_ratio": 0.42 }),
        );

        let frame = to_browser_frame_pretty(&event).expect("pretty serialization should succeed");
        assert!(frame.contains('\n'));
        assert!(frame.contains("  "));
    }

    #[test]
    fn test_browser_projection_excludes_internal_fields() {
        let event = EventEnvelope::new(
            "session-browser",
            "quantum.state",
            "adapter_quantum",
            json!({ "collapse_ratio": 0.42 }),
        );

        let projection =
            to_browser_projection(&event).expect("projection serialization should succeed");
        let parsed: serde_json::Value = serde_json::from_str(&projection).unwrap();

        assert_eq!(parsed["event_id"], event.event_id);
        assert_eq!(parsed["topic"], "quantum.state");
        assert_eq!(parsed["source"], "adapter_quantum");
        assert!(parsed.get("version").is_none());
        assert!(parsed.get("session_id").is_none());
    }

    #[test]
    fn test_browser_timeline_entry_format() {
        let event = EventEnvelope::new(
            "session-browser",
            "quantum.state",
            "adapter_quantum",
            json!({ "collapse_ratio": 0.42 }),
        );

        let entry = to_browser_timeline_entry(&event).expect("timeline entry should succeed");
        let parsed: serde_json::Value = serde_json::from_str(&entry).unwrap();

        assert_eq!(parsed["topic"], "quantum.state");
        assert_eq!(parsed["source"], "adapter_quantum");
        assert_eq!(parsed["summary"], "quantum.state from adapter_quantum");
        assert!(parsed.get("timestamp").is_some());
    }

    #[test]
    fn test_browser_projection_preserves_payload() {
        let payload = json!({
            "state_dim": 16,
            "collapse_ratio": 0.42,
            "nested": { "value": 123 }
        });

        let event = EventEnvelope::new(
            "session-browser",
            "quantum.state",
            "adapter_quantum",
            payload.clone(),
        );

        let projection =
            to_browser_projection(&event).expect("projection serialization should succeed");
        let parsed: serde_json::Value = serde_json::from_str(&projection).unwrap();

        assert_eq!(parsed["payload"], payload);
    }

    #[test]
    fn test_browser_timeline_entry_includes_trace_id() {
        let event = EventEnvelope::new(
            "session-browser",
            "quantum.state",
            "adapter_quantum",
            json!({}),
        );

        let entry = to_browser_timeline_entry(&event).expect("timeline entry should succeed");
        let parsed: serde_json::Value = serde_json::from_str(&entry).unwrap();

        assert_eq!(parsed["trace_id"], event.trace_id);
    }
}
