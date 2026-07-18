//! A persistence Sink: writes one per-Run directory holding `events.jsonl` (the
//! control plane, one record per line) and raw sidecar output files (the data
//! plane). The Sink — not the Kernel — stamps each record with a monotonic `seq`
//! and a wall-clock `ts` on receipt.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use kernel::{Event, Sink, Stream};
use serde::Serialize;

/// The control-plane log filename inside a Run directory.
const EVENTS_FILE: &str = "events.jsonl";

/// Orchestrator-owned run-metadata filename inside a Run directory. Holds facts
/// the orchestrator (not the Kernel) knows about a Run — currently the Step
/// environment — kept out of the Kernel's `events.jsonl`.
const META_FILE: &str = "meta.json";

/// Writes a single Run's records under `dir`.
pub struct FileSink {
    dir: PathBuf,
    events: File,
    /// Monotonic record counter assigned on receipt.
    seq: u64,
    /// Sidecar files already referenced in the log, so the pointer record is
    /// written once per stream rather than once per chunk.
    referenced: HashSet<String>,
}

/// One `events.jsonl` line carrying a control-plane Event, stamped with the
/// Sink-assigned `seq` and wall-clock `ts` (epoch milliseconds).
#[derive(Serialize)]
struct EventRecord<'a> {
    seq: u64,
    ts: u128,
    event: &'a Event,
}

/// One `events.jsonl` line pointing at a Step's raw sidecar output file. Keeps
/// the bulk bytes out of the control plane while making them discoverable from
/// it.
#[derive(Serialize)]
struct OutputRecord<'a> {
    seq: u64,
    ts: u128,
    output: OutputRef<'a>,
}

#[derive(Serialize)]
struct OutputRef<'a> {
    step: &'a str,
    activation: u32,
    stream: Stream,
    file: &'a str,
}

impl FileSink {
    /// Create the Run directory (and parents) and open its events log.
    pub fn create(dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        let events = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(EVENTS_FILE))?;
        Ok(FileSink { dir, events, seq: 0, referenced: HashSet::new() })
    }

    /// Record the populated Step environment once, as orchestrator-owned run
    /// metadata in `meta.json` — not a Kernel Event. Whatever the
    /// Step environment holds is logged, so future members are covered without
    /// changing this method. Best-effort: a metadata write failure must not abort
    /// the Run.
    pub fn record_environment(&self, environment: &[(String, PathBuf)]) {
        let environment: serde_json::Map<String, serde_json::Value> = environment
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::from(v.to_string_lossy().into_owned())))
            .collect();
        let meta = serde_json::json!({ "environment": environment });
        if let Ok(text) = serde_json::to_string(&meta) {
            let _ = std::fs::write(self.dir.join(META_FILE), text);
        }
    }

    /// Take the next `seq` and the current wall-clock `ts`. Bundled because a
    /// record stamps both at the same receipt instant.
    fn stamp(&mut self) -> (u64, u128) {
        let seq = self.seq;
        self.seq += 1;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        (seq, ts)
    }

    /// Append one JSON line to the events log.
    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        self.events.write_all(line.as_bytes())?;
        self.events.write_all(b"\n")
    }

    /// Append `bytes` to a Step's raw sidecar file, opening it on first write.
    fn append_sidecar(&self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(name))?;
        file.write_all(bytes)
    }
}

impl Sink for FileSink {
    fn emit(&mut self, event: &Event) {
        let (seq, ts) = self.stamp();
        let record = EventRecord { seq, ts, event };
        // Persistence is best-effort: an I/O error here must not abort the Run.
        if let Ok(line) = serde_json::to_string(&record) {
            let _ = self.write_line(&line);
        }
    }

    fn on_output(&mut self, step: &str, activation: u32, stream: Stream, bytes: &[u8]) {
        let name = sidecar_name(step, activation, stream);
        // A persistence Sink must not abort the Run on an I/O hiccup; the live
        // path keeps going and the loss is confined to the durable copy.
        let _ = self.append_sidecar(&name, bytes);

        // Pointer to the sidecar, written once per stream so the log stays a
        // lean reference rather than echoing every chunk.
        if self.referenced.insert(name.clone()) {
            let (seq, ts) = self.stamp();
            let record = OutputRecord {
                seq,
                ts,
                output: OutputRef { step, activation, stream, file: &name },
            };
            if let Ok(line) = serde_json::to_string(&record) {
                let _ = self.write_line(&line);
            }
        }
    }
}

