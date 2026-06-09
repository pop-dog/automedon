//! The single-token run loop. Executes one flat Frame; nested (Composite)
//! Workflows are not yet supported.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::event::{Event, Fault};
use crate::ir::{Gate, GateKey, GateTarget, Step, Workflow, DEFAULT_BUDGET};
use crate::Sink;

/// Run a flat Workflow to an Exit Gate, emitting Events to `sink`.
///
/// Returns the Workflow exit code, or the Fault that prevented reaching an Exit
/// Gate. Panics on malformed input (a Gate/entry pointing at a missing Step, or
/// a command that fails to spawn) — those are setup bugs, not model Faults, and
/// are not validated here.
pub fn run(workflow: &Workflow, sink: &mut dyn Sink) -> Result<i32, Fault> {
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

    let mut current: &str = &workflow.entry;
    let mut message: Vec<u8> = Vec::new();

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

            let (code, out) = invoke(step, &message);
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
/// capture the exit code and stdout (the out-Message). Bytes move opaquely.
fn invoke(step: &Step, in_message: &[u8]) -> (i32, Vec<u8>) {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&step.command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn step command {:?}: {e}", step.command));

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(in_message); // pipe may close early; that's fine
    }
    let output = child.wait_with_output().expect("failed to wait on step");
    // No exit code => killed by signal; treat as a routable failure code.
    let code = output.status.code().unwrap_or(-1);
    (code, output.stdout)
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::event::{Event, Fault};
    use crate::ir::{Gate, GateKey, GateTarget, Step, Workflow, DEFAULT_BUDGET};
    use crate::Sink;

    /// A Sink that records every emitted Event for inspection.
    #[derive(Default)]
    struct MockSink {
        events: Vec<Event>,
    }

    impl Sink for MockSink {
        fn emit(&mut self, event: &Event) {
            self.events.push(event.clone());
        }
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
        assert_eq!(run(&wf, &mut sink).unwrap(), 10);
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
        assert_eq!(run(&wf, &mut sink).unwrap(), 99);
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
        assert_eq!(run(&wf, &mut sink).unwrap(), 42);

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
        match run(&wf, &mut sink) {
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
        match run(&wf, &mut sink) {
            Err(Fault::UnhandledExhaustion { step }) => assert_eq!(step, "loop"),
            other => panic!("expected UnhandledExhaustion, got {other:?}"),
        }
    }

    #[test]
    fn budget_cascade_prefers_step_then_workflow_then_default() {
        // Step Budget overrides the Workflow default.
        let mut sink = MockSink::default();
        run(&looping_workflow(Some(5), Some(2)), &mut sink).unwrap();
        assert_eq!(entered(&sink), 2);

        // Workflow default applies when the Step has no Budget.
        let mut sink = MockSink::default();
        run(&looping_workflow(Some(3), None), &mut sink).unwrap();
        assert_eq!(entered(&sink), 3);

        // The hardcoded default applies when neither is set.
        let mut sink = MockSink::default();
        run(&looping_workflow(None, None), &mut sink).unwrap();
        assert_eq!(entered(&sink), DEFAULT_BUDGET as usize);
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
        assert_eq!(run(&wf, &mut sink).unwrap(), 0);
    }
}
