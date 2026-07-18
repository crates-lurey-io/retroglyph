//! Performance benchmarks for retroglyph (not published).
//!
//! Kept as a separate top-level crate rather than folded into `examples/` (see
//! `examples/AGENTS.md`, "No perf/benchmark examples here"): that crate's docs-gallery/regression-
//! suite purpose (does an example render correctly) is a different concern from performance
//! measurement (how fast does an operation run), and its snapshot harness isn't built to gate on
//! throughput. Benchmark targets live under `benches/benches/*.rs`; this `lib.rs` exists only
//! because Cargo requires a package to have at least one target.
