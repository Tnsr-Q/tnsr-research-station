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
    pub skipped: usize,
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

enum SubscriptionFilter {
    Exact(String),
    Prefix(String),
}

impl SubscriptionFilter {
    fn matches(&self, topic: &str) -> bool {
        match self {
            SubscriptionFilter::Exact(pattern) => topic == pattern,
            SubscriptionFilter::Prefix(prefix) => topic.starts_with(prefix),
        }
    }
}

struct Subscription {
    filter: SubscriptionFilter,
    sender: Sender<EventEnvelope>,
}

pub struct EventBus {
    subscriptions: Vec<Subscription>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            subscriptions: Vec::new(),
        }
    }

    pub fn subscribe(&mut self, topic: impl Into<String>) -> Receiver<EventEnvelope> {
        let (tx, rx) = mpsc::channel();
        self.subscriptions.push(Subscription {
            filter: SubscriptionFilter::Exact(topic.into()),
            sender: tx,
        });
        rx
    }

    pub fn subscribe_prefix(&mut self, prefix: impl Into<String>) -> Receiver<EventEnvelope> {
        let (tx, rx) = mpsc::channel();
        self.subscriptions.push(Subscription {
            filter: SubscriptionFilter::Prefix(prefix.into()),
            sender: tx,
        });
        rx
    }

    pub fn publish(&mut self, event: EventEnvelope) -> PublishReport {
        let mut attempted = 0;
        let mut delivered = 0;
        let mut failed = 0;
        let mut skipped = 0;

        // Filter and deliver to matching subscribers, prune dead ones
        self.subscriptions.retain(|sub| {
            if sub.filter.matches(&event.topic) {
                attempted += 1;
                match sub.sender.send(event.clone()) {
                    Ok(_) => {
                        delivered += 1;
                        true // Keep alive subscriber
                    }
                    Err(_) => {
                        failed += 1;
                        false // Prune dead subscriber
                    }
                }
            } else {
                skipped += 1;
                true // Keep non-matching subscriber
            }
        });

        PublishReport {
            attempted,
            delivered,
            failed,
            skipped,
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
        let rx_live = bus.subscribe("quantum.state");
        let rx_dropped = bus.subscribe("quantum.state");
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
        assert_eq!(report.skipped, 0);

        let _received = rx_live
            .recv()
            .expect("live subscriber should receive event");
    }

    #[test]
    fn test_exact_topic_matching() {
        let mut bus = EventBus::new();
        let rx_quantum = bus.subscribe("quantum.state");
        let rx_supervisor = bus.subscribe("supervisor.registered");

        let event = EventEnvelope::new(
            "session-a",
            "quantum.state",
            "adapter_quantum",
            json!({ "n": 1 }),
        );
        let report = bus.publish(event);

        assert_eq!(report.attempted, 1);
        assert_eq!(report.delivered, 1);
        assert_eq!(report.failed, 0);
        assert_eq!(report.skipped, 1);

        assert!(rx_quantum.try_recv().is_ok());
        assert!(rx_supervisor.try_recv().is_err());
    }

    #[test]
    fn test_prefix_matching() {
        let mut bus = EventBus::new();
        let rx_prefix = bus.subscribe_prefix("supervisor.");
        let rx_exact = bus.subscribe("quantum.state");

        let event1 = EventEnvelope::new(
            "session-a",
            "supervisor.registered",
            "supervisor",
            json!({ "plugin": "test" }),
        );
        let report1 = bus.publish(event1);

        assert_eq!(report1.attempted, 1);
        assert_eq!(report1.delivered, 1);
        assert_eq!(report1.failed, 0);
        assert_eq!(report1.skipped, 1);

        let event2 = EventEnvelope::new(
            "session-a",
            "supervisor.admitted",
            "supervisor",
            json!({ "plugin": "test" }),
        );
        let report2 = bus.publish(event2);

        assert_eq!(report2.attempted, 1);
        assert_eq!(report2.delivered, 1);
        assert_eq!(report2.skipped, 1);

        assert_eq!(rx_prefix.try_recv().unwrap().topic, "supervisor.registered");
        assert_eq!(rx_prefix.try_recv().unwrap().topic, "supervisor.admitted");
        assert!(rx_exact.try_recv().is_err());
    }

    #[test]
    fn test_dead_subscribers_are_pruned() {
        let mut bus = EventBus::new();
        let rx1 = bus.subscribe("quantum.state");
        let rx2 = bus.subscribe("quantum.state");

        drop(rx2); // Drop one subscriber

        let event1 = EventEnvelope::new(
            "session-a",
            "quantum.state",
            "adapter_quantum",
            json!({ "n": 1 }),
        );
        let report1 = bus.publish(event1);

        assert_eq!(report1.attempted, 2);
        assert_eq!(report1.delivered, 1);
        assert_eq!(report1.failed, 1);

        // Second publish should only attempt delivery to 1 subscriber (pruned)
        let event2 = EventEnvelope::new(
            "session-a",
            "quantum.state",
            "adapter_quantum",
            json!({ "n": 2 }),
        );
        let report2 = bus.publish(event2);

        assert_eq!(report2.attempted, 1);
        assert_eq!(report2.delivered, 1);
        assert_eq!(report2.failed, 0);

        drop(rx1);
    }

    #[test]
    fn test_unmatched_subscribers_receive_nothing() {
        let mut bus = EventBus::new();
        let rx_quantum = bus.subscribe("quantum.state");
        let rx_supervisor = bus.subscribe("supervisor.registered");

        let event = EventEnvelope::new(
            "session-a",
            "rag.query",
            "adapter_rag",
            json!({ "text": "test" }),
        );
        let report = bus.publish(event);

        assert_eq!(report.attempted, 0);
        assert_eq!(report.delivered, 0);
        assert_eq!(report.failed, 0);
        assert_eq!(report.skipped, 2);

        assert!(rx_quantum.try_recv().is_err());
        assert!(rx_supervisor.try_recv().is_err());
    }
}
