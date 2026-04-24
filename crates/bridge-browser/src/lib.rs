use runtime_core::EventEnvelope;

pub fn to_browser_frame(event: &EventEnvelope) -> Result<String, serde_json::Error> {
    serde_json::to_string(event)
}
