use runtime_core::EventEnvelope;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct JsonlReplayLog {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl JsonlReplayLog {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ReplayError> {
        let path = path.as_ref().to_path_buf();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        Ok(Self {
            path,
            writer: BufWriter::new(file),
        })
    }

    pub fn append(&mut self, event: &EventEnvelope) -> Result<(), ReplayError> {
        serde_json::to_writer(&mut self.writer, event)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn read_all(path: impl AsRef<Path>) -> Result<Vec<EventEnvelope>, ReplayError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut events = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            events.push(serde_json::from_str::<EventEnvelope>(&line)?);
        }

        Ok(events)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_core::EventEnvelope;
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
            log.append(&event).expect("append event");
        }

        let events = JsonlReplayLog::read_all(&path).expect("read replay log");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, event.event_id);
        assert_eq!(events[0].trace_id, event.trace_id);

        let _ = std::fs::remove_file(path);
    }
}
