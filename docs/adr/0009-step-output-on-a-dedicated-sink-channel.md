# Step output travels on a dedicated Sink channel, not the Event stream

To diagnose a failed Run after the fact, a Sink must be able to persist what a
Step actually printed — today stderr is inherited by the terminal and lost. The
Kernel will capture Step output and deliver it to Sinks, but **as bulk data on a
channel separate from the Event stream**, keeping Events a lean control-plane
trace.

## Decision

- **Two planes, one Sink.** `Sink` gains a second method alongside `emit(&Event)`:
  `on_output(step, activation, stream, &bytes)`. `emit` carries control-plane
  *transitions* (the existing Event vocabulary); `on_output` carries the
  data-plane *bulk bytes* a Step writes. Bulk output is never wrapped in an Event.
- **The Kernel captures, never persists.** `invoke` pipes the child's stdout and
  stderr (instead of inheriting stderr) and streams chunks to `on_output` as they
  arrive. The Kernel still opens no files and chooses no format (ADR-0003, 0005).
- **Run identity is the orchestrator's, not the Kernel's.** The orchestrator (the
  driver that embeds the Kernel and constructs its Sinks) mints a **UUIDv7** Run
  ID and hands it to the Sinks at construction. The Kernel remains unaware of Run
  identity; it is a persistence concern. UUIDv7 is time-sortable, filesystem-safe,
  and *is* a 128-bit UUID, so a future OpenTelemetry Sink can use it as a TraceId
  directly. The choice is opaque and reversible — not itself load-bearing.
- **Metadata is Sink-assigned.** A persistence Sink stamps each record with a
  monotonic `seq` and a wall-clock `ts` on receipt (see ADR-0005). The Kernel adds
  neither, staying dependency-light and deterministic.

## Consequences

- The console trace must re-emit `on_output` to keep Step output visible live now
  that stderr is piped rather than inherited; a persistence Sink writes it to disk.
- Correlating the two planes is trivial because execution is single-token
  (ADR-0004): every `on_output` for a Step falls between that Step's `StepEntered`
  and `StepExited`. The `activation` index disambiguates a Step run more than once
  under its Budget (and seeds a future OTel SpanId).
- Bulk output stays raw (no base64 inflation of the record stream) and a future
  OpenTelemetry Sink maps the Event stream to spans without having to strip bulk
  payloads out of it.

## Considered and rejected

- **Output rides as Events** (a `StepOutput` event, or fattening `StepExited`).
  Rejected: an Event records *one transition*, but output is a continuous stream.
  Forcing it into Events means either buffering until the Step exits (no live view
  of a multi-minute agent Step) or emitting hundreds of chunk-events that pollute
  the control trace and burden every Sink — the opposite of the trace-mappable
  stream we want. The lone benefit (no interface change) is marginal against this.
- **Kernel stays bare** (keep inheriting stderr; persist only control Events).
  Rejected: it cannot meet the goal — "why did this Step exit non-zero?" stays
  unanswerable because the bytes are never captured by anything.
- **OpenTelemetry as the mechanism.** Deferred, not adopted: it belongs in a Sink,
  not the Kernel, and does nothing about the capture gap above. It also wants a
  collector/backend that is heavy for a local CLI. Kept viable as a future Sink by
  keeping the Event stream trace-shaped and Run identity a 128-bit UUID.
