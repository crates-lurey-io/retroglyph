# ADR 007: Software Rendering Backend

**Status:** Draft **Date:** 2026-06-19 **Parent:** [ADR 001: Architecture](001-architecture.md)

## Context

`rg` currently ships with a `HeadlessBackend` (for testing) and a `CrosstermBackend` (for TTY
usage). To provide rich visual capabilities (custom fonts, pixel-perfect layouts, sub-cell offsets)
while remaining highly portable, we require a graphical backend.

As researched in `docs/references/backends/software-window.md`, a pure software rendering backend
(CPU-based) provides significant advantages over GPU backends for terminal grids: no complex shader
dependencies, flawless headless operation (e.g. `Xvfb`), zero GPU driver issues, and trivial
WebAssembly portability.

## Decisions & Rust API Guidelines

1.  **Window & Blitting:** We will use **`winit`** for cross-platform window creation and input
    events, coupled with **`softbuffer`** for pushing CPU-computed `Vec<u32>` pixel arrays to the
    window surface.
2.  **Thread Architecture:** Because `winit` strictly requires control of the main thread
    (especially on macOS), the `SoftwareBackend` will invert control. It will run the `winit` event
    loop on the main thread and optionally spawn the user's game loop on a background thread,
    communicating events and frame buffers via channels.
3.  **Glyph Rasterization:** We will use **`fontdue`** for text rasterization. It is extremely fast
    and lightweight.
4.  **Builder Pattern (C-BUILDER):** Complex window configuration (font size, window dimensions,
    title) will be constructed via a `SoftwareBackendBuilder`.
5.  **Good Errors (C-GOOD-ERR):** The backend will provide a dedicated `SoftwareBackendError` type
    implementing `std::error::Error` for font loading or window creation failures, rather than
    panicking.
6.  **Common Traits (C-COMMON-TRAITS):** All public configuration types will eagerly implement
    `Debug`, `Clone`, `PartialEq`, `Eq`, and `Default`.

---

## Detailed Implementation Milestones

### M1: Winit + Softbuffer Skeleton & Threading Model

**Goal:** Establish the `winit` loop and handle the main-thread constraint.

**1. Add Dependencies (`Cargo.toml`)**

```toml
[features]
default = []
software = ["winit", "softbuffer", "fontdue"]

[dependencies]
winit = { version = "0.29", optional = true }
softbuffer = { version = "0.4", optional = true }
fontdue = { version = "0.8", optional = true }
```

**2. Define Configuration & Errors (`src/backend/software/config.rs`)**

```rust
use std::fmt;

#[derive(Debug)]
pub enum SoftwareBackendError {
    WindowCreation(winit::error::OsError),
    Softbuffer(softbuffer::SoftBufferError),
    FontParse(&'static str),
}
impl std::error::Error for SoftwareBackendError {}
impl fmt::Display for SoftwareBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // ... C-GOOD-ERR concise lowercase formatting ...
        write!(f, "software backend error")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareBackendOptions {
    pub window_title: String,
    pub cell_width: u16,
    pub cell_height: u16,
    pub font_bytes: Vec<u8>,
}

impl Default for SoftwareBackendOptions {
    fn default() -> Self {
        Self {
            window_title: String::from("rg application"),
            cell_width: 10,
            cell_height: 20,
            font_bytes: Vec::new(), // In M2, this will default to an embedded font
        }
    }
}

// C-BUILDER implementation
pub struct SoftwareBackendBuilder {
    options: SoftwareBackendOptions,
}

impl SoftwareBackendBuilder {
    pub fn new() -> Self { Self { options: SoftwareBackendOptions::default() } }
    pub fn title(mut self, title: &str) -> Self { self.options.window_title = title.to_string(); self }
    pub fn cell_size(mut self, width: u16, height: u16) -> Self {
        self.options.cell_width = width;
        self.options.cell_height = height;
        self
    }
    pub fn font(mut self, bytes: &[u8]) -> Self { self.options.font_bytes = bytes.to_vec(); self }
    pub fn build(self) -> Result<SoftwareBackend, SoftwareBackendError> {
        SoftwareBackend::new(self.options)
    }
}
```

**3. The Backend Struct (`src/backend/software/mod.rs`)** Because `winit` takes over the main
thread, the backend must execute the loop.

```rust
pub struct SoftwareBackend {
    options: SoftwareBackendOptions,
    // Note: We cannot hold the window and surface directly here if we want
    // the user to call `terminal.draw()` on a background thread.
    // Instead, we store the config and initialize the window inside a `run` method.
}

impl SoftwareBackend {
    fn new(options: SoftwareBackendOptions) -> Result<Self, SoftwareBackendError> {
        Ok(Self { options })
    }

    /// Consumes the backend, taking over the main thread to run the `winit` loop.
    /// The user provides a closure that acts as the "update/render" game loop,
    /// which will be spawned on a background thread.
    pub fn run<F>(self, mut app_loop: F) -> Result<(), SoftwareBackendError>
    where
        F: FnMut(&mut crate::Terminal<Self>) + Send + 'static
    {
        // 1. Create winit EventLoop
        // 2. Create Window
        // 3. Initialize softbuffer Context and Surface
        // 4. Setup mpsc channels for Input Events (Window -> Game Thread)
        // 5. Setup mpsc channels for Framebuffers (Game Thread -> Window)
        // 6. Spawn `app_loop` on a new thread, passing it a channel-backed proxy backend.
        // 7. Call event_loop.run(...) and block the main thread.
        todo!()
    }
}
```

_Design Note:_ This inverted control flow is the standard Rust solution for `winit` + game loops
without resorting to `unsafe` hacks.

