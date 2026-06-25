# ADR 011: WASM Portability Roadmap (Revised)

**Status:** Accepted **Date:** 2026-06-20 **Deps:**
[ADR 007: Software Rendering Backend](007-software-backend.md) **Replaces:**
`011-wasm-portability.md`

## Context

The software backend uses `std::thread::spawn` (impossible in browser WASM) and `mpsc` channels to
communicate between a game thread and the winit event loop. winit's event loop must block on native
(`run_app`) but return immediately on WASM (`spawn_app`).

**Key finding from code review:** `softbuffer` 0.4.8 already supports WASM natively (Tier 2). The
pixel buffer format (`Vec<u32>` in `0x00RRGGBB`) and the `softbuffer::Surface` API work on both
platforms. No canvas rendering rewrite is needed.

### Future GPU Backends (Design Target)

This refactor is the foundation for a tiered WASM rendering stack:

| Tier        | Backend    | Mechanism                | Binary Size | Relative Speed |
| ----------- | ---------- | ------------------------ | ----------- | -------------- |
| 3 (current) | softbuffer | Canvas 2D `putImageData` | ~80 KB      | 1x (baseline)  |
| 2 (future)  | glow       | WebGL2 data-texture quad | ~80 KB      | 4-10x          |
| 1 (future)  | wgpu       | WebGPU instanced quads   | ~500 KB     | 10x+           |

All three tiers share the same winit event loop architecture proposed here. The unified,
single-threaded model is permanent across all backends. Where they differ is in how the rendered
frame is presented to the window surface — this ADR ensures that step is abstracted so adding glow
or wgpu later requires no event loop changes, only a new `Backend` implementation.

See `docs/references/backends/webgl2.md` and `docs/references/backends/wgpu-webgpu.md` for the full
GPU backend research.

## Required Changes

### 1. Architectural Verdict: Unify the Event Loop

The core change: move the game loop **inside** the winit `ApplicationHandler`, eliminating the
background thread and mpsc channels entirely. The `ApplicationHandler` is generic over the backend
type so it works unchanged with `SoftwareRenderer`, a future `GlowRenderer`, or a future
`WgpuRenderer`.

**Current architecture (native only):**

```text
Main Thread:                    Background Thread:
  EventLoop::run_app()            loop { app_loop(&mut term) }
  ┌─────────────────────┐        ┌────────────────────────┐
  │ WindowApp           │        │ Terminal<SoftwareRend> │
  │  event_tx (→game)   │◄──────►│  poll_event ← event_rx │
  │  frame_rx (←game)   │        │  flush() → frame_tx    │
  │  RedrawRequested:   │        └────────────────────────┘
  │    copy to surface  │
  └─────────────────────┘
```

**Proposed architecture (native + WASM):**

```text
Main Thread (unified, backend-agnostic):
  ┌──────────────────────────────────────────┐
  │ WindowApp<B> where B: Backend            │
  │  terminal: Option<Terminal<B>>            │
  │  app_loop: FnMut(&mut Terminal<B>)        │
  │                                           │
  │  on RedrawRequested:                      │
  │    1. (self.app_loop)(&mut self.terminal)  │
  │    2. self.terminal.backend_mut().present()│
  │                                           │
  │  on KeyboardInput:                        │
  │    translate → term.backend_mut()          │
  │                .push_event(event)          │
  └──────────────────────────────────────────┘

  #[cfg(not(target_arch = "wasm32"))]
  EventLoop::run_app(&mut app)   // blocks main thread

  #[cfg(target_arch = "wasm32")]
  EventLoopExtWebSys::spawn_app(event_loop, app)  // returns immediately
```

The key difference from the threaded architecture: the `ApplicationHandler` doesn't know what
backend it's driving. It calls `app_loop()`, then calls `backend.present()` — each backend decides
how to get its rendered frame to the screen.

### 2. Replace mpsc Channels with Direct State

**Events:** Instead of `mpsc::Sender` → `mpsc::Receiver` → `poll_event()`, add a `VecDeque<Event>`
buffer directly inside `SoftwareRenderer`:

