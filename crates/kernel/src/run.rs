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
