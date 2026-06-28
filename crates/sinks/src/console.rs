//! A trace Sink: one line per Kernel transition on stdout, plus a live, dim,
//! step-prefixed tee of Step output on stderr. The console rendering — ANSI
//! styling and the Event-to-line mapping — is a Module concern, kept out of the
//! Kernel (ADR-0003).

use kernel::{Event, Fault, GateKey, GateTarget, Sink, Stream};

// ANSI helpers, to avoid a styling-crate dependency.
const B: &str = "\x1b[1m"; // bold
const D: &str = "\x1b[2m"; // dim
const R: &str = "\x1b[0m"; // reset

/// A trace Sink: one line per Kernel transition, plus a live tee of Step output
/// (dim, step-prefixed) on stderr. `quiet` suppresses the output tee for a
/// control-only trace; the control Events still print.
pub struct ConsoleSink {
    quiet: bool,
}

impl ConsoleSink {
    /// Construct a console trace Sink. When `quiet`, the live Step-output tee on
    /// stderr is suppressed; the control Events still print.
    pub fn new(quiet: bool) -> Self {
        ConsoleSink { quiet }
    }
}

impl Sink for ConsoleSink {
    fn emit(&mut self, event: &Event) {
        println!("{}", render(event));
    }

    fn on_output(&mut self, step: &str, _activation: u32, _stream: Stream, bytes: &[u8]) {
        if self.quiet {
            return;
        }
        // Tee to stderr so Step output never mixes with a downstream consumer
        // of the orchestrator's stdout.
        eprint!("{}", dim_prefixed(step, bytes));
    }
}

/// Render a chunk of Step output as dim, step-prefixed lines for the live view.
fn dim_prefixed(step: &str, bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.lines()
        .map(|line| format!("  {D}{step} ▏ {line}{R}\n"))
        .collect()
}

fn render(event: &Event) -> String {
    match event {
        Event::RunStarted => format!("{B}● RUN started{R}"),
        Event::StepEntered { step } => format!("  {B}▶ enter{R} {step}"),
        Event::StepExited { step, code } => format!("  {D}■ exit{R}  {step} {D}->{R} code {B}{code}{R}"),
        Event::BudgetConsumed { step, remaining } => {
            format!("  {D}· budget {step}: {remaining} left{R}")
        }
        Event::Exhausted { step } => format!("  {B}⊘ EXHAUSTED{R} {step} {D}(budget spent){R}"),
        Event::FramePushed { step, workflow, depth } => {
            format!("  {B}╆ push{R} {step} {D}->{R} {workflow} {D}(depth {depth}){R}")
        }
        Event::FramePopped { step, workflow } => {
            format!("  {D}╄ pop{R}   {step} {D}<-{R} {workflow}")
        }
        Event::GateTaken { step, key, target } => {
            format!("  {D}↳ gate{R} {step} {D}[{}]{R} {D}->{R} {}", fmt_key(key), fmt_target(target))
        }
        Event::MessagePassed { from, to, bytes } => {
            format!("  {D}✉ message {from} -> {to} ({bytes} bytes){R}")
        }
        Event::FaultRaised { fault } => format!("  {B}✗ FAULT{R} {}", fmt_fault(fault)),
        Event::RunEnded { code } => format!("{B}◆ RUN ended -> exit {code}{R}"),
    }
}

fn fmt_key(key: &GateKey) -> String {
    match key {
        GateKey::Code(n) => n.to_string(),
        GateKey::Default => "*".to_string(),
        GateKey::Exhausted => "EXHAUSTED".to_string(),
        GateKey::Fault => "FAULT".to_string(),
    }
}

fn fmt_target(target: &GateTarget) -> String {
    match target {
        GateTarget::Step(s) => s.clone(),
        GateTarget::Exit(code) => format!("EXIT {code}"),
    }
}

fn fmt_fault(fault: &Fault) -> String {
    match fault {
        Fault::UnhandledOutcome { step, code } => {
            format!("unhandled outcome: {step} exited {code} with no matching Gate")
        }
        Fault::UnhandledExhaustion { step } => {
            format!("unhandled exhaustion: {step} spent its Budget with no EXHAUSTED Gate")
        }
        Fault::DepthOverflow { workflow } => {
            format!("depth overflow: entering {workflow} would exceed the max Depth")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::dim_prefixed;

    #[test]
    fn dim_prefixed_tags_each_line_with_the_step() {
        let rendered = dim_prefixed("build", b"first\nsecond\n");
        assert!(rendered.contains("build ▏ first"));
        assert!(rendered.contains("build ▏ second"));
    }
}
