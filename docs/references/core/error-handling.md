# Research: Error Handling Design for a Multi-Backend Terminal/Grid Rendering Library

## Summary

A Rust terminal/grid rendering library with multiple backends (terminal emulators via crossterm, GPU
via wgpu, native windows via winit) needs a layered error strategy. The core principle: separate
_construction-time_ errors (fallible, return `Result`) from _frame-time_ operations (mostly
infallible on the hot path, with a status-enum for GPU surface acquisition). Use `thiserror` for the
public error types; it generates identical code to hand-written impls and is not a semver-visible
dependency. Reserve panics strictly for invariant violations (programmer bugs), never for runtime
conditions like GPU context loss or terminal resize.

## Findings

### 1. Unified error type across backends

**Design a two-level error hierarchy: a backend-specific inner error and a backend-agnostic outer
error.**

wgpu demonstrates this pattern well. `CreateSurfaceError` has a public struct wrapping a
`pub(crate) CreateSurfaceErrorKind` enum with `#[cfg]`-gated variants for each backend (Hal, Web,
RawHandle). The same pattern appears in `RequestDeviceError` (Core, WebGpu, Custom variants). This
keeps the public API stable while letting each backend contribute its own error details.

For a grid rendering library, the recommended shape:

```rust
/// Errors that can occur during renderer initialization.
#[derive(Debug)]
#[non_exhaustive]
pub enum CreateRendererError {
    /// Window creation failed (winit OsError, etc.)
    Window(WindowError),
    /// GPU device/surface creation failed
    Gpu(GpuError),
    /// Terminal backend initialization failed (e.g., not a TTY)
    Terminal(TerminalError),
    /// Font loading or rasterization failed
    Font(FontError),
}

/// Errors during frame presentation.
#[derive(Debug)]
#[non_exhaustive]
pub enum PresentError {
    /// GPU surface was lost; must recreate surface
    SurfaceLost,
    /// GPU surface is outdated; must reconfigure
    SurfaceOutdated,
    /// GPU device was lost; must recreate device and all resources
    DeviceLost,
    /// Terminal I/O error during flush
    Terminal(std::io::Error),
}
```

