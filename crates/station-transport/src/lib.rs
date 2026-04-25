use runtime_core::EventEnvelope;
#[cfg(feature = "subprocess")]
use serde_json::json;
#[cfg(feature = "subprocess")]
use std::io::{BufRead, BufReader, Write};
#[cfg(feature = "subprocess")]
use std::process::{Child, ChildStdin, Command, Stdio};
#[cfg(feature = "subprocess")]
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
#[cfg(feature = "subprocess")]
use std::thread;
#[cfg(feature = "subprocess")]
use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("transport not started")]
    NotStarted,

    #[error("transport already started")]
    AlreadyStarted,

    #[error("send failed: {0}")]
    SendFailed(String),

    #[error("transport timeout: {0}")]
    Timeout(String),

    #[error("transport error: {0}")]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportKind {
    Local,
    Null,
    Wasm,
    WebSocket,
    Grpc,
    ConnectRpc,
    Pyro5,
    Subprocess,
    Ffi,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportConfig {
    pub id: String,
    pub kind: TransportKind,
    pub endpoint: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub working_dir: Option<String>,
    pub env_allowlist: Vec<String>,
    pub timeout_ms: Option<u64>,
}

pub fn build_transport(config: &TransportConfig) -> Result<Box<dyn Transport>, TransportError> {
    match config.kind {
        TransportKind::Local => Ok(Box::new(LocalTransport::new(config.id.clone()))),
        TransportKind::Null => Ok(Box::new(NullTransport::new(config.id.clone()))),
        #[cfg(feature = "subprocess")]
        TransportKind::Subprocess => {
            let command = config.command.clone().ok_or_else(|| {
                TransportError::Other("subprocess transport requires command".to_string())
            })?;
            Ok(Box::new(
                SubprocessTransport::new(config.id.clone(), command, config.args.clone())
                    .with_working_dir(config.working_dir.clone())
                    .with_env_allowlist(config.env_allowlist.clone())
                    .with_shutdown_timeout_ms(config.timeout_ms),
            ))
        }
        #[cfg(not(feature = "subprocess"))]
        TransportKind::Subprocess => Err(TransportError::Other(
            "transport kind not enabled".to_string(),
        )),
        TransportKind::Wasm
        | TransportKind::WebSocket
        | TransportKind::Grpc
        | TransportKind::ConnectRpc
        | TransportKind::Pyro5
        | TransportKind::Ffi => Err(TransportError::Other(
            "transport kind not enabled".to_string(),
        )),
    }
}

pub trait Transport {
    fn id(&self) -> &str;
    fn start(&mut self) -> Result<(), TransportError>;
    fn stop(&mut self) -> Result<(), TransportError>;
    fn send(&mut self, event: &EventEnvelope) -> Result<(), TransportError>;
    fn drain_candidate_events(&mut self) -> Vec<EventEnvelope> {
        Vec::new()
    }
    fn drain_evidence_events(&mut self) -> Vec<EventEnvelope> {
        Vec::new()
    }
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
#[cfg(feature = "subprocess")]
#[derive(Debug)]
pub struct SubprocessTransport {
    id: String,
    state: TransportState,
    command: String,
    args: Vec<String>,
    working_dir: Option<String>,
    env_allowlist: Vec<String>,
    child: Option<Child>,
    child_stdin: Option<ChildStdin>,
    line_rx: Option<Receiver<SidecarLine>>,
    candidate_events: Vec<EventEnvelope>,
    evidence_events: Vec<EventEnvelope>,
    session_id: Option<String>,
    shutdown_timeout: Duration,
}

#[cfg(feature = "subprocess")]
#[derive(Debug)]
enum SidecarLine {
    Stdout(String),
    Stderr(String),
}

#[cfg(feature = "subprocess")]
impl SubprocessTransport {
    pub fn new(id: impl Into<String>, command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            id: id.into(),
            state: TransportState::Stopped,
            command: command.into(),
            args,
            working_dir: None,
            env_allowlist: Vec::new(),
            child: None,
            child_stdin: None,
            line_rx: None,
            candidate_events: Vec::new(),
            evidence_events: Vec::new(),
            session_id: None,
            shutdown_timeout: Duration::from_secs(1),
        }
    }

