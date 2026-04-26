use std::time::{SystemTime, UNIX_EPOCH};

use artifact_ledger::ArtifactLedger;
use plugin_registry::{PluginKind, PluginRegistry, TransportKind};
use station_replay::JsonlReplayLog;
use station_run::{
    write_manifest_json, ArtifactSummary, CapabilityClaimSummary, PluginSummary, RunManifest,
    RunStatus, SchemaSummary,
};
use station_schema::{FieldType, SchemaRegistry};

use crate::{context::KernelContext, errors::KernelError};

pub fn seal_run(
    context: KernelContext,
    status: RunStatus,
    failure_reason: Option<String>,
) -> Result<RunManifest, KernelError> {
    let manifest = build_run_manifest(
        BuildManifestInput {
            run_id: context.supervisor.session.run_id,
            profile_name: context.supervisor.session.profile_name,
            status,
            started_at_ms: context.started_at_ms,
            failure_reason,
        },
        &context.events_path,
        &context.registry,
        &context.schemas,
        &context.ledger,
    )?;

    write_manifest_json(&manifest, &context.manifest_path)?;

    Ok(manifest)
}

fn now_ms() -> Result<u128, std::time::SystemTimeError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
}

fn plugin_kind_wire(kind: &PluginKind) -> &'static str {
    match kind {
        PluginKind::Compute => "compute",
        PluginKind::Renderer => "renderer",
        PluginKind::Bridge => "bridge",
        PluginKind::Memory => "memory",
        PluginKind::Agent => "agent",
        PluginKind::Verifier => "verifier",
        PluginKind::Semantic => "semantic",
    }
}

fn transport_kind_wire(kind: &TransportKind) -> &'static str {
    match kind {
        TransportKind::Local => "local",
        TransportKind::Wasm => "wasm",
        TransportKind::WebSocket => "websocket",
        TransportKind::Grpc => "grpc",
        TransportKind::ConnectRpc => "connectrpc",
        TransportKind::Pyro5 => "pyro5",
        TransportKind::Subprocess => "subprocess",
        TransportKind::Ffi => "ffi",
    }
}

fn build_plugin_summaries(registry: &PluginRegistry) -> Vec<PluginSummary> {
    let mut plugins: Vec<PluginSummary> = registry
        .plugins()
        .into_iter()
        .map(|m| PluginSummary {
            id: m.id.clone(),
            kind: plugin_kind_wire(&m.kind).into(),
            transport: transport_kind_wire(&m.transport).into(),
            version: m.version.clone(),
            artifact_hash: m.artifact_hash.clone(),
            sidecar_args_hash: m.transport_config.sidecar_args_hash.clone(),
            publishes: m.publishes.clone(),
            subscribes: m.subscribes.clone(),
            capabilities: m.capabilities.clone(),
            capability_claims: m
                .capability_claims
                .iter()
                .map(|c| CapabilityClaimSummary {
                    name: c.name.clone(),
                    enabled: c.enabled,
                    required: c.required,
                    deterministic: c.deterministic,
                    replay_safe: c.replay_safe,
                    projection_only: c.projection_only,
                    emits_artifacts: c.emits_artifacts,
                    requires_network: c.requires_network,
                    requires_filesystem: c.requires_filesystem,
                    requires_gpu: c.requires_gpu,
                    max_runtime_ms: c.max_runtime_ms,
                })
                .collect(),
        })
        .collect();
    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    plugins
}

fn build_schema_summaries(schemas: &SchemaRegistry) -> Vec<SchemaSummary> {
    let mut schema_summaries: Vec<SchemaSummary> = schemas
        .schemas()
        .into_iter()
        .map(|s| SchemaSummary {
            id: s.id.clone(),
            topic: s.topic.clone(),
            version: s.version.clone(),
            schema_hash: s.schema_hash.clone(),
            required_fields: s
                .required_fields
                .iter()
                .map(|(name, field_type)| {
                    let type_str = match field_type {
                        FieldType::Integer => "Integer",
                        FieldType::Number => "Number",
                        FieldType::String => "String",
                        FieldType::Boolean => "Boolean",
                        FieldType::Object => "Object",
                        FieldType::Array => "Array",
                    };
                    (name.clone(), type_str.to_string())
                })
                .collect(),
        })
        .collect();
    schema_summaries.sort_by(|a, b| a.topic.cmp(&b.topic));
    schema_summaries
}

fn build_artifact_summaries(ledger: &ArtifactLedger) -> Vec<ArtifactSummary> {
    let mut artifacts: Vec<ArtifactSummary> = ledger
        .records()
        .iter()
        .map(|r| ArtifactSummary {
            artifact_id: r.artifact_id.clone(),
            source: r.source.clone(),
            hash: r.hash.clone(),
            algorithm: r.algorithm.clone(),
            byte_len: r.byte_len,
            content_type: r.content_type.clone(),
            trace_id: r.trace_id.clone(),
            parent_hash: r.parent_hash.clone(),
            schema_hash: r.schema_hash.clone(),
        })
        .collect();
    artifacts.sort_by(|a, b| a.artifact_id.cmp(&b.artifact_id));
    artifacts
}

struct BuildManifestInput {
    run_id: String,
    profile_name: String,
    status: RunStatus,
    started_at_ms: u128,
    failure_reason: Option<String>,
}

fn build_run_manifest(
    input: BuildManifestInput,
    events_path: &std::path::Path,
    registry: &PluginRegistry,
    schemas: &SchemaRegistry,
    ledger: &ArtifactLedger,
) -> Result<RunManifest, KernelError> {
    let (records_verified, last_hash, verification_error) =
        match JsonlReplayLog::verify_chain(events_path) {
            Ok(report) => (report.records_verified, report.last_record_hash, None),
            Err(err) => (0, None, Some(err.to_string())),
        };

    let plugins = build_plugin_summaries(registry);
    let schema_summaries = build_schema_summaries(schemas);
    let artifacts = build_artifact_summaries(ledger);

    let replay_valid = verification_error.is_none();

    Ok(RunManifest {
        run_id: input.run_id,
        profile_name: input.profile_name,
        status: input.status,
        event_log_path: "events.jsonl".into(),
        manifest_path: "manifest.json".into(),
        started_at_ms: input.started_at_ms,
        completed_at_ms: Some(now_ms()?),
        records_verified,
        last_record_hash: last_hash,
        replay_valid,
        verification_error,
        failure_reason: input.failure_reason,
        plugin_count: plugins.len(),
        schema_count: schema_summaries.len(),
        artifact_count: artifacts.len(),
        plugins,
        schemas: schema_summaries,
        artifacts,
    })
}
