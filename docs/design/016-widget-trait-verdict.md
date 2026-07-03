# ADR 016: Widget-Trait Verdict (Immediate-Mode Widgets Stay Functions)

**Status:** Accepted, partially amended **Date:** 2026-07-01 **Amended by:**
[ADR 014: Workspace Split](014-workspace-split.md) (2026-07-02) -- `retroglyph-widgets` ships as
part of the workspace split regardless of a second consumer, overriding the "wait for a second
consumer" call below. The _design_ verdict (free functions, no `Widget` trait, no retained widget
tree) still stands; only the _timing_ of publishing a crate for it changed. **Related:**
[ADR 008: Layer Composition](008-layer-composition.md)

## Context

The dashboard demo (`examples/dashboard.rs`, plan in `.matan/dashboard-demo.md`) was built as the
second UI-heavy consumer after the scrolling roguelike. Its explicit job was not to ship a system
monitor but to answer one design question with evidence rather than speculation:

> Does a `Widget` trait earn its keep, or are immediate-mode draw functions enough?

The demo was built the way the plan prescribed: plain functions first (`gauge`, `sparkline`,
`table`, `meter_ramp` in `examples/util/draw.rs`), a small `Rect` splitter
(`examples/util/layout.rs`), and app-owned state (the ratatui `*State` pattern) in the `Dashboard`
struct. It renders four widget kinds across five panels with a keyboard-selectable process table,
driven by a `FrameClock` at a fixed 8 Hz.

## Decision

**Keep the widgets as free functions in `draw.rs`. Do not introduce a `Widget` trait yet, and do not
extract `retroglyph-widgets`.**

## Evidence

The plan named three criteria. Here is how each played out.

### 1. Do the widgets share one signature and compose?

Partially, and the shared shape did _not_ create pressure for a trait. Every helper converged on
`(&mut Terminal<B>, area: Rect, …)` and draws directly into a sub-`Rect`. Composition happens at the
_layout_ layer (`split_v`/`split_h` hand each widget a `Rect`), not through widget nesting. A widget
never contains another widget; the app slices the screen and calls each function. That is exactly
the case where a trait buys nothing — there is no heterogeneous collection to store, no `dyn Widget`
list to iterate, no recursive `render` to dispatch.

### 2. Is stateful rendering awkward with free functions + app state?

No. History lives in the app as bounded `VecDeque` ring buffers; the sim mutates state on the
`FrameClock` step and the draw pass reads it. Widgets stay ephemeral and take borrowed slices
(`&[f32]`, `&[Vec<String>]`). The ratatui `StatefulWidget` split (`render(&mut State)`) would only
help if a widget needed to persist scroll/selection _inside itself_ — but selection lives naturally
in `Dashboard::selected`, and passing it as a parameter (`table(…, selected)`) was cleaner than
threading a mutable state object.

### 3. Did either pain show up?

Neither did. The functions compose through `Rect` slicing, stay generic over `B: Backend` (so they
remain headless-testable — see `renders_panels_headless`), and read app state by borrow. A trait
here would be abstraction for its own sake.

## What did earn its keep

- **The layout splitter.** `split_v`/`split_h` with `Constraint::{Fixed, Percent, Fill}` was the
  first thing the multi-panel UI needed and nothing in the repo did it. It is the real reusable
  seed, more so than any widget trait. It is unit-tested and small; a future `retroglyph-widgets`
  (or a `layout` module) should grow from here.
- **App-owned state.** Confirmed the immediate-mode + `*State`-in-the-app pattern scales to a busy
  screen without a retained widget tree.

## Consequences

- The widget helpers remain in `examples/util/draw.rs` until a _second_ real consumer appears with a
  composition or storage need that functions genuinely cannot serve. ADR 014 still reserves the
  `retroglyph-widgets` name for that day; this ADR just records that the day has not arrived.
- Revisit the trait question if/when we hit: a heterogeneous widget collection
  (`Vec<Box<dyn Widget>>`), recursive container widgets, or focus/event routing across widgets.
  Those are the deferred `retroglyph-app` concerns and are explicit non-goals of this demo.
- The two loop-ergonomics gaps the demo surfaced are tracked separately (they do not affect this
  decision): `rg_run!`'s `tick` hides `Frame.dt` (worked around with `util::timestep::Stopwatch`),
  and `run_blocking` has no frame pacing on crossterm (worked around by pacing via `poll(timeout)`).
  Both are candidate follow-ups for a loop-ergonomics pass, not blockers.
