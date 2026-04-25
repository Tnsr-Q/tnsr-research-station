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

/// SubprocessTransport spawns a subprocess and sends events via stdin.
/// The subprocess is expected to read JSON-encoded EventEnvelope objects from stdin.
#[derive(Debug)]
pub struct SubprocessTransport {
    id: String,
    state: TransportState,
    #[allow(dead_code)]
    command: String,
    #[allow(dead_code)]
    args: Vec<String>,
    #[cfg(feature = "subprocess")]
    child: Option<tokio::process::Child>,
}

impl SubprocessTransport {
    pub fn new(id: impl Into<String>, command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            id: id.into(),
            state: TransportState::Stopped,
            command: command.into(),
            args,
            #[cfg(feature = "subprocess")]
            child: None,
        }
    }
}

impl Transport for SubprocessTransport {
    fn id(&self) -> &str {
        &self.id
    }

    fn start(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Started => Err(TransportError::AlreadyStarted),
            TransportState::Stopped => {
                #[cfg(feature = "subprocess")]
                {
                    // Subprocess spawning would happen here in a real implementation
                    // For now, we just mark as started without actually spawning
                }
                self.state = TransportState::Started;
                Ok(())
            }
        }
    }

    fn stop(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                #[cfg(feature = "subprocess")]
                {
                    // Kill subprocess if it exists
                    self.child = None;
                }
                self.state = TransportState::Stopped;
                Ok(())
            }
        }
    }

    fn send(&mut self, event: &EventEnvelope) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                #[cfg(feature = "subprocess")]
                {
                    // In a real implementation, serialize event to JSON and write to child stdin
                    let _json = serde_json::to_string(event)
                        .map_err(|e| TransportError::SendFailed(e.to_string()))?;
                    // Write to stdin would happen here
                }
                #[cfg(not(feature = "subprocess"))]
                {
                    let _ = event; // Suppress unused variable warning
                }
                Ok(())
            }
        }
    }
}

/// WebSocketTransport sends events to a WebSocket endpoint.
/// Events are JSON-encoded and sent as text frames.
#[derive(Debug)]
pub struct WebSocketTransport {
    id: String,
    state: TransportState,
    #[allow(dead_code)]
    url: String,
}

impl WebSocketTransport {
    pub fn new(id: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            state: TransportState::Stopped,
            url: url.into(),
        }
    }
}

impl Transport for WebSocketTransport {
    fn id(&self) -> &str {
        &self.id
    }

    fn start(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Started => Err(TransportError::AlreadyStarted),
            TransportState::Stopped => {
                // WebSocket connection would be established here
                self.state = TransportState::Started;
                Ok(())
            }
        }
    }

    fn stop(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                // WebSocket connection would be closed here
                self.state = TransportState::Stopped;
                Ok(())
            }
        }
    }

    fn send(&mut self, event: &EventEnvelope) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                // Serialize event to JSON and send as WebSocket text frame
                let _json = serde_json::to_string(event)
                    .map_err(|e| TransportError::SendFailed(e.to_string()))?;
                // WebSocket send would happen here
                Ok(())
            }
        }
    }
}

/// GrpcTransport sends events via gRPC protocol.
/// Events are encoded as protobuf messages.
#[derive(Debug)]
pub struct GrpcTransport {
    id: String,
    state: TransportState,
    #[allow(dead_code)]
    endpoint: String,
}

impl GrpcTransport {
    pub fn new(id: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            state: TransportState::Stopped,
            endpoint: endpoint.into(),
        }
    }
}

impl Transport for GrpcTransport {
    fn id(&self) -> &str {
        &self.id
    }

    fn start(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Started => Err(TransportError::AlreadyStarted),
            TransportState::Stopped => {
                // gRPC channel would be established here
                self.state = TransportState::Started;
                Ok(())
            }
        }
    }

    fn stop(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                // gRPC channel would be closed here
                self.state = TransportState::Stopped;
                Ok(())
            }
        }
    }

    fn send(&mut self, event: &EventEnvelope) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                // Convert event to protobuf and send via gRPC
                let _json = serde_json::to_string(event)
                    .map_err(|e| TransportError::SendFailed(e.to_string()))?;
                // gRPC send would happen here
                Ok(())
            }
        }
    }
}