/// The sidecar filename for a Step's output: `<step>.<activation>.<stream>`.
fn sidecar_name(step: &str, activation: u32, stream: Stream) -> String {
    let stream = match stream {
        Stream::Stdout => "stdout",
        Stream::Stderr => "stderr",
    };
    format!("{step}.{activation}.{stream}")
}

#[cfg(test)]
mod tests {
    use super::FileSink;
    use kernel::{Event, Sink, Stream};
    use std::path::{Path, PathBuf};

    /// A throwaway directory under the system temp dir, removed on Drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("ao-test-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Parse every line of a Run's events.jsonl into JSON values.
    fn read_jsonl(run_dir: &Path) -> Vec<serde_json::Value> {
        let text = std::fs::read_to_string(run_dir.join("events.jsonl")).unwrap();
        text.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    #[test]
    fn each_emitted_event_is_a_jsonl_record_with_seq_and_ts() {
        let tmp = TempDir::new();
        let run_dir = tmp.0.join("run");
        {
            let mut sink = FileSink::create(run_dir.clone()).unwrap();
            sink.emit(&Event::RunStarted);
            sink.emit(&Event::RunEnded { code: 0 });
        }
        let records = read_jsonl(&run_dir);
        assert_eq!(records.len(), 2);

        // seq is monotonic from zero; ts is present and non-decreasing.
        assert_eq!(records[0]["seq"].as_u64(), Some(0));
        assert_eq!(records[1]["seq"].as_u64(), Some(1));
        let ts0 = records[0]["ts"].as_u64().expect("ts present");
        let ts1 = records[1]["ts"].as_u64().expect("ts present");
        assert!(ts0 > 0 && ts1 >= ts0);

        // The control-plane Event itself rides in the record.
        assert!(records[1]["event"].to_string().contains("RunEnded"));
    }

    #[test]
    fn jsonl_references_each_sidecar_once() {
        let tmp = TempDir::new();
        let run_dir = tmp.0.join("run");
        {
            let mut sink = FileSink::create(run_dir.clone()).unwrap();
            // Two chunks for the same sidecar, then a chunk for a different one.
            sink.on_output("build", 0, Stream::Stderr, b"line one\n");
            sink.on_output("build", 0, Stream::Stderr, b"line two\n");
            sink.on_output("build", 0, Stream::Stdout, b"ok\n");
        }
        // One reference per distinct sidecar (not per chunk), each carrying the
        // sidecar filename so the bulk bytes are discoverable from the log.
        let files: Vec<String> = read_jsonl(&run_dir)
            .iter()
            .filter_map(|r| r.get("output").map(|o| o["file"].as_str().unwrap().to_string()))
            .collect();
        assert_eq!(files.len(), 2);
        assert!(files.contains(&"build.0.stderr".to_string()));
        assert!(files.contains(&"build.0.stdout".to_string()));
    }

    #[test]
    fn step_environment_is_recorded_once_as_run_metadata() {
        use std::path::PathBuf;

        let tmp = TempDir::new();
        let run_dir = tmp.0.join("run");
        {
            let sink = FileSink::create(run_dir.clone()).unwrap();
            sink.record_environment(&[
                ("AUTOMEDON_WORKFLOW_DIR".to_string(), PathBuf::from("/wf")),
                ("AUTOMEDON_RUN_DIR".to_string(), PathBuf::from("/tmp/runs/abc")),
            ]);
        }
        // The Step environment lands in an orchestrator-owned metadata file, not
        // in the Kernel's events.jsonl.
        assert!(!run_dir.join("events.jsonl").exists() || {
            let log = std::fs::read_to_string(run_dir.join("events.jsonl")).unwrap();
            !log.contains("AUTOMEDON_RUN_DIR")
        });
        let meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(run_dir.join("meta.json")).unwrap())
                .unwrap();
        assert_eq!(meta["environment"]["AUTOMEDON_WORKFLOW_DIR"], "/wf");
        assert_eq!(meta["environment"]["AUTOMEDON_RUN_DIR"], "/tmp/runs/abc");
    }

    #[test]
    fn step_output_is_recoverable_from_a_sidecar_file() {
        let tmp = TempDir::new();
        let run_dir = tmp.0.join("run");
        {
            let mut sink = FileSink::create(run_dir.clone()).unwrap();
            sink.on_output("build", 0, Stream::Stderr, b"error: boom\n");
        }
        let sidecar = run_dir.join("build.0.stderr");
        assert_eq!(std::fs::read(sidecar).unwrap(), b"error: boom\n");
    }
}
