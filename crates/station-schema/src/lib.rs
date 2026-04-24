use runtime_core::EventEnvelope;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PayloadSchema {
    pub id: String,
    pub topic: String,
    pub version: String,
    pub required_fields: Vec<String>,
    pub schema_hash: String,
}

#[derive(Default)]
pub struct SchemaRegistry {
    by_topic: HashMap<String, PayloadSchema>,
}

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("schema already registered for topic: {0}")]
    DuplicateTopic(String),

    #[error("no schema registered for topic: {0}")]
    MissingSchema(String),

    #[error("event topic {topic} missing required field: {field}")]
    MissingRequiredField { topic: String, field: String },

    #[error("event schema hash mismatch for topic {topic}: expected {expected}, found {found:?}")]
    SchemaHashMismatch {
        topic: String,
        expected: String,
        found: Option<String>,
    },
}

impl PayloadSchema {
    pub fn new(
        id: impl Into<String>,
        topic: impl Into<String>,
        version: impl Into<String>,
        required_fields: Vec<String>,
    ) -> Self {
        let id = id.into();
        let topic = topic.into();
        let version = version.into();

        let canonical = serde_json::json!({
            "id": id,
            "topic": topic,
            "version": version,
            "required_fields": required_fields,
        });

        let bytes = serde_json::to_vec(&canonical).expect("schema serialization cannot fail");
        let schema_hash = format!("sha256:{}", hex::encode(Sha256::digest(bytes)));

        Self {
            id,
            topic,
            version,
            required_fields,
            schema_hash,
        }
    }
}

impl SchemaRegistry {
    pub fn register(&mut self, schema: PayloadSchema) -> Result<(), SchemaError> {
        if self.by_topic.contains_key(&schema.topic) {
            return Err(SchemaError::DuplicateTopic(schema.topic));
        }

        self.by_topic.insert(schema.topic.clone(), schema);
        Ok(())
    }

    pub fn schema_for_topic(&self, topic: &str) -> Option<&PayloadSchema> {
        self.by_topic.get(topic)
    }

    pub fn attach_schema_hash(&self, event: &mut EventEnvelope) -> Result<(), SchemaError> {
        let schema = self
            .schema_for_topic(&event.topic)
            .ok_or_else(|| SchemaError::MissingSchema(event.topic.clone()))?;

        event.schema_hash = Some(schema.schema_hash.clone());
        Ok(())
    }

    pub fn validate(&self, event: &EventEnvelope) -> Result<(), SchemaError> {
        let schema = self
            .schema_for_topic(&event.topic)
            .ok_or_else(|| SchemaError::MissingSchema(event.topic.clone()))?;

        if event.schema_hash.as_deref() != Some(schema.schema_hash.as_str()) {
            return Err(SchemaError::SchemaHashMismatch {
                topic: event.topic.clone(),
                expected: schema.schema_hash.clone(),
                found: event.schema_hash.clone(),
            });
        }

        for field in &schema.required_fields {
            if !has_field(&event.payload, field) {
                return Err(SchemaError::MissingRequiredField {
                    topic: event.topic.clone(),
                    field: field.clone(),
                });
            }
        }

        Ok(())
    }

    pub fn schema_count(&self) -> usize {
        self.by_topic.len()
    }

    pub fn schemas(&self) -> Vec<&PayloadSchema> {
        self.by_topic.values().collect()
    }
}

fn has_field(payload: &Value, field: &str) -> bool {
    match payload {
        Value::Object(map) => map.contains_key(field),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_core::EventEnvelope;
    use serde_json::json;

    #[test]
    fn test_schema_hash_is_attached_and_validates() {
        let mut registry = SchemaRegistry::default();

        let schema = PayloadSchema::new(
            "tnsr.quantum.state.v1",
            "quantum.state",
            "1",
            vec![
                "state_dim".to_string(),
                "collapse_ratio".to_string(),
                "euler_characteristic".to_string(),
            ],
        );

        registry.register(schema).expect("register schema");

        let mut event = EventEnvelope::new(
            "run-test",
            "quantum.state",
            "adapter_quantum",
            json!({
                "state_dim": 16,
                "collapse_ratio": 0.42,
                "euler_characteristic": 8
            }),
        );

        registry
            .attach_schema_hash(&mut event)
            .expect("attach schema hash");

        registry.validate(&event).expect("validate event");
    }

    #[test]
    fn test_missing_required_field_fails() {
        let mut registry = SchemaRegistry::default();

        let schema = PayloadSchema::new(
            "tnsr.quantum.state.v1",
            "quantum.state",
            "1",
            vec!["state_dim".to_string(), "collapse_ratio".to_string()],
        );

        registry.register(schema).expect("register schema");

        let mut event = EventEnvelope::new(
            "run-test",
            "quantum.state",
            "adapter_quantum",
            json!({
                "state_dim": 16
            }),
        );

        registry
            .attach_schema_hash(&mut event)
            .expect("attach schema hash");

        let err = registry
            .validate(&event)
            .expect_err("missing collapse_ratio should fail");

        assert!(matches!(err, SchemaError::MissingRequiredField { .. }));
    }

    #[test]
    fn test_schema_hash_mismatch_fails() {
        let mut registry = SchemaRegistry::default();

        let schema = PayloadSchema::new(
            "tnsr.quantum.state.v1",
            "quantum.state",
            "1",
            vec!["state_dim".to_string()],
        );

        registry.register(schema).expect("register schema");

        let mut event = EventEnvelope::new(
            "run-test",
            "quantum.state",
            "adapter_quantum",
            json!({ "state_dim": 16 }),
        );

        event.schema_hash = Some("sha256:bad".to_string());

        let err = registry
            .validate(&event)
            .expect_err("bad schema hash should fail");

        assert!(matches!(err, SchemaError::SchemaHashMismatch { .. }));
    }
}
