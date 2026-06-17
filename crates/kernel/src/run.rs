//! The run loop: a stack machine over Frames. Each Frame is one activation of a
//! Workflow; a Composite Step pushes a child Frame and surfaces its exit code to
//! the parent. The Frame stack is realised by recursion — `run_frame` calls
//! itself for a child — so each Frame's Budgets and activation counters are fresh
//! locals (re-invoking a sub-Workflow starts with full Budgets) and Depth is the
//! recursion depth.

use std::collections::HashMap;

use crate::event::{Event, Fault};
use crate::ir::{Gate, GateKey, GateTarget, Registry, Step, StepBody, Workflow, DEFAULT_BUDGET};
use crate::{Sink, StepExecutor};

/// Hardcoded max-Depth default, mirroring [`DEFAULT_BUDGET`].
pub const DEFAULT_MAX_DEPTH: u32 = 10;

/// Per-Run execution policy, distinct from a Workflow definition. Depth is a
/// property of *running* a Workflow (how deep composition may nest), not of the
/// Workflow itself, so it lives here rather than in the IR. `#[non_exhaustive]`
/// so further knobs can be added without breaking callers — construct via
/// [`RunConfig::default`] and adjust with the `with_*` setters.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RunConfig {
    /// Maximum Frame Depth; entering a Composite Step past this raises an
    /// uncatchable [`Fault::DepthOverflow`].
    pub max_depth: u32,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self { max_depth: DEFAULT_MAX_DEPTH }
    }
}

impl RunConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cap Frame Depth at `max_depth`.
    pub fn with_max_depth(mut self, max_depth: u32) -> Self {
        self.max_depth = max_depth;
        self
    }
}

/// Run the registry's root Workflow to an Exit Gate, emitting Events to `sink`.
///
/// `initial_message` seeds the entry Step's in-Message, letting a Run carry
/// arguments; pass `&[]` for no input. Each leaf Step is run through `executor`
/// (the Step-execution seam), so routing can be driven with canned outcomes in
/// tests and real subprocesses in production. Returns the root Workflow's exit
/// code, or the Fault that prevented reaching an Exit Gate. Panics on malformed
/// input (a Gate/entry pointing at a missing Step, or a Composite Step naming a
/// missing Workflow) — those are setup bugs, not model Faults, validated at load.
pub fn run(
    registry: &Registry,
    initial_message: &[u8],
    config: &RunConfig,
    executor: &mut dyn StepExecutor,
    sink: &mut dyn Sink,
) -> Result<i32, Fault> {
    sink.emit(&Event::RunStarted);
    // The root Frame is Depth 1; a Composite push makes its child Depth 2, etc.
    match run_frame(registry, &registry.root, 1, initial_message, config, executor, sink) {
        Ok((code, _out)) => {
            sink.emit(&Event::RunEnded { code });
            Ok(code)
        }
        // The originating Frame already announced the Fault; it unwound to here
        // uncaught, so the Run fails.
        Err(fault) => Err(fault),
    }
}

/// What a traversed Gate resolves to within a Frame.
enum Flow<'a> {
    /// Continue at this successor Step.
    Goto(&'a str),
    /// Leave the Frame with this exit code.
    Done(i32),
}