```rust
// In SoftwareRenderer:
struct RenderContext {
    // ... existing fields ...
    event_buffer: VecDeque<Event>,       // ← new, replaces event_rx
    // frame_tx/frame_rx removed entirely
}
```

The `ApplicationHandler` pushes events directly:

```rust
fn window_event(&mut self, ..., event: WindowEvent) {
    match event {
        WindowEvent::KeyboardInput { event, .. } => {
            if let Some(e) = translate_key(event, self.modifiers) {
                self.terminal.backend_mut().push_event(e);
            }
        }
        // ...
    }
}
```

**Frames:** No `frame_tx`/`frame_rx` needed. After `app_loop(&mut term)`, the handler calls
`backend.present()`. For `SoftwareRenderer` this copies the pixel buffer to the softbuffer surface.
A future `GlowRenderer` would upload data textures and draw a full-screen quad. A future
`WgpuRenderer` would submit a command buffer. The handler stays the same in all cases.

### 3. Add `present()` to the `Backend` trait

The `Backend` trait gains a `present()` method that the `ApplicationHandler` calls after each tick.
This replaces the per-backend surface handling that's currently hardcoded in `WindowApp`.

```rust
trait Backend {
    // ... existing methods ...

    /// Present the current frame to the window surface.
    ///
    /// Called by the ApplicationHandler after each game tick.
    /// - SoftwareRenderer: copies pixel_buf → softbuffer surface
    /// - GlowRenderer: uploads data textures → draws full-screen quad
    /// - WgpuRenderer: submits command buffer → presents swap chain
    fn present(&mut self);
}
```

The `ApplicationHandler` no longer holds a `softbuffer::Surface` directly. Instead, the backend owns
its surface (or equivalent) internally. `WindowApp` becomes fully backend-agnostic:

```rust
struct WindowApp<B: Backend, F> {
    terminal: Option<Terminal<B>>,
    app_loop: F,
    window: Option<Arc<Window>>,
    modifiers: KeyModifiers,
}
```

No `surface: Option<softbuffer::Surface<...>>`. No softbuffer imports in the handler at all.

### 4. SoftwareRenderer::push_event

Add a method to push events directly, bypassing the channel:

```rust
impl SoftwareRenderer {
    pub fn push_event(&mut self, event: Event) {
        self.ctx.event_buffer.push_back(event);
    }
}
```

`Backend::poll_event()` reads from the buffer instead of the channel:

```rust
fn poll_event(&mut self, timeout: Duration) -> Option<Event> {
    if timeout == Duration::ZERO {
        self.ctx.event_buffer.pop_front()
    } else if self.ctx.event_buffer.is_empty() {
        // Busy-wait for timeout (or use a condvar if event_buffer can be signaled)
        // For WASM, timeout always behaves like Duration::ZERO
        // since blocking isn't allowed.
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::sleep(timeout);
        self.ctx.event_buffer.pop_front()
    } else {
        self.ctx.event_buffer.pop_front()
    }
}
```

### 5. WindowApp Owns the Terminal and Closure (Generic over B)

`WindowApp` is generic over `B: Backend` and does not reference softbuffer types:

```rust
struct WindowApp<B: Backend, F> {
    terminal: Option<Terminal<B>>,
    app_loop: F,
    window: Option<Arc<Window>>,
    modifiers: KeyModifiers,
}
```

The backend creates and owns its window surface internally. For `SoftwareRenderer` this means:

```rust
impl SoftwareRenderer {
    /// Prepare the window surface (called from resumed).
    fn init_surface(&mut self, window: &Arc<Window>) {
        let context = softbuffer::Context::new(window.clone()).unwrap();
        let surface = softbuffer::Surface::new(&context, window.clone()).unwrap();
        self.ctx.window_surface = Some(WindowSurface { context, surface });
        // resize to initial dimensions...
    }
}
```

