# ADR 015: Cross-Backend Rendering and Loop Consistency

**Status:** Accepted **Date:** 2026-07-01 **Amends:**
[ADR 008: Layers and Sub-cell Offsets](008-layer-composition.md) (§5, M4) **Relates to:**
[ADR 001: Architecture](001-architecture.md) (§2, §3, §6),
[ADR 007: Software Backend](007-software-backend.md),
[ADR 011: WASM Portability](011-wasm-portability-revised.md),
[ADR 014: Workspace Split](014-workspace-split.md)

**Sequencing:** Lands _before_ [ADR 014: Workspace Split](014-workspace-split.md). This is the
API-stabilizing work ADR 001 §1 named as the precondition for splitting ("split when the API
stabilizes"): it settles `Tile`'s public shape across features (Decision 3), the `Backend` trait's
`composites_layers` capability (Decision 1), and core's loop surface (Decision 2). Doing it while
everything is one unpublished crate keeps these cross-cutting changes free; deferring them past the
split turns each into a breaking change across the core → crossterm → software → facade publish
chain. Suggested order: Decision 3, then Decision 1, then Decision 2 (at minimum its core contract)
before the split.

## Context

Three inconsistencies have surfaced now that multiple backends and several example games exist. Each
one is small on its own, but each pushes complexity onto game code that the library should absorb.
All three are cheapest to fix before the workspace split (ADR 014) freezes crate boundaries and the
public `retroglyph-core` surface.

### 1. Layers are a software-only feature in practice

ADR 008 §5 decided that the default `draw_layers` extracts layer 0 and ignores higher layers, and
that this is "correct behaviour for an ANSI terminal." `Crossterm` and `Headless` therefore never
see layers 1+. The reasoning held when layers were assumed to be a _pixel-compositing_ concern
(sub-cell offsets, alpha).

In practice, games use layers as a _logical authoring_ convenience: terrain on 0, entities on 1, UI
on 2. `examples/roguelike_dungeon.rs` builds its whole scene this way, then works around the
terminal by remapping every layer to 0:

```rust
// examples/roguelike_dungeon.rs
let layer = |n: u8| if cfg!(feature = "software") { n } else { 0 };
term.layer(layer(2)); // UI layer on software, layer 0 on crossterm
```

Every multi-layer game that wants to run on crossterm must carry this hack. Worse, remapping to 0
means later layers overwrite earlier ones by draw order rather than by z-order, so on crossterm the
"compositing" is accidental. Layers should render identically on every backend; only _how_ they
composite (cell flattening vs pixel blitting) should differ.

### 2. The game-loop control model is inverted between backends

ADR 001 §6 committed to a user-owned loop (`while running { draw; present; poll }`), and `Crossterm`
honours it. The software backend (ADR 007) is driven by winit, which owns the loop, so it exposes
`run_windowed(closure)` instead. WASM (ADR 011) makes the inversion mandatory: frames are gated by
`requestAnimationFrame`.

`examples/util/mod.rs` hides the split behind the `rg_run!` macro, which pays for it with lazy
`Option<State>` init, a `quit` flag, and a `std::process::exit(0)` that bypasses normal teardown:

```rust
backend.run_windowed(move |term| {
    if quit { return; }
    if state.is_none() { state = Some($init(&mut *term)); }
    if !$tick(&mut *term, state.as_mut().unwrap()) {
        quit = true;
        #[cfg(not(target_arch = "wasm32"))]
        ::std::process::exit(0);
    }
});
```

The macro exists only to paper over an asymmetry the library created. Game code that wants to target
both backends must either use the macro or hand-write both loop shapes. There is also no shared
frame-timing story: `FixedStep` lives in `examples/util/timestep.rs` and is re-instantiated per
game.

### 3. `Tile`'s field layout changes with the `egc` feature

`Tile::flags` and `Tile::extra` (and the `flags()`, `grapheme()`, `extra()` accessors) only exist
when `egc` is enabled. Downstream code that inspects tiles must become `cfg`-aware, and the
`retroglyph-core` crate that ADR 014 publishes would expose a public struct whose shape shifts by
feature. This is a poor stability guarantee for a foundational type.

## Decision

### Decision 1: Composite layers in the core, not in each backend

`Terminal::present` becomes responsible for producing backend-appropriate content. Backends declare
one capability:

```rust
pub trait Backend {
    /// Whether this backend composites layers itself (per-pixel), receiving the
    /// raw layered stream. Backends that render one glyph per cell return
    /// `false` and receive a pre-flattened, single-layer stream.
    fn composites_layers(&self) -> bool { false }
    // ... existing methods unchanged ...
}
```

- `SoftwareRenderer` returns `true`. It keeps receiving the raw `(layer, Pos, &Tile)` stream and
  blits per-pixel exactly as ADR 008 M4 specifies (sub-cell offsets, transparent higher-layer
  backgrounds).
- `Crossterm` and `Headless` return `false` (the default). `Terminal` flattens all allocated layers
  into a single composited frame, diffs _that_ against the previously composited frame, and hands
  the backend a layer-0-only stream.

Cell flattening rule (matches the software renderer's pixel semantics and the existing `Grid::blit`
transparency convention):

- Start from layer 0's tile (its `bg` fills the cell).
- For each higher allocated layer at that cell, in ascending order:
  - if `glyph != ' '`, replace `glyph`, `fg`, and `modifiers`;
  - if `bg != Color::Default`, replace `bg`.

`Terminal` holds two single-layer scratch grids (`flattened_current`, `flattened_previous`) used
only on the non-compositing path, so diff efficiency is preserved for the terminal. The
`cfg!(feature = "software")` remap in the roguelike example is deleted; games author on layers
unconditionally.

This amends ADR 008 §5 ("ignore higher layers") and M4 (which put the layer-0 extraction in the
default `draw_layers`). The extraction logic moves up into `present` as true flattening.

While `present`/`diff` are being touched, replace the per-layer `Box<dyn Iterator>` in `Grid::diff`
with a small enum iterator (`Empty | Full | Diff`) to drop the per-frame heap allocation. This is a
mechanical change riding along with the compositing rework, not a separate project.

### Decision 2: One `App`-driven loop; the loop decomposes across crates

Introduce a callback-driven entry point as the recommended way to run a game. The game implements
one trait; the low-level `poll`/`present` API stays for turn-based games and headless tests (ADR 001
§6 preserved, not replaced).

The loop is three separable pieces with distinct homes. This mirrors the fact that only _some_
backends can own a generic loop, and that the inverted loop is a _windowing_ concern shared across
the whole windowed backend family (software today; wgpu / opengl / webgl2 later, see ADR 014).

**1. The contract (core).** `App` is the dual of `Backend`: `Backend` is the output contract, `App`
is the update contract. Both are dep-free and belong together in `retroglyph-core`. It imposes no
loop; it is a callback shape.

```rust
pub enum Flow { Continue, Exit }

pub struct Frame {
    pub dt: Duration,   // supplied by the driver; core::time::Duration (no_std)
    pub number: u64,    // monotonic frame counter
}

pub trait App<B: Backend> {
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow;
}
```

Games target `impl<B: Backend> App<B> for Game`, so one impl runs on every backend (terminal,
software, and future GPU) unchanged.

**2. The generic blocking driver (core, `std`).** Fully generic over `B`, so it covers crossterm,
headless, and any future non-inverted backend with zero per-backend loop code:

```rust
pub fn step<B: Backend, A: App<B>>(term: &mut Terminal<B>, app: &mut A, frame: &Frame) -> Flow;
pub fn run_blocking<B: Backend, A: App<B>>(term: Terminal<B>, app: A); // while Flow::Continue { step(..) }
```

`Crossterm::run(app)` is a one-liner over `run_blocking`. `step` is factored out so the inverted
driver shares the exact per-frame body.

**3. The inverted driver (windowing layer).** winit / wasm owns the loop and needs a `'static`
closure, so this cannot be generic. It lives with the windowing code, **not** in each renderer,
because it is identical for every windowed backend (software, wgpu, gl). Today it sits inside
`retroglyph-software` as a self-contained, liftable module; when the second windowed backend lands
it extracts to `retroglyph-window` (ADR 014). The inverted `run` drives the same `App` by calling
core's `step` once per `requestAnimationFrame` / redraw. Returning `Flow::Exit` breaks the loop and
unwinds so `Drop` runs, with no `Option<State>` init and no `process::exit`.

`FrameClock` (graduating `examples/util/timestep.rs`) becomes a **pure accumulator**:
`advance(dt) -> pending_steps`, taking `dt` as input rather than reading a clock. This makes it
no_std-clean (lives in core, unconditional) and removes the wasm `Instant`-returns-zero shim: the
platform clock read stays in each driver (`run_blocking` uses `Instant`; the windowing driver uses
winit / rAF frame timing).

The `retroglyph` meta crate re-exports a feature-selected `run`: terminal backends resolve to core's
`run_blocking`; any windowed backend resolves to the windowing layer's `run` with the chosen
renderer. This replaces `rg_run!`. Turn-based games ignore `App` and keep calling `poll`/`present`.

### Decision 3: `Tile`'s layout is stable across `egc`, and lean

`Tile`'s public shape and accessors (`flags()`, `grapheme()`, `extra()`) become unconditional. Only
the _segmentation logic_ (grapheme splitting, wide-char spacer placement in `write_grapheme`) stays
behind the `egc` feature. The `retroglyph-core` public API no longer changes shape by feature.

Measured `size_of::<Tile>()` today: 32 bytes with `egc` (align 8, forced by the inline
`Option<Arc<String>>`), 20 bytes without. Two implementations were considered:

1. **Side-table (rejected for now).** Move `extra` out of `Tile` into a sparse per-layer map,
   keeping only a `flags` bit. This shrinks `Tile` toward 16-20 bytes and moves the `Arc` off the
   hot path. It was rejected during implementation because the draw path delivers `&Tile` to
   backends via `Backend::draw_layers`, and both crossterm and software read the full grapheme
   (`cell.extra`) at draw time. A side-table would force widening the draw item to
   `(layer, Pos, &Tile, Option<&str>)` across the `Backend` trait and every call site and test. That
   is a larger `Backend`-trait change that belongs with the input/output trait split ADR 014 defers
   to `retroglyph-window` extraction, not with this ADR.
2. **Inline + unconditional (chosen).** `flags: TileFlags` and `extra: Option<Arc<String>>` become
   unconditional inline fields; `dx`/`dy` stay inline. Without `egc`, `write_grapheme` never runs,
   so `extra` is always `None` and `flags` always empty; they cost storage but never allocate.
   `Tile` is 32 bytes across every feature configuration.

The chosen approach satisfies this ADR's actual requirement, a `Tile` whose _public shape does not
change by feature_, which is what makes the `retroglyph-core` split (ADR 014) clean. The size shrink
is deferred to the trait-split work, where widening or replacing the draw item is already on the
table. The no-`egc` build grows from 20 to 32 bytes; that is the accepted cost of a stable public
type, and no-`egc` size-sensitive users can be revisited by the deferred side-table.

## Consequences

- Multi-layer games render identically on crossterm, headless, and software with no `cfg` branches.
  The roguelike example loses its `layer()` closure.
- Crossterm gains real z-ordered compositing (topmost non-transparent glyph wins) instead of
  last-write-wins, which is a behaviour change for any code that relied on draw order across layers.
  No shipped example does.
- `present` does more work on the non-compositing path (one flatten pass over allocated layers). For
  single-layer games the flatten is a cheap copy of layer 0; the diff still sends only changed
  cells. Multi-layer terminal games trade a full-grid flatten for correctness. Acceptable at
  terminal grid sizes; revisit if a dashboard-scale demo shows it on a profile.
- `rg_run!` is removed. Examples shrink to an `App` impl plus one `run(app)` call. The macro's
  `Option<State>` and `process::exit` disappear.
- `FrameClock` in core gives every game a consistent fixed-timestep option and a real `dt`,
  including a monotonic frame counter for animation.
- `Tile` gains a stable public shape across `egc`: `flags`/`extra`/`dx`/`dy` and their accessors are
  always present. It is 32 bytes in every configuration (the no-`egc` build grows from 20 to 32).
  The `size_of::<Tile>()` TODO in `tile.rs` stays open, now pointing at the deferred side-table as
  the shrink path.
- No draw-path or `Backend`-trait change: backends keep reading `cell.extra` from the `&Tile` they
  already receive.

## Risks

- **Compositing semantics drift.** The cell-flatten rule must match the software renderer's pixel
  rule or the same game looks different per backend. Mitigation: a shared snapshot test that renders
  one multi-layer scene through `Headless` (flattened) and asserts the composited glyph/fg/bg per
  cell, plus the existing software pixel snapshot.
- **`App` generic ergonomics.** `App<B: Backend>` ties the impl to one backend type. Games that
  target both crossterm and software implement it once with a generic
  `impl<B: Backend> App<B> for Game`, mirroring the current generic `tick<B>` fns. Documented in the
  migration note.
- **Behaviour change for draw-order-dependent code.** Covered above; no shipped example depends on
  it, but call it out in the changelog.
- **Scope creep into a game framework.** `App` + `FrameClock` is deliberately the floor, not a
  scene/widget system. Widgets, input maps, and cameras remain out of scope here (see Non-goals) and
  belong to the follow-up crates discussed separately.

## Non-goals

- **Widgets, input maps, cameras, scene loaders.** These are the `retroglyph-app` /
  `retroglyph-widgets` opportunity, tracked outside this ADR.
- **Per-cell tile stacking.** Still rejected (ADR 008). Flattening picks one tile per cell; it does
  not retain a stack.
- **Making crossterm represent sub-cell offsets.** `dx`/`dy` remain visual-only and ignored on cell
  backends, unchanged from ADR 008.
- **Removing the user-owned loop.** `poll`/`present` stay first-class for turn-based and headless
  use.

## Migration plan

1. Add `Backend::composites_layers` (default `false`); `SoftwareRenderer` overrides to `true`. Move
   layer-0 extraction out of the default `draw_layers` and into a new `Terminal` flatten pass; add
   the two scratch grids. Swap `Grid::diff`'s boxed iterator for an enum iterator.
2. Make `Tile`'s fields (`flags`, `extra`, `dx`, `dy`) and accessors (`flags()`, `grapheme()`,
   `extra()`) unconditional and inline; gate only the segmentation logic (`write_grapheme`,
   `cap_grapheme`, spacer placement) on `egc`. Update the non-`egc` code paths that assumed the
   fields were absent.
3. Add `Flow`, `Frame`, `App`, `FrameClock` to core. Implement `run` on `Crossterm`, `Headless`, and
   `SoftwareBackend`. Re-export a feature-selected `run` from the facade. Delete `rg_run!` and port
   examples.
4. Delete the `cfg!(feature = "software")` layer remap in `examples/roguelike_dungeon.rs`; add the
   cross-backend compositing snapshot test.

## References

- [ADR 001: Architecture](001-architecture.md) — user-owned loop, cell model
- [ADR 007: Software Backend](007-software-backend.md) — winit-owned loop
- [ADR 008: Layers and Sub-cell Offsets](008-layer-composition.md) — amended here
- [ADR 011: WASM Portability](011-wasm-portability-revised.md) — rAF frame gating
- [ADR 014: Workspace Split](014-workspace-split.md) — crate boundaries this precedes
- `examples/roguelike_dungeon.rs` — the layer-remap workaround
- `examples/util/mod.rs`, `examples/util/timestep.rs` — `rg_run!`, `FixedStep`