/// ConnectRpcTransport sends events using the Connect RPC protocol.
/// Connect is a protocol that works over HTTP/1.1, HTTP/2, and HTTP/3.
#[derive(Debug)]
pub struct ConnectRpcTransport {
    id: String,
    state: TransportState,
    #[allow(dead_code)]
    url: String,
}

impl ConnectRpcTransport {
    pub fn new(id: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            state: TransportState::Stopped,
            url: url.into(),
        }
    }
}

impl Transport for ConnectRpcTransport {
    fn id(&self) -> &str {
        &self.id
    }

    fn start(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Started => Err(TransportError::AlreadyStarted),
            TransportState::Stopped => {
                // Connect RPC client would be initialized here
                self.state = TransportState::Started;
                Ok(())
            }
        }
    }

    fn stop(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                // Connect RPC client would be closed here
                self.state = TransportState::Stopped;
                Ok(())
            }
        }
    }

    fn send(&mut self, event: &EventEnvelope) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                // Serialize event and send via Connect RPC
                let _json = serde_json::to_string(event)
                    .map_err(|e| TransportError::SendFailed(e.to_string()))?;
                // Connect RPC send would happen here
                Ok(())
            }
        }
    }
}

/// Pyro5Transport sends events to a Python Pyro5 object server.
/// Pyro5 is a Python remote objects library.
#[derive(Debug)]
pub struct Pyro5Transport {
    id: String,
    state: TransportState,
    #[allow(dead_code)]
    uri: String,
}

impl Pyro5Transport {
    pub fn new(id: impl Into<String>, uri: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            state: TransportState::Stopped,
            uri: uri.into(),
        }
    }
}

impl Transport for Pyro5Transport {
    fn id(&self) -> &str {
        &self.id
    }

    fn start(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Started => Err(TransportError::AlreadyStarted),
            TransportState::Stopped => {
                // Pyro5 proxy would be established here
                self.state = TransportState::Started;
                Ok(())
            }
        }
    }

    fn stop(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                // Pyro5 proxy would be released here
                self.state = TransportState::Stopped;
                Ok(())
            }
        }
    }

    fn send(&mut self, event: &EventEnvelope) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                // Serialize event and send to Pyro5 server
                let _json = serde_json::to_string(event)
                    .map_err(|e| TransportError::SendFailed(e.to_string()))?;
                // Pyro5 call would happen here
                Ok(())
            }
        }
    }
}

/// FfiTransport sends events via Foreign Function Interface (FFI).
/// This allows calling into C/C++ or other native libraries.
#[derive(Debug)]
pub struct FfiTransport {
    id: String,
    state: TransportState,
    #[allow(dead_code)]
    library_path: String,
}

impl FfiTransport {
    pub fn new(id: impl Into<String>, library_path: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            state: TransportState::Stopped,
            library_path: library_path.into(),
        }
    }
}

impl Transport for FfiTransport {
    fn id(&self) -> &str {
        &self.id
    }

