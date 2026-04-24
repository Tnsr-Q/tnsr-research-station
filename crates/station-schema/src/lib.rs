use runtime_core::EventEnvelope;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FieldType {
    Integer,
    Number,
    String,
}

#[derive(Debug, Clone)]
pub struct PayloadSchema {
    pub id: String,
    pub topic: String,
    pub version: String,
    pub required_fields: Vec<(String, FieldType)>,
    pub schema_hash: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SchemaFile {
    id: String,
    topic: String,
    version: String,
    required_fields: Vec<(String, String)>,
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

    #[error("event topic {topic} field {field} has wrong type: expected {expected:?}, found {found:?}")]
    WrongFieldType {
        topic: String,
        field: String,
        expected: FieldType,
        found: String,
    },

    #[error("event schema hash mismatch for topic {topic}: expected {expected}, found {found:?}")]
    SchemaHashMismatch {
        topic: String,
        expected: String,
        found: Option<String>,
    },
}

impl PayloadSchema {
    pub fn new(id: impl Into<String>, topic: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            topic: topic.into(),
            version: version.into(),
            required_fields: Vec::new(),
            schema_hash: String::new(),
        }
    }

    pub fn required(mut self, field: impl Into<String>, field_type: FieldType) -> Self {
        self.required_fields.push((field.into(), field_type));
        self
    }

    pub fn build(mut self) -> Self {
        let canonical = serde_json::json!({
            "id": self.id,
            "topic": self.topic,
            "version": self.version,
            "required_fields": self.required_fields,
        });

        let bytes = serde_json::to_vec(&canonical).expect("schema serialization cannot fail");
        self.schema_hash = format!("sha256:{}", hex::encode(Sha256::digest(bytes)));
        self
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

        for (field, expected_type) in &schema.required_fields {
            match get_field(&event.payload, field) {
                None => {
                    return Err(SchemaError::MissingRequiredField {
                        topic: event.topic.clone(),
                        field: field.clone(),
                    });
                }
                Some(value) => {
                    if !matches_type(value, expected_type) {
                        return Err(SchemaError::WrongFieldType {
                            topic: event.topic.clone(),
                            field: field.clone(),
                            expected: expected_type.clone(),
                            found: type_name(value),
                        });
                    }
                }
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

fn get_field<'a>(payload: &'a Value, field: &str) -> Option<&'a Value> {
    match payload {
        Value::Object(map) => map.get(field),
        _ => None,
    }
}

fn matches_type(value: &Value, expected_type: &FieldType) -> bool {
    match expected_type {
        FieldType::Integer => value.is_i64() || value.is_u64(),
        FieldType::Number => value.is_f64() || value.is_i64() || value.is_u64(),
        FieldType::String => value.is_string(),
    }
}

fn type_name(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "boolean".to_string(),
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "integer".to_string()
            } else {
                "number".to_string()
            }
        }
        Value::String(_) => "string".to_string(),
        Value::Array(_) => "array".to_string(),
        Value::Object(_) => "object".to_string(),
    }
}

pub fn load_schema_json(path: impl AsRef<Path>) -> Result<PayloadSchema, SchemaError> {
    let data = fs::read_to_string(path)
        .map_err(|e| SchemaError::MissingSchema(format!("Failed to read schema file: {}", e)))?;
    let schema_file: SchemaFile = serde_json::from_str(&data)
        .map_err(|e| SchemaError::MissingSchema(format!("Failed to parse schema JSON: {}", e)))?;

    let mut builder = PayloadSchema::new(&schema_file.id, &schema_file.topic, &schema_file.version);

    for (field_name, field_type_str) in schema_file.required_fields {
        let field_type = match field_type_str.as_str() {
            "Integer" => FieldType::Integer,
            "Number" => FieldType::Number,
            "String" => FieldType::String,
            _ => return Err(SchemaError::MissingSchema(format!(
                "Unknown field type: {}",
                field_type_str
            ))),
        };
        builder = builder.required(field_name, field_type);
    }

    Ok(builder.build())
}