**No channels.** No background thread. `WindowApp` is the single owner of all game state, and knows
nothing about softbuffer — only the `Backend` trait.

### 6. `SoftwareBackend::run()` creates the backend and passes to the generic handler

```rust
impl SoftwareBackend {
    pub fn run_windowed<F>(self, app_loop: F) -> Result<(), SoftwareBackendError>
    where
        F: FnMut(&mut crate::Terminal<SoftwareRenderer>) + 'static,
    {
        let event_loop = EventLoop::new().map_err(SoftwareBackendError::EventLoop)?;

        let renderer = self.create_renderer(); // extracts from run_headless
        let terminal = crate::Terminal::new(renderer);

        let app = WindowApp {
            terminal: Some(terminal),
            app_loop,
            window: None,
            modifiers: KeyModifiers::NONE,
        };

        #[cfg(not(target_arch = "wasm32"))]
        {
            event_loop.run_app(app).map_err(SoftwareBackendError::EventLoop)
        }

        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::EventLoopExtWebSys;
            event_loop.spawn_app(app);
            Ok(())
        }
    }
}
```

Key differences from current:

- `Send` bound removed from `F` (no longer sent across threads)
- `'static` bound stays (winit requires it)
- Background thread eliminated
- `create_renderer()` produces `SoftwareRenderer` without channels

A future `GlowBackend::run()` or `WgpuBackend::run()` would use the exact same pattern but create a
different renderer type.

### 7. Surface Initialization: Backend-Owned, Handler-Initiated

The `ApplicationHandler::resumed()` still creates the window, but instead of holding surface state
in the handler, it passes the window to the backend:

```rust
impl ApplicationHandler for WindowApp<SoftwareRenderer, ...> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title(&self.title))
                .unwrap(),
        );

        // Delegate surface creation to the backend.
        // SoftwareRenderer creates softbuffer Surface internally.
        if let Some(term) = self.terminal.as_mut() {
            term.backend_mut().init_surface(&window);
        }

        self.window = Some(window);
    }
}
```

This keeps the handler backend-agnostic — a `GlowRenderer` or `WgpuRenderer` would implement
`init_surface()` differently, creating WebGL2 contexts or wgpu surfaces respectively.

### 8. RedrawRequested: Abstract `present()` Call

```rust
WindowEvent::RedrawRequested => {
    let Some(term) = self.terminal.as_mut() else { return };

    // Run one game tick.
    (self.app_loop)(term);

    // Present the rendered frame.
    // - SoftwareRenderer: copies pixel_buf to softbuffer surface
    // - GlowRenderer: uploads data textures + draws full-screen quad
    // - WgpuRenderer: submits command buffer
    term.backend_mut().present();
}
```

No direct `pixels()` access in the handler. No `softbuffer::buffer_mut()` or `copy_from_slice()`.
The backend owns all the rendering details.

### 9. Event Delivery Without Channels

```rust
WindowEvent::KeyboardInput { event, .. } => {
    if let Some(term) = self.terminal.as_mut() {
        if let Some(e) = translate_key(event, self.modifiers) {
            term.backend_mut().push_event(e);
        }
    }
}

WindowEvent::ModifiersChanged(new_mods) => {
    let mut km = KeyModifiers::NONE;
    if new_mods.state().shift_key()   { km |= KeyModifiers::SHIFT; }
    if new_mods.state().control_key() { km |= KeyModifiers::CONTROL; }
    if new_mods.state().alt_key()     { km |= KeyModifiers::ALT; }
    self.modifiers = km;
}

WindowEvent::CloseRequested => {
    event_loop.exit();
}

WindowEvent::Resized(physical_size) => {
    if let Some(term) = self.terminal.as_mut() {
        let cell_size = term.backend().cell_size();
        let cols = physical_size.width / cell_size.0;
        let rows = physical_size.height / cell_size.1;
        let new_w = cols * cell_size.0;
        let new_h = rows * cell_size.1;
        term.backend_mut().resize_surface(new_w, new_h);
        term.backend_mut().push_event(Event::Resize(
            cols.max(1) as u16,
            rows.max(1) as u16,
        ));
    }
}
```

