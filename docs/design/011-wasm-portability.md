# ADR 011: WASM Portability Roadmap

**Status:** Draft **Date:** 2026-06-20 **Parent:** [ADR 007: Software Rendering Backend](007-software-backend.md)

## Context

The software rendering backend uses `winit` + `softbuffer` for window creation and pixel blitting,
with the game loop on a background thread communicating via `mpsc` channels. This architecture is
incompatible with WASM in two fundamental ways:

1. **No `std::thread`:** WASM (in the browser) does not support `std::thread::spawn`. The
   `#[cfg(target_arch = "wasm32")]` compilation target replaces threading with
   `wasm-bindgen`'s async model and `SharedArrayBuffer`-based workers (with `COOP`/`COEP` headers).
2. **No blocking main thread:** `event_loop.run_app()` blocks the calling thread, which is not
   allowed in browser WASM. Control must yield back to the browser event loop.

This document describes what changes would be needed for a WASM-compatible backend.

---

## Current Architecture (Native)

```
┌─────────────────────────────────────┐
│         Main Thread                 │
│  SoftwareBackend::run()             │
│  ┌───────────────────────────────┐  │
│  │ winit EventLoop::run_app()    │  │  blocks
│  │ ┌─────────────────────────┐   │  │
│  │ │ WindowApp               │   │  │
│  │ │  event_tx (-> game)     │   │  │
│  │ │  frame_rx (<- game)     │   │  │
│  │ │  on RedrawRequested:    │   │  │
│  │ │    drain frame_rx       │   │  │
│  │ │    copy to softbuffer   │   │  │
│  │ └─────────────────────────┘   │  │
│  └───────────────────────────────┘  │
│                                      │
│  ┌───────────────────────────────┐  │
│  │ Background Thread            │  │
│  │  loop { app_loop(&mut term) }│  │
│  │  term.present() → flush()    │  │
│  │  → try_send(frame)          │  │
│  └───────────────────────────────┘  │
└─────────────────────────────────────┘
```

## Required WASM Architecture

```
┌─────────────────────────────────────────┐
│           Main Thread (Browser)         │
│  winit EventLoop (non-blocking)         │
│  ┌───────────────────────────────────┐  │
│  │ WindowApp (also holds game state) │  │
│  │                                   │  │
│  │ on RedrawRequested:               │  │
│  │   1. game.update()                │  │  ◄── single step, no blocking
│  │   2. game.render() → pixel buffer │  │
│  │   3. copy to canvas ImageData     │  │  ◄── instead of softbuffer
│  │                                   │  │
│  │ on WindowEvent::KeyboardInput:    │  │
│  │   translate to rg::Event          │  │
│  │   deliver to game.handle_input()  │  │
│  └───────────────────────────────────┘  │
└─────────────────────────────────────────┘

Browser rendering: canvas.getContext("2d").putImageData()
Instead of:        softbuffer::Surface::buffer_mut()
```

## What Changes Are Needed

### 1. Separate the "run" abstraction from the threading model

Currently `SoftwareBackend::run()` takes ownership of the main thread and the game loop. For WASM,
the game loop must be driven by the browser's `requestAnimationFrame` callback, which winit
exposes as `RedrawRequested`.

**The core insight:** `SoftwareBackend` already separates configuration from execution.
`SoftwareRenderer` already implements `Backend`. For WASM, instead of calling `backend.run(|term| { ... })`,
the application would create a renderer and drive the loop itself:

```rust
// Current (native only):
let backend = SoftwareBackendBuilder::new().build()?;
backend.run(|term| {
    // game loop body
})?;

// WASM-compatible pattern (also works on native):
let mut renderer = SoftwareBackendBuilder::new().build()?.run_headless()?;
let mut terminal = Terminal::new(renderer);

// Application owns the event loop:
event_loop.run_app(&mut MyApp { terminal });
```

### 2. Replace softbuffer with canvas rendering on WASM

The pixel buffer format is already `Vec<u32>` in `0x00RRGGBB` layout. On native, this goes to
`softbuffer::Surface`. On WASM, it goes to a `<canvas>` element via `ImageData`:

```diff
 // In WindowApp::window_event(), RedrawRequested handler:
+// Native:
 if let Ok(mut buffer) = surface.buffer_mut() {
     buffer.copy_from_slice(&self.last_frame);
     let _ = buffer.present();
 }

+// WASM (behind cfg(target_arch = "wasm32")):
+if let Some(canvas) = &self.canvas {
+    let ctx = canvas.get_context("2d")?;
+    let image_data = ImageData::new_with_u8_clamped_array(
+        rgba_bytes(&self.last_frame), // convert 0x00RRGGBB → RGBA
+        self.win_w, self.win_h,
+    );
+    ctx.put_image_data(image_data, 0.0, 0.0);
+}
```

### 3. Remove `std::thread::spawn`

The background thread is the biggest blocker. Every `app_loop()` iteration must become a single
step driven by the winit event loop:

```diff
 // Current:
-pub fn run<F>(self, app_loop: F) -> Result<()>
-where F: FnMut(&mut Terminal<SoftwareRenderer>) + Send + 'static
-{
-    // ... create channels, spawn thread, block on event loop
-}

 // WASM-compatible:
 // Application creates renderer, drives loop via event_loop.run_app()
 // The game state lives in the ApplicationHandler impl.
```

