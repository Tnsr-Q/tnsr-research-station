use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct ArtifactRecord {
    pub artifact_id: String,
    pub source: String,
    pub hash: String,
    pub created_at_ms: u128,
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
        let hash = sha256(data);
        self.records.push(ArtifactRecord {
            artifact_id: artifact_id.into(),
            source: source.into(),
            hash: hash.clone(),
            created_at_ms: now_ms(),
        });
        hash
    }

    pub fn records(&self) -> &[ArtifactRecord] {
        &self.records
    }
}