The handler doesn't track `cell_w`/`cell_h` or `win_w`/`win_h` — those are the backend's
responsibility.

### 10. SoftwareRenderer Changes

Remove channels from `RenderContext`. Add `event_buffer: VecDeque<Event>` and
`window_surface: Option<WindowSurface>`.

```rust
struct WindowSurface {
    context: softbuffer::Context<Arc<Window>>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
}

struct RenderContext {
    event_buffer: VecDeque<Event>,  // replaces event_rx
    pixel_buf: GridBuf<u32, Vec<u32>, RowMajor>,
    window_surface: Option<WindowSurface>,
    cell_w: u32,
    cell_h: u32,
}

impl SoftwareRenderer {
    pub(crate) fn create(
        options: SoftwareBackend,
        buf_w: usize,
        buf_h: usize,
        cell_w: u32,
        cell_h: u32,
    ) -> Self {
        Self {
            options,
            ctx: RenderContext {
                event_buffer: VecDeque::new(),
                pixel_buf: GridBuf::from_buffer(vec![0u32; buf_w * buf_h], buf_w),
                window_surface: None,
                cell_w,
                cell_h,
            },
            // sprite_cache kept as-is
        }
    }

    /// Push an event into the internal buffer (called by ApplicationHandler).
    pub fn push_event(&mut self, event: Event) {
        self.ctx.event_buffer.push_back(event);
    }

    /// Initialize the window surface (called from resumed).
    pub fn init_surface(&mut self, window: &Arc<Window>) {
        let context = softbuffer::Context::new(window.clone()).unwrap();
        let surface = softbuffer::Surface::new(&context, window.clone()).unwrap();
        self.ctx.window_surface = Some(WindowSurface { context, surface });
    }

    /// Resize the window surface.
    pub fn resize_surface(&mut self, width: u32, height: u32) {
        if let Some(surf) = &mut self.ctx.window_surface {
            if let (Some(w), Some(h)) = (
                NonZeroU32::new(width),
                NonZeroU32::new(height),
            ) {
                let _ = surf.surface.resize(w, h);
            }
        }
    }

    /// Present the pixel buffer to the window.
    ///
    /// This is the softbuffer-specific implementation of `Backend::present()`.
    pub fn present(&mut self) {
        let Some(surface) = self.ctx.window_surface.as_mut() else { return };
        let Ok(mut buffer) = surface.surface.buffer_mut() else { return };
        let pixels = self.ctx.pixel_buf.as_ref();
        if pixels.len() == buffer.len() {
            buffer.copy_from_slice(pixels);
        } else {
            buffer.fill(0);
        }
        let _ = buffer.present();
    }
}
```

**`Backend::poll_event`** reads from `event_buffer`:

```rust
fn poll_event(&mut self, timeout: Duration) -> Option<Event> {
    if let Some(event) = self.ctx.event_buffer.pop_front() {
        return Some(event);
    }
    if timeout != Duration::ZERO {
        // On native we can block, but there's nothing to wait on
        // since events arrive asynchronously from winit.
        // Just return None immediately — the game loop will poll again.
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::sleep(timeout);
    }
    None
}
```

**`flush()`** becomes a no-op:

```rust
fn flush(&mut self) {
    // No-op. The pixel buffer is presented via Backend::present().
}
```

**`is_connected()`** returns false after `CloseRequested` or WebGL context loss. Kept meaningful:

```rust
fn is_connected(&self) -> bool {
    !self.ctx.closed
}
```

Where `ctx.closed` is set by `CloseRequested` and (for future GPU backends) by `webglcontextlost`
events.

### 11. Changes to `run_headless()`

`run_headless()` loses its channels and passes cell dimensions:

