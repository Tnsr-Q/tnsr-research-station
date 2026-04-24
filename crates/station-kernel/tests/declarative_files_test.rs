use plugin_registry::{load_plugin_manifest_json, PluginKind, PluginRegistry};
use station_run::load_run_profile_json;
use station_schema::{load_schema_json, SchemaRegistry};
use std::path::PathBuf;

/// Get the repository root directory (assumes tests run from the workspace root)
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn default_profile_loads() {
    let profile_path = repo_root().join("profiles/default.profile.json");
    let profile = load_run_profile_json(&profile_path).expect("Failed to load default profile");

    assert_eq!(profile.name, "default");
    assert!(!profile.plugin_manifests.is_empty(), "Profile should reference plugin manifests");
    assert!(!profile.schema_files.is_empty(), "Profile should reference schema files");
}

#[test]
fn quantum_plugin_manifest_loads() {
    let plugin_path = repo_root().join("plugins/quantum-hybrid.plugin.json");
    let manifest = load_plugin_manifest_json(&plugin_path).expect("Failed to load quantum plugin manifest");

    assert_eq!(manifest.id, "adapter_quantum");
    assert_eq!(manifest.kind, PluginKind::Compute);
    assert!(!manifest.publishes.is_empty(), "Quantum plugin should publish topics");
}

#[test]
fn rag_plugin_manifest_loads() {
    let plugin_path = repo_root().join("plugins/rag.plugin.json");
    let manifest = load_plugin_manifest_json(&plugin_path).expect("Failed to load RAG plugin manifest");

    assert_eq!(manifest.id, "adapter_rag");
    assert_eq!(manifest.kind, PluginKind::Semantic);
    assert!(!manifest.publishes.is_empty(), "RAG plugin should publish topics");
}

#[test]
fn quantum_state_schema_loads() {
    let schema_path = repo_root().join("schemas/quantum-state.schema.json");
    let schema = load_schema_json(&schema_path).expect("Failed to load quantum state schema");

    assert_eq!(schema.id, "tnsr.quantum.state.v1");
    assert_eq!(schema.topic, "quantum.state");
    assert_eq!(schema.required_fields.len(), 3, "Schema should have 3 required fields");
    assert!(!schema.schema_hash.is_empty(), "Schema should have a hash");
}

#[test]
fn profile_references_existing_plugins() {
    let profile_path = repo_root().join("profiles/default.profile.json");
    let profile = load_run_profile_json(&profile_path).expect("Failed to load default profile");

    let root = repo_root();

    for plugin_path in &profile.plugin_manifests {
        let full_path = root.join(plugin_path);
        assert!(
            full_path.exists(),
            "Plugin manifest {} referenced by profile does not exist",
            plugin_path
        );

        let manifest = load_plugin_manifest_json(&full_path)
            .expect(&format!("Failed to load plugin manifest {}", plugin_path));
        assert!(!manifest.id.is_empty(), "Plugin manifest should have an id");
    }
}

#[test]
fn profile_references_existing_schemas() {
    let profile_path = repo_root().join("profiles/default.profile.json");
    let profile = load_run_profile_json(&profile_path).expect("Failed to load default profile");

    let root = repo_root();

    for schema_path in &profile.schema_files {
        let full_path = root.join(schema_path);
        assert!(
            full_path.exists(),
            "Schema file {} referenced by profile does not exist",
            schema_path
        );

        let schema = load_schema_json(&full_path)
            .expect(&format!("Failed to load schema {}", schema_path));
        assert!(!schema.topic.is_empty(), "Schema should have a topic");
    }
}

#[test]
fn quantum_schema_has_expected_fields() {
    let schema_path = repo_root().join("schemas/quantum-state.schema.json");
    let schema = load_schema_json(&schema_path).expect("Failed to load quantum state schema");

    let mut registry = SchemaRegistry::default();
    registry.register(schema).expect("Failed to register schema");

    let schema = registry.schema_for_topic("quantum.state").expect("Schema should be registered");

    // Verify the schema has the expected fields with correct types
    assert_eq!(schema.required_fields.len(), 3);

    let field_names: Vec<&String> = schema.required_fields.iter().map(|(name, _)| name).collect();
    assert!(field_names.contains(&&"state_dim".to_string()));
    assert!(field_names.contains(&&"collapse_ratio".to_string()));
    assert!(field_names.contains(&&"euler_characteristic".to_string()));
}

#[test]
fn proto_file_exists() {
    let proto_path = repo_root().join("proto/tnsr_event_v1.proto");
    assert!(proto_path.exists(), "Protocol buffer definition should exist");

    let content = std::fs::read_to_string(&proto_path).expect("Failed to read proto file");
    assert!(content.contains("EventEnvelope"), "Proto should define EventEnvelope");
    assert!(content.contains("version"), "Proto should include version field");
    assert!(content.contains("parent_id"), "Proto should include parent_id field");
    assert!(content.contains("created_at_ms"), "Proto should include created_at_ms field");
    assert!(content.contains("schema_hash"), "Proto should include schema_hash field");
}

#[test]
fn all_profile_plugins_can_be_registered() {
    let profile_path = repo_root().join("profiles/default.profile.json");
    let profile = load_run_profile_json(&profile_path).expect("Failed to load default profile");

    let root = repo_root();
    let mut registry = PluginRegistry::default();

    for plugin_path in &profile.plugin_manifests {
        let full_path = root.join(plugin_path);
        let manifest = load_plugin_manifest_json(&full_path)
            .expect(&format!("Failed to load plugin manifest {}", plugin_path));

        registry.register(manifest).expect(&format!(
            "Failed to register plugin from {}",
            plugin_path
        ));
    }

    // Should have registered at least 2 plugins (quantum and rag)
    assert!(
        registry.plugins().len() >= 2,
        "Should have registered at least 2 plugins"
    );
}

#[test]
fn all_profile_schemas_can_be_registered() {
    let profile_path = repo_root().join("profiles/default.profile.json");
    let profile = load_run_profile_json(&profile_path).expect("Failed to load default profile");

    let root = repo_root();
    let mut registry = SchemaRegistry::default();

    for schema_path in &profile.schema_files {
        let full_path = root.join(schema_path);
        let schema = load_schema_json(&full_path)
            .expect(&format!("Failed to load schema {}", schema_path));

        registry.register(schema).expect(&format!(
            "Failed to register schema from {}",
            schema_path
        ));
    }

    // Should have registered at least 2 schemas
    assert!(
        registry.schemas().len() >= 2,
        "Should have registered at least 2 schemas"
    );
}
