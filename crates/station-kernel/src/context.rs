use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use artifact_ledger::ArtifactLedger;
use plugin_registry::{load_plugin_manifest_json, PluginRegistry};
use runtime_core::{EventBus, EventEnvelope};
use station_replay::JsonlReplayLog;
use station_run::{load_run_profile_json, RunProfile};
use station_schema::{load_schema_json, SchemaRegistry};
use station_supervisor::StationSupervisor;
use station_transport::Transport;

use crate::errors::KernelError;

pub struct KernelContext {
    pub profile: RunProfile,
    pub profile_dir: PathBuf,
    pub started_at_ms: u128,
    pub run_dir: PathBuf,
    pub events_path: PathBuf,
    pub manifest_path: PathBuf,
    pub registry: PluginRegistry,
    pub schemas: SchemaRegistry,
    pub ledger: ArtifactLedger,
    pub bus: EventBus,
    pub supervisor: StationSupervisor,
    pub replay: JsonlReplayLog,
    pub transports: HashMap<String, Box<dyn Transport>>,
    pub inflight_by_plugin: HashMap<String, HashMap<String, EventEnvelope>>,
}

impl KernelContext {
    pub fn from_profile_path(path: impl AsRef<Path>) -> Result<Self, KernelError> {
        let profile_path = path.as_ref().to_path_buf();
        let profile = load_run_profile_json(&profile_path)?;
        let profile_dir = profile_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let supervisor =
            StationSupervisor::new_with_permissions(&profile.name, profile.permissions.clone());
        let run_dir = PathBuf::from("runs").join(&supervisor.session.run_id);
        let events_path = run_dir.join("events.jsonl");
        let manifest_path = run_dir.join("manifest.json");

        Ok(Self {
            profile,
            profile_dir,
            started_at_ms: now_ms()?,
            run_dir,
            events_path: events_path.clone(),
            manifest_path,
            registry: PluginRegistry::default(),
            schemas: SchemaRegistry::default(),
            ledger: ArtifactLedger::default(),
            bus: EventBus::new(),
            supervisor,
            replay: JsonlReplayLog::open(events_path)?,
            transports: HashMap::new(),
            inflight_by_plugin: HashMap::new(),
        })
    }

    pub fn resolve_profile_path(&self, path: &str) -> PathBuf {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return path;
        }

        let profile_relative = self.profile_dir.join(&path);
        if profile_relative.exists() {
            return profile_relative;
        }

        if let Some(profile_parent) = self.profile_dir.parent() {
            let workspace_relative = profile_parent.join(&path);
            if workspace_relative.exists() {
                return workspace_relative;
            }
        }

        PathBuf::from(".").join(path)
    }
}

fn now_ms() -> Result<u128, std::time::SystemTimeError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
}

pub fn load_profile_plugin_manifest(
    context: &KernelContext,
    path: &str,
) -> Result<plugin_registry::PluginManifest, KernelError> {
    Ok(load_plugin_manifest_json(
        context.resolve_profile_path(path),
    )?)
}

pub fn load_profile_schema(
    context: &KernelContext,
    path: &str,
) -> Result<station_schema::PayloadSchema, KernelError> {
    Ok(load_schema_json(context.resolve_profile_path(path))?)
}