```rust
pub fn run_headless(self) -> SoftwareRenderer {
    let font = self.font.as_ref().expect(
        "run_headless() requires a font; supply one via SoftwareBackendBuilder::font()",
    );

    let cell_w = u32::from(font.glyph_width) * u32::from(self.scale);
    let cell_h = u32::from(font.glyph_height) * u32::from(self.scale);
    let buf_w = usize::from(self.cols) * usize::try_from(cell_w).unwrap();
    let buf_h = usize::from(self.rows) * usize::try_from(cell_h).unwrap();

    SoftwareRenderer::create(self, buf_w, buf_h, cell_w, cell_h)
}
```

No channels, no dummy senders/receivers.

### 12. Cargo.toml Changes

Minimal. `softbuffer` stays as a dependency (it works on WASM). No new crates needed for the core
refactor. Add `web-sys` + `wasm-bindgen` only if the WASM example needs them:

```toml
# softbuffer 0.4 already supports WASM — no change needed here.
# The `software` feature stays the same.

[target.'cfg(target_arch = "wasm32")'.dev-dependencies]
wasm-bindgen = "0.2"

# For the WASM example:
[[example]]
name = "wasm_demo"
required-features = ["software-default-font"]
```

### 13. Backend Trait Additions

The `Backend` trait gains methods that the new architecture requires. These have default
implementations for backends that don't need them:

```rust
pub trait Backend {
    // ... existing methods ...

    /// Present the current frame to the display.
    ///
    /// - SoftwareRenderer: copies pixel buffer to softbuffer surface
    /// - GlowRenderer: uploads data textures + draws full-screen quad
    /// - WgpuRenderer: submits render pass + presents swap chain
    ///
    /// Called by the ApplicationHandler after each game tick.
    /// The default implementation is a no-op (used by headless backends).
    fn present(&mut self) {}

    /// Push an external event into the backend's event buffer.
    ///
    /// Called from the ApplicationHandler when winit events are translated.
    fn push_event(&mut self, event: Event) {
        let _ = event;
    }

    /// Initialize the window surface (called from resumed).
    ///
    /// - SoftwareRenderer: creates softbuffer context + surface
    /// - GlowRenderer: creates WebGL2 context from canvas
    /// - WgpuRenderer: creates wgpu surface + configures swap chain
    fn init_surface(&mut self, window: &Arc<Window>) {
        let _ = window;
    }

    /// Resize the window surface.
    fn resize_surface(&mut self, width: u32, height: u32) {
        let _ = (width, height);
    }

    /// Return the cell size in pixels (width, height).
    fn cell_size(&self) -> (u32, u32);
}
```

The `Headless` and `Crossterm` backends get trivial implementations — `present()` is a no-op,
`push_event()` is a no-op, `init_surface()` is a no-op.

## Implementation Plan

**Order:** M1 → M2 → M3 → M4 → M5 → M6 (M4 structurally depends on M1-M3).

### M1: Eliminate mpsc channels from SoftwareRenderer (est. 2-3h)

- Add `event_buffer: VecDeque<Event>` to `RenderContext`
- Add `SoftwareRenderer::push_event(&mut self, Event)`
- Remove `event_rx` parameter from `SoftwareRenderer::create()`
- Remove `frame_tx` field from `RenderContext`
- Update `poll_event()` to read from `event_buffer` instead of `event_rx`
- Update `flush()` to no-op
- Update `run_headless()` — remove channel creation, pass cell dimensions
- Add `SoftwareRenderer::init_surface()`, `resize_surface()`, `present()` methods
- All existing tests still pass (they bypass channels entirely via `pixels()` + `draw_layers()`)

### M2: Add `push_event()` to `Backend`, introduce `WindowedBackend` subtrait (est. 1h)

- Add `push_event()` to `Backend` trait with default no-op
- Wire up `Headless::push_event()` (already exists as an inherent method) to the trait
- Add no-op `push_event()` for `Crossterm`
- Define `WindowedBackend: Backend` in `src/backend/mod.rs`:
  - `fn present(&mut self);`
  - `fn init_surface(&mut self, window: &Arc<Window>) -> Result<(), Error>;`
  - `fn resize_surface(&mut self, width: u32, height: u32);`
  - `fn cell_size(&self) -> (u32, u32);`
