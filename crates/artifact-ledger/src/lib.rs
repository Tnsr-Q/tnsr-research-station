use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct ArtifactRecord {
    pub artifact_id: String,
    pub source: String,
    pub hash: String,
    pub algorithm: String,
    pub byte_len: usize,
    pub content_type: Option<String>,
    pub trace_id: Option<String>,
    pub parent_hash: Option<String>,
    pub schema_hash: Option<String>,
    pub created_at_ms: u128,
}

#[derive(Debug, Clone)]
pub struct ArtifactRecordRequest {
    pub artifact_id: String,
    pub source: String,
    pub data: Vec<u8>,
    pub content_type: Option<String>,
    pub trace_id: Option<String>,
    pub parent_hash: Option<String>,
    pub schema_hash: Option<String>,
}

#[derive(Default)]
pub struct ArtifactLedger {
    records: Vec<ArtifactRecord>,
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

fn sha256(data: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(data)))
}

impl ArtifactLedger {
    pub fn record(
        &mut self,
        artifact_id: impl Into<String>,
        source: impl Into<String>,
        data: &[u8],
    ) -> String {
        let record = self.record_bytes(ArtifactRecordRequest {
            artifact_id: artifact_id.into(),
            source: source.into(),
            data: data.to_vec(),
            content_type: None,
            trace_id: None,
            parent_hash: None,
            schema_hash: None,
        });
        record.hash
    }

    pub fn record_bytes(&mut self, request: ArtifactRecordRequest) -> ArtifactRecord {
        let hash = sha256(&request.data);
        let record = ArtifactRecord {
            artifact_id: request.artifact_id,
            source: request.source,
            hash,
            algorithm: "sha256".to_string(),
            byte_len: request.data.len(),
            content_type: request.content_type,
            trace_id: request.trace_id,
            parent_hash: request.parent_hash,
            schema_hash: request.schema_hash,
            created_at_ms: now_ms(),
        };

        self.records.push(record.clone());
        record
    }

    pub fn records(&self) -> &[ArtifactRecord] {
        &self.records
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_known_vector() {
        let hash = sha256(b"abc");
        assert_eq!(
            hash,
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
