use runtime_core::EventEnvelope;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::fd::FromRawFd;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("replay chain verification failed at index {index}: {reason}")]
    VerificationFailed { index: u64, reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayRecord {
    pub index: u64,
    pub event: EventEnvelope,
    pub event_hash: String,
    pub previous_record_hash: Option<String>,
    pub record_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayVerificationReport {
    pub records_verified: usize,
    pub last_record_hash: Option<String>,
}

pub struct JsonlReplayLog {
    path: PathBuf,
    writer: BufWriter<File>,
    next_index: u64,
    last_record_hash: Option<String>,
}

impl JsonlReplayLog {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ReplayError> {
        let path = path.as_ref().to_path_buf();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let existing_records = Self::read_all_records(&path)?;
        let next_index = existing_records.last().map_or(0, |r| r.index + 1);
        let last_record_hash = existing_records.last().map(|r| r.record_hash.clone());

        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        Ok(Self {
            path,
            writer: BufWriter::new(file),
            next_index,
            last_record_hash,
        })
    }

    pub fn append_record(&mut self, event: &EventEnvelope) -> Result<ReplayRecord, ReplayError> {
        let record = ReplayRecord::new(
            self.next_index,
            event.clone(),
            self.last_record_hash.clone(),
        )?;

        serde_json::to_writer(&mut self.writer, &record)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;

        self.next_index += 1;
        self.last_record_hash = Some(record.record_hash.clone());

        Ok(record)
    }

    pub fn append(&mut self, event: &EventEnvelope) -> Result<(), ReplayError> {
        self.append_record(event).map(|_| ())
    }

    pub fn read_all(path: impl AsRef<Path>) -> Result<Vec<EventEnvelope>, ReplayError> {
        Self::read_all_events(path)
    }

    pub fn read_all_events(path: impl AsRef<Path>) -> Result<Vec<EventEnvelope>, ReplayError> {
        let records = Self::read_all_records(path)?;
        Ok(records.into_iter().map(|record| record.event).collect())
    }

    pub fn read_all_records(path: impl AsRef<Path>) -> Result<Vec<ReplayRecord>, ReplayError> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut records = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            records.push(serde_json::from_str::<ReplayRecord>(&line)?);
        }

        Ok(records)
    }

    pub fn verify_chain(path: impl AsRef<Path>) -> Result<ReplayVerificationReport, ReplayError> {
        let records = Self::read_all_records(path)?;
        let mut previous_record_hash: Option<String> = None;

        for (expected_index, record) in records.iter().enumerate() {
            let expected_index = expected_index as u64;
            if record.index != expected_index {
                return Err(ReplayError::VerificationFailed {
                    index: record.index,
                    reason: format!(
                        "expected contiguous index {}, found {}",
                        expected_index, record.index
                    ),
                });
            }

            let expected_event_hash = ReplayRecord::compute_event_hash(&record.event)?;
            if record.event_hash != expected_event_hash {
                return Err(ReplayError::VerificationFailed {
                    index: record.index,
                    reason: "event_hash mismatch".into(),
                });
            }

            if record.previous_record_hash != previous_record_hash {
                return Err(ReplayError::VerificationFailed {
                    index: record.index,
                    reason: "previous_record_hash mismatch".into(),
                });
            }

            let expected_record_hash = ReplayRecord::compute_record_hash(
                record.index,
                &record.event_hash,
                record.previous_record_hash.as_deref(),
            );

            if record.record_hash != expected_record_hash {
                return Err(ReplayError::VerificationFailed {
                    index: record.index,
                    reason: "record_hash mismatch".into(),
                });
            }

            previous_record_hash = Some(record.record_hash.clone());
        }

        Ok(ReplayVerificationReport {
            records_verified: records.len(),
            last_record_hash: previous_record_hash,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn debug_set_broken_writer(&mut self) -> Result<(), ReplayError> {
        let mut pipe_fds = [0; 2];
        let rc = unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
        if rc != 0 {
            return Err(ReplayError::Io(std::io::Error::last_os_error()));
        }

        let close_rc = unsafe { libc::close(pipe_fds[0]) };
        if close_rc != 0 {
            return Err(ReplayError::Io(std::io::Error::last_os_error()));
        }

        let write_end = unsafe { File::from_raw_fd(pipe_fds[1]) };
        self.writer = BufWriter::new(write_end);
        Ok(())
    }
}

impl ReplayRecord {
    fn new(
        index: u64,
        event: EventEnvelope,
        previous_record_hash: Option<String>,
    ) -> Result<Self, ReplayError> {
        let event_hash = Self::compute_event_hash(&event)?;
        let record_hash =
            Self::compute_record_hash(index, &event_hash, previous_record_hash.as_deref());

        Ok(Self {
            index,
            event,
            event_hash,
            previous_record_hash,
            record_hash,
        })
    }

    fn compute_event_hash(event: &EventEnvelope) -> Result<String, ReplayError> {
        let canonical_event = canonicalize_value(serde_json::to_value(event)?);
        let event_bytes = serde_json::to_vec(&canonical_event)?;
        Ok(sha256_hex(&event_bytes))
    }

    fn compute_record_hash(
        index: u64,
        event_hash: &str,
        previous_record_hash: Option<&str>,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(index.to_string().as_bytes());
        hasher.update(event_hash.as_bytes());
        if let Some(previous_record_hash) = previous_record_hash {
            hasher.update(previous_record_hash.as_bytes());
        }

        format!("{:x}", hasher.finalize())
    }
}

fn canonicalize_value(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));

            let mut canonical_map = Map::new();
            for (key, value) in entries {
                canonical_map.insert(key, canonicalize_value(value));
            }

            Value::Object(canonical_map)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize_value).collect()),
        other => other,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_append_and_read_event() {
        let path = std::env::temp_dir().join(format!("tnsr-replay-{}.jsonl", ulid::Ulid::new()));

        let event = EventEnvelope::new(
            "run-test",
            "quantum.state",
            "adapter_quantum",
            json!({ "state_dim": 16 }),
        );

        {
            let mut log = JsonlReplayLog::open(&path).expect("open replay log");
            let record = log.append_record(&event).expect("append event");
            assert_eq!(record.index, 0);
            assert!(record.previous_record_hash.is_none());
        }

        let events = JsonlReplayLog::read_all_events(&path).expect("read replay log");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, event.event_id);
        assert_eq!(events[0].trace_id, event.trace_id);

        let report = JsonlReplayLog::verify_chain(&path).expect("verify replay chain");
        assert_eq!(report.records_verified, 1);
        assert!(report.last_record_hash.is_some());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_verify_chain_fails_when_record_is_tampered() {
        let path = std::env::temp_dir().join(format!("tnsr-replay-{}.jsonl", ulid::Ulid::new()));

        let event = EventEnvelope::new(
            "run-test",
            "quantum.state",
            "adapter_quantum",
            json!({ "state_dim": 16 }),
        );

        {
            let mut log = JsonlReplayLog::open(&path).expect("open replay log");
            log.append_record(&event).expect("append event");
        }

        let mut records = JsonlReplayLog::read_all_records(&path).expect("read records");
        records[0].event.payload = json!({ "state_dim": 99 });

        let mut file = File::create(&path).expect("rewrite tampered log");
        for record in &records {
            serde_json::to_writer(&mut file, record).expect("write tampered record");
            file.write_all(b"\n").expect("write newline");
        }

        let err = JsonlReplayLog::verify_chain(&path).expect_err("tampering should be detected");
        assert!(matches!(err, ReplayError::VerificationFailed { .. }));

        let _ = std::fs::remove_file(path);
    }
}
