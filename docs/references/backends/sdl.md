# Research: SDL2/SDL3 Backend for Rust Terminal/Grid Rendering

## Summary

SDL provides a proven, hardware-accelerated 2D rendering API (`SDL_Renderer`) that maps directly to
cell-grid rendering via texture atlas + `SDL_RenderCopy` blitting. The `sdl2` Rust crate (v0.38) is
mature and stable; the `sdl3` crate is in active migration from SDL2 bindings but not yet
production-ready. For a terminal grid library, SDL offers a simpler path to hardware-accelerated 2D
than winit+wgpu, with built-in gamepad/joystick support, but introduces a C dependency and is less
idiomatic for the Rust ecosystem.

## Findings

### 1. SDL2 vs SDL3 Rust Crate Ecosystem

1. **`sdl2` crate (v0.38) is mature and stable.** It wraps SDL 2.0 with idiomatic Rust types,
   lifetime-safe textures (or opt-in `unsafe-textures` for easier ownership), and features for
   `ttf`, `image`, `mixer`, `gfx`. Supports a `bundled` feature that compiles SDL2 from source,
   eliminating system dependency issues. Has `raw-window-handle` support for interop with
   wgpu/Vulkan. [crates.io/crates/sdl2](https://crates.io/crates/sdl2)

2. **`sdl3` crate (v0.x) is a fork of rust-sdl2, still in migration.** The SDL3 C API itself
   stabilized in late 2024, but the Rust bindings are incomplete. Tests are being fixed, examples
   being updated, not all SDL3 new features are wrapped yet. Migration checklist on the repo shows
   ongoing work. Uses `sdl3-sys` (by maia-s) for low-level bindings. Extension library support:
   SDL_image, SDL_mixer, SDL_ttf are supported; SDL_gfx, SDL_net, SDL_shadercross are not yet.
   [crates.io/crates/sdl3](https://crates.io/crates/sdl3),
   [github.com/vhspace/sdl3-rs](https://github.com/vhspace/sdl3-rs)

3. **SDL3 new features relevant to grid rendering:** `SDL_RenderTextureTiled` for tiling a texture
   region across an area (useful for background fills), GPU compute API for advanced rendering,
   improved HiDPI support, better colorspace management, pen/tablet API, and system tray support.
   [wiki.libsdl.org/SDL3/NewFeatures](https://wiki.libsdl.org/SDL3/NewFeatures)

4. **Recommendation:** Target SDL2 (`sdl2` crate) for now. It is battle-tested, well-documented, and
   the `bundled` feature makes distribution straightforward. Plan for SDL3 migration later when the
   Rust bindings stabilize, since the API concepts are nearly identical.

### 2. SDL's 2D Renderer (SDL_Renderer)

5. **SDL_Renderer is a hardware-accelerated 2D rendering abstraction.** It sits on top of OpenGL,
   Direct3D, Metal, or Vulkan (selected automatically). It provides: single pixel points, lines,
   filled rectangles, texture blitting (copy/copy_ex with rotation), blend modes, render-to-texture,
   logical resolution scaling, and viewport/clipping support.
   [docs.rs/sdl2/latest/sdl2/render](https://docs.rs/sdl2/latest/sdl2/render/index.html)

6. **Canvas<Window> is the primary Rust interface.** You call `window.into_canvas().build()` to get
   a `Canvas<Window>`, then use `canvas.copy()` (wrapping `SDL_RenderCopy`) to blit texture regions.
   The workflow is: `canvas.clear()`, draw everything with `copy()` calls, then `canvas.present()`
   to flip the backbuffer. VSync is opt-in via `.present_vsync()`.
   [docs.rs/sdl2/.../Canvas](https://docs.rs/sdl2/latest/sdl2/render/struct.Canvas.html)

7. **TextureCreator manages GPU texture lifetime.** Created from a Canvas, it ensures textures
   cannot outlive their parent renderer. Three access modes: `Static` (upload once, blit many),
   `Streaming` (CPU-updateable), `Target` (render-to-texture). For a glyph atlas, `Static` is the
   right choice.
   [docs.rs/sdl2/.../TextureCreator](https://docs.rs/sdl2/latest/sdl2/render/struct.TextureCreator.html)

8. **SDL3 adds `SDL_RenderTextureTiled`** which tiles a source rectangle across a destination area.
   Directly useful for filling cell backgrounds with a single call rather than looping
   `SDL_RenderCopy` per cell.
   [wiki.libsdl.org/SDL3/SDL_RenderTextureTiled](https://wiki.libsdl.org/SDL3/SDL_RenderTextureTiled)

### 3. Cell Grid Rendering with SDL (Texture Atlas Approach)

9. **Texture atlas pattern for cell grids:** Pre-render all needed glyphs into a single large
   texture (e.g., 512x512 or 1024x1024). Each glyph occupies a known rectangle. To draw a cell, call
   `canvas.copy(&atlas, src_rect, dst_rect)` where `src_rect` picks the glyph from the atlas and
   `dst_rect` positions it in the grid. This is the same approach BearLibTerminal uses (though BLT
   uses raw OpenGL).

10. **Rendering loop for an 80x50 grid:**

    ```
    canvas.clear();
    for each cell (x, y):
        // Draw background
        canvas.set_draw_color(cell.bg);
        canvas.fill_rect(dst_rect(x, y));
        // Draw glyph
        atlas_texture.set_color_mod(cell.fg.r, cell.fg.g, cell.fg.b);
        canvas.copy(&atlas_texture, glyph_src_rect, dst_rect(x, y));
    canvas.present();
    ```

    This issues 4000 draw calls for an 80x50 grid. SDL batches these internally (SDL2 added render
    batching in 2.0.10), so the actual GPU command count is much lower.

11. **Color modulation via `texture.set_color_mod(r, g, b)` and `set_alpha_mod(a)`** allows tinting
    a white-on-transparent glyph atlas with arbitrary foreground colors without needing separate
    textures per color. This is the standard approach for colored text in SDL 2D games.

12. **Performance considerations:** SDL_Renderer is not a GPU draw-call powerhouse. For an 80x50
    grid (4000 cells), it is more than adequate. For very large grids (200x100+), the per-cell
    `canvas.copy()` loop may become a bottleneck. Mitigation: render-to-texture caching (only
    re-render dirty cells to a cached texture), or batch changed cells. SDL_Renderer internally
    batches sequential copy calls with the same texture, so the atlas approach naturally benefits
    from batching.

### 4. SDL Event/Input System vs winit

13. **SDL event model is a polled queue.** You call `event_pump.poll_iter()` in your main loop to
    drain events. The `Event` enum has 51 variants covering keyboard, mouse, window, joystick,
    gamepad, touch, drop, audio device, and custom user events. Events carry a `timestamp` and
    relevant fields inline (no trait-based dispatch).
    [docs.rs/sdl2/latest/sdl2/event/enum.Event](https://docs.rs/sdl2/latest/sdl2/event/enum.Event.html)

14. **SDL has built-in gamepad/joystick support** with the GameController API (SDL2) / Gamepad API
    (SDL3). This includes a database of known controller mappings, rumble, LED control, touchpad,
    and sensor support. winit has no gamepad support at all; you need a separate crate like `gilrs`.
    SDL3 expanded this with cap-sense, Steam Deck integration, and more.

15. **winit uses a callback/closure model** (`event_loop.run(|event, target| { ... })`) rather than
    polling. This is more idiomatic for Rust's ownership model and required on some platforms
    (macOS, iOS, web). SDL's polling model is simpler to reason about but requires the user to
    manage the main loop manually.

16. **Key differences for a terminal grid library:**
    - SDL: `Keycode` + `Scancode` + `Mod` in `KeyDown`/`KeyUp`, plus separate `TextInput` events for
      Unicode text. Gamepad events are first-class.
    - winit: `KeyEvent` with `PhysicalKey`/`LogicalKey`, text via `text` field. No gamepad. Better
      IME support in recent versions.
    - SDL handles multiple mice/keyboards (SDL3), which winit does not.
    - SDL provides its own clipboard, cursor, and text input management.

### 5. Font Rendering: SDL_ttf vs fontdue/cosmic-text + SDL Textures

17. **SDL_ttf wraps FreeType for TrueType rendering.** With the `ttf` feature, the `sdl2` crate
    exposes `Font::render_char()` and `Font::render()` which produce SDL Surfaces. These surfaces
    are then uploaded as textures via `texture_creator.create_texture_from_surface()`. This is
    convenient but adds FreeType as another C dependency.
    [docs.rs/sdl2/latest/sdl2/ttf](https://docs.rs/sdl2/latest/sdl2/ttf/index.html)

18. **Preferred approach: fontdue/cosmic-text for rasterization, SDL texture for upload.** Rasterize
    glyphs in pure Rust with `fontdue` (fast, simple, no C deps) or `cosmic-text` (full shaping,
    complex scripts). Write the resulting bitmaps into an RGBA pixel buffer, then upload to SDL via
    `texture_creator.create_texture_streaming()` + `texture.update(None, pixels, pitch)` or by
    creating an `SDL_Surface` from the pixel data and using `create_texture_from_surface()`.

19. **Atlas construction workflow:**

    ```
    // 1. Rasterize with fontdue
    let (metrics, bitmap) = font.rasterize('A', 16.0);
    // 2. Pack into atlas pixel buffer (bin-packing)
    atlas_pixels[y_offset..][..row_len].copy_from_slice(&bitmap_row);
    // 3. Upload atlas to SDL texture once
    let tex = texture_creator.create_texture_static(format, atlas_w, atlas_h)?;
    tex.update(None, &atlas_pixels, pitch)?;
    // 4. Store glyph -> Rect mapping for lookup during rendering
    ```

    This eliminates SDL_ttf/FreeType entirely, keeping the C dependency surface to just SDL itself.

### 6. Advantages of SDL over winit+GPU

20. **Simpler 2D API with no shader code.** SDL_Renderer provides `copy()`, `fill_rect()`, color
    modulation, blend modes, and render-to-texture without writing any GLSL/WGSL. A winit+wgpu
    backend requires vertex buffers, shader programs, pipeline state, bind groups, etc.

21. **Built-in gamepad/joystick/controller support.** For a roguelike-focused library, controller
    input matters. SDL's gamepad database and hotplug support are industry-standard. winit has zero
    gamepad support.

22. **Battle-tested in game development.** SDL is used by hundreds of commercial games and has been
    iterated for 25+ years. Edge cases around fullscreen, multi-monitor, resolution changes, and
    input handling are well-covered. BearLibTerminal itself was originally built on SDL (for
    windowing, though it used OpenGL for rendering).

23. **Audio and other multimedia.** If the library or downstream users want sound effects, SDL_mixer
    is right there. This doesn't directly relate to grid rendering but adds value for game-dev
    users.

24. **`bundled` feature on the sdl2 crate** compiles SDL from source, making deployment a
    `cargo build` with no external dependency hunting. This significantly reduces the "C dependency
    pain" for end users.

### 7. Disadvantages of SDL

25. **C dependency and FFI boundary.** Even with `bundled`, SDL is a C library. Texture lifetimes in
    the `sdl2` crate use complex lifetime annotations (or require `unsafe-textures` opt-in). Error
    handling comes as string messages from C, not structured Rust errors. Debug builds can be harder
    when stepping across FFI.

26. **Less idiomatic Rust.** SDL's API was designed for C. The Rust wrappers are good but feel
    different from native Rust libraries. The `Canvas` / `TextureCreator` / `Texture` lifetime dance
    is a common pain point. State is held in opaque C structs.

27. **Duplicate functionality with winit.** If the project already has a winit backend, SDL
    duplicates window management, event handling, and (partially) input. Users can't mix SDL windows
    with winit windows. Having both backends means maintaining two parallel platform abstraction
    layers.

28. **Ecosystem friction.** Most modern Rust game/graphics projects use winit+wgpu (or winit+glow).
    Crates like `egui`, `bevy`, `pixels`, and `softbuffer` all target winit. Using SDL means the
    library can't trivially interop with these ecosystems (though `raw-window-handle` support
    helps).

29. **Single-threaded rendering constraint.** `SDL_Renderer` is not designed for multi-threaded use.
    All rendering must happen on the main thread. This is also true for most graphics APIs, but SDL
    makes it explicit and enforced.

### 8. How bracket-lib's SDL Support Worked

30. **bracket-lib never had a direct SDL backend.** Its backends are: OpenGL (via glutin/winit,
    default), WebGL (for wasm32), wgpu (via winit), crossterm (terminal), and curses
    (ncurses/pdcurses). The HAL module (`bracket-terminal/src/hal/`) selects at compile time via
    feature flags. There is no SDL feature flag.
    [github.com/amethyst/bracket-lib](https://github.com/amethyst/bracket-lib)

31. **bracket-lib's OpenGL backend is the closest analog.** It uses winit for windowing + OpenGL
    (via the `gl` crate) for rendering. It creates a texture atlas from tilesets, uploads to GL
    textures, and draws quads per cell, similar to what an SDL backend would do but with raw GL
    calls. This is architecturally what an SDL backend would replace: swap winit for SDL's window
    management and swap raw GL for SDL_Renderer.

32. **The lesson from bracket-lib:** The atlas-based grid rendering pattern is portable across
    backends. The core logic (cell grid state, dirty tracking, atlas packing) stays the same. Only
    the "upload texture" and "blit rect" primitives change between backends.

### 9. Cross-Platform Story

33. **SDL officially supports:** Windows, macOS, Linux, iOS, Android. Community/vendor ports exist
    for Nintendo Switch, PlayStation, Xbox (via official SDL ports under NDA), Haiku, FreeBSD, and
    others. This is broader than winit, which supports Windows, macOS, Linux, iOS, Android, and web
    (via wasm).

34. **Console (Nintendo/PlayStation/Xbox) support** is a key differentiator. SDL has official ports
    maintained under platform NDA. winit has no console support. For a roguelike library aiming at
    indie game release on consoles, SDL is the path.

35. **Web support:** SDL3 has Emscripten support for compiling to WebAssembly. The `sdl2` Rust crate
    also supports wasm32 targets via Emscripten. However, winit's native web support (via
    `web-sys`/`wasm-bindgen` without Emscripten) is more ergonomic for pure-Rust web deployment.

36. **Mobile:** Both SDL and winit support iOS and Android. SDL's mobile support is more mature and
    battle-tested in shipped games. SDL handles the Android/iOS lifecycle (app background/foreground
    events) natively.

### 10. Trade-offs vs winit+softbuffer and winit+wgpu

37. **SDL vs winit+softbuffer:**
    - softbuffer is CPU-only rendering to a window surface. Zero GPU acceleration.
    - SDL_Renderer is GPU-accelerated. For a static 80x50 grid, softbuffer is fine. For animations,
      effects, smooth scrolling, or large grids, SDL wins on performance.
    - softbuffer is pure Rust, zero C deps, tiny binary impact. SDL adds ~1-3MB.
    - softbuffer requires you to manage your own pixel buffer and font rasterization. SDL_Renderer
      gives you texture management and blitting primitives.
    - Verdict: SDL is strictly more capable for 2D grid rendering. softbuffer is simpler and lighter
      if GPU acceleration isn't needed.

38. **SDL vs winit+wgpu:**
    - wgpu gives full GPU pipeline control. You can do instanced rendering (one draw call for 4000
      cells), custom shaders for effects, compute shaders for advanced processing.
    - SDL_Renderer is limited to its fixed-function 2D API. No custom shaders (in SDL2; SDL3 adds a
      GPU API but the Rust bindings don't wrap it yet).
    - wgpu is pure Rust (Rust-native implementation). SDL is C with Rust bindings.
    - wgpu + winit is the modern Rust ecosystem standard. More community support, more examples,
      better tooling integration.
    - wgpu has a steeper learning curve. SDL_Renderer can be productive in minutes.
    - Verdict: wgpu is more powerful and more Rust-native. SDL is faster to implement, simpler to
      maintain, and has gamepad support. For a terminal grid library where the rendering is
      inherently simple (blit rectangles), SDL's simplicity is a genuine advantage.

39. **Hybrid approach (SDL for windowing/input, wgpu/GL for rendering):** The `sdl2` crate supports
    `raw-window-handle`, meaning you can use SDL for window creation, event handling, and gamepad
    input, but render with wgpu or raw OpenGL. This gives you SDL's input advantages without being
    limited to SDL_Renderer's 2D API. BearLibTerminal itself did something similar: SDL for
    windowing, OpenGL for rendering.

## Sources

- Kept: [sdl2 crate README](https://crates.io/crates/sdl2) - Primary source for Rust SDL2 bindings
  features, build options, API examples
- Kept: [sdl3 crate README](https://crates.io/crates/sdl3) - Current state of SDL3 Rust bindings
  migration
- Kept: [SDL3 NewFeatures wiki](https://wiki.libsdl.org/SDL3/NewFeatures) - What SDL3 adds over SDL2
- Kept: [sdl2::render docs](https://docs.rs/sdl2/latest/sdl2/render/index.html) - 2D renderer API
  reference
- Kept: [Canvas<T> docs](https://docs.rs/sdl2/latest/sdl2/render/struct.Canvas.html) - Drawing
  methods, texture blitting, render-to-texture
- Kept: [TextureCreator docs](https://docs.rs/sdl2/latest/sdl2/render/struct.TextureCreator.html) -
  Texture creation and lifetime management
- Kept: [sdl2::event::Event docs](https://docs.rs/sdl2/latest/sdl2/event/enum.Event.html) - Full
  event type reference (51 variants)
- Kept: [SDL3 CategoryGamepad wiki](https://wiki.libsdl.org/SDL3/CategoryGamepad) - Gamepad API
  scope and features
- Kept: [SDL3 SDL_RenderTextureTiled](https://wiki.libsdl.org/SDL3/SDL_RenderTextureTiled) - New
  tiled rendering primitive
- Kept:
  [SDL3 SDL_CreateTextureFromSurface](https://wiki.libsdl.org/SDL3/SDL_CreateTextureFromSurface) -
  Texture creation from surface
- Kept: [sdl2::ttf docs](https://docs.rs/sdl2/latest/sdl2/ttf/index.html) - TTF font rendering
  module
- Kept: [bracket-lib GitHub](https://github.com/amethyst/bracket-lib) - Backend architecture,
  feature flags, HAL design
- Kept: [BearLibTerminal GitHub](https://github.com/cfyzium/bearlibterminal) - Original SDL-based
  terminal library reference
- Dropped: bracket-lib HAL directory listing - GitHub rendered navigation chrome instead of useful
  content
- Dropped: BearLibTerminal Source directory - Same GitHub rendering issue

## Gaps

1. **BearLibTerminal's actual SDL usage code** could not be inspected directly. The README states it
   uses OpenGL for rendering (not SDL_Renderer). Whether SDL was used only for windowing or also for
   early rendering versions is unclear from available documentation. The project's build system uses
   CMake and links against both SDL2 and OpenGL.

2. **SDL_Renderer batching performance benchmarks** for high cell counts (10,000+) could not be
   found. The claim that SDL batches draw calls internally (added in SDL 2.0.10) is documented but
   quantitative data on throughput vs. raw OpenGL or wgpu instanced rendering is not readily
   available.

3. **sdl3 crate timeline** for production readiness is uncertain. The migration checklist is public
   but there's no stated release date or stability guarantee. The SDL3 C library itself hit stable
   (3.2.0), but the Rust bindings lag.

4. **Console platform (Switch/PS/Xbox) SDL usage from Rust** is undocumented publicly due to NDA
   restrictions. It's known to work (Celeste, other indie titles ship with SDL on consoles), but the
   Rust cross-compilation story for those targets is not clear.

5. **SDL3 GPU API Rust bindings** (`SDL_shadercross`, compute, custom shaders) are explicitly marked
   as not supported in the sdl3 crate. This limits SDL3's advantage over SDL2 for Rust users.