    pub fn with_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }

    pub fn with_shutdown_timeout_ms(mut self, timeout_ms: Option<u64>) -> Self {
        if let Some(ms) = timeout_ms {
            self.shutdown_timeout = Duration::from_millis(ms);
        }
        self
    }

    pub fn with_working_dir(mut self, working_dir: Option<String>) -> Self {
        self.working_dir = working_dir;
        self
    }

    pub fn with_env_allowlist(mut self, env_allowlist: Vec<String>) -> Self {
        self.env_allowlist = env_allowlist;
        self
    }

    pub fn admit_candidate_events<F>(&mut self, mut policy_admit: F)
    where
        F: FnMut(&EventEnvelope) -> bool,
    {
        let mut retained = Vec::new();
        let transport_id = self.id.clone();
        let candidates = std::mem::take(&mut self.candidate_events);
        for event in candidates {
            if policy_admit(&event) {
                retained.push(event);
            } else {
                let event_id = event.event_id.clone();
                let topic = event.topic.clone();
                self.evidence_events.push(self.evidence(
                    "policy.runtime.denied",
                    json!({
                        "transport_id": transport_id.clone(),
                        "reason": "subprocess-originating event denied by policy engine",
                        "event_id": event_id,
                        "topic": topic
                    }),
                ));
            }
        }
        self.candidate_events = retained;
    }

    pub fn candidate_events(&self) -> &[EventEnvelope] {
        &self.candidate_events
    }

    pub fn take_evidence_events(&mut self) -> Vec<EventEnvelope> {
        std::mem::take(&mut self.evidence_events)
    }

    fn spawn_line_reader<T: std::io::Read + Send + 'static>(
        reader: T,
        tx: Sender<SidecarLine>,
        is_stderr: bool,
    ) {
        thread::spawn(move || {
            let reader = BufReader::new(reader);
            for line in reader.lines() {
                let Ok(line) = line else {
                    break;
                };
                let send_result = if is_stderr {
                    tx.send(SidecarLine::Stderr(line))
                } else {
                    tx.send(SidecarLine::Stdout(line))
                };
                if send_result.is_err() {
                    break;
                }
            }
        });
    }

    fn pump_sidecar_output(&mut self) {
        let Some(rx) = &self.line_rx else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(SidecarLine::Stdout(line)) => {
                    match serde_json::from_str::<EventEnvelope>(&line) {
                        Ok(candidate) => {
                            self.candidate_events.push(candidate);
                        }
                        Err(error) => {
                            self.evidence_events.push(self.evidence(
                                "transport.runtime.stdout_parse_failed",
                                json!({
                                    "transport_id": self.id,
                                    "line": line,
                                    "error": error.to_string()
                                }),
                            ));
                        }
                    }
                }
                Ok(SidecarLine::Stderr(line)) => {
                    self.evidence_events.push(self.evidence(
                        "transport.runtime.stderr",
                        json!({
                            "transport_id": self.id,
                            "line": line
                        }),
                    ));
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
    }

    fn check_child_status(&mut self) {
        let status = match self.child.as_mut().map(|child| child.try_wait()) {
            Some(Ok(Some(status))) => Some(status),
            _ => None,
        };
        if let Some(status) = status.filter(|status| !status.success()) {
            self.evidence_events.push(self.evidence(
                "transport.runtime.failed",
                json!({
                    "transport_id": self.id,
                    "reason": format!("nonzero child exit: {:?}", status.code())
                }),
            ));
        }
    }

    fn evidence(&self, topic: &str, payload: serde_json::Value) -> EventEnvelope {
        EventEnvelope::new(
            self.session_id
                .clone()
                .unwrap_or_else(|| "unknown-run".to_string()),
            topic,
            "station_transport",
            payload,
        )
    }
}

#[cfg(feature = "subprocess")]
impl Transport for SubprocessTransport {
    fn id(&self) -> &str {
        &self.id
    }

