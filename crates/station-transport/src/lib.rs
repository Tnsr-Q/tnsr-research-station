use runtime_core::EventEnvelope;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("transport not started")]
    NotStarted,

    #[error("transport already started")]
    AlreadyStarted,

    #[error("send failed: {0}")]
    SendFailed(String),

    #[error("transport error: {0}")]
    Other(String),
}

pub trait Transport {
    fn id(&self) -> &str;
    fn start(&mut self) -> Result<(), TransportError>;
    fn stop(&mut self) -> Result<(), TransportError>;
    fn send(&mut self, event: &EventEnvelope) -> Result<(), TransportError>;
}

#[derive(Debug)]
enum TransportState {
    Stopped,
    Started,
}

/// LocalTransport is an in-memory transport for testing and local execution.
/// It collects all sent events in a vector without actually transmitting them.
#[derive(Debug)]
pub struct LocalTransport {
    id: String,
    state: TransportState,
    events: Vec<EventEnvelope>,
}

impl LocalTransport {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            state: TransportState::Stopped,
            events: Vec::new(),
        }
    }

    pub fn events(&self) -> &[EventEnvelope] {
        &self.events
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }
}

impl Transport for LocalTransport {
    fn id(&self) -> &str {
        &self.id
    }

    fn start(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Started => Err(TransportError::AlreadyStarted),
            TransportState::Stopped => {
                self.state = TransportState::Started;
                Ok(())
            }
        }
    }

    fn stop(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                self.state = TransportState::Stopped;
                Ok(())
            }
        }
    }

    fn send(&mut self, event: &EventEnvelope) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                self.events.push(event.clone());
                Ok(())
            }
        }
    }
}

/// NullTransport is a no-op transport that discards all events.
/// Useful for benchmarking and testing scenarios where event delivery is not needed.
#[derive(Debug)]
pub struct NullTransport {
    id: String,
    state: TransportState,
}

impl NullTransport {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            state: TransportState::Stopped,
        }
    }
}

impl Transport for NullTransport {
    fn id(&self) -> &str {
        &self.id
    }

    fn start(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Started => Err(TransportError::AlreadyStarted),
            TransportState::Stopped => {
                self.state = TransportState::Started;
                Ok(())
            }
        }
    }

    fn stop(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                self.state = TransportState::Stopped;
                Ok(())
            }
        }
    }

    fn send(&mut self, _event: &EventEnvelope) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                // Intentionally discard the event
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_local_transport_must_start_before_send() {
        let mut transport = LocalTransport::new("test-local");
        let event = EventEnvelope::new("run-test", "test.topic", "test_plugin", json!({}));

        let result = transport.send(&event);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TransportError::NotStarted));
    }

    #[test]
    fn test_local_transport_collects_events() {
        let mut transport = LocalTransport::new("test-local");
        transport.start().expect("start should succeed");

        let event1 = EventEnvelope::new("run-test", "test.topic1", "test_plugin", json!({"n": 1}));
        let event2 = EventEnvelope::new("run-test", "test.topic2", "test_plugin", json!({"n": 2}));

        transport.send(&event1).expect("send event1");
        transport.send(&event2).expect("send event2");

        assert_eq!(transport.events().len(), 2);
        assert_eq!(transport.events()[0].topic, "test.topic1");
        assert_eq!(transport.events()[1].topic, "test.topic2");
    }

    #[test]
    fn test_local_transport_cannot_start_twice() {
        let mut transport = LocalTransport::new("test-local");
        transport.start().expect("first start should succeed");

        let result = transport.start();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TransportError::AlreadyStarted));
    }

    #[test]
    fn test_local_transport_stop_then_start_again() {
        let mut transport = LocalTransport::new("test-local");
        transport.start().expect("start");
        transport.stop().expect("stop");
        transport.start().expect("start again should succeed");

        let event = EventEnvelope::new("run-test", "test.topic", "test_plugin", json!({}));
        transport.send(&event).expect("send after restart");

        assert_eq!(transport.events().len(), 1);
    }

    #[test]
    fn test_local_transport_clear_events() {
        let mut transport = LocalTransport::new("test-local");
        transport.start().expect("start");

        let event = EventEnvelope::new("run-test", "test.topic", "test_plugin", json!({}));
        transport.send(&event).expect("send");

        assert_eq!(transport.events().len(), 1);

        transport.clear();
        assert_eq!(transport.events().len(), 0);
    }

    #[test]
    fn test_null_transport_discards_events() {
        let mut transport = NullTransport::new("test-null");
        transport.start().expect("start should succeed");

        let event = EventEnvelope::new("run-test", "test.topic", "test_plugin", json!({"n": 1}));

        transport.send(&event).expect("send should succeed");
        // No way to verify event was discarded, but send succeeding is the test
    }

    #[test]
    fn test_null_transport_must_start_before_send() {
        let mut transport = NullTransport::new("test-null");
        let event = EventEnvelope::new("run-test", "test.topic", "test_plugin", json!({}));

        let result = transport.send(&event);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TransportError::NotStarted));
    }

    #[test]
    fn test_null_transport_cannot_start_twice() {
        let mut transport = NullTransport::new("test-null");
        transport.start().expect("first start should succeed");

        let result = transport.start();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TransportError::AlreadyStarted));
    }

    #[test]
    fn test_transport_ids() {
        let local = LocalTransport::new("local-1");
        let null = NullTransport::new("null-1");

        assert_eq!(local.id(), "local-1");
        assert_eq!(null.id(), "null-1");
    }
}
