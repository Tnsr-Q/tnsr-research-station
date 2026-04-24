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
    pub fn new(topic: impl Into<String>, source: impl Into<String>, payload: Value) -> Self {
        Self {
            version: "tnsr.event.v1".into(),
            event_id: id("evt"),
            trace_id: id("trace"),
            parent_id: None,
            session_id: id("session"),
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

    pub fn publish(&self, event: EventEnvelope) {
        for tx in &self.subscribers {
            let _ = tx.send(event.clone());
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