    fn start(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Started => Err(TransportError::AlreadyStarted),
            TransportState::Stopped => {
                let mut command = Command::new(&self.command);
                command
                    .args(&self.args)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                if let Some(working_dir) = &self.working_dir {
                    command.current_dir(working_dir);
                }
                if !self.env_allowlist.is_empty() {
                    command.env_clear();
                    for key in &self.env_allowlist {
                        if let Ok(value) = std::env::var(key) {
                            command.env(key, value);
                        }
                    }
                }
                let mut child = command
                    .spawn()
                    .map_err(|err| TransportError::Other(err.to_string()))?;
                self.child_stdin = child.stdin.take();

                let (tx, rx) = mpsc::channel();
                if let Some(stdout) = child.stdout.take() {
                    Self::spawn_line_reader(stdout, tx.clone(), false);
                }
                if let Some(stderr) = child.stderr.take() {
                    Self::spawn_line_reader(stderr, tx, true);
                }

                self.child = Some(child);
                self.line_rx = Some(rx);
                self.state = TransportState::Started;
                Ok(())
            }
        }
    }

    fn stop(&mut self) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                self.pump_sidecar_output();
                self.child_stdin = None;
                let mut exit_status: Option<std::process::ExitStatus> = None;
                let mut timed_out = false;
                if let Some(child) = self.child.as_mut() {
                    let start_wait = Instant::now();
                    loop {
                        if let Ok(Some(status)) = child.try_wait() {
                            exit_status = Some(status);
                            break;
                        }
                        if start_wait.elapsed() >= self.shutdown_timeout {
                            timed_out = true;
                            child
                                .kill()
                                .map_err(|err| TransportError::Other(err.to_string()))?;
                            let status = child
                                .wait()
                                .map_err(|err| TransportError::Other(err.to_string()))?;
                            exit_status = Some(status);
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
                self.pump_sidecar_output();
                if let Some(status) = exit_status.filter(|status| !status.success()) {
                    self.evidence_events.push(self.evidence(
                        "transport.runtime.failed",
                        json!({
                            "transport_id": self.id,
                            "reason": format!("nonzero child exit: {:?}", status.code())
                        }),
                    ));
                }
                if timed_out {
                    self.evidence_events.push(self.evidence(
                        "transport.runtime.timeout",
                        json!({
                            "transport_id": self.id,
                            "timeout_ms": self.shutdown_timeout.as_millis()
                        }),
                    ));
                    self.evidence_events.push(self.evidence(
                        "transport.runtime.killed",
                        json!({ "transport_id": self.id }),
                    ));
                }

                self.child = None;
                self.child_stdin = None;
                self.line_rx = None;
                self.state = TransportState::Stopped;
                Ok(())
            }
        }
    }

    fn send(&mut self, event: &EventEnvelope) -> Result<(), TransportError> {
        match self.state {
            TransportState::Stopped => Err(TransportError::NotStarted),
            TransportState::Started => {
                self.session_id = Some(event.session_id.clone());
                let json_line = serde_json::to_string(event)
                    .map_err(|err| TransportError::SendFailed(err.to_string()))?;
                let stdin = self
                    .child_stdin
                    .as_mut()
                    .ok_or_else(|| TransportError::SendFailed("child stdin unavailable".into()))?;
                stdin
                    .write_all(json_line.as_bytes())
                    .map_err(|err| TransportError::SendFailed(err.to_string()))?;
                stdin
                    .write_all(b"\n")
                    .map_err(|err| TransportError::SendFailed(err.to_string()))?;
                stdin
                    .flush()
                    .map_err(|err| TransportError::SendFailed(err.to_string()))?;

                self.pump_sidecar_output();
                self.check_child_status();
                Ok(())
            }
        }
    }

    fn drain_candidate_events(&mut self) -> Vec<EventEnvelope> {
        self.pump_sidecar_output();
        std::mem::take(&mut self.candidate_events)
    }

    fn drain_evidence_events(&mut self) -> Vec<EventEnvelope> {
        self.pump_sidecar_output();
        self.take_evidence_events()
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
    fn builds_local_transport_from_config() {
        let config = TransportConfig {
            id: "local-from-config".to_string(),
            kind: TransportKind::Local,
            endpoint: None,
            command: None,
            args: vec![],
            working_dir: None,
            env_allowlist: vec![],
            timeout_ms: None,
        };

        let transport = build_transport(&config).expect("local transport should build");
        assert_eq!(transport.id(), "local-from-config");
    }

    #[test]
    fn builds_null_transport_from_config() {
        let config = TransportConfig {
            id: "null-from-config".to_string(),
            kind: TransportKind::Null,
            endpoint: None,
            command: None,
            args: vec![],
            working_dir: None,
            env_allowlist: vec![],
            timeout_ms: None,
        };

        let transport = build_transport(&config).expect("null transport should build");
        assert_eq!(transport.id(), "null-from-config");
    }

    #[test]
    #[cfg(not(feature = "subprocess"))]
    fn rejects_subprocess_transport_until_feature_enabled() {
        let config = TransportConfig {
            id: "subprocess-from-config".to_string(),
            kind: TransportKind::Subprocess,
            endpoint: None,
            command: Some("/bin/cat".to_string()),
            args: vec![],
            working_dir: None,
            env_allowlist: vec![],
            timeout_ms: None,
        };

        let result = build_transport(&config);
        assert!(matches!(
            result,
            Err(TransportError::Other(msg)) if msg == "transport kind not enabled"
        ));
    }

    #[test]
    #[cfg(feature = "subprocess")]
    fn builds_subprocess_transport_when_feature_enabled() {
        let config = TransportConfig {
            id: "subprocess-from-config".to_string(),
            kind: TransportKind::Subprocess,
            endpoint: None,
            command: Some("/bin/cat".to_string()),
            args: vec![],
            working_dir: None,
            env_allowlist: vec![],
            timeout_ms: None,
        };

        let transport = build_transport(&config).expect("subprocess transport should build");
        assert_eq!(transport.id(), "subprocess-from-config");
    }

    #[test]
    fn transport_must_start_before_send() {
        let config = TransportConfig {
            id: "local-send-check".to_string(),
            kind: TransportKind::Local,
            endpoint: None,
            command: None,
            args: vec![],
            working_dir: None,
            env_allowlist: vec![],
            timeout_ms: None,
        };
        let mut transport = build_transport(&config).expect("local transport should build");

        let event = EventEnvelope::new("run-test", "test.topic", "test_plugin", json!({}));
        let result = transport.send(&event);

        assert!(matches!(result, Err(TransportError::NotStarted)));
    }

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
    #[cfg(feature = "subprocess")]
    #[cfg(unix)]
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
    #[cfg(feature = "subprocess")]
    #[cfg(unix)]
    fn subprocess_emits_timeout_and_kill_evidence() {
        let mut transport =
            SubprocessTransport::new("test-subprocess", "/bin/sleep", vec!["5".into()])
                .with_shutdown_timeout(std::time::Duration::from_millis(10));
        transport.start().expect("start should succeed");
        transport.stop().expect("stop should succeed");
        let evidence = transport.take_evidence_events();
        assert!(evidence
            .iter()
            .any(|event| event.topic == "transport.runtime.timeout"));
        assert!(evidence
            .iter()
            .any(|event| event.topic == "transport.runtime.killed"));
    }

    #[test]
    #[cfg(feature = "subprocess")]
    #[cfg(unix)]
    fn subprocess_stdout_candidates_require_policy_admission() {
        let mut transport = SubprocessTransport::new("test-subprocess", "/bin/cat", vec![]);
        transport.start().expect("start should succeed");
        let event = EventEnvelope::new("run-test", "test.topic", "test_plugin", json!({"n": 1}));
        transport.send(&event).expect("send should succeed");
        std::thread::sleep(std::time::Duration::from_millis(20));
        transport.send(&event).expect("send should succeed");
        transport.admit_candidate_events(|_| false);
        assert!(transport.candidate_events().is_empty());
        let evidence = transport.take_evidence_events();
        assert!(evidence
            .iter()
            .any(|evt| evt.topic == "policy.runtime.denied"));
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
        #[cfg(feature = "subprocess")]
        {
            let mut subprocess = SubprocessTransport::new("test", "/bin/cat", vec![]);
            subprocess.start().expect("first start");
            assert!(subprocess.start().is_err());
        }

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