### M2: Fontdue Integration & Glyph Caching

**Goal:** Parse TTF fonts and cache rasterized alpha masks to prevent re-rasterizing the same
character.

**1. The Glyph Cache (`src/backend/software/font.rs`)**

```rust
use std::collections::HashMap;
use fontdue::{Font, FontSettings};

pub struct GlyphCache {
    font: Font,
    size_px: f32,
    /// Maps a character to its rasterized alpha mask and metrics
    cache: HashMap<char, RasterizedGlyph>,
}

pub struct RasterizedGlyph {
    pub width: usize,
    pub height: usize,
    pub coverage: Vec<u8>, // Alpha values 0-255
    pub left_offset: i32,
    pub top_offset: i32,
}

impl GlyphCache {
    pub fn new(font_bytes: &[u8], size_px: f32) -> Result<Self, SoftwareBackendError> {
        let font = Font::from_bytes(font_bytes, FontSettings::default())
            .map_err(SoftwareBackendError::FontParse)?;
        Ok(Self { font, size_px, cache: HashMap::new() })
    }

    pub fn get_or_rasterize(&mut self, ch: char) -> &RasterizedGlyph {
        self.cache.entry(ch).or_insert_with(|| {
            let (metrics, coverage) = self.font.rasterize(ch, self.size_px);
            RasterizedGlyph {
                width: metrics.width,
                height: metrics.height,
                coverage,
                left_offset: metrics.xmin,
                top_offset: metrics.ymin,
            }
        })
    }
}
```

_(Note on ADR 006 integration: For V0.1 of this backend, if a cell contains `extra` grapheme data,
we will only rasterize the primary `glyph: char`. Full ligature shaping requires `cosmic-text` and
is deferred to V0.2 of the graphical backend)._

### M3: Grid Compositing

**Goal:** Translate the `Grid` cells into `softbuffer` pixels.

**1. Blitting Algorithm** Inside the backend's draw routine (executed when `Terminal::present()` is
called):

```rust
fn blit_grid(
    buffer: &mut [u32],
    buffer_width: usize,
    grid: &crate::grid::Grid,
    cache: &mut GlyphCache,
    cell_w: usize,
    cell_h: usize,
) {
    for (idx, cell) in grid.cells().iter().enumerate() {
        let col = idx % grid.width() as usize;
        let row = idx / grid.width() as usize;

        let px_x = col * cell_w;
        let px_y = row * cell_h;

        // 1. Draw Background
        let bg_color = resolve_color(cell.style.bg); // Convert to 0x00RRGGBB
        for y in 0..cell_h {
            for x in 0..cell_w {
                buffer[(px_y + y) * buffer_width + (px_x + x)] = bg_color;
            }
        }

        // 2. Composite Foreground Glyph
        if cell.glyph != ' ' {
            let fg_color = resolve_color(cell.style.fg);
            let glyph = cache.get_or_rasterize(cell.glyph);

            // Center the glyph within the cell (simplified)
            let draw_x = px_x as i32 + glyph.left_offset;
            let draw_y = px_y as i32 + cell_h as i32 - glyph.height as i32 - glyph.top_offset;

            for gy in 0..glyph.height {
                for gx in 0..glyph.width {
                    let alpha = glyph.coverage[gy * glyph.width + gx] as u32;
                    if alpha > 0 {
                        let target_x = draw_x + gx as i32;
                        let target_y = draw_y + gy as i32;

                        if target_x >= 0 && target_y >= 0 && target_x < buffer_width as i32 {
                            let idx = (target_y as usize) * buffer_width + (target_x as usize);
                            buffer[idx] = blend_colors(bg_color, fg_color, alpha);
                        }
                    }
                }
            }
        }
    }
}

fn blend_colors(bg: u32, fg: u32, alpha: u32) -> u32 {
    if alpha == 255 { return fg; }
    if alpha == 0 { return bg; }

    // Fast integer alpha blending
    let inv_alpha = 255 - alpha;
    let r = (((fg >> 16) & 0xFF) * alpha + ((bg >> 16) & 0xFF) * inv_alpha) / 255;
    let g = (((fg >> 8) & 0xFF) * alpha + ((bg >> 8) & 0xFF) * inv_alpha) / 255;
    let b = ((fg & 0xFF) * alpha + (bg & 0xFF) * inv_alpha) / 255;

    (r << 16) | (g << 8) | b
}
```

### M4: Input Translation

**Goal:** Map `winit` events to `rg::Event`.

When the `winit` event loop receives `WindowEvent::KeyboardInput`, it translates and sends it over
the channel to the game thread:

```rust
fn translate_key(input: winit::event::KeyEvent) -> Option<crate::event::Event> {
    use winit::keyboard::{Key, NamedKey};

    if !input.state.is_pressed() {
        return None; // Only trigger on press for terminal parity
    }

    let code = match input.logical_key {
        Key::Named(NamedKey::Enter) => crate::event::KeyCode::Enter,
        Key::Named(NamedKey::Escape) => crate::event::KeyCode::Esc,
        Key::Named(NamedKey::ArrowUp) => crate::event::KeyCode::Up,
        Key::Character(c) => {
            let mut chars = c.chars();
            let ch = chars.next()?;
            crate::event::KeyCode::Char(ch)
        }
        _ => return None,
    };

    Some(crate::event::Event::Key(crate::event::KeyEvent {
        code,
        modifiers: crate::event::KeyModifiers::empty(), // Map modifiers dynamically
    }))
}
```

By following this detailed plan, we achieve a robust, software-rendered window capable of displaying
high-fidelity fonts at 60fps without requiring GPU drivers, completely decoupled from the logic
thread.