pub fn write_schema_json(
    schema: &PayloadSchema,
    path: impl AsRef<Path>,
) -> Result<(), SchemaError> {
    let schema_file = SchemaFile {
        id: schema.id.clone(),
        topic: schema.topic.clone(),
        version: schema.version.clone(),
        required_fields: schema
            .required_fields
            .iter()
            .map(|(name, field_type)| {
                let type_str = match field_type {
                    FieldType::Integer => "Integer",
                    FieldType::Number => "Number",
                    FieldType::String => "String",
                };
                (name.clone(), type_str.to_string())
            })
            .collect(),
    };

    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)
            .map_err(|e| SchemaError::MissingSchema(format!("Failed to create directory: {}", e)))?;
    }

    let json = serde_json::to_string_pretty(&schema_file)
        .map_err(|e| SchemaError::MissingSchema(format!("Failed to serialize schema: {}", e)))?;

    fs::write(path, json)
        .map_err(|e| SchemaError::MissingSchema(format!("Failed to write schema file: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_core::EventEnvelope;
    use serde_json::json;

    #[test]
    fn test_schema_hash_is_attached_and_validates() {
        let mut registry = SchemaRegistry::default();

        let schema = PayloadSchema::new("tnsr.quantum.state.v1", "quantum.state", "1")
            .required("state_dim", FieldType::Integer)
            .required("collapse_ratio", FieldType::Number)
            .required("euler_characteristic", FieldType::Integer)
            .build();

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

        let schema = PayloadSchema::new("tnsr.quantum.state.v1", "quantum.state", "1")
            .required("state_dim", FieldType::Integer)
            .required("collapse_ratio", FieldType::Number)
            .build();

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

        let schema = PayloadSchema::new("tnsr.quantum.state.v1", "quantum.state", "1")
            .required("state_dim", FieldType::Integer)
            .build();

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

    #[test]
    fn test_wrong_type_fails() {
        let mut registry = SchemaRegistry::default();

        let schema = PayloadSchema::new("tnsr.quantum.state.v1", "quantum.state", "1")
            .required("state_dim", FieldType::Integer)
            .required("collapse_ratio", FieldType::Number)
            .build();

        registry.register(schema).expect("register schema");

        // Provide a string instead of an integer for state_dim
        let mut event = EventEnvelope::new(
            "run-test",
            "quantum.state",
            "adapter_quantum",
            json!({
                "state_dim": "not_an_integer",
                "collapse_ratio": 0.42
            }),
        );

        registry
            .attach_schema_hash(&mut event)
            .expect("attach schema hash");

        let err = registry
            .validate(&event)
            .expect_err("wrong type should fail");

        assert!(matches!(err, SchemaError::WrongFieldType { .. }));
    }

    #[test]
    fn test_schema_hash_changes_when_field_types_change() {
        let schema1 = PayloadSchema::new("tnsr.quantum.state.v1", "quantum.state", "1")
            .required("state_dim", FieldType::Integer)
            .build();

        let schema2 = PayloadSchema::new("tnsr.quantum.state.v1", "quantum.state", "1")
            .required("state_dim", FieldType::Number)
            .build();

        assert_ne!(
            schema1.schema_hash, schema2.schema_hash,
            "schema hash should change when field types change"
        );
    }

    #[test]
    fn test_valid_payload_passes() {
        let mut registry = SchemaRegistry::default();

        let schema = PayloadSchema::new("tnsr.quantum.state.v1", "quantum.state", "1")
            .required("state_dim", FieldType::Integer)
            .required("collapse_ratio", FieldType::Number)
            .required("euler_characteristic", FieldType::Integer)
            .build();

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

        // This should pass without error
        registry.validate(&event).expect("valid payload should pass");
    }

    #[test]
    fn test_schema_round_trips_json() {
        let schema = PayloadSchema::new("tnsr.quantum.state.v1", "quantum.state", "1")
            .required("state_dim", FieldType::Integer)
            .required("collapse_ratio", FieldType::Number)
            .required("euler_characteristic", FieldType::Integer)
            .build();

        let path = std::env::temp_dir().join("tnsr-schema-test.json");

        write_schema_json(&schema, &path).unwrap();
        let loaded = load_schema_json(&path).unwrap();

        assert_eq!(loaded.id, "tnsr.quantum.state.v1");
        assert_eq!(loaded.topic, "quantum.state");
        assert_eq!(loaded.version, "1");
        assert_eq!(loaded.required_fields.len(), 3);
        assert_eq!(loaded.schema_hash, schema.schema_hash);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_string_field_validates() {
        let mut registry = SchemaRegistry::default();

        let schema = PayloadSchema::new("tnsr.rag.result.v1", "rag.result", "1")
            .required("message", FieldType::String)
            .build();

        registry.register(schema).expect("register schema");

        let mut event = EventEnvelope::new(
            "run-test",
            "rag.result",
            "adapter_rag",
            json!({
                "message": "test message"
            }),
        );

        registry
            .attach_schema_hash(&mut event)
            .expect("attach schema hash");

        registry.validate(&event).expect("valid string field should pass");
    }

    #[test]
    fn test_string_field_wrong_type_fails() {
        let mut registry = SchemaRegistry::default();

        let schema = PayloadSchema::new("tnsr.rag.result.v1", "rag.result", "1")
            .required("message", FieldType::String)
            .build();

        registry.register(schema).expect("register schema");

        let mut event = EventEnvelope::new(
            "run-test",
            "rag.result",
            "adapter_rag",
            json!({
                "message": 123
            }),
        );

        registry
            .attach_schema_hash(&mut event)
            .expect("attach schema hash");

        let err = registry
            .validate(&event)
            .expect_err("integer instead of string should fail");

        assert!(matches!(err, SchemaError::WrongFieldType { .. }));
    }
}
