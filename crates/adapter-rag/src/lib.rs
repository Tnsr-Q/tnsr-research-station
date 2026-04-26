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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rag_log_event_has_correct_topic() {
        let event = rag_log_event("test-session", "trace-123".to_string(), "test message");
        assert_eq!(event.topic, "rag.result");
    }

    #[test]
    fn test_rag_log_event_has_correct_source() {
        let event = rag_log_event("test-session", "trace-123".to_string(), "test message");
        assert_eq!(event.source, "adapter_rag");
    }

    #[test]
    fn test_rag_log_event_preserves_message() {
        let message = "Hello, RAG!";
        let event = rag_log_event("test-session", "trace-123".to_string(), message);
        assert_eq!(event.payload["message"], message);
    }

    #[test]
    fn test_rag_log_event_preserves_trace_id() {
        let trace_id = "custom-trace-456";
        let event = rag_log_event("test-session", trace_id.to_string(), "test");
        assert_eq!(event.trace_id, trace_id);
    }

    #[test]
    fn test_rag_log_event_handles_empty_message() {
        let event = rag_log_event("test-session", "trace-123".to_string(), "");
        assert_eq!(event.payload["message"], "");
    }

    #[test]
    fn test_rag_log_event_handles_multiline_message() {
        let message = "Line 1\nLine 2\nLine 3";
        let event = rag_log_event("test-session", "trace-123".to_string(), message);
        assert_eq!(event.payload["message"], message);
    }

    #[test]
    fn test_rag_log_event_handles_unicode() {
        let message = "Hello 世界 🌍";
        let event = rag_log_event("test-session", "trace-123".to_string(), message);
        assert_eq!(event.payload["message"], message);
    }
}