Mark enums `#[non_exhaustive]` so new error variants can be added without breaking downstream code.
[wgpu source: surface.rs](https://github.com/gfx-rs/wgpu/blob/trunk/wgpu/src/api/surface.rs) |
[wgpu source: device.rs](https://github.com/gfx-rs/wgpu/blob/trunk/wgpu/src/api/device.rs)

### 2. thiserror vs manual Error impl

**Use thiserror. It generates byte-identical code to hand-written impls and does not appear in your
public API.**

From the thiserror docs: "Thiserror deliberately does not appear in your public API. You get the
same thing as if you had written an implementation of `std::error::Error` by hand, and switching
from handwritten impls to thiserror or vice versa is not a breaking change."

Key features that matter for a multi-backend library:

- `#[from]` generates `From` impls for backend-specific error types
- `#[source]` properly chains error sources for `Error::source()`
- `#[error(transparent)]` delegates Display and source to an inner error, useful for opaque wrapper
  types
- `#[error("...")]` with field interpolation for human-readable messages

The only reason to avoid thiserror would be `no_std` without `alloc`, but even that is supported
since thiserror 2.x. For library code, thiserror is the standard choice; anyhow is for applications.

```rust
use thiserror::Error;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum CreateRendererError {
    #[error("window creation failed")]
    Window(#[from] WindowError),
    #[error("GPU initialization failed")]
    Gpu(#[from] GpuError),
    #[error("terminal initialization failed")]
    Terminal(#[from] TerminalError),
    #[error("font loading failed: {path}")]
    Font { path: String, #[source] source: FontError },
}
```

[thiserror docs](https://docs.rs/thiserror/latest/thiserror/)

### 3. Fallible vs infallible operations

**The hot-path cell-write operation (`put`) should be infallible. Initialization, presentation, and
resource loading should be fallible.**

BearLibTerminal's API makes this case clearly: `terminal_put(x, y, code)` returns void.
`terminal_clear()` returns void. `terminal_refresh()` returns void. Only `terminal_open()` returns a
bool (success/failure). The rationale: writing a glyph to an in-memory grid buffer cannot fail. The
buffer is pre-allocated, coordinates are bounds-checked (or silently clamped/ignored), and no I/O
occurs until presentation.

Ratatui's `Backend` trait takes a different approach: every method returns
`Result<(), Self::Error>`, including `draw()`, `clear()`, and `flush()`. This makes sense for
ratatui because `draw()` may immediately write escape sequences to a terminal (I/O can fail). But
for a library that double-buffers into an in-memory grid and then presents, the separation is
cleaner.

Recommended classification:

| Operation                        | Fallibility                           | Rationale                                                                  |
| -------------------------------- | ------------------------------------- | -------------------------------------------------------------------------- |
| `Renderer::new()`                | `Result<Self, CreateRendererError>`   | Window, GPU, font loading can all fail                                     |
| `Grid::put(x, y, glyph)`         | infallible (no return or `&mut self`) | Writes to in-memory buffer; out-of-bounds silently ignored or debug_assert |
| `Grid::put_checked(x, y, glyph)` | `Option<()>` or `bool`                | Optional checked variant returns None if OOB                               |
| `Grid::clear()`                  | infallible                            | Zeroes in-memory buffer                                                    |
| `Renderer::present(&grid)`       | `Result<(), PresentError>`            | GPU surface acquire, terminal I/O can fail                                 |
| `Renderer::resize(w, h)`         | `Result<(), ResizeError>`             | GPU surface reconfigure can fail                                           |
| `FontAtlas::load(path)`          | `Result<Self, FontError>`             | File I/O, parsing can fail                                                 |

The key insight: separate the "grid manipulation" layer (infallible, operates on memory) from the
"rendering/presentation" layer (fallible, touches OS/GPU/terminal).

[BearLibTerminal reference](http://foo.wyrd.name/en:bearlibterminal:reference) |
[ratatui Backend trait](https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/backend.rs)

### 4. GPU context loss recovery

**wgpu uses a status enum (not Result) for surface texture acquisition. GPU context loss requires
full resource recreation.**

wgpu's `Surface::get_current_texture()` returns `CurrentSurfaceTexture`, an enum with seven
variants:

```rust
pub enum CurrentSurfaceTexture {
    Success(SurfaceTexture),   // All good
    Suboptimal(SurfaceTexture), // Works but reconfigure recommended
    Timeout,                    // Skip this frame, try again
    Occluded,                   // Window minimized, skip frame
    Outdated,                   // Call configure() and retry
    Lost,                       // Recreate surface entirely
    Validation,                 // Validation error caught by error scope
}
```

This is a status enum, not a Result. The caller pattern-matches on outcomes and takes different
recovery actions. This is better than `Result<SurfaceTexture, SurfaceError>` because `Suboptimal`
carries a usable texture (it's not an "error"), and different failure modes require different
recovery strategies.

Recovery hierarchy:

1. **Timeout / Occluded**: Skip the frame. Try again next iteration.
2. **Suboptimal**: Use the texture this frame, but reconfigure before next frame.
3. **Outdated**: Call `surface.configure()` with current window size, then retry immediately.
4. **Lost**: Recreate the surface via `instance.create_surface()`, then configure, then retry. If
   this also fails, the GPU device itself may be lost.
5. **Device Lost**: Detected via `Device::set_device_lost_callback()`. Must recreate the entire
   device, all GPU resources (buffers, textures, pipelines), and surface.

For WebGL, context loss fires a `webglcontextlost` DOM event. The app must call
`event.preventDefault()` to signal it wants to recover, then wait for `webglcontextrestored` before
recreating all GL resources. `WebGLRenderingContext.isContextLost()` can be polled.

For OpenGL (desktop), `GL_CONTEXT_LOST` (ARB_robustness) can be checked after
`glGetGraphicsResetStatus()`. Most desktop GL implementations rarely lose context, but it does
happen on driver crashes or GPU resets.

[wgpu CurrentSurfaceTexture](https://docs.rs/wgpu/latest/wgpu/enum.CurrentSurfaceTexture.html) |
[MDN webglcontextlost](https://developer.mozilla.org/en-US/docs/Web/API/HTMLCanvasElement/webglcontextlost_event)

### 5. Terminal resize race conditions

**Terminal resize is inherently racy. Query size at frame start, not per-operation.**

The SIGWINCH signal (Unix) or console event (Windows) notifies that the terminal has been resized,
but the actual new size may not be available until after the signal handler returns. Multiple resize
events can arrive in rapid succession (user dragging window edge). The `ioctl(TIOCGWINSZ)` call to
get the new size can return stale values if called during the resize.

The safe pattern:

1. Set a flag on resize signal (atomic bool or channel).
2. At the top of the frame loop, if the flag is set, query the new terminal size.
3. Reallocate the in-memory grid to match the new size.
4. Redraw the entire frame (no partial update assumptions).

Do not query terminal size mid-frame or assume it's stable between put() calls. The grid buffer
should have its own authoritative size; the terminal size is consulted only at presentation time.

Ratatui handles this by calling `backend.size()` at the start of `Terminal::draw()` and using that
size for the entire frame. If a resize happens mid-frame, it's picked up on the next frame.

For GPU backends, resize is handled differently: winit delivers `WindowEvent::Resized(PhysicalSize)`
events, and the application must call `surface.configure()` with the new dimensions before the next
present.

### 6. Graceful degradation (backend fallback)

**Attempt backends in order of preference. Surface-level try/fallback, not per-operation
switching.**

The fallback should happen at initialization time, not mid-operation:

```rust
pub fn create_renderer(config: &Config) -> Result<Box<dyn Renderer>, CreateRendererError> {
    if config.allow_gpu {
        match GpuRenderer::new(config) {
            Ok(r) => return Ok(Box::new(r)),
            Err(e) => log::warn!("GPU backend unavailable: {e}, falling back"),
        }
    }
    match TerminalRenderer::new(config) {
        Ok(r) => Ok(Box::new(r)),
        Err(e) => Err(CreateRendererError::AllBackendsFailed {
            gpu: config.allow_gpu.then_some(gpu_err),
            terminal: e,
        }),
    }
}
```

Mid-session fallback (e.g., GPU device lost, fall back to terminal) is architecturally complex and
usually not worth implementing. The better strategy: on device lost, attempt to recreate the GPU
resources. If that fails after N retries, report the error to the application and let it decide
whether to restart with a different backend.

winit's error model supports this: `EventLoopError::NotSupported(NotSupportedError)` explicitly
indicates that a backend feature isn't available, allowing the caller to try alternatives.
[winit error types](https://docs.rs/winit/0.30.13/winit/error/index.html)

### 7. Error context and chaining

**Use `#[source]` and `#[from]` for error chaining. Add context via enum variant fields, not string
wrappers.**

The `Error::source()` method forms a chain of causes. thiserror's `#[source]` attribute implements
this correctly. Prefer structured context (typed fields) over string context:

```rust
#[derive(Error, Debug)]
pub enum FontError {
    #[error("failed to read font file: {path}")]
    Io { path: PathBuf, #[source] source: std::io::Error },

    #[error("failed to parse font: {path}")]
    Parse { path: PathBuf, #[source] source: FontParseError },

    #[error("missing required glyph U+{codepoint:04X} in font {font_name}")]
    MissingGlyph { font_name: String, codepoint: u32 },
}
```

This is better than `anyhow::Context` for libraries because:

- Callers can match on specific variants
- No dynamic allocation for context strings
- The error chain is fully typed

For the private/internal boundary, a pattern from wgpu: public error types wrap `pub(crate)` error
kind enums. This lets internal code use detailed error variants while keeping the public surface
minimal.

### 8. Panic policy

**Libraries should almost never panic. Panics are for violated invariants (programmer bugs), not
runtime failures.**

The Rust Book's guideline: panic when "some assumption, guarantee, contract, or invariant has been
broken" and continuing would be unsafe or nonsensical. Return `Result` for all expected failures.

The Rust API Guidelines (C-VALIDATE) prescribe a preference order: static enforcement via types >
dynamic enforcement returning Result > dynamic enforcement with panic > debug_assert.

Concrete policy for a rendering library:

| Situation                                           | Action                                                 |
| --------------------------------------------------- | ------------------------------------------------------ |
| Out-of-bounds grid access in `put()`                | Silent no-op (like BearLibTerminal) or `debug_assert!` |
| GPU device lost                                     | Return error, let caller handle                        |
| Terminal I/O failure                                | Return error                                           |
| Font file not found                                 | Return error from loader                               |
| Double-initialization                               | Panic (programmer bug, violated invariant)             |
| Internal state corruption                           | Panic with descriptive message                         |
| Invalid enum discriminant from FFI                  | Panic (unreachable state)                              |
| Null pointer from C API                             | Panic (violated contract)                              |
| Index into pre-allocated vec with known-valid index | `unreachable!()` or `expect("index was validated")`    |

wgpu's approach: `Surface::configure()` panics if an old SurfaceTexture is still alive (programmer
bug: violated ownership contract). But `get_current_texture()` returns a status enum for all runtime
conditions. Destructors (C-DTOR-FAIL) must never panic; use a separate `close()` method that returns
`Result` for cleanup errors.

[Rust API Guidelines: Dependability](https://rust-lang.github.io/api-guidelines/dependability.html)
| [Rust Book: To panic or not](https://doc.rust-lang.org/book/ch09-03-to-panic-or-not-to-panic.html)

### 9. How ratatui, crossterm, wgpu, and winit structure their error types

**ratatui**: Uses an associated type `Backend::Error: core::error::Error` on the Backend trait. Each
backend impl specifies its own error type. CrosstermBackend uses `std::io::Error`. TestBackend also
uses `std::io::Error`. This is maximally flexible but means the library's `Terminal<B>` is generic
over the error type, which propagates generics everywhere.

**crossterm**: Uses `std::io::Result<T>` (i.e., `Result<T, std::io::Error>`) for all operations. No
custom error types. Simple, but loses the ability to distinguish "terminal doesn't support this
feature" from "write failed" from "not a TTY". This is the simplest possible approach.

**wgpu**: Multiple specific error types per operation:

- `CreateSurfaceError` (surface creation)
- `RequestDeviceError` (device creation)
- `CurrentSurfaceTexture` (surface acquisition, status enum not Result)
- `DeviceLostReason` enum + callback for device loss
- Error scopes (`push_error_scope` / `pop_error_scope`) for GPU validation errors
- No single unified error type. Each fallible operation has its own error.

This per-operation approach is the most precise but produces many types.

**winit**: Small focused error types:

- `EventLoopError` enum: NotSupported, Os, RecreationAttempt, ExitFailure
- `OsError` struct (opaque, wraps platform-specific error with Display)
- `NotSupportedError` struct (the backend doesn't support this operation)
- `ExternalError` enum: NotSupported, Os (for public API use)

winit keeps `OsError` opaque (private fields) so the internal representation can change without
semver breaks. Good pattern for wrapping platform-specific errors.

### 10. Concrete Rust error type design

Pulling together all findings, here's a concrete error type hierarchy for a multi-backend grid
rendering library:

```rust
use std::path::PathBuf;
use thiserror::Error;

// ── Initialization Errors ──

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum CreateRendererError {
    #[error("window creation failed")]
    Window(#[source] WindowError),

    #[error("GPU initialization failed")]
    Gpu(#[source] GpuError),

    #[error("terminal initialization failed")]
    Terminal(#[source] TerminalError),

    #[error("no suitable backend available")]
    NoBackend,
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum WindowError {
    #[error("platform error: {0}")]
    Os(String), // Opaque, wraps winit::OsError

    #[error("operation not supported on this platform")]
    NotSupported,
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum GpuError {
    #[error("no suitable GPU adapter found")]
    NoAdapter,

    #[error("GPU device request failed")]
    DeviceRequest(String),

    #[error("surface creation failed")]
    SurfaceCreation(String),
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum TerminalError {
    #[error("not a terminal (not a TTY)")]
    NotATty,

    #[error("terminal I/O error")]
    Io(#[from] std::io::Error),
}

// ── Font Errors ──

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum FontError {
    #[error("failed to read font file: {path}")]
    Io { path: PathBuf, #[source] source: std::io::Error },

    #[error("failed to parse font: {path}")]
    Parse { path: PathBuf, #[source] source: Box<dyn std::error::Error + Send + Sync> },

    #[error("font has no glyph for U+{codepoint:04X}")]
    MissingGlyph { codepoint: u32 },

    #[error("failed to rasterize glyph U+{codepoint:04X} at size {size}px")]
    Rasterize { codepoint: u32, size: f32 },
}

// ── Frame/Present Errors ──

/// Result of attempting to acquire a frame for rendering.
/// Modeled after wgpu's CurrentSurfaceTexture: a status enum, not a Result.
#[derive(Debug)]
pub enum FrameStatus<F> {
    /// Frame is ready for rendering.
    Ready(F),

    /// Frame is usable but the surface should be reconfigured.
    Suboptimal(F),

    /// Surface is outdated. Call resize() and try again.
    NeedsResize,

    /// Surface/device was lost. Call recreate() or reinitialize.
    Lost,

    /// Window is occluded (minimized). Skip this frame.
    Occluded,
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum PresentError {
    #[error("GPU surface lost")]
    SurfaceLost,

    #[error("GPU device lost")]
    DeviceLost,

    #[error("terminal I/O error")]
    TerminalIo(#[from] std::io::Error),
}

// ── Grid Operations (infallible) ──

// These are NOT error types. Grid operations like put(), clear(),
// set_fg(), set_bg() operate on in-memory buffers and cannot fail.
// Out-of-bounds coordinates are silently ignored (or debug_assert!).
//
// impl Grid {
//     pub fn put(&mut self, x: u32, y: u32, glyph: char) { /* no Result */ }
//     pub fn clear(&mut self) { /* no Result */ }
// }
```

Design rationale:

- `#[non_exhaustive]` on all public enums so variants can be added in minor versions
- Per-domain error types (Font, Window, Gpu, Terminal) rather than one mega-enum
- `CreateRendererError` aggregates domain errors for the initialization path
- `FrameStatus<F>` is a status enum (like wgpu's `CurrentSurfaceTexture`), not a Result
- `PresentError` is for the final present/flush step
- Grid manipulation has no error types at all
- String-wrapped platform errors keep the public API stable (like winit's `OsError`)

## Sources

- **Kept**: [wgpu surface.rs](https://github.com/gfx-rs/wgpu/blob/trunk/wgpu/src/api/surface.rs) -
  Primary source for GPU error handling patterns, CreateSurfaceError design
- **Kept**:
  [wgpu surface_texture.rs](https://github.com/gfx-rs/wgpu/blob/trunk/wgpu/src/api/surface_texture.rs) -
  CurrentSurfaceTexture status enum pattern
- **Kept**: [wgpu device.rs](https://github.com/gfx-rs/wgpu/blob/trunk/wgpu/src/api/device.rs) -
  RequestDeviceError, device lost callback, error scopes
- **Kept**:
  [ratatui Backend trait](https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/backend.rs) -
  Associated error type pattern, all-Result API design
- **Kept**: [winit error types](https://docs.rs/winit/0.30.13/winit/error/index.html) -
  EventLoopError, OsError, NotSupportedError hierarchy
- **Kept**: [thiserror docs](https://docs.rs/thiserror/latest/thiserror/) - Derive macro
  documentation, #[from]/#[source]/#[error] attributes
- **Kept**: [BearLibTerminal reference](http://foo.wyrd.name/en:bearlibterminal:reference) -
  Infallible API design precedent
- **Kept**:
  [Rust Book ch9.3](https://doc.rust-lang.org/book/ch09-03-to-panic-or-not-to-panic.html) - Panic vs
  Result guidelines
- **Kept**:
  [Rust API Guidelines: Dependability](https://rust-lang.github.io/api-guidelines/dependability.html) -
  C-VALIDATE, C-DTOR-FAIL
- **Kept**:
  [MDN webglcontextlost](https://developer.mozilla.org/en-US/docs/Web/API/HTMLCanvasElement/webglcontextlost_event) -
  WebGL context loss handling
- **Kept**:
  [MDN WebGL best practices](https://developer.mozilla.org/en-US/docs/Web/API/WebGL_API/WebGL_best_practices) -
  Context loss recovery guidance
- **Dropped**: crossterm main docs page - Only showed API usage examples, error types are just
  `std::io::Error` re-exports, no custom error design to study

## Gaps

1. **M-PANIC-IS-STOP**: The task referenced "Microsoft guidelines M-PANIC-IS-STOP" but web search
   was unavailable. Based on the name, this likely refers to treating panics as process-stopping
   events (not recoverable), consistent with the Rust panic policy described above. The Rust API
   Guidelines and Rust Book coverage should be equivalent.

2. **OpenGL `GL_CONTEXT_LOST` specifics**: Could not fetch ARB_robustness spec details due to search
   provider unavailability. The general pattern is known: call `glGetGraphicsResetStatus()` and
   check for `GL_GUILTY_CONTEXT_RESET` / `GL_INNOCENT_CONTEXT_RESET` / `GL_UNKNOWN_CONTEXT_RESET`.
   Desktop GL context loss is rare compared to WebGL/mobile.

3. **Real-world terminal resize race condition data**: No empirical data on how often resize races
   cause issues in practice. The mitigation (query once per frame, use that size throughout) is
   well-established wisdom but hard to find formal analysis of.

4. **thiserror 2.x no_std support details**: thiserror 2.x claims no_std support, but the exact
   feature flag configuration for `no_std + alloc` was not verified in source.
