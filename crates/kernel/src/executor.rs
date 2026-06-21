//! The Step-execution seam. Routing (in `run`) decides *which* Gate to take; a
//! [`StepExecutor`] decides *how* a Step actually runs. Splitting the two lets
//! the routing core be tested with canned outcomes while the subprocess plumbing
//! stays behind one adapter, and lets a future parallel executor (ADR-0004) be an
//! additive adapter rather than a Kernel change.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use crate::{Sink, Stream};

/// Runs one leaf Step's command and reports back its outcome. The whole Step ABI
/// (ADR-0003 — "a Step is any process that exits with an integer") lives behind
/// this single method; `run` never spawns a process itself. Only leaf
/// (`StepBody::Command`) Steps reach an executor — Composite Steps are run by the
/// engine's Frame stack, never here — so the seam takes the command, not the Step.
///
/// The Sink is passed in by `&mut` rather than carried across threads: it is not
/// `Send`, so an executor that fans output out over worker threads must funnel
/// the chunks back and call `on_output` from this (the calling) thread, in
/// receipt order. `execute` returns the exit code and the out-Message (stdout),
/// having already streamed every output chunk to `on_output`.
pub trait StepExecutor {
    fn execute(
        &mut self,
        command: &str,
        in_message: &[u8],
        name: &str,
        activation: u32,
        sink: &mut dyn Sink,
    ) -> (i32, Vec<u8>);
}

/// The production [`StepExecutor`]: each Step is an `sh -c` subprocess with the
/// working directory inherited. Carries the Step environment — the ambient,
/// Run-constant context (`$WORKFLOW_DIR`, `$RUN_DIR`) the engine provides — and
/// layers it onto every spawn's inherited env (ADR-0010). Constant for the Run.
#[derive(Default)]
pub struct SubprocessExecutor {
    /// Name/path pairs injected into each child's environment per spawn,
    /// overlaying the inherited env. Empty means "inherit only".
    env: Vec<(String, PathBuf)>,
}

impl SubprocessExecutor {
    /// An Executor that injects no Step environment of its own; children inherit
    /// the driver's env unchanged.
    pub fn new() -> Self {
        Self::default()
    }

    /// An Executor carrying the Step environment to inject into every spawned
    /// Step, layered on the inherited env (ADR-0010).
    pub fn with_env(env: Vec<(String, PathBuf)>) -> Self {
        Self { env }
    }
}

impl StepExecutor for SubprocessExecutor {
    /// Spawn the process, pipe the in-Message to stdin, capture the exit code and
    /// stdout (the out-Message). Both stdout and stderr are piped (not inherited)
    /// and their chunks streamed to the Sink's output channel as they arrive, so
    /// a Run's output is captured without losing the live view. Bytes move
    /// opaquely.
    fn execute(
        &mut self,
        command: &str,
        in_message: &[u8],
        name: &str,
        activation: u32,
        sink: &mut dyn Sink,
    ) -> (i32, Vec<u8>) {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(command)
            // Layer the Step environment over the inherited env; `envs` adds to
            // (does not clear) what the child would inherit.
            .envs(self.env.iter().map(|(k, v)| (k, v.as_os_str())))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn step command {command:?}: {e}"));

        // All three pipes must be serviced concurrently: stdin is fed from its own
        // thread while two reader threads drain stdout/stderr. Writing stdin to
        // completion up front would deadlock when the in-Message exceeds the OS pipe
        // buffer — the child fills its stdout pipe (which nobody is yet reading) and
        // stops draining stdin, so both sides block. A Step's stdout becomes the next
        // Step's stdin, so Messages can grow arbitrarily large in a real Workflow.
        let stdin_writer = child.stdin.take().map(|mut stdin| {
            let in_message = in_message.to_vec();
            thread::spawn(move || {
                let _ = stdin.write_all(&in_message); // pipe may close early; that's fine
            })
        });

        // A reader thread per stream funnels chunks through one channel. The Sink is
        // not `Send`, so the threads only carry bytes and this thread does every
        // `on_output` call, in receipt order.
        let (tx, rx) = mpsc::channel::<(Stream, Vec<u8>)>();
        let stdout_reader = pipe_reader(child.stdout.take(), Stream::Stdout, tx.clone());
        let stderr_reader = pipe_reader(child.stderr.take(), Stream::Stderr, tx);

        let mut out = Vec::new();
        for (stream, chunk) in rx {
            if stream == Stream::Stdout {
                out.extend_from_slice(&chunk);
            }
            sink.on_output(name, activation, stream, &chunk);
        }
        stdout_reader.join().expect("stdout reader panicked");
        stderr_reader.join().expect("stderr reader panicked");
        if let Some(writer) = stdin_writer {
            writer.join().expect("stdin writer panicked");
        }

        let status = child.wait().expect("failed to wait on step");
        // No exit code => killed by signal; treat as a routable failure code.
        let code = status.code().unwrap_or(-1);
        (code, out)
    }
}

/// Spawn a thread that reads `pipe` to EOF in chunks, forwarding each chunk to
/// `tx` tagged with its `stream`. A `None` pipe yields an immediately-finished
/// thread. The thread owns its `tx` clone; when every clone is dropped the
/// receiver's iteration ends.
fn pipe_reader(
    pipe: Option<impl Read + Send + 'static>,
    stream: Stream,
    tx: mpsc::Sender<(Stream, Vec<u8>)>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let Some(mut pipe) = pipe else { return };
        let mut buf = [0u8; 8192];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send((stream, buf[..n].to_vec())).is_err() {
                        break; // receiver gone; nothing more to do
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::SubprocessExecutor;
    use crate::{Event, Sink, StepExecutor};

    /// A Sink that ignores everything; these tests assert on the out-Message,
    /// not the output channel.
    struct NullSink;
    impl Sink for NullSink {
        fn emit(&mut self, _event: &Event) {}
    }

    #[test]
    fn with_env_injects_the_step_environment_into_the_child() {
        // Both Step environment members ride into the child as real environment
        // variables, so a command can read what the engine provided.
        let mut exec = SubprocessExecutor::with_env(vec![
            ("WORKFLOW_DIR".to_string(), PathBuf::from("/wf")),
            ("RUN_DIR".to_string(), PathBuf::from("/tmp/agent-orchestrator/runs/abc")),
        ]);
        let (code, out) = exec.execute(
            "printf '%s|%s' \"$WORKFLOW_DIR\" \"$RUN_DIR\"",
            &[],
            "s",
            0,
            &mut NullSink,
        );
        assert_eq!(code, 0);
        assert_eq!(out, b"/wf|/tmp/agent-orchestrator/runs/abc");
    }
}
