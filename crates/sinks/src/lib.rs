//! Sinks — concrete Modules over the Kernel's `Sink` Observer interface.
//! The Kernel never persists or renders anything itself; these
//! implementations do, and they depend on the Kernel, never the reverse:
//!
//! * [`ConsoleSink`] — a live trace on the console.
//! * [`FileSink`] — the durable per-Run JSONL log.
//! * [`Tee`] — fan-out so the Kernel's single Sink slot drives several at once.
//!
//! Plus the durable Sink's support: log-directory resolution ([`runs_dir`],
//! [`resolve_keep`], [`run_scratch_dir`], [`resolve_max_depth`]) and run-directory
//! retention ([`prune`]). These stay env/argv-free pure functions; the
//! composition root reads the environment and passes the values in.

mod config;
mod console;
mod file_sink;
mod retention;
mod tee;

pub use config::{resolve_keep, resolve_max_depth, run_scratch_dir, runs_dir, DEFAULT_KEEP};
pub use console::ConsoleSink;
pub use file_sink::FileSink;
pub use retention::prune;
pub use tee::Tee;
