use runtime_core::EventEnvelope;
use serde_json::json;

pub fn quantum_state_event(session_id: impl Into<String>) -> EventEnvelope {
    EventEnvelope::new(
        session_id,
        "quantum.state",
        "adapter_quantum",
        json!({
            "state_dim": 16,
            "collapse_ratio": 0.42,
            "euler_characteristic": 8,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quantum_state_event_has_correct_topic() {
        let event = quantum_state_event("test-session");
        assert_eq!(event.topic, "quantum.state");
    }

    #[test]
    fn test_quantum_state_event_has_correct_source() {
        let event = quantum_state_event("test-session");
        assert_eq!(event.source, "adapter_quantum");
    }

    #[test]
    fn test_quantum_state_event_includes_required_fields() {
        let event = quantum_state_event("test-session");
        assert!(event.payload["state_dim"].is_number());
        assert!(event.payload["collapse_ratio"].is_number());
        assert!(event.payload["euler_characteristic"].is_number());
    }

    #[test]
    fn test_quantum_state_event_has_expected_values() {
        let event = quantum_state_event("test-session");
        assert_eq!(event.payload["state_dim"], 16);
        assert_eq!(event.payload["collapse_ratio"], 0.42);
        assert_eq!(event.payload["euler_characteristic"], 8);
    }

    #[test]
    fn test_quantum_state_event_generates_unique_event_ids() {
        let event1 = quantum_state_event("test-session");
        let event2 = quantum_state_event("test-session");
        assert_ne!(event1.event_id, event2.event_id);
    }

    #[test]
    fn test_quantum_state_event_preserves_session_id() {
        let session_id = "custom-session-123";
        let event = quantum_state_event(session_id);
        assert_eq!(event.session_id, session_id);
    }
}
