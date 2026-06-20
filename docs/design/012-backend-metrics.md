# ADR 012: Backend Metrics & Instrumentation

**Status:**Draft**Date:**2026-06-20**Parent:** [ADR 007: Software Rendering Backend](007-software-backend.md)

## Context

The software, crossterm, and headless backends have no runtime metrics or instrumentation. This makes
it hard to diagnose performance issues — frame drops, rendering bottlenecks, and diff efficiency.
Adding metrics would help both library developers (debugging) and end users (choosing backends,
tuning parameters).

## Research Summary

Three dominant patterns exist in the ecosystem (see `docs/references/` for deep dives):

| Pattern | Example | Approach | Pros | Cons |
|---------|---------|----------|------|------|
| **Centralized store** | Bevy `DiagnosticsStore` | Named diagnostic paths, deferred collection via system buffer, EMA/SMA smoothing | Rich history, multi-consumer, lazy evaluation | ECS-specific; heavyweight for simple use |
| **Flat snapshot struct** | notcurses `ncstats` | ~36 cumulative `u64` fields, atomic copy on query | Simple, cheap, thread-safe | No history, no smoothing |
| **Macro-generated registries** | `metered` crate | `#[measure]` annotations generate per-method counters | Zero-cost if disabled, per-instance | Proc-macro complexity, HdrHistogram allocations |

## Proposed Design

A lightweight hybrid of the notcurses and metered patterns, appropriate for a non-ECS library:

### Core types

```rust
/// Backend-agnostic metrics that every backend can report.
pub struct BackendMetrics {
    /// Cumulative frames rendered.
    pub frames_rendered: AtomicU64,
    /// Cells changed in the last frame (diff count).
    pub cells_this_frame: AtomicU64,
    /// Cumulative cells changed across all frames.
    pub cells_total: AtomicU64,
}

/// Software-backend-specific metrics.
pub struct SoftwareMetrics {
    /// Cumulative pixel buffer flushes that were dropped (channel full).
    pub frame_drops: AtomicU64,
    /// Cumulative sprite blend operations performed.
    pub sprite_blends: AtomicU64,
    /// Cumulative opaque sprite writes (alpha=255 fast path hits).
    pub sprite_opaque_writes: AtomicU64,
}

/// Crossterm-backend-specific metrics.
pub struct CrosstermMetrics {
    /// Cumulative ANSI bytes written to the terminal.
    pub ansi_bytes_written: AtomicU64,
    /// Cumulative cursor position moves.
    pub cursor_moves: AtomicU64,
}
```

### Collection

Metrics are collected via **lock-free atomic increments** in hot paths (flush, draw, blit_sprite).
This matches the notcurses pattern and avoids allocation/contention in the render loop.

```rust
// In SoftwareRenderer::flush():
if self.metrics.frame_drops.fetch_add(1, Ordering::Relaxed) > 0 {
    // Track frame drops
}
```

### Consumption

Consumers query metrics via snapshot methods on each backend:

```rust
impl SoftwareRenderer {
    /// Returns an atomic snapshot of all software backend metrics.
    pub fn metrics_snapshot(&self) -> SoftwareMetricsSnapshot {
        SoftwareMetricsSnapshot {
            frame_drops: self.metrics.frame_drops.load(Ordering::Acquire),
            sprite_blends: self.metrics.sprite_blends.load(Ordering::Acquire),
            sprite_opaque_writes: self.metrics.sprite_opaque_writes.load(Ordering::Acquire),
        }
    }
}
```

### Feature flag

Metrics are **always compiled** (zero-cost when not read — single atomic load at snapshot time).
No feature flag is needed. The `AtomicU64` fields add 24 bytes per metrics struct.

## Files to modify

- `src/metrics.rs` (new) — BackendMetrics, SoftwareMetrics, CrosstermMetrics structs
- `src/backend/mod.rs` — Add `metrics()` method to Backend trait (default returns None)
- `src/backend/software/mod.rs` — Add SoftwareMetrics to SoftwareRenderer, instrument flush/draw
- `src/backend/crossterm.rs` — Add CrosstermMetrics to CrosstermBackend, instrument write calls
- `src/backend/headless.rs` — Add BackendMetrics to Headless, instrument draw

## Non-goals

- **Frame time measurement** — deferred to a separate frame pacing effort (requires high-res timers)
- **Per-cell diff tracking** — notcurses tracks ellelisions/emissions; rg can add this later
- **Histogram-based metrics** — Bevy's SMA/EMA is sufficient; HdrHistogram adds allocation in hot path
- **Logging or export** — snapshot structs are pull-based; logging is a consumer concern

## References

- [notcurses_stats(3)] — 36-field cumulative stats struct with atomic snapshots
- [Bevy FrameTimeDiagnosticsPlugin] — Centralized store pattern with deferred collection
- [Ratatui buffer diff benchmarks] — Proves diff count as most valuable rendering metric
- [metered crate] — Macro-generated per-instance metric registries
