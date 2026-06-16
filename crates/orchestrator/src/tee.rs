//! A fan-out Sink: forwards every `emit` and `on_output` to each wrapped Sink,
//! so the Kernel's single Sink slot can drive several Modules (e.g. a live
//! console trace and a durable file log) at once.

use kernel::{Event, Sink, Stream};

/// Broadcasts to its wrapped Sinks in registration order.
pub struct Tee {
    sinks: Vec<Box<dyn Sink>>,
}

impl Tee {
    pub fn new(sinks: Vec<Box<dyn Sink>>) -> Self {
        Tee { sinks }
    }
}

impl Sink for Tee {
    fn emit(&mut self, event: &Event) {
        for sink in &mut self.sinks {
            sink.emit(event);
        }
    }

    fn on_output(&mut self, step: &str, activation: u32, stream: Stream, bytes: &[u8]) {
        for sink in &mut self.sinks {
            sink.on_output(step, activation, stream, bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Tee;
    use kernel::{Event, Sink, Stream};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A Sink that tallies the calls it received, shared so the test can read it
    /// back after the Tee has taken ownership.
    #[derive(Default)]
    struct Counter {
        emits: usize,
        outputs: usize,
    }

    #[derive(Clone, Default)]
    struct SharedCounter(Rc<RefCell<Counter>>);

    impl Sink for SharedCounter {
        fn emit(&mut self, _event: &Event) {
            self.0.borrow_mut().emits += 1;
        }
        fn on_output(&mut self, _step: &str, _activation: u32, _stream: Stream, _bytes: &[u8]) {
            self.0.borrow_mut().outputs += 1;
        }
    }

    #[test]
    fn forwards_both_channels_to_every_sink() {
        let a = SharedCounter::default();
        let b = SharedCounter::default();
        let mut tee = Tee::new(vec![Box::new(a.clone()), Box::new(b.clone())]);

        tee.emit(&Event::RunStarted);
        tee.on_output("s", 0, Stream::Stdout, b"hi");

        for shared in [&a, &b] {
            let c = shared.0.borrow();
            assert_eq!(c.emits, 1);
            assert_eq!(c.outputs, 1);
        }
    }
}