- `Headless` and `Crossterm` only impl `Backend` — no window stubs
- Verify nothing breaks — all existing code still compiles

### M3: Refactor WindowApp to be backend-agnostic (est. 3-4h)

- Make `WindowApp<B, F>` generic over `B: WindowedBackend`
- Move softbuffer surface + dimension tracking into `SoftwareRenderer`
- Remove `surface`, `context`, `last_frame`, `win_w`, `win_h`, `cell_w`, `cell_h` from `WindowApp`
- `resumed()`: create window, call `term.backend_mut().init_surface(&window)`
- `RedrawRequested`: call `app_loop(&mut terminal)`, then `term.backend_mut().present()`
- Keyboard/mouse events: `terminal.backend_mut().push_event(translated)`
- `Resized`: query `backend.cell_size()`, call `backend.resize_surface()`
- Remove `std::thread::spawn` from `SoftwareBackend::run()`
- Remove `Send` bound from `F` — no thread boundary
- Remove `is_connected()` from `SoftwareRenderer` — no background thread

### M4: Add WASM event loop path (est. 1h)

- Add `#[cfg(target_arch = "wasm32")]` branch in `SoftwareBackend::run()`:
  `event_loop.spawn_app(app)` via `winit::platform::web::EventLoopExtWebSys`
- Keep `#[cfg(not(target_arch = "wasm32"))]` branch: `event_loop.run_app(app)`
- This is a thin cfg split — all structural work is done in M1-M3

### M5: WASM example (est. 2h)

- Create `examples/wasm_demo.rs` — same game loop as `software_demo.rs`
- Add `wasm-bindgen` dev-dependency for `#[wasm_bindgen(start)]` entry point
- Also create `examples/wasm_tileset_demo.rs` if `software-tilesets` feature is active

```rust
// examples/wasm_demo.rs — same game loop, WASM entry point added
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() {
    main();
}

fn main() {
    // identical to software_demo.rs
    SoftwareBackendBuilder::new()
        .title("retroglyph WASM demo")
        .grid_size(50, 20)
        .scale(2)
        .build()
        .unwrap()
        .run_windowed(|term| {
            draw(term);
            if let Some(event) = term.poll(Duration::from_millis(16)) {
                // handle input
            }
        })
        .expect("event loop failed");
}
```

### M6: Cleanup (est. 0.5h)

- Remove unused imports: `std::sync::mpsc`, `std::sync::Arc`, `std::sync::atomic::AtomicBool`
- Remove `is_connected()` impl from `SoftwareRenderer` (keep on `Crossterm` — still meaningful)
- Update doc comments on `SoftwareBackend::run()` (no thread, no channels)

## Files Changed

| File                             | Changes                                                                                                                                           |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/backend/mod.rs`             | Define `WindowedBackend: Backend` subtrait with `present()`, `init_surface()`, `resize_surface()`, `cell_size()`. Add `push_event()` to `Backend` |
| `src/backend/headless.rs`        | Add `push_event()` implementation (already exists — wire to trait). No window stubs                                                               |
| `src/backend/crossterm.rs`       | Add `push_event()` implementation. No window stubs                                                                                                |
| `src/backend/software/mod.rs`    | **Major refactor** — remove channels, add event_buffer + window_surface, implement new trait methods,                                             |
|                                  | refactor WindowApp to be generic, split run() for native/WASM                                                                                     |
| `src/backend/software/config.rs` | Maybe remove `Softbuffer` error variant (no longer used?) or keep for surface errors                                                              |
| `Cargo.toml`                     | Minimal — no new deps, `softbuffer` stays                                                                                                         |
| `examples/software_demo.rs`      | No changes needed                                                                                                                                 |
| `examples/wasm_demo.rs`          | **New file** — same game loop, `#[wasm_bindgen(start)]` entry point                                                                               |

## What Stays the Same