/// Run one Frame (one activation of `id`) to an Exit Gate, returning its exit
/// code and out-Message, or the Fault it could not handle. A Composite Step
/// recurses into a child Frame at `depth + 1`; the child's surfaced code routes
/// through the *parent* Composite Step's Gates exactly as a leaf's code does.
fn run_frame(
    registry: &Registry,
    id: &str,
    depth: u32,
    in_message: &[u8],
    config: &RunConfig,
    executor: &mut dyn StepExecutor,
    sink: &mut dyn Sink,
) -> Result<(i32, Vec<u8>), Fault> {
    let workflow: &Workflow = registry
        .workflows
        .get(id)
        .unwrap_or_else(|| panic!("registry references missing workflow {id:?}"));

    // The Frame: per-Step remaining Budget, resolved by the cascade up front.
    // Fresh on every entry, so re-invoking a sub-Workflow restores full Budgets.
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
    let mut message: Vec<u8> = in_message.to_vec();

    loop {
        let step = workflow
            .steps
            .get(current)
            .unwrap_or_else(|| panic!("workflow {id:?} references missing step {current:?}"));

        // Pick the Gate to traverse. Either the Budget is spent (the EXHAUSTED
        // Gate, taken *before* the Step runs), or the Step produces an exit code
        // — by running its command, or by running its child sub-Workflow to an
        // Exit Gate — which routes through the same code/Default Gates. A child's
        // Fault is the one path that routes through the FAULT Gate instead.
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

            match &step.body {
                StepBody::Command(command) => {
                    let activation = activations[current];
                    *activations.get_mut(current).unwrap() += 1;
                    let (code, out) = executor.execute(command, &message, current, activation, sink);
                    sink.emit(&Event::StepExited { step: current.to_string(), code });
                    message = out;
                    match route(step, code) {
                        Some(g) => g,
                        None => return raise(sink, Fault::UnhandledOutcome { step: current.to_string(), code }),
                    }
                }
                StepBody::Workflow(child) => {
                    // Entering a Composite Step pushes a Frame; the Depth cap is a
                    // Run policy, and overflow is an uncatchable Fault.
                    if depth >= config.max_depth {
                        return raise(sink, Fault::DepthOverflow { workflow: child.clone() });
                    }
                    sink.emit(&Event::FramePushed {
                        step: current.to_string(),
                        workflow: child.clone(),
                        depth: depth + 1,
                    });
                    let outcome =
                        run_frame(registry, child, depth + 1, &message, config, executor, sink);
                    sink.emit(&Event::FramePopped {
                        step: current.to_string(),
                        workflow: child.clone(),
                    });
                    match outcome {
                        Ok((code, out)) => {
                            // The child reached an Exit Gate; surface its code and
                            // route it through the parent exactly as a leaf's.
                            sink.emit(&Event::StepExited { step: current.to_string(), code });
                            message = out;
                            match route(step, code) {
                                Some(g) => g,
                                None => return raise(sink, Fault::UnhandledOutcome { step: current.to_string(), code }),
                            }
                        }
                        Err(fault) => {
                            // Depth overflow is never offered to a FAULT Gate.
                            if matches!(fault, Fault::DepthOverflow { .. }) {
                                return Err(fault);
                            }
                            // Present the child's Fault to this Composite Step's
                            // FAULT Gate; absent, the Fault bubbles (this Frame
                            // faults too, unwinding toward the nearest handler).
                            match find_gate(step, &GateKey::Fault) {
                                Some(g) => g,
                                None => return Err(fault),
                            }
                        }
                    }
                }
            }
        };

        match act_on_gate(sink, current, gate, &message) {
            Flow::Goto(next) => current = next,
            Flow::Done(code) => return Ok((code, message)),
        }
    }
}

/// Select the Gate a leaf/Composite exit `code` unlocks: an exact `Code`, else
/// the `Default` Gate (which catches unmatched integers only).
fn route(step: &Step, code: i32) -> Option<&Gate> {
    find_gate(step, &GateKey::Code(code)).or_else(|| find_gate(step, &GateKey::Default))
}

fn find_gate<'a>(step: &'a Step, key: &GateKey) -> Option<&'a Gate> {
    step.gates.iter().find(|g| &g.key == key)
}

/// Traverse `gate`: announce it, and either move to its successor Step (passing
/// the Message) or leave the Frame with its Exit code.
fn act_on_gate<'a>(sink: &mut dyn Sink, current: &str, gate: &'a Gate, message: &[u8]) -> Flow<'a> {
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
            Flow::Goto(next)
        }
        GateTarget::Exit(code) => Flow::Done(*code),
    }
}

