use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use super::error::RunError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSummary {
    pub id: String,
    pub kind: String,
    pub transport: String,
    pub version: String,
    pub artifact_hash: String,
    pub publishes: Vec<String>,
    pub subscribes: Vec<String>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaSummary {
    pub id: String,
    pub topic: String,
    pub version: String,
    pub schema_hash: String,
    pub required_fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactSummary {
    pub artifact_id: String,
    pub source: String,
    pub hash: String,
    pub algorithm: String,
    pub byte_len: usize,
    pub content_type: Option<String>,
    pub trace_id: Option<String>,
    pub parent_hash: Option<String>,
    pub schema_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    pub run_id: String,
    pub profile_name: String,
    pub status: RunStatus,
    pub event_log_path: String,
    pub manifest_path: String,
    pub started_at_ms: u128,
    pub completed_at_ms: Option<u128>,
    pub records_verified: usize,
    pub last_record_hash: Option<String>,
    pub replay_valid: bool,
    pub verification_error: Option<String>,
    pub plugin_count: usize,
    pub schema_count: usize,
    pub artifact_count: usize,
    pub plugins: Vec<PluginSummary>,
    pub schemas: Vec<SchemaSummary>,
    pub artifacts: Vec<ArtifactSummary>,
}

pub fn write_manifest_json(
    manifest: &RunManifest,
    path: impl AsRef<Path>,
) -> Result<(), RunError> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(manifest)?;
    fs::write(path, json)?;
    Ok(())
}

pub fn load_manifest_json(
    path: impl AsRef<Path>,
) -> Result<RunManifest, RunError> {
    let data = fs::read_to_string(path)?;
    let manifest = serde_json::from_str(&data)?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_status_serializes_as_snake_case() {
        let json = serde_json::to_string(&RunStatus::Completed).unwrap();
        assert_eq!(json, "\"completed\"");

        let json = serde_json::to_string(&RunStatus::Failed).unwrap();
        assert_eq!(json, "\"failed\"");
    }

    #[test]
    fn manifest_round_trips_json() {
        let id = ulid::Ulid::new().to_string();
        let path = std::env::temp_dir().join(format!("tnsr-run-manifest-{}.json", id));

        let manifest = RunManifest {
            run_id: "run-test".into(),
            profile_name: "default".into(),
            status: RunStatus::Completed,
            event_log_path: "events.jsonl".into(),
            manifest_path: "manifest.json".into(),
            started_at_ms: 1,
            completed_at_ms: Some(2),
            records_verified: 3,
            last_record_hash: Some("abc".into()),
            replay_valid: true,
            verification_error: None,
            plugin_count: 0,
            schema_count: 0,
            artifact_count: 0,
            plugins: vec![],
            schemas: vec![],
            artifacts: vec![],
        };

        write_manifest_json(&manifest, &path).unwrap();
        let loaded = load_manifest_json(&path).unwrap();

        assert_eq!(loaded.run_id, "run-test");
        assert_eq!(loaded.status, RunStatus::Completed);
        assert!(loaded.replay_valid);

        let _ = std::fs::remove_file(path);
    }
}
