use runtime_core::EventEnvelope;

pub fn to_browser_frame(event: &EventEnvelope) -> Result<String, serde_json::Error> {
    serde_json::to_string(event)
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
}
