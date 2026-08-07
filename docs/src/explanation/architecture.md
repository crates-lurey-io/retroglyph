# Architecture

`Terminal<B>` owns a double-buffered `Grid` and the `Backend` lifecycle (resize, present,
events). Drawing itself goes entirely through `Surface`, handed out by `Terminal::draw`/
`Terminal::surface`: a game calls `term.draw(|s| { s.put(...); ... })` once per frame, and
`present` diffs the current frame against the previous one, sending only changed cells to the
`Backend`. `B` is the only thing that changes between a headless test and a real window or
terminal:

```text
              ┌───────────────────────────┐
              │      App::update(...)      │  game logic, once, generic over B
              └──────────────┬─────────────┘
                             │ term.draw(|s| ...): writes through Surface
                             ▼
              ┌───────────────────────────┐
              │       Terminal<B>          │  double-buffered Grid, cell diff
              └──────────────┬─────────────┘
                             │ draw / draw_layers / poll_event
                             ▼
              ┌───────────────────────────┐
              │  B: Output + Input + Cursor │  the only piece that swaps out
              └──────────────┬─────────────┘
                             │
       ┌─────────────────────┼─────────────────────┐
       ▼                     ▼                      ▼
 Headless (core)      Crossterm                SoftwareRenderer
 in-memory grid,      (retroglyph-crossterm)   (retroglyph-software)
 synthetic events     real TTY, ANSI output    winit window, pixels
```

`Headless` stores presented content in memory and lets tests inject synthetic `Event`s with
`Headless::push_event`; nothing there talks to a real terminal or window. Swapping `Headless` for
`Crossterm` or `SoftwareRenderer` changes only the `B` type parameter: `App` implementations,
`Terminal` calls, and game logic are unchanged. `run_on` drives `Terminal<Headless>` and
`Terminal<Crossterm>` identically; the software backend's windowed loop drives
`Terminal<SoftwareRenderer>` through the same `App` contract, inverted because winit owns the
event loop instead of handing control back to a driver function.

See `examples/headless.rs` (`cargo run -p retroglyph-core --example headless`) for the smallest
possible use of `Headless`, depending on nothing but `retroglyph-core`.

## `Input` and `Output` are independent facets

`Input` and `Output` are independent facets of `Backend`, which does not fit a window as one
type: some event loop owns input, while a per-renderer surface owns output. `WindowBackend`
(`retroglyph-window`) reunites the two, implementing `Output` by delegating to its wrapped
`Presenter`, `Input` via its own event queue, and the no-op default `Cursor`, so `Terminal` gets
the full `Backend` it needs while renderer crates (`retroglyph-software`, `retroglyph-gl`,
`retroglyph-wgpu`) implement only `Presenter`.

Because `WindowBackend` owns input, a `Presenter` should **not** implement `Input` or `Cursor`
itself for windowed use: those impls would be dead (the event loop pushes to *this* queue, not
the presenter's) and would silently miss the `Mouse(Moved)` coalescing that `WindowBackend`'s
`push_event` applies. A presenter that also wants a direct headless `Terminal<Self>` input path
(as `retroglyph-software` does for pixel tests) may still implement `Input` for that path,
accepting that a bare queue does not coalesce; a presenter with no such path (as `retroglyph-gl`)
implements only `Presenter`.

With the `winit` feature enabled, `winit::run_windowed` and `winit::run_app` own the event loop,
call `push_event` as winit events are translated, and call `Presenter::present` once per frame;
callers never touch `WindowBackend` directly. With `winit` disabled, `retroglyph-window` exports
no event loop at all: a caller driving its own loop (SDL2, tao, a custom driver) constructs
`WindowBackend::new(presenter)` itself, calls `push_event` for each translated input event, and
calls `Terminal::present` (which drives `Presenter::flush`) plus `presenter_mut().present()` once
per frame.

## Presenting is automatic

`winit::run_windowed` and `winit::run_app` (and every `_with_proxy`/`_on` variant of either) call
`Terminal::present` for you, once, right after the app's per-frame closure or `App::update`
returns: you no longer need to (and, for a stale-content bug fixed by this behavior, should not
rely on remembering to) call it yourself. Calling it yourself is still supported and has no ill
effect (the driver detects it already ran and skips its own call), for example if you also want
to observe `Terminal::present`'s `Result` directly.

For the `App`-based drivers (`run_app` and friends), the automatic present is skipped entirely on
`Flow::Idle`, leaving the previous frame on screen; every other `Flow` variant (including
`Continue`) presents as usual.