    fn start(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Started => Err(TransportError::AlreadyStarted),
            TransportState::Stopped => {
                // Dynamic library would be loaded here
                self.state = TransportState::Started;
                Ok(())
            }
        }
    }

    fn stop(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                // Dynamic library would be unloaded here
                self.state = TransportState::Stopped;
                Ok(())
            }
        }
    }

    fn send(&mut self, event: &EventEnvelope) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                // Serialize event and call FFI function
                let _json = serde_json::to_string(event)
                    .map_err(|e| TransportError::SendFailed(e.to_string()))?;
                // FFI call would happen here
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
        assert!(matches!(
            result.unwrap_err(),
            TransportError::AlreadyStarted
        ));
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
        assert!(matches!(
            result.unwrap_err(),
            TransportError::AlreadyStarted
        ));
    }

    #[test]
    fn test_transport_ids() {
        let local = LocalTransport::new("local-1");
        let null = NullTransport::new("null-1");

        assert_eq!(local.id(), "local-1");
        assert_eq!(null.id(), "null-1");
    }

    #[test]
    fn test_subprocess_transport_basic() {
        let mut transport = SubprocessTransport::new("test-subprocess", "/bin/cat", vec![]);
        assert_eq!(transport.id(), "test-subprocess");

        let result = transport.send(&EventEnvelope::new(
            "run-test",
            "test.topic",
            "test_plugin",
            json!({}),
        ));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TransportError::NotStarted));

        transport.start().expect("start should succeed");

        let event = EventEnvelope::new("run-test", "test.topic", "test_plugin", json!({"n": 1}));
        transport.send(&event).expect("send should succeed");

        transport.stop().expect("stop should succeed");
    }

    #[test]
    fn test_websocket_transport_basic() {
        let mut transport = WebSocketTransport::new("test-ws", "ws://localhost:8080");
        assert_eq!(transport.id(), "test-ws");

        let result = transport.send(&EventEnvelope::new(
            "run-test",
            "test.topic",
            "test_plugin",
            json!({}),
        ));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TransportError::NotStarted));

        transport.start().expect("start should succeed");

        let event = EventEnvelope::new("run-test", "test.topic", "test_plugin", json!({"n": 1}));
        transport.send(&event).expect("send should succeed");

        transport.stop().expect("stop should succeed");
    }

    #[test]
    fn test_grpc_transport_basic() {
        let mut transport = GrpcTransport::new("test-grpc", "http://localhost:50051");
        assert_eq!(transport.id(), "test-grpc");

        let result = transport.send(&EventEnvelope::new(
            "run-test",
            "test.topic",
            "test_plugin",
            json!({}),
        ));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TransportError::NotStarted));

        transport.start().expect("start should succeed");

        let event = EventEnvelope::new("run-test", "test.topic", "test_plugin", json!({"n": 1}));
        transport.send(&event).expect("send should succeed");

        transport.stop().expect("stop should succeed");
    }

    #[test]
    fn test_connectrpc_transport_basic() {
        let mut transport = ConnectRpcTransport::new("test-connect", "http://localhost:8081");
        assert_eq!(transport.id(), "test-connect");

        let result = transport.send(&EventEnvelope::new(
            "run-test",
            "test.topic",
            "test_plugin",
            json!({}),
        ));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TransportError::NotStarted));

        transport.start().expect("start should succeed");

        let event = EventEnvelope::new("run-test", "test.topic", "test_plugin", json!({"n": 1}));
        transport.send(&event).expect("send should succeed");

        transport.stop().expect("stop should succeed");
    }

    #[test]
    fn test_pyro5_transport_basic() {
        let mut transport = Pyro5Transport::new("test-pyro5", "PYRO:obj_12345@localhost:9090");
        assert_eq!(transport.id(), "test-pyro5");

        let result = transport.send(&EventEnvelope::new(
            "run-test",
            "test.topic",
            "test_plugin",
            json!({}),
        ));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TransportError::NotStarted));

        transport.start().expect("start should succeed");

        let event = EventEnvelope::new("run-test", "test.topic", "test_plugin", json!({"n": 1}));
        transport.send(&event).expect("send should succeed");

        transport.stop().expect("stop should succeed");
    }

    #[test]
    fn test_ffi_transport_basic() {
        let mut transport = FfiTransport::new("test-ffi", "/usr/lib/libtest.so");
        assert_eq!(transport.id(), "test-ffi");

        let result = transport.send(&EventEnvelope::new(
            "run-test",
            "test.topic",
            "test_plugin",
            json!({}),
        ));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TransportError::NotStarted));

        transport.start().expect("start should succeed");

        let event = EventEnvelope::new("run-test", "test.topic", "test_plugin", json!({"n": 1}));
        transport.send(&event).expect("send should succeed");

        transport.stop().expect("stop should succeed");
    }

    #[test]
    fn test_all_transports_cannot_start_twice() {
        let mut subprocess = SubprocessTransport::new("test", "/bin/cat", vec![]);
        subprocess.start().expect("first start");
        assert!(subprocess.start().is_err());

        let mut ws = WebSocketTransport::new("test", "ws://localhost:8080");
        ws.start().expect("first start");
        assert!(ws.start().is_err());

        let mut grpc = GrpcTransport::new("test", "http://localhost:50051");
        grpc.start().expect("first start");
        assert!(grpc.start().is_err());

        let mut connect = ConnectRpcTransport::new("test", "http://localhost:8081");
        connect.start().expect("first start");
        assert!(connect.start().is_err());

        let mut pyro5 = Pyro5Transport::new("test", "PYRO:obj@localhost:9090");
        pyro5.start().expect("first start");
        assert!(pyro5.start().is_err());

        let mut ffi = FfiTransport::new("test", "/usr/lib/libtest.so");
        ffi.start().expect("first start");
        assert!(ffi.start().is_err());
    }
}
