use super::error::RunError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunProfile {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub plugin_manifests: Vec<String>,
    #[serde(default)]
    pub schema_files: Vec<String>,
    #[serde(default)]
    pub permissions: RuntimePermissions,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimePermissions {
    #[serde(default)]
    pub allow_network: bool,

    #[serde(default)]
    pub allow_filesystem: bool,

    #[serde(default)]
    pub allow_gpu: bool,

    #[serde(default)]
    pub allow_projection_only: bool,

    #[serde(default)]
    pub allow_nondeterministic: bool,
}

pub fn load_run_profile_json(path: impl AsRef<Path>) -> Result<RunProfile, RunError> {
    let data = fs::read_to_string(path)?;
    let profile = serde_json::from_str(&data)?;
    Ok(profile)
}

pub fn write_run_profile_json(
    profile: &RunProfile,
    path: impl AsRef<Path>,
) -> Result<(), RunError> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(profile)?;
    fs::write(path, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_profile_round_trips_json() {
        let id = ulid::Ulid::new().to_string();
        let path = std::env::temp_dir().join(format!("tnsr-run-profile-{}.json", id));

        let profile = RunProfile {
            name: "test-profile".into(),
            description: Some("Test profile description".into()),
            plugin_manifests: vec![
                "plugins/quantum.plugin.json".into(),
                "plugins/rag.plugin.json".into(),
            ],
            schema_files: vec!["schemas/quantum.schema.json".into()],
            permissions: RuntimePermissions::default(),
        };

        write_run_profile_json(&profile, &path).unwrap();
        let loaded = load_run_profile_json(&path).unwrap();

        assert_eq!(loaded.name, "test-profile");
        assert_eq!(loaded.description, Some("Test profile description".into()));
        assert_eq!(loaded.plugin_manifests.len(), 2);
        assert_eq!(loaded.schema_files.len(), 1);
        assert!(!loaded.permissions.allow_network);
        assert!(!loaded.permissions.allow_gpu);

        let _ = std::fs::remove_file(path);
    }
}