- `Backend` trait gains `push_event()` but existing rendering methods (`draw`, `draw_layers`,
  `flush`, `poll_event`, `clear`, etc.) are unchanged
- New `WindowedBackend: Backend` subtrait introduced for window-surface operations only
- `Terminal<B>` — no changes (always generic over backend)
- `Grid`, `Tile`, `Style`, `Color` — no changes
- `blit_cell`, `blit_glyph`, `blit_sprite` — no changes
- All font and tileset code — no changes
- Event types (`Event`, `KeyCode`, etc.) — no changes

## Non-Goals (This ADR)

- **Threaded WASM** (`SharedArrayBuffer` + `Atomics`): Not targeted. Single-threaded is simpler and
  sufficient for roguelike workloads.
- **`wasm-pack` integration**: Out of scope.
- **Pixel performance optimization**: softbuffer's canvas `putImageData` is fast enough for terminal
  grid resolutions at 60fps.
- **WebGL2 backend** (`glow`): Deferred to a future ADR. The `WindowedBackend` subtrait
  (`present()`, `init_surface()`) is designed to make this a drop-in.
- **WebGPU backend** (`wgpu`): Deferred to a future ADR. Same `WindowedBackend` trait surface.
- **Canvas 2D fallback chain**: Not yet needed. softbuffer handles Canvas 2D. When glow and wgpu
  arrive they will try WebGPU → WebGL2 → softbuffer fallback.

## Future Backend Considerations

### How glow (WebGL2) Would Use This Architecture

A `GlowRenderer` implementing `WindowedBackend` would:

- **`init_surface()`**: Get the HTML canvas from winit's `Window`, create a WebGL2 context via
  `glow::Context::from_webgl2_context()`, upload the glyph atlas texture, compile shaders.
- **`present()`**: Upload three data textures (glyph indices, fg colors, bg colors) via
  `texSubImage2D`, set uniforms, draw a full-screen quad with the composite fragment shader.
- **`poll_event()`**: Same `VecDeque<Event>` pattern as `SoftwareRenderer`.
- **`cell_size()`**: Return `(glyph_w * scale, glyph_h * scale)`.

No changes needed to `WindowApp`, `ApplicationHandler`, `WindowedBackend`, or `Backend` trait.

See `docs/references/backends/webgl2.md` for the full data-texture shader design.

### How wgpu (WebGPU) Would Use This Architecture

A `WgpuRenderer` implementing `WindowedBackend` would:

- **`init_surface()`**: Create a `wgpu::Surface` from winit's `Window`, create `wgpu::Device` and
  `wgpu::Queue`, compile WGSL shaders, create render pipeline and instance buffer.
- **`present()`**: Map the instance buffer with dirty-row tracking, write per-cell instance data
  (position, UV, fg/bg colors), submit the render pass for instanced quads, present the swap chain
  texture.
- **`poll_event()`**: Same `VecDeque<Event>` pattern.
- **`resize_surface()`**: Call `surface.configure()` with new dimensions.

No changes needed to `WindowApp`, `ApplicationHandler`, `WindowedBackend`, or `Backend` trait.

See `docs/references/backends/wgpu-webgpu.md` for the full wgpu rendering architecture.

### Fallback Chain

Once all three backends exist, the fallback order at runtime would be:

```rust
// Pseudocode — not part of this ADR.
fn create_backend(config, window, canvas) -> Box<dyn WindowedBackend> {
    // Tier 1: WebGPU
    #[cfg(feature = "backend-wgpu")]
    if let Ok(backend) = WgpuRenderer::try_new(window) {
        return Box::new(backend);
    }
    // Tier 2: WebGL2
    #[cfg(feature = "backend-glow")]
    if let Ok(backend) = GlowRenderer::try_new(canvas) {
        return Box::new(backend);
    }
    // Tier 3: Canvas 2D (always available)
    Box::new(SoftwareRenderer::create(config, ...))
}
```

This matches the xterm.js fallback strategy: try WebGL2, fall back to Canvas 2D.
