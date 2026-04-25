use std::sync::mpsc::RecvError;

use plugin_registry::PluginError;
use station_policy::PolicyError;
use station_replay::ReplayError;
use station_run::RunError;
use station_schema::SchemaError;
use station_supervisor::SupervisorError;
use station_transport::TransportError;

#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("run error: {0}")]
    Run(#[from] RunError),

    #[error("plugin error: {0}")]
    Plugin(#[from] PluginError),

    #[error("schema error: {0}")]
    Schema(#[from] SchemaError),

    #[error("supervisor error: {0}")]
    Supervisor(#[from] SupervisorError),

    #[error("policy error: {0}")]
    Policy(#[from] PolicyError),

    #[error("transport error: {0}")]
    Transport(#[from] TransportError),

    #[error("replay error: {0}")]
    Replay(#[from] ReplayError),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("time error: {0}")]
    Time(#[from] std::time::SystemTimeError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("channel receive error: {0}")]
    Recv(#[from] RecvError),

    #[error("plugin registration rejected: {0}")]
    PluginRegistration(String),

    #[error("event publication failed for {failed} subscribers")]
    PublicationFailed { failed: usize },
}
