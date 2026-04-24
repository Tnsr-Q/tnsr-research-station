use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{SystemTime, UNIX_EPOCH};
use ulid::Ulid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub version: String,
    pub event_id: String,
    pub trace_id: String,
    pub parent_id: Option<String>,
    pub session_id: String,
    pub topic: String,
    pub source: String,
    pub created_at_ms: u128,
    pub payload: Value,
    pub input_hash: Option<String>,
    pub artifact_hash: Option<String>,
    pub schema_hash: Option<String>,
    pub plugin_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishReport {
    pub attempted: usize,
    pub delivered: usize,
    pub failed: usize,
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

fn id(prefix: &str) -> String {
    format!("{}-{}", prefix, Ulid::new())
}

impl EventEnvelope {
    pub fn new(
        session_id: impl Into<String>,
        topic: impl Into<String>,
        source: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            version: "tnsr.event.v1".into(),
            event_id: id("evt"),
            trace_id: id("trace"),
            parent_id: None,
            session_id: session_id.into(),
            topic: topic.into(),
            source: source.into(),
            created_at_ms: now_ms(),
            payload,
            input_hash: None,
            artifact_hash: None,
            schema_hash: None,
            plugin_hash: None,
        }
    }

    pub fn child_of(
        parent: &Self,
        topic: impl Into<String>,
        source: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            version: "tnsr.event.v1".into(),
            event_id: id("evt"),
            trace_id: parent.trace_id.clone(),
            parent_id: Some(parent.event_id.clone()),
            session_id: parent.session_id.clone(),
            topic: topic.into(),
            source: source.into(),
            created_at_ms: now_ms(),
            payload,
            input_hash: None,
            artifact_hash: None,
            schema_hash: None,
            plugin_hash: None,
        }
    }
}

pub struct EventBus {
    subscribers: Vec<Sender<EventEnvelope>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            subscribers: Vec::new(),
        }
    }

    pub fn subscribe(&mut self) -> Receiver<EventEnvelope> {
        let (tx, rx) = mpsc::channel();
        self.subscribers.push(tx);
        rx
    }

    pub fn publish(&self, event: EventEnvelope) -> PublishReport {
        let attempted = self.subscribers.len();
        let mut delivered = 0;

        for tx in &self.subscribers {
            if tx.send(event.clone()).is_ok() {
                delivered += 1;
            }
        }

        PublishReport {
            attempted,
            delivered,
            failed: attempted.saturating_sub(delivered),
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct SessionState {
    pub run_id: String,
    pub profile_name: String,
    pub started_at_ms: u128,
}

impl SessionState {
    pub fn new(profile_name: impl Into<String>) -> Self {
        Self {
            run_id: id("run"),
            profile_name: profile_name.into(),
            started_at_ms: now_ms(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_event_ids_are_distinct() {
        let a = EventEnvelope::new(
            "session-a",
            "quantum.state",
            "adapter_quantum",
            json!({ "n": 1 }),
        );
        let b = EventEnvelope::new(
            "session-a",
            "quantum.state",
            "adapter_quantum",
            json!({ "n": 2 }),
        );

        assert_ne!(a.event_id, b.event_id);
    }

    #[test]
    fn test_child_event_preserves_trace_id() {
        let parent = EventEnvelope::new(
            "session-a",
            "quantum.state",
            "adapter_quantum",
            json!({ "step": 1 }),
        );
        let child = EventEnvelope::child_of(
            &parent,
            "quantum.derived",
            "adapter_quantum",
            json!({ "step": 2 }),
        );

        assert_eq!(child.trace_id, parent.trace_id);
        assert_eq!(child.parent_id.as_deref(), Some(parent.event_id.as_str()));
        assert_eq!(child.session_id, parent.session_id);
    }

    #[test]
    fn test_publish_report_counts_delivery() {
        let mut bus = EventBus::new();
        let rx_live = bus.subscribe();
        let rx_dropped = bus.subscribe();
        drop(rx_dropped);

        let event = EventEnvelope::new(
            "session-a",
            "quantum.state",
            "adapter_quantum",
            json!({ "n": 1 }),
        );
        let report = bus.publish(event);

        assert_eq!(report.attempted, 2);
        assert_eq!(report.delivered, 1);
        assert_eq!(report.failed, 1);

        let _received = rx_live
            .recv()
            .expect("live subscriber should receive event");
    }
}