### 4. Conditional compilation in mod.rs

The module structure already uses `#[cfg(feature = "software")]`. A WASM feature
(implied by `target_arch = "wasm32"` or an explicit feature flag) would gate:

```diff
 // In src/backend/mod.rs:
 #[cfg(feature = "software")]
 pub mod software;

+#[cfg(all(feature = "software", target_arch = "wasm32"))]
+pub mod software_wasm;
```

The WASM module would use `web-sys` + `wasm-bindgen` instead of `winit` + `softbuffer`.
`winit` itself does support WASM, so the event loop handling can stay shared — only the
pixel surface changes.

### 5. Feature flags in Cargo.toml

```diff
 [features]
 software = ["dep:winit", "dep:softbuffer", "std"]
+software-wasm = ["dep:winit", "dep:wasm-bindgen", "dep:web-sys", "std"]

 [target.'cfg(not(target_arch = "wasm32"))'.dependencies]
 softbuffer = { version = "0.4", optional = true }

+[target.'cfg(target_arch = "wasm32")'.dependencies]
+wasm-bindgen = { version = "0.2", optional = true }
+web-sys = { version = "0.3", optional = true, features = ["HtmlCanvasElement", "ImageData", "CanvasRenderingContext2d"] }
```

### 6. Merge Terminal::run_loop into ApplicationHandler

Instead of `SoftwareBackend::run()` owning both the event loop AND the game loop, the application
would wire them together:

```rust
// Shared: Backend trait, SoftwareRenderer, Terminal, Grid, etc.
// These are all WASM-compatible already (no platform-specific code).

// Native-only: SoftwareBackend::run() convenience method.
// Uses std::thread and blocks the main thread.

// WASM: Application implements ApplicationHandler, calls
// terminal.backend_mut().pixels() for canvas rendering.
```

### 7. Update the Backend trait

The current `Backend` trait already works for WASM (no platform assumptions):

```rust
pub trait Backend {
    fn draw<'a, I>(&mut self, content: I) where I: Iterator<Item = (Pos, &'a Tile)>;
    fn flush(&mut self);
    fn size(&self) -> Size;
    fn clear(&mut self);
    fn poll_event(&mut self, timeout: Duration) -> Option<Event>;
    // is_connected() was added in Phase 1 — already WASM-friendly
    fn is_connected(&self) -> bool { true }
}
```

No trait changes needed.

## Implementation Plan

### Milestone 1: Separate `run()` from the rendering core

**Goal:** Make `SoftwareRenderer` usable without `SoftwareBackend::run()`. This is already true
(`run_headless()` creates a renderer without a thread or event loop). Add a compile-time check
that `SoftwareRenderer` is usable on WASM.

**Files:** `src/backend/software/mod.rs`

**Changes:**
- Verify `SoftwareRenderer` has no `std::thread` or blocking dependencies
- Move `SoftwareBackend::run()` logic into a helper that requires `std` (already behind `#[cfg]`)

### Milestone 2: Create a canvas rendering path

**Goal:** Replace softbuffer with canvas `ImageData` on WASM.

**Files:** New file `src/backend/software/wasm_canvas.rs`

**Changes:**
- Implement a `CanvasRenderer` that wraps a `<canvas>` element
- Implement `Backend` for `CanvasRenderer` using `web-sys` APIs
- Pixel format conversion: `0x00RRGGBB` → RGBA bytes for `ImageData`

### Milestone 3: Add winit WASM event loop example

**Goal:** Show how to use `SoftwareRenderer` in a browser context.

**Files:** New file `examples/wasm_demo.rs`

**Changes:**
- Example that creates a `SoftwareRenderer`, sets up a `<canvas>`, and drives the loop
- Uses `winit`'s WASM-compatible `EventLoop` API
- No `std::thread::spawn`

### Milestone 4: `SoftwareBackend::run()` on WASM (optional convenience)

**Goal:** Provide the same ergonomic `run()` API on WASM (behind a `wasm-bindgen-futures` bridge).

**Changes:**
- Wrap the winit event loop in an async `run()` that returns a `Future`
- Use `wasm-bindgen-futures::spawn_local` to drive the loop

### Non-goals

- **Threaded WASM** (`SharedArrayBuffer` + `Atomics`): Not targeted. Single-threaded event-driven
  rendering is simpler and sufficient for roguelike workloads.
- **`wasm-pack` integration**: Out of scope for this ADR.
- **Pixel performance optimization**: Canvas `putImageData` is already fast enough for terminal
  grid resolutions (< 1920×1080).

## Estimated Effort

| Milestone | Effort | Dependencies |
|-----------|--------|--------------|
| M1: Separate core from threading | 1-2 hours | Already largely done |
| M2: Canvas rendering path | 3-4 hours | `web-sys` familiarity |
| M3: WASM demo example | 2-3 hours | winit WASM setup |
| M4: WASM run() convenience | 2-3 hours | `wasm-bindgen-futures` |

**Total:** ~8-12 hours for a minimal WASM port.