fn raise(sink: &mut dyn Sink, fault: Fault) -> Result<(i32, Vec<u8>), Fault> {
    sink.emit(&Event::FaultRaised { fault: fault.clone() });
    Err(fault)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{run, RunConfig};
    use crate::event::{Event, Fault};
    use crate::ir::{Gate, GateKey, GateTarget, Registry, Step, StepBody, Workflow, DEFAULT_BUDGET};
    use crate::{Sink, StepExecutor, Stream, SubprocessExecutor};

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

    /// A [`StepExecutor`] that returns a canned `(code, bytes)` per Step name,
    /// the same outcome for every activation, and streams the bytes to the Sink's
    /// `on_output`. Lets routing tests exercise the core with no shell and no I/O.
    #[derive(Default)]
    struct CannedExecutor {
        outcomes: std::collections::HashMap<String, (i32, Vec<u8>)>,
    }

    impl CannedExecutor {
        /// One Step always exiting with `code` and no output.
        fn returning(step: &str, code: i32) -> Self {
            Self::default().with(step, code)
        }

        fn with(mut self, step: &str, code: i32) -> Self {
            self.outcomes.insert(step.to_string(), (code, Vec::new()));
            self
        }
    }

    impl StepExecutor for CannedExecutor {
        fn execute(
            &mut self,
            _command: &str,
            _in_message: &[u8],
            name: &str,
            activation: u32,
            sink: &mut dyn Sink,
        ) -> (i32, Vec<u8>) {
            let (code, out) = self
                .outcomes
                .get(name)
                .unwrap_or_else(|| panic!("no canned outcome for step {name:?}"))
                .clone();
            if !out.is_empty() {
                sink.on_output(name, activation, Stream::Stdout, &out);
            }
            (code, out)
        }
    }

    fn gate(key: GateKey, target: GateTarget) -> Gate {
        Gate { key, target, when: None }
    }

    fn step(command: &str, budget: Option<u32>, gates: Vec<Gate>) -> Step {
        Step { body: StepBody::Command(command.into()), budget, gates }
    }

    /// A Composite Step: enter the named child Workflow, with `gates` routing its
    /// surfaced exit code (or, via a FAULT Gate, its Fault).
    fn composite(child: &str, budget: Option<u32>, gates: Vec<Gate>) -> Step {
        Step { body: StepBody::Workflow(child.into()), budget, gates }
    }

    fn workflow(entry: &str, default_budget: Option<u32>, steps: Vec<(&str, Step)>) -> Workflow {
        Workflow {
            entry: entry.into(),
            default_budget,
            steps: steps.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        }
    }

    /// Wrap a single Workflow as the root of a registry, so the flat-Workflow
    /// tests drive the registry-based engine unchanged.
    fn single(wf: Workflow) -> Registry {
        Registry { root: "main".into(), workflows: HashMap::from([("main".to_string(), wf)]) }
    }

    /// Build a registry from `(id, Workflow)` pairs rooted at the first id.
    fn registry(root: &str, workflows: Vec<(&str, Workflow)>) -> Registry {
        Registry {
            root: root.into(),
            workflows: workflows.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        }
    }

    /// Drive a single flat Workflow with default config, the common test path.
    fn drive(
        wf: Workflow,
        msg: &[u8],
        exec: &mut dyn StepExecutor,
        sink: &mut dyn Sink,
    ) -> Result<i32, Fault> {
        run(&single(wf), msg, &RunConfig::default(), exec, sink)
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
        let mut exec = CannedExecutor::returning("s", 0);
        let mut sink = MockSink::default();
        assert_eq!(drive(wf, &[], &mut exec, &mut sink).unwrap(), 10);
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
        let mut exec = CannedExecutor::returning("s", 5);
        let mut sink = MockSink::default();
        assert_eq!(drive(wf, &[], &mut exec, &mut sink).unwrap(), 99);
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
        let mut exec = CannedExecutor::returning("loop", 1);
        let mut sink = MockSink::default();
        assert_eq!(drive(wf, &[], &mut exec, &mut sink).unwrap(), 42);

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
        let mut exec = CannedExecutor::returning("s", 7);
        let mut sink = MockSink::default();
        match drive(wf, &[], &mut exec, &mut sink) {
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
        let mut exec = CannedExecutor::returning("loop", 1);
        let mut sink = MockSink::default();
        match drive(wf, &[], &mut exec, &mut sink) {
            Err(Fault::UnhandledExhaustion { step }) => assert_eq!(step, "loop"),
            other => panic!("expected UnhandledExhaustion, got {other:?}"),
        }
    }

    #[test]
    fn budget_cascade_prefers_step_then_workflow_then_default() {
        // The looping Step always fails, so the cascade alone decides how many
        // activations precede Exhaustion.
        let mut exec = CannedExecutor::returning("loop", 1);

        // Step Budget overrides the Workflow default.
        let mut sink = MockSink::default();
        drive(looping_workflow(Some(5), Some(2)), &[], &mut exec, &mut sink).unwrap();
        assert_eq!(entered(&sink), 2);

        // Workflow default applies when the Step has no Budget.
        let mut sink = MockSink::default();
        drive(looping_workflow(Some(3), None), &[], &mut exec, &mut sink).unwrap();
        assert_eq!(entered(&sink), 3);

        // The hardcoded default applies when neither is set.
        let mut sink = MockSink::default();
        drive(looping_workflow(None, None), &[], &mut exec, &mut sink).unwrap();
        assert_eq!(entered(&sink), DEFAULT_BUDGET as usize);
    }

    #[test]
    fn step_stderr_is_delivered_to_on_output() {
        // A Step's stderr is captured and streamed to the Sink, so a failed
        // Run is diagnosable.
        let wf = workflow(
            "s",
            None,
            vec![("s", step("echo boom >&2; exit 0", None, vec![gate(GateKey::Code(0), GateTarget::Exit(0))]))],
        );
        let mut exec = SubprocessExecutor::new();
        let mut sink = MockSink::default();
        drive(wf, &[], &mut exec, &mut sink).unwrap();
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
        let mut exec = SubprocessExecutor::new();
        let mut sink = MockSink::default();
        drive(wf, &[], &mut exec, &mut sink).unwrap();
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
        let mut exec = SubprocessExecutor::new();
        let mut sink = MockSink::default();
        drive(wf, &[], &mut exec, &mut sink).unwrap();
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
            let mut exec = SubprocessExecutor::new();
            let mut sink = MockSink::default();
            let code = drive(wf, &big, &mut exec, &mut sink).unwrap();
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
        let mut exec = SubprocessExecutor::new();
        let mut sink = MockSink::default();
        assert_eq!(drive(wf, &[], &mut exec, &mut sink).unwrap(), 0);
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
        let mut exec = SubprocessExecutor::new();
        let mut sink = MockSink::default();
        assert_eq!(drive(wf, b"5", &mut exec, &mut sink).unwrap(), 0);
    }

    /// Index of the first Event satisfying `pred`, for asserting relative order.
    fn position(sink: &MockSink, pred: impl Fn(&Event) -> bool) -> usize {
        sink.events.iter().position(pred).expect("expected event not emitted")
    }

    #[test]
    fn composite_step_surfaces_child_exit_code_and_routes_in_parent() {
        // The child reaches an Exit Gate with code 7; the parent's Composite Step
        // routes that surfaced code exactly as it would a leaf's exit code.
        let child = workflow(
            "work",
            None,
            vec![("work", step("noop", None, vec![gate(GateKey::Code(7), GateTarget::Exit(7))]))],
        );
        let parent = workflow(
            "call",
            None,
            vec![("call", composite("child", None, vec![gate(GateKey::Code(7), GateTarget::Exit(100))]))],
        );
        let reg = registry("main", vec![("main", parent), ("child", child)]);
        let mut exec = CannedExecutor::returning("work", 7);
        let mut sink = MockSink::default();
        assert_eq!(run(&reg, &[], &RunConfig::default(), &mut exec, &mut sink).unwrap(), 100);
    }

    #[test]
    fn composite_step_emits_the_frame_bracket_in_order() {
        // A Composite Step brackets the child Frame: StepEntered -> FramePushed ->
        // (child) -> FramePopped -> StepExited{surfaced code}.
        let child = workflow(
            "work",
            None,
            vec![("work", step("noop", None, vec![gate(GateKey::Code(0), GateTarget::Exit(3))]))],
        );
        let parent = workflow(
            "call",
            None,
            vec![("call", composite("sub", None, vec![gate(GateKey::Code(3), GateTarget::Exit(0))]))],
        );
        let reg = registry("main", vec![("main", parent), ("sub", child)]);
        let mut exec = CannedExecutor::returning("work", 0);
        let mut sink = MockSink::default();
        run(&reg, &[], &RunConfig::default(), &mut exec, &mut sink).unwrap();

        let entered = position(&sink, |e| matches!(e, Event::StepEntered { step } if step == "call"));
        let pushed = position(&sink, |e| matches!(e, Event::FramePushed { step, .. } if step == "call"));
        let popped = position(&sink, |e| matches!(e, Event::FramePopped { step, .. } if step == "call"));
        let exited = position(&sink, |e| matches!(e, Event::StepExited { step, code } if step == "call" && *code == 3));
        assert!(entered < pushed && pushed < popped && popped < exited, "frame bracket out of order");
    }

    #[test]
    fn each_sub_workflow_invocation_gets_fresh_budgets() {
        // The child's leaf always fails under a Budget of 2, exhausting to its
        // Exit Gate. The parent invokes the child twice; if Budget lived anywhere
        // but the Frame, the second invocation would start spent and never run the
        // leaf. Four leaf activations prove Budget resets per Frame.
        let child = workflow(
            "task",
            None,
            vec![(
                "task",
                step(
                    "fail",
                    Some(2),
                    vec![
                        gate(GateKey::Default, GateTarget::Step("task".into())),
                        gate(GateKey::Exhausted, GateTarget::Exit(0)),
                    ],
                ),
            )],
        );
        let parent = workflow(
            "first",
            None,
            vec![
                ("first", composite("sub", None, vec![gate(GateKey::Code(0), GateTarget::Step("second".into()))])),
                ("second", composite("sub", None, vec![gate(GateKey::Code(0), GateTarget::Exit(0))])),
            ],
        );
        let reg = registry("main", vec![("main", parent), ("sub", child)]);
        let mut exec = CannedExecutor::returning("task", 1);
        let mut sink = MockSink::default();
        assert_eq!(run(&reg, &[], &RunConfig::default(), &mut exec, &mut sink).unwrap(), 0);

        let leaf_runs = sink
            .events
            .iter()
            .filter(|e| matches!(e, Event::StepEntered { step } if step == "task"))
            .count();
        assert_eq!(leaf_runs, 4, "each sub-Workflow invocation re-runs the leaf to its full Budget");
    }

    #[test]
    fn fault_gate_catches_a_childs_fault() {
        // The child faults (an exit code no Gate covers); the parent's FAULT Gate
        // is the catch clause, recovering to its own Exit code.
        let child = workflow(
            "boom",
            None,
            vec![("boom", step("x", None, vec![gate(GateKey::Code(0), GateTarget::Exit(0))]))],
        );
        let parent = workflow(
            "call",
            None,
            vec![("call", composite("sub", None, vec![gate(GateKey::Fault, GateTarget::Exit(55))]))],
        );
        let reg = registry("main", vec![("main", parent), ("sub", child)]);
        let mut exec = CannedExecutor::returning("boom", 9);
        let mut sink = MockSink::default();
        assert_eq!(run(&reg, &[], &RunConfig::default(), &mut exec, &mut sink).unwrap(), 55);
    }

    #[test]
    fn child_fault_bubbles_when_parent_has_no_fault_gate() {
        // With no FAULT Gate, the child's Fault unwinds frame-by-frame; reaching
        // the root uncaught, it fails the Run carrying its origin.
        let child = workflow(
            "boom",
            None,
            vec![("boom", step("x", None, vec![gate(GateKey::Code(0), GateTarget::Exit(0))]))],
        );
        let parent = workflow(
            "call",
            None,
            vec![("call", composite("sub", None, vec![gate(GateKey::Code(0), GateTarget::Exit(0))]))],
        );
        let reg = registry("main", vec![("main", parent), ("sub", child)]);
        let mut exec = CannedExecutor::returning("boom", 9);
        let mut sink = MockSink::default();
        match run(&reg, &[], &RunConfig::default(), &mut exec, &mut sink) {
            Err(Fault::UnhandledOutcome { step, code }) => {
                assert_eq!(step, "boom");
                assert_eq!(code, 9);
            }
            other => panic!("expected the child's Fault to bubble, got {other:?}"),
        }
    }

    #[test]
    fn self_reference_trips_the_uncatchable_depth_cap() {
        // A Workflow that names itself recurses until the Depth cap. The FAULT
        // Gate must NOT catch the resulting DepthOverflow — it is uncatchable —
        // so the Run fails carrying the offending child id.
        let deep = workflow(
            "recurse",
            None,
            vec![("recurse", composite("deep", None, vec![gate(GateKey::Fault, GateTarget::Exit(0))]))],
        );
        let reg = registry("deep", vec![("deep", deep)]);
        let mut exec = CannedExecutor::default();
        let mut sink = MockSink::default();
        let config = RunConfig::default().with_max_depth(3);
        match run(&reg, &[], &config, &mut exec, &mut sink) {
            Err(Fault::DepthOverflow { workflow }) => assert_eq!(workflow, "deep"),
            other => panic!("expected DepthOverflow, got {other:?}"),
        }
        // The cap held: only Frames up to the limit were pushed (Depths 2 and 3).
        let pushes = sink.events.iter().filter(|e| matches!(e, Event::FramePushed { .. })).count();
        assert_eq!(pushes, 2);
    }

    #[test]
    fn composite_step_threads_the_message_through_the_child() {
        // The Composite Step's in-Message seeds the child's entry Step, and the
        // child's Exit-Gate out-Message becomes the Composite Step's out-Message:
        // `cat` echoes the seed back out of the child, and the parent's successor
        // exits with the number it reads — reaching 0 proves the round trip.
        let child = workflow(
            "echo_in",
            None,
            vec![("echo_in", step("cat", None, vec![gate(GateKey::Code(0), GateTarget::Exit(0))]))],
        );
        let parent = workflow(
            "call",
            None,
            vec![
                ("call", composite("sub", None, vec![gate(GateKey::Code(0), GateTarget::Step("consume".into()))])),
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
        let reg = registry("main", vec![("main", parent), ("sub", child)]);
        let mut exec = SubprocessExecutor::new();
        let mut sink = MockSink::default();
        assert_eq!(run(&reg, b"5", &RunConfig::default(), &mut exec, &mut sink).unwrap(), 0);
    }

    #[test]
    fn composite_step_budget_exhaustion_routes_before_pushing() {
        // A Composite Step's Budget lives in its parent Frame and is spent per
        // entry; once spent, the EXHAUSTED Gate fires *instead of* pushing a child
        // Frame, so the child never runs.
        let child = workflow(
            "work",
            None,
            vec![("work", step("noop", None, vec![gate(GateKey::Code(0), GateTarget::Exit(0))]))],
        );
        let parent = workflow(
            "call",
            None,
            vec![(
                "call",
                composite(
                    "sub",
                    Some(1),
                    vec![
                        gate(GateKey::Code(0), GateTarget::Step("call".into())),
                        gate(GateKey::Exhausted, GateTarget::Exit(7)),
                    ],
                ),
            )],
        );
        let reg = registry("main", vec![("main", parent), ("sub", child)]);
        let mut exec = CannedExecutor::returning("work", 0);
        let mut sink = MockSink::default();
        assert_eq!(run(&reg, &[], &RunConfig::default(), &mut exec, &mut sink).unwrap(), 7);
        // The Composite Step entered once (Budget 1); the child ran exactly once.
        let pushes = sink.events.iter().filter(|e| matches!(e, Event::FramePushed { .. })).count();
        assert_eq!(pushes, 1);
    }
}
