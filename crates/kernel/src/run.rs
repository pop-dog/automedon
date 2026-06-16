//! The single-token run loop. Executes one flat Frame; nested (Composite)
//! Workflows are not yet supported.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use crate::event::{Event, Fault};
use crate::ir::{Gate, GateKey, GateTarget, Step, Workflow, DEFAULT_BUDGET};
use crate::{Sink, Stream};

/// Run a flat Workflow to an Exit Gate, emitting Events to `sink`.
///
/// `initial_message` seeds the entry Step's in-Message, letting a Run carry
/// arguments; pass `&[]` for no input. Returns the Workflow exit code, or the
/// Fault that prevented reaching an Exit Gate. Panics on malformed input (a
/// Gate/entry pointing at a missing Step, or a command that fails to spawn) —
/// those are setup bugs, not model Faults, and are not validated here.
pub fn run(workflow: &Workflow, initial_message: &[u8], sink: &mut dyn Sink) -> Result<i32, Fault> {
    sink.emit(&Event::RunStarted);

    // The Frame: per-Step remaining Budget, resolved by the cascade up front.
    let mut remaining: HashMap<&str, u32> = workflow
        .steps
        .iter()
        .map(|(name, step)| {
            let budget = step
                .budget
                .or(workflow.default_budget)
                .unwrap_or(DEFAULT_BUDGET);
            (name.as_str(), budget)
        })
        .collect();

    // Per-Step activation counter: how many times each Step has been entered so
    // far. The pre-increment value is the 0-based activation index handed to the
    // output channel, disambiguating a Step run more than once under its Budget.
    let mut activations: HashMap<&str, u32> = workflow
        .steps
        .keys()
        .map(|name| (name.as_str(), 0u32))
        .collect();

    let mut current: &str = &workflow.entry;
    let mut message: Vec<u8> = initial_message.to_vec();

    loop {
        let step = workflow
            .steps
            .get(current)
            .unwrap_or_else(|| panic!("workflow references missing step {current:?}"));

        // Pick the Gate to traverse — either by running the Step, or, if its
        // Budget is spent, the EXHAUSTED Gate (taken *before* the Step runs).
        let gate: &Gate = if remaining[current] == 0 {
            sink.emit(&Event::Exhausted { step: current.to_string() });
            match find_gate(step, &GateKey::Exhausted) {
                Some(g) => g,
                None => return raise(sink, Fault::UnhandledExhaustion { step: current.to_string() }),
            }
        } else {
            *remaining.get_mut(current).unwrap() -= 1;
            sink.emit(&Event::BudgetConsumed {
                step: current.to_string(),
                remaining: remaining[current],
            });
            sink.emit(&Event::StepEntered { step: current.to_string() });

            let activation = activations[current];
            *activations.get_mut(current).unwrap() += 1;
            let (code, out) = invoke(step, &message, current, activation, sink);
            sink.emit(&Event::StepExited { step: current.to_string(), code });
            message = out;

            match find_gate(step, &GateKey::Code(code)).or_else(|| find_gate(step, &GateKey::Default)) {
                Some(g) => g,
                None => return raise(sink, Fault::UnhandledOutcome { step: current.to_string(), code }),
            }
        };

        sink.emit(&Event::GateTaken {
            step: current.to_string(),
            key: gate.key.clone(),
            target: gate.target.clone(),
        });

        match &gate.target {
            GateTarget::Step(next) => {
                sink.emit(&Event::MessagePassed {
                    from: current.to_string(),
                    to: next.clone(),
                    bytes: message.len(),
                });
                current = next;
            }
            GateTarget::Exit(code) => {
                sink.emit(&Event::RunEnded { code: *code });
                return Ok(*code);
            }
        }
    }
}

fn find_gate<'a>(step: &'a Step, key: &GateKey) -> Option<&'a Gate> {
    step.gates.iter().find(|g| &g.key == key)
}

fn raise(sink: &mut dyn Sink, fault: Fault) -> Result<i32, Fault> {
    sink.emit(&Event::FaultRaised { fault: fault.clone() });
    Err(fault)
}

/// The Step ABI: spawn a process (cwd inherited), pipe the in-Message to stdin,
/// capture the exit code and stdout (the out-Message). Both stdout and stderr
/// are piped (not inherited) and their chunks streamed to the Sink's output
/// channel as they arrive, so a Run's output is captured without losing the live
/// view. Bytes move opaquely.
fn invoke(
    step: &Step,
    in_message: &[u8],
    name: &str,
    activation: u32,
    sink: &mut dyn Sink,
) -> (i32, Vec<u8>) {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&step.command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn step command {:?}: {e}", step.command));

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
    use super::run;
    use crate::event::{Event, Fault};
    use crate::ir::{Gate, GateKey, GateTarget, Step, Workflow, DEFAULT_BUDGET};
    use crate::Sink;

    use crate::Stream;

    /// A Sink that records every emitted Event and every output chunk for
    /// inspection.
    #[derive(Default)]
    struct MockSink {
        events: Vec<Event>,
        outputs: Vec<(String, u32, Stream, Vec<u8>)>,
    }

    impl Sink for MockSink {
        fn emit(&mut self, event: &Event) {
            self.events.push(event.clone());
        }

        fn on_output(&mut self, step: &str, activation: u32, stream: Stream, bytes: &[u8]) {
            self.outputs.push((step.to_string(), activation, stream, bytes.to_vec()));
        }
    }

    /// Concatenate every output chunk recorded for a (step, stream) pair, across
    /// all activations, into one buffer.
    fn captured(sink: &MockSink, step: &str, stream: Stream) -> Vec<u8> {
        sink.outputs
            .iter()
            .filter(|(s, _, st, _)| s == step && *st == stream)
            .flat_map(|(_, _, _, bytes)| bytes.iter().copied())
            .collect()
    }

    fn gate(key: GateKey, target: GateTarget) -> Gate {
        Gate { key, target, when: None }
    }

    fn step(command: &str, budget: Option<u32>, gates: Vec<Gate>) -> Step {
        Step { command: command.into(), budget, gates }
    }

    fn workflow(entry: &str, default_budget: Option<u32>, steps: Vec<(&str, Step)>) -> Workflow {
        Workflow {
            entry: entry.into(),
            default_budget,
            steps: steps.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        }
    }

    fn entered(sink: &MockSink) -> usize {
        sink.events.iter().filter(|e| matches!(e, Event::StepEntered { .. })).count()
    }

    /// A single self-looping Step that always fails, used to exercise the Budget.
    fn looping_workflow(default_budget: Option<u32>, step_budget: Option<u32>) -> Workflow {
        workflow(
            "loop",
            default_budget,
            vec![(
                "loop",
                step(
                    "exit 1",
                    step_budget,
                    vec![
                        gate(GateKey::Default, GateTarget::Step("loop".into())),
                        gate(GateKey::Exhausted, GateTarget::Exit(0)),
                    ],
                ),
            )],
        )
    }

    #[test]
    fn code_gate_takes_precedence_over_default() {
        let wf = workflow(
            "s",
            None,
            vec![(
                "s",
                step(
                    "exit 0",
                    None,
                    vec![
                        gate(GateKey::Code(0), GateTarget::Exit(10)),
                        gate(GateKey::Default, GateTarget::Exit(99)),
                    ],
                ),
            )],
        );
        let mut sink = MockSink::default();
        assert_eq!(run(&wf, &[], &mut sink).unwrap(), 10);
    }

    #[test]
    fn default_gate_catches_unmatched_integer() {
        let wf = workflow(
            "s",
            None,
            vec![(
                "s",
                step(
                    "exit 5",
                    None,
                    vec![
                        gate(GateKey::Code(0), GateTarget::Exit(10)),
                        gate(GateKey::Default, GateTarget::Exit(99)),
                    ],
                ),
            )],
        );
        let mut sink = MockSink::default();
        assert_eq!(run(&wf, &[], &mut sink).unwrap(), 99);
    }

    #[test]
    fn exhaustion_routes_after_exactly_budget_activations() {
        let wf = workflow(
            "loop",
            None,
            vec![(
                "loop",
                step(
                    "exit 1",
                    Some(3),
                    vec![
                        gate(GateKey::Code(0), GateTarget::Exit(0)),
                        gate(GateKey::Default, GateTarget::Step("loop".into())),
                        gate(GateKey::Exhausted, GateTarget::Exit(42)),
                    ],
                ),
            )],
        );
        let mut sink = MockSink::default();
        assert_eq!(run(&wf, &[], &mut sink).unwrap(), 42);

        // The Step runs exactly Budget times, then Exhaustion fires once.
        assert_eq!(entered(&sink), 3);
        assert_eq!(
            sink.events.iter().filter(|e| matches!(e, Event::Exhausted { .. })).count(),
            1
        );

        // Budget decrements to zero across the activations.
        let remaining: Vec<u32> = sink
            .events
            .iter()
            .filter_map(|e| match e {
                Event::BudgetConsumed { remaining, .. } => Some(*remaining),
                _ => None,
            })
            .collect();
        assert_eq!(remaining, vec![2, 1, 0]);
    }

    #[test]
    fn unmapped_exit_code_raises_unhandled_outcome() {
        let wf = workflow(
            "s",
            None,
            vec![("s", step("exit 7", None, vec![gate(GateKey::Code(0), GateTarget::Exit(0))]))],
        );
        let mut sink = MockSink::default();
        match run(&wf, &[], &mut sink) {
            Err(Fault::UnhandledOutcome { step, code }) => {
                assert_eq!(step, "s");
                assert_eq!(code, 7);
            }
            other => panic!("expected UnhandledOutcome, got {other:?}"),
        }
        // The Fault is announced on the Event stream before it is returned.
        assert!(sink.events.iter().any(|e| matches!(e, Event::FaultRaised { .. })));
    }

    #[test]
    fn spent_budget_without_exhausted_gate_raises_fault() {
        let wf = workflow(
            "loop",
            None,
            vec![(
                "loop",
                step("exit 1", Some(2), vec![gate(GateKey::Default, GateTarget::Step("loop".into()))]),
            )],
        );
        let mut sink = MockSink::default();
        match run(&wf, &[], &mut sink) {
            Err(Fault::UnhandledExhaustion { step }) => assert_eq!(step, "loop"),
            other => panic!("expected UnhandledExhaustion, got {other:?}"),
        }
    }

    #[test]
    fn budget_cascade_prefers_step_then_workflow_then_default() {
        // Step Budget overrides the Workflow default.
        let mut sink = MockSink::default();
        run(&looping_workflow(Some(5), Some(2)), &[], &mut sink).unwrap();
        assert_eq!(entered(&sink), 2);

        // Workflow default applies when the Step has no Budget.
        let mut sink = MockSink::default();
        run(&looping_workflow(Some(3), None), &[], &mut sink).unwrap();
        assert_eq!(entered(&sink), 3);

        // The hardcoded default applies when neither is set.
        let mut sink = MockSink::default();
        run(&looping_workflow(None, None), &[], &mut sink).unwrap();
        assert_eq!(entered(&sink), DEFAULT_BUDGET as usize);
    }

    #[test]
    fn step_stderr_is_delivered_to_on_output() {
        // A Step's stderr is captured and streamed to the Sink (no longer
        // inherited by the terminal), so a failed Run is diagnosable.
        let wf = workflow(
            "s",
            None,
            vec![("s", step("echo boom >&2; exit 0", None, vec![gate(GateKey::Code(0), GateTarget::Exit(0))]))],
        );
        let mut sink = MockSink::default();
        run(&wf, &[], &mut sink).unwrap();
        assert_eq!(captured(&sink, "s", Stream::Stderr), b"boom\n");
    }

    #[test]
    fn step_stdout_is_both_captured_as_message_and_teed_to_on_output() {
        // stdout is the out-Message (piped to the successor) AND mirrored to the
        // output channel, so a Sink can persist it without intercepting routing.
        let wf = workflow(
            "s",
            None,
            vec![("s", step("echo hello", None, vec![gate(GateKey::Code(0), GateTarget::Exit(0))]))],
        );
        let mut sink = MockSink::default();
        run(&wf, &[], &mut sink).unwrap();
        assert_eq!(captured(&sink, "s", Stream::Stdout), b"hello\n");
    }

    #[test]
    fn activation_index_increments_across_budget() {
        // A Step run three times under its Budget tags its output with a rising
        // 0-based activation index, so repeated runs stay distinguishable.
        let wf = workflow(
            "loop",
            None,
            vec![(
                "loop",
                step(
                    "echo tick >&2; exit 1",
                    Some(3),
                    vec![
                        gate(GateKey::Default, GateTarget::Step("loop".into())),
                        gate(GateKey::Exhausted, GateTarget::Exit(0)),
                    ],
                ),
            )],
        );
        let mut sink = MockSink::default();
        run(&wf, &[], &mut sink).unwrap();
        let activations: Vec<u32> = sink
            .outputs
            .iter()
            .filter(|(_, _, stream, _)| *stream == Stream::Stderr)
            .map(|(_, activation, _, _)| *activation)
            .collect();
        assert_eq!(activations, vec![0, 1, 2]);
    }

    #[test]
    fn large_in_message_does_not_deadlock_with_streaming_output() {
        use std::sync::mpsc;
        use std::thread;
        use std::time::Duration;

        // `cat` echoes its stdin to stdout, exercising the three-pipe hazard: if
        // the in-Message is written to stdin in full before the output readers
        // start, the child's stdout pipe fills, blocking it from draining stdin,
        // while the parent blocks writing stdin — a deadlock. The in-Message far
        // exceeds the OS pipe buffer so the race is forced, not chanced.
        let big = vec![b'x'; 1 << 20]; // 1 MiB
        let wf = workflow(
            "cat",
            None,
            vec![("cat", step("cat", None, vec![gate(GateKey::Code(0), GateTarget::Exit(0))]))],
        );

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut sink = MockSink::default();
            let code = run(&wf, &big, &mut sink).unwrap();
            let echoed = captured(&sink, "cat", Stream::Stdout).len();
            let _ = tx.send((code, echoed));
        });

        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok((code, echoed)) => {
                assert_eq!(code, 0);
                assert_eq!(echoed, 1 << 20, "the whole in-Message should echo through");
            }
            Err(_) => panic!("invoke deadlocked writing a large in-Message before draining output"),
        }
    }

    #[test]
    fn message_is_piped_to_successor_stdin() {
        // `emit` writes "5" to stdout; `consume` exits with the number it reads
        // from stdin, so reaching exit 0 proves the Message arrived intact.
        let wf = workflow(
            "emit",
            None,
            vec![
                ("emit", step("echo 5", None, vec![gate(GateKey::Code(0), GateTarget::Step("consume".into()))])),
                (
                    "consume",
                    step(
                        "read n; exit \"$n\"",
                        None,
                        vec![
                            gate(GateKey::Code(5), GateTarget::Exit(0)),
                            gate(GateKey::Default, GateTarget::Exit(1)),
                        ],
                    ),
                ),
            ],
        );
        let mut sink = MockSink::default();
        assert_eq!(run(&wf, &[], &mut sink).unwrap(), 0);
    }

    #[test]
    fn initial_message_seeds_entry_step_stdin() {
        // The entry Step exits with the number it reads from stdin, so reaching
        // exit 0 proves the seeded initial Message arrived at the entry Step.
        let wf = workflow(
            "entry",
            None,
            vec![(
                "entry",
                step(
                    "read n; exit \"$n\"",
                    None,
                    vec![
                        gate(GateKey::Code(5), GateTarget::Exit(0)),
                        gate(GateKey::Default, GateTarget::Exit(1)),
                    ],
                ),
            )],
        );
        let mut sink = MockSink::default();
        assert_eq!(run(&wf, b"5", &mut sink).unwrap(), 0);
    }
}
