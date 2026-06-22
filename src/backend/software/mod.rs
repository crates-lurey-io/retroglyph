//! Software rendering backend: winit window + softbuffer pixel blitting.
//!
//! # Architecture
//!
//! [`SoftwareBackend`] is a pure-config type (font, grid size, scale). It does
//! **not** implement [`Backend`] directly.  Call [`run_windowed`](SoftwareBackend::run_windowed)
//! to open a window and run the game loop, or
//! [`run_headless`](SoftwareBackend::run_headless) to obtain a
//! [`SoftwareRenderer`] that renders into memory without a window.
//!
//! [`SoftwareRenderer`] implements [`Backend`] and always has an active
//! rendering context — no `Option`, no runtime panics from missing state.

pub mod bitmap_font;
pub mod config;

#[cfg(feature = "software-tilesets")]
pub mod sprite_cache;
#[cfg(feature = "software-tilesets")]
pub mod tileset;

use crate::backend::Backend;
use crate::color::{AnsiColor, Color};

pub use bitmap_font::BitmapFont;
pub use config::{SoftwareBackend, SoftwareBackendBuilder, SoftwareBackendError};
pub mod windowed;
pub use windowed::WindowedBackend;

use crate::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crate::grid::{Pos, Size};
use crate::style::CellModifier;
use crate::tile::Tile;
#[cfg(feature = "software-tilesets")]
use alpha_blend::rgba::U8x4Rgba;
use bitmap_font::BitmapFont as Font;
use grixy::buf::GridBuf;
use grixy::ops::GridWrite;
use grixy::ops::layout::RowMajor;
#[cfg(feature = "software-tilesets")]
use sprite_cache::{Sprite, SpriteCache};
use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

// ── Public types ──────────────────────────────────────────────────────────────

/// The running half of the software backend.
///
/// A running software renderer, produced by [`SoftwareBackend::run_windowed`] or
/// [`SoftwareBackend::run_headless`].
///
/// Unlike [`SoftwareBackend`] (which is just configuration), this type
/// always has an active rendering context — its pixel buffer is always
/// available.  The `ctx` field is never `None`, so [`Backend`] methods never
/// panic for missing initialisation.
///
/// Call [`pixels`](Self::pixels) to inspect the rendered output, or use
/// [`Backend::draw`] and
/// [`Backend::draw_layers`] to render
/// into it.
pub struct SoftwareRenderer {
    options: SoftwareBackend,
    /// The bitmap font, extracted from `options.font` at construction time.
    /// Always present; the `Option` wrapper in `SoftwareBackend` is only for
    /// the builder validation step.
    font: BitmapFont,
    ctx: RenderContext,
    #[cfg(feature = "software-tilesets")]
    sprite_cache: Arc<SpriteCache>,
}

/// Softbuffer window surface.
///
/// Holds both the `Context` and `Surface`. The `_context` must outlive
/// `surface` (softbuffer requires it), but is only stored, not read.
struct WindowSurface {
    _context: softbuffer::Context<Arc<Window>>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
}

/// Errors that can occur when initializing a window surface.
#[derive(Debug)]
pub enum SurfaceError {
    /// Failed to create the softbuffer context from the window.
    Context(softbuffer::SoftBufferError),
    /// Failed to create the softbuffer surface from the window.
    Surface(softbuffer::SoftBufferError),
}

impl core::fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Context(e) => write!(f, "softbuffer context: {e}"),
            Self::Surface(e) => write!(f, "softbuffer surface: {e}"),
        }
    }
}

impl std::error::Error for SurfaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Context(e) | Self::Surface(e) => Some(e),
        }
    }
}

struct RenderContext {
    event_buffer: VecDeque<Event>,
    pixel_buf: GridBuf<u32, Vec<u32>, RowMajor>,
    window_surface: Option<WindowSurface>,
    cell_w: u32,
    cell_h: u32,
}

impl SoftwareRenderer {
    /// Creates a new renderer with the given buffer and cell dimensions.
    pub(crate) fn create(
        options: SoftwareBackend,
        font: BitmapFont,
        buf_w: usize,
        buf_h: usize,
        cell_w: u32,
        cell_h: u32,
        #[cfg(feature = "software-tilesets")] sprite_cache: Arc<SpriteCache>,
    ) -> Self {
        Self {
            options,
            font,
            ctx: RenderContext {
                event_buffer: VecDeque::new(),
                pixel_buf: GridBuf::from_buffer(vec![0u32; buf_w * buf_h], buf_w),
                window_surface: None,
                cell_w,
                cell_h,
            },
            #[cfg(feature = "software-tilesets")]
            sprite_cache,
        }
    }

    /// Returns a slice of the rendered pixel buffer (`0x00RRGGBB` format).
    ///
    /// The buffer length is `cols * (glyph_width * scale) * rows * (glyph_height * scale)`.
    /// Each `u32` is a packed RGB pixel with the top byte unused.
    ///
    /// This is always available — there is no `Option` wrapper because
    /// `SoftwareRenderer` is guaranteed to have an active rendering context.
    #[must_use]
    pub fn pixels(&self) -> &[u32] {
        self.ctx.pixel_buf.as_ref()
    }

    /// Push an event into the internal buffer.
    pub fn push_event(&mut self, event: Event) {
        self.ctx.event_buffer.push_back(event);
    }

    /// Initialize the window surface from a winit window.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError`] if the softbuffer context or surface cannot be created.
    pub fn init_surface(&mut self, window: &Arc<Window>) -> Result<(), SurfaceError> {
        let context = softbuffer::Context::new(window.clone()).map_err(SurfaceError::Context)?;
        let surface =
            softbuffer::Surface::new(&context, window.clone()).map_err(SurfaceError::Surface)?;
        self.ctx.window_surface = Some(WindowSurface {
            _context: context,
            surface,
        });
        Ok(())
    }

    /// Resize the window surface.
    pub fn resize_surface(&mut self, width: u32, height: u32) {
        if let Some(surf) = &mut self.ctx.window_surface
            && let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height))
        {
            let _ = surf.surface.resize(w, h);
        }
    }

    /// Present the pixel buffer to the window surface.
    ///
    /// # Errors
    ///
    /// Returns `Err(SurfaceError::Surface(...))` if the softbuffer buffer
    /// can't be acquired or presented (e.g., context lost on WASM or DRI/KMS
    /// page flip pending).
    pub fn present(&mut self) -> Result<(), SurfaceError> {
        let Some(surface) = self.ctx.window_surface.as_mut() else {
            return Ok(()); // headless mode, nothing to present
        };
        let mut buffer = surface
            .surface
            .buffer_mut()
            .map_err(SurfaceError::Surface)?;
        let pixels = self.ctx.pixel_buf.as_ref();
        if pixels.len() == buffer.len() {
            buffer.copy_from_slice(pixels);
        } else {
            buffer.fill(0);
        }
        buffer.present().map_err(SurfaceError::Surface)
    }
}

// ── Run (windowed) ────────────────────────────────────────────────────────────

impl SoftwareBackend {
    /// Opens a window and runs the game loop.
    ///
    /// Consumes this config; runs `app_loop` on every frame tick, driven by
    /// the winit event loop.  On native this blocks the calling thread; on
    /// WASM it returns immediately (the event loop continues in the browser).
    ///
    /// The closure receives `&mut Terminal<SoftwareRenderer>` and is called
    /// on every tick.  Return from the closure to continue; the loop only
    /// stops when the window is closed.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use retroglyph::backend::software::SoftwareBackendBuilder;
    /// use retroglyph::event::{Event, KeyCode};
    /// use std::time::Duration;
    ///
    /// SoftwareBackendBuilder::new()
    ///     .title("Demo")
    ///     .grid_size(80, 25)
    ///     .scale(2)
    ///     .build()
    ///     .unwrap()
    ///     .run_windowed(|term| {
    ///         term.clear();
    ///         term.print(0, 0, "Hello");
    ///         term.present();
    ///
    ///         if let Some(event) = term.poll(Duration::from_millis(16)) {
    ///             if let Event::Key(k) = event {
    ///                 if k.code == KeyCode::Escape { std::process::exit(0); }
    ///             }
    ///         }
    ///     })
    ///     .expect("event loop failed");
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `font` is `None`.
    ///
    /// # Errors
    ///
    /// Returns [`SoftwareBackendError::EventLoop`] if the event loop fails to
    /// start.
    /// Creates the renderer via [`run_headless`](Self::run_headless) and
    /// wraps it in a windowed event loop.
    ///
    /// See [`run_headless`](Self::run_headless) for renderer creation; this
    /// method adds winit window + event loop setup on top.
    pub fn run_windowed<F>(self, app_loop: F) -> Result<(), SoftwareBackendError>
    where
        F: FnMut(&mut crate::Terminal<SoftwareRenderer>) + 'static,
    {
        // Compute window size before consuming `self` in run_headless().
        let glyph = self
            .font
            .as_ref()
            .expect("SoftwareBackendBuilder::build() returns Err(NoFont) if no font");
        let cell_w = u32::from(glyph.glyph_width) * u32::from(self.scale);
        let cell_h = u32::from(glyph.glyph_height) * u32::from(self.scale);
        let win_w = u32::from(self.cols) * cell_w;
        let win_h = u32::from(self.rows) * cell_h;
        let title = self.window_title.clone();

        let renderer = self.run_headless();
        let terminal = crate::Terminal::new(renderer);
        let event_loop = EventLoop::new().map_err(SoftwareBackendError::EventLoop)?;

        let build_app = |terminal, app_loop| WindowApp {
            terminal: Some(terminal),
            app_loop,
            window: None,
            title: title.clone(),
            init_size: InitWindowSize {
                width: win_w,
                height: win_h,
            },
            current_modifiers: KeyModifiers::NONE,
        };

        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut app = build_app(terminal, app_loop);
            event_loop
                .run_app(&mut app)
                .map_err(SoftwareBackendError::EventLoop)
        }

        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::EventLoopExtWebSys;
            let app = build_app(terminal, app_loop);
            event_loop.spawn_app(app);
            Ok(())
        }
    }

    /// Creates a headless renderer that renders into an internal buffer
    /// without opening a window.
    ///
    /// Unlike [`run_windowed`](Self::run_windowed), this does not block — it returns a
    /// [`SoftwareRenderer`] immediately.  The renderer's pixel buffer can be
    /// inspected via [`SoftwareRenderer::pixels`].  Flushing is a no-op
    /// (the buffer stays in memory).
    ///
    /// This is primarily useful for testing pixel-level output without
    /// needing a window or event loop.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use retroglyph::backend::software::SoftwareBackendBuilder;
    /// use retroglyph::tile::Tile;
    /// use retroglyph::style::Style;
    /// use retroglyph::grid::Pos;
    /// use retroglyph::Color;
    ///
    /// let mut renderer = SoftwareBackendBuilder::new()
    ///     .grid_size(1, 1)
    ///     .scale(1)
    ///     .build()
    ///     .unwrap()
    ///     .run_headless();
    ///
    /// // Render a red cell on layer 0.
    /// let tile = Tile {
    ///     glyph: ' ',
    ///     style: Style::new().bg(Color::Rgb { r: 255, g: 0, b: 0 }),
    ///     ..Tile::default()
    /// };
    /// renderer.draw_layers([(0, Pos::new(0, 0), &tile)].into_iter());
    ///
    /// assert!(renderer.pixels().iter().all(|&p| p == 0x00FF_0000));
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `font` is `None`.
    #[must_use]
    pub fn run_headless(self) -> SoftwareRenderer {
        let font = self.font.expect(
            "run_headless() requires a font; supply one via SoftwareBackendBuilder::font()",
        );

        let cell_w = u32::from(font.glyph_width) * u32::from(self.scale);
        let cell_h = u32::from(font.glyph_height) * u32::from(self.scale);
        // u32 always fits in usize (all targets: 32- and 64-bit).
        let buf_w = usize::from(self.cols) * usize::try_from(cell_w).unwrap();
        let buf_h = usize::from(self.rows) * usize::try_from(cell_h).unwrap();

        #[cfg(feature = "software-tilesets")]
        let sprite_cache = if self.tilesets.is_empty() {
            Arc::new(SpriteCache::new())
        } else {
            let mut cache = SpriteCache::new();
            for opts in &self.tilesets {
                cache
                    .load(opts)
                    .expect("tileset loading failed; check tileset file path and format");
            }
            Arc::new(cache)
        };

        SoftwareRenderer::create(
            self,
            font,
            buf_w,
            buf_h,
            cell_w,
            cell_h,
            #[cfg(feature = "software-tilesets")]
            sprite_cache,
        )
    }
}

// ── Backend impl ────────────────────────────────────────────────────────────────

impl Backend for SoftwareRenderer {
    type Error = core::convert::Infallible;

    fn push_event(&mut self, event: Event) {
        Self::push_event(self, event);
    }

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (Pos, &'a Tile)>,
    {
        let font = &self.font;
        let scale = usize::from(self.options.scale);
        let cols = self.options.cols;
        let glyph_w = usize::from(font.glyph_width) * scale;
        let glyph_h = usize::from(font.glyph_height) * scale;
        let buf_w = usize::from(cols) * glyph_w;

        #[cfg(feature = "software-tilesets")]
        let sprite_cache = Some(&*self.sprite_cache);

        for (pos, cell) in content {
            blit_cell(
                self.ctx.pixel_buf.as_mut(),
                buf_w,
                pos,
                cell,
                font,
                glyph_w,
                glyph_h,
                scale,
                #[cfg(feature = "software-tilesets")]
                sprite_cache,
            );
        }
        Ok(())
    }

    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u8, Pos, &'a Tile)>,
    {
        // Clear the entire buffer before redrawing.  Pixel-based backends
        // get the full frame (see `needs_full_frame`), so this wipes any
        // orphaned pixels from sub-cell offset spill in the previous frame.
        self.ctx.pixel_buf.clear();

        let font = &self.font;
        let scale = usize::from(self.options.scale);
        let cols = self.options.cols;
        let cell_w = usize::from(font.glyph_width) * scale;
        let cell_h = usize::from(font.glyph_height) * scale;
        let buf_w = usize::from(cols) * cell_w;
        #[cfg(feature = "software-tilesets")]
        let buf_h = self.ctx.pixel_buf.as_ref().len() / buf_w;

        #[cfg(feature = "software-tilesets")]
        let sprite_cache = Some(&*self.sprite_cache);

        for (layer_id, pos, tile) in content {
            let px_x = usize::from(pos.x) * cell_w;
            let px_y = usize::from(pos.y) * cell_h;

            if layer_id == 0 {
                let bg = resolve_color(tile.style.bg, DEFAULT_BG);
                let rect = ixy::Rect::new(px_x, px_y, cell_w, cell_h);
                self.ctx.pixel_buf.fill_rect_solid(rect, bg);
            }

            // Sprite cache dispatch: sprite wins over bitmap font.
            #[cfg(feature = "software-tilesets")]
            if let Some(sprite) = sprite_cache.and_then(|c| c.get(tile.glyph)) {
                blit_sprite(
                    self.ctx.pixel_buf.as_mut(),
                    buf_w,
                    buf_h,
                    px_x,
                    px_y,
                    tile,
                    sprite,
                    scale,
                );
                continue;
            }

            blit_glyph(
                self.ctx.pixel_buf.as_mut(),
                buf_w,
                px_x,
                px_y,
                tile,
                font,
                cell_w,
                cell_h,
                scale,
            );
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        // No-op. Frame is presented via WindowedBackend::present() in windowed
        // mode, or accessed directly via pixels() in headless/testing mode.
        Ok(())
    }

    fn size(&self) -> Size {
        Size {
            width: self.options.cols,
            height: self.options.rows,
        }
    }

    fn resize(&mut self, size: Size) {
        self.options.cols = size.width;
        self.options.rows = size.height;
        let font = &self.font;
        let cell_w = usize::from(font.glyph_width) * usize::from(self.options.scale);
        let cell_h = usize::from(font.glyph_height) * usize::from(self.options.scale);
        let new_w = usize::from(size.width) * cell_w;
        let new_h = usize::from(size.height) * cell_h;
        self.ctx.pixel_buf.resize(new_w, new_h);
        #[allow(clippy::cast_possible_truncation)]
        {
            self.ctx.cell_w = cell_w as u32;
            self.ctx.cell_h = cell_h as u32;
        }
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.ctx.pixel_buf.clear();
        Ok(())
    }

    fn needs_full_frame(&self) -> bool {
        true
    }

    fn poll_event(&mut self, _timeout: Duration) -> Option<Event> {
        // Non-blocking: the game loop is driven by `about_to_wait` →
        // `request_redraw`, so there is no background thread to sleep on.
        // All backends return immediately regardless of platform.
        self.ctx.event_buffer.pop_front()
    }

    fn set_cursor_visible(&mut self, _visible: bool) {
        // No hardware cursor in software mode.
    }

    fn set_cursor_position(&mut self, _position: Pos) {
        // No hardware cursor in software mode.
    }
}

// ── WindowedBackend impl ─────────────────────────────────────────────────────────

impl WindowedBackend for SoftwareRenderer {
    fn present(&mut self) -> Result<(), SurfaceError> {
        Self::present(self)
    }

    fn init_surface(&mut self, window: &Arc<Window>) -> Result<(), SurfaceError> {
        Self::init_surface(self, window)
    }

    fn resize_surface(&mut self, width: u32, height: u32) {
        Self::resize_surface(self, width, height);
    }

    fn cell_size(&self) -> (u32, u32) {
        (self.ctx.cell_w, self.ctx.cell_h)
    }

    fn push_window_event(&mut self, event: Event) {
        Self::push_event(self, event);
    }
}

// ── Grid compositing ──────────────────────────────────────────────────────────

/// Render one grid cell into `buffer` using 1-bit bitmap glyph data.
///
/// Each set bit in the font row maps to `fg`; each clear bit maps to `bg`.
/// No alpha blending is needed — bitmap fonts are 1-bit.
/// When `scale > 1` each source pixel becomes a `scale×scale` block.
///
/// If `sprite_cache` contains a sprite for the cell's glyph, the bitmap font
/// path is skipped in favor of [`blit_sprite`].
#[allow(clippy::cast_possible_truncation, clippy::too_many_arguments)]
fn blit_cell(
    buffer: &mut [u32],
    buf_w: usize,
    pos: Pos,
    cell: &Tile,
    font: &Font,
    cell_w: usize,
    cell_h: usize,
    scale: usize,
    #[cfg(feature = "software-tilesets")] sprite_cache: Option<&SpriteCache>,
) {
    let px_x = pos.x as usize * cell_w;
    let px_y = pos.y as usize * cell_h;

    #[cfg(feature = "software-tilesets")]
    if let Some(sprite) = sprite_cache.and_then(|c| c.get(cell.glyph())) {
        let buf_h = buffer.len() / buf_w;
        blit_sprite(buffer, buf_w, buf_h, px_x, px_y, cell, sprite, scale);
        return;
    }

    let mut fg = resolve_color(cell.style().fg, DEFAULT_FG);
    let mut bg = resolve_color(cell.style().bg, DEFAULT_BG);

    if cell.style().modifiers().contains(CellModifier::REVERSE) {
        core::mem::swap(&mut fg, &mut bg);
    }

    let glyph_index = font.char_to_index(cell.glyph());
    let rows = font.rows(glyph_index);
    let src_w = usize::from(font.glyph_width);

    for (src_y, &mask) in rows.iter().enumerate() {
        for src_x in 0..src_w {
            let bit = (mask >> (src_w - 1 - src_x)) & 1;
            let pixel = if bit != 0 { fg } else { bg };

            for dy in 0..scale {
                let y = px_y + src_y * scale + dy;
                if y >= px_y + cell_h {
                    break;
                }
                for dx in 0..scale {
                    let x = px_x + src_x * scale + dx;
                    if x >= px_x + cell_w {
                        break;
                    }
                    let idx = y * buf_w + x;
                    if idx < buffer.len() {
                        buffer[idx] = pixel;
                    }
                }
            }
        }
    }

    // Fill remaining strip below the glyph rows with background.
    let glyph_used_h = rows.len() * scale;
    for y_off in glyph_used_h..cell_h {
        let y = px_y + y_off;
        for x in 0..cell_w {
            let idx = y * buf_w + px_x + x;
            if idx < buffer.len() {
                buffer[idx] = bg;
            }
        }
    }
}

/// Blit a glyph's set bits into `buffer` at `(px_x, px_y)` plus sub-cell
/// offset from `tile.dx`/`tile.dy`.  Only the foreground (glyph) pixels are
/// painted — background is left untouched.
#[allow(clippy::cast_possible_truncation, clippy::too_many_arguments)]
fn blit_glyph(
    buffer: &mut [u32],
    buf_w: usize,
    px_x: usize,
    px_y: usize,
    tile: &Tile,
    font: &Font,
    _cell_w: usize,
    _cell_h: usize,
    scale: usize,
) {
    if tile.glyph == ' ' {
        return;
    }

    #[allow(clippy::cast_possible_wrap)]
    let fg = if tile.style.modifiers().contains(CellModifier::REVERSE) {
        resolve_color(tile.style.bg, DEFAULT_BG)
    } else {
        resolve_color(tile.style.fg, DEFAULT_FG)
    };

    #[allow(clippy::cast_possible_wrap)]
    let origin_x = px_x as i64 + i64::from(tile.dx) * scale as i64;
    #[allow(clippy::cast_possible_wrap)]
    let origin_y = px_y as i64 + i64::from(tile.dy) * scale as i64;

    let glyph_index = font.char_to_index(tile.glyph);
    let rows = font.rows(glyph_index);
    let src_w = usize::from(font.glyph_width);
    let buf_h = buffer.len() / buf_w;

    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::similar_names
    )]
    for (src_y, &mask) in rows.iter().enumerate() {
        for src_x in 0..src_w {
            if (mask >> (src_w - 1 - src_x)) & 1 == 0 {
                continue;
            }
            for sdy in 0..scale {
                let y = origin_y + (src_y * scale + sdy) as i64;
                if y < 0 || y as usize >= buf_h {
                    continue;
                }
                let y = y as usize;
                for sdx in 0..scale {
                    let x = origin_x + (src_x * scale + sdx) as i64;
                    if x < 0 || x as usize >= buf_w {
                        continue;
                    }
                    let x = x as usize;
                    let idx = y * buf_w + x;
                    if idx < buffer.len() {
                        buffer[idx] = fg;
                    }
                }
            }
        }
    }
}

/// Blit a decoded RGBA8 sprite into `buffer` with alpha blending.
///
/// The sprite's top-left corner is at pixel `(cell_px_x + tile.dx * scale,
/// cell_px_y + tile.dy * scale)`. If `spacing_cells > 1`, the sprite's pixels
/// extend beyond the anchor cell into adjacent cells.
///
/// Pixels outside `buffer` bounds are silently clipped.
///
/// Blending uses pure integer `U8x4Rgba::source_over`. Fully opaque pixels
/// (alpha == 255) skip blending entirely and write directly to the buffer.
#[cfg(feature = "software-tilesets")]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::too_many_arguments
)]
fn blit_sprite(
    buffer: &mut [u32],
    buf_w: usize,
    buf_h: usize,
    cell_px_x: usize,
    cell_px_y: usize,
    tile: &Tile,
    sprite: &Sprite,
    scale: usize,
) {
    let origin_x = cell_px_x as i64 + i64::from(tile.dx) * scale as i64;
    let origin_y = cell_px_y as i64 + i64::from(tile.dy) * scale as i64;

    let src_w = sprite.pixel_width as usize;
    let src_h = sprite.pixel_height as usize;

    for src_y in 0..src_h {
        for src_x in 0..src_w {
            let src_idx = (src_y * src_w + src_x) * 4;
            let src = U8x4Rgba::new(
                sprite.pixels[src_idx],
                sprite.pixels[src_idx + 1],
                sprite.pixels[src_idx + 2],
                sprite.pixels[src_idx + 3],
            );

            if src.is_transparent() {
                continue;
            }

            // Fast path: fully opaque pixels write directly, no blending.
            // Most roguelike sprites are opaque, so this skips U8x4Rgba
            // construction + source_over for the common case.
            let rgb = u32::from(src.r) << 16 | u32::from(src.g) << 8 | u32::from(src.b);
            if src.alpha() == 255 {
                for dy in 0..scale {
                    #[allow(clippy::cast_sign_loss)]
                    let dst_y = origin_y + (src_y * scale + dy) as i64;
                    if dst_y < 0 || dst_y as usize >= buf_h {
                        continue;
                    }
                    let dst_y = dst_y as usize;
                    for dx in 0..scale {
                        #[allow(clippy::cast_sign_loss)]
                        let dst_x = origin_x + (src_x * scale + dx) as i64;
                        if dst_x < 0 || dst_x as usize >= buf_w {
                            continue;
                        }
                        let dst_x = dst_x as usize;
                        let dst_idx = dst_y * buf_w + dst_x;
                        if dst_idx < buffer.len() {
                            buffer[dst_idx] = rgb;
                        }
                    }
                }
                continue;
            }

            // Each source pixel maps to `scale x scale` destination pixels.
            for dy in 0..scale {
                #[allow(clippy::cast_sign_loss)]
                let dst_y = origin_y + (src_y * scale + dy) as i64;
                if dst_y < 0 || dst_y as usize >= buf_h {
                    continue;
                }
                let dst_y = dst_y as usize;

                for dx in 0..scale {
                    #[allow(clippy::cast_sign_loss)]
                    let dst_x = origin_x + (src_x * scale + dx) as i64;
                    if dst_x < 0 || dst_x as usize >= buf_w {
                        continue;
                    }
                    let dst_x = dst_x as usize;

                    let dst_idx = dst_y * buf_w + dst_x;
                    if dst_idx >= buffer.len() {
                        continue;
                    }

                    let dst = U8x4Rgba::from_rgb_u32(buffer[dst_idx]);
                    let blended = src.source_over(dst);
                    buffer[dst_idx] = blended.to_rgb_u32();
                }
            }
        }
    }
}

/// Default foreground when [`Color::Default`] is used.
const DEFAULT_FG: u32 = 0x00d4_d4d4;
/// Default background when [`Color::Default`] is used.
const DEFAULT_BG: u32 = 0x0000_0000;

/// Resolve a [`Color`] to a packed `0x00RRGGBB` value, substituting
/// `default_rgb` for [`Color::Default`].
fn resolve_color(color: Color, default_rgb: u32) -> u32 {
    match color {
        Color::Default => default_rgb,
        Color::Rgb { r, g, b } => (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
        Color::Ansi(a) => ansi_to_rgb(a),
        Color::Indexed(idx) => indexed_to_rgb(idx),
    }
}

/// Standard xterm 16-color palette, 0x00RRGGBB.
#[allow(clippy::match_same_arms)]
const fn ansi_to_rgb(color: AnsiColor) -> u32 {
    match color {
        AnsiColor::Black => 0x0000_0000,
        AnsiColor::Red => 0x0080_0000,
        AnsiColor::Green => 0x0000_8000,
        AnsiColor::Yellow => 0x0080_8000,
        AnsiColor::Blue => 0x0000_0080,
        AnsiColor::Magenta => 0x0080_0080,
        AnsiColor::Cyan => 0x0000_8080,
        AnsiColor::White => 0x00c0_c0c0,
        AnsiColor::BrightBlack => 0x0080_8080,
        AnsiColor::BrightRed => 0x00ff_0000,
        AnsiColor::BrightGreen => 0x0000_ff00,
        AnsiColor::BrightYellow => 0x00ff_ff00,
        AnsiColor::BrightBlue => 0x0000_00ff,
        AnsiColor::BrightMagenta => 0x00ff_00ff,
        AnsiColor::BrightCyan => 0x0000_ffff,
        AnsiColor::BrightWhite => 0x00ff_ffff,
    }
}

/// Maps xterm 256-color index to 0x00RRGGBB.
fn indexed_to_rgb(idx: u8) -> u32 {
    if let Ok(ansi) = AnsiColor::try_from(idx) {
        return ansi_to_rgb(ansi);
    }
    if idx < 232 {
        let i = idx - 16;
        let b = i % 6;
        let g = (i / 6) % 6;
        let r = i / 36;
        let scale = |v: u8| if v == 0 { 0u32 } else { u32::from(v) * 40 + 55 };
        return (scale(r) << 16) | (scale(g) << 8) | scale(b);
    }
    let grey = u32::from(idx - 232) * 10 + 8;
    (grey << 16) | (grey << 8) | grey
}

// ── Input translation ─────────────────────────────────────────────────────────

/// Translates a winit key event into an [`Event`].
///
/// Returns `None` for key releases or unhandled keys.
#[allow(clippy::needless_pass_by_value)]
fn translate_key(input: winit::event::KeyEvent, modifiers: KeyModifiers) -> Option<Event> {
    use winit::keyboard::{Key, NamedKey};

    if !input.state.is_pressed() {
        return None;
    }

    let code = match input.logical_key {
        Key::Named(NamedKey::Enter) => KeyCode::Enter,
        Key::Named(NamedKey::Escape) => KeyCode::Escape,
        Key::Named(NamedKey::Backspace) => KeyCode::Backspace,
        Key::Named(NamedKey::Delete) => KeyCode::Delete,
        Key::Named(NamedKey::Insert) => KeyCode::Insert,
        Key::Named(NamedKey::Tab) => KeyCode::Tab,
        Key::Named(NamedKey::ArrowUp) => KeyCode::Up,
        Key::Named(NamedKey::ArrowDown) => KeyCode::Down,
        Key::Named(NamedKey::ArrowLeft) => KeyCode::Left,
        Key::Named(NamedKey::ArrowRight) => KeyCode::Right,
        Key::Named(NamedKey::Home) => KeyCode::Home,
        Key::Named(NamedKey::End) => KeyCode::End,
        Key::Named(NamedKey::PageUp) => KeyCode::PageUp,
        Key::Named(NamedKey::PageDown) => KeyCode::PageDown,
        Key::Named(NamedKey::F1) => KeyCode::F(1),
        Key::Named(NamedKey::F2) => KeyCode::F(2),
        Key::Named(NamedKey::F3) => KeyCode::F(3),
        Key::Named(NamedKey::F4) => KeyCode::F(4),
        Key::Named(NamedKey::F5) => KeyCode::F(5),
        Key::Named(NamedKey::F6) => KeyCode::F(6),
        Key::Named(NamedKey::F7) => KeyCode::F(7),
        Key::Named(NamedKey::F8) => KeyCode::F(8),
        Key::Named(NamedKey::F9) => KeyCode::F(9),
        Key::Named(NamedKey::F10) => KeyCode::F(10),
        Key::Named(NamedKey::F11) => KeyCode::F(11),
        Key::Named(NamedKey::F12) => KeyCode::F(12),
        Key::Character(ref s) => {
            let ch = s.chars().next()?;
            KeyCode::Char(ch)
        }
        _ => return None,
    };

    Some(Event::Key(KeyEvent { code, modifiers }))
}

/// Translates winit modifier state into our [`KeyModifiers`].
fn translate_modifiers(state: winit::keyboard::ModifiersState) -> KeyModifiers {
    let mut m = KeyModifiers::NONE;
    if state.shift_key() {
        m |= KeyModifiers::SHIFT;
    }
    if state.control_key() {
        m |= KeyModifiers::CONTROL;
    }
    if state.alt_key() {
        m |= KeyModifiers::ALT;
    }
    m
}

// Translate a winit `RawKeyEvent` (from `DeviceEvent::Key`) into our `Event`.
//
// This is the primary input path on WASM, where keyboard events fire as
// `DeviceEvent::Key` from a document-level listener (no canvas focus needed)
// rather than `WindowEvent::KeyboardInput` (which requires canvas focus).
// On native this is a secondary path behind `WindowEvent::KeyboardInput`.
// ── winit ApplicationHandler (main thread) ────────────────────────────────────

/// Initial window dimensions used before the first Resized event.
struct InitWindowSize {
    width: u32,
    height: u32,
}

struct WindowApp<B: WindowedBackend, F> {
    terminal: Option<crate::Terminal<B>>,
    app_loop: F,
    window: Option<Arc<Window>>,
    title: String,
    init_size: InitWindowSize,
    /// Current modifier key state, updated by `ModifiersChanged` events.
    current_modifiers: KeyModifiers,
}

impl<B: WindowedBackend, F> WindowApp<B, F> {
    /// Create the window and initialize the surface.
    ///
    /// Returns `Some(window)` on success, logs and returns `None` on failure.
    fn create_window_and_surface(&mut self, event_loop: &ActiveEventLoop) -> Option<Arc<Window>> {
        let attrs = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(winit::dpi::PhysicalSize::new(
                self.init_size.width,
                self.init_size.height,
            ));

        #[cfg(target_family = "wasm")]
        let attrs = {
            use winit::platform::web::WindowAttributesExtWebSys;
            attrs.with_append(true)
        };

        let window = Arc::new(match event_loop.create_window(attrs) {
            Ok(w) => w,
            Err(e) => {
                log::error!("window creation failed: {e}");
                event_loop.exit();
                return None;
            }
        });

        if let Some(term) = self.terminal.as_mut() {
            if let Err(e) = term.backend_mut().init_surface(&window) {
                log::error!("surface init failed: {e}");
                event_loop.exit();
                return None;
            }
            // Set the initial surface size (required on WASM before first present).
            term.backend_mut()
                .resize_surface(self.init_size.width, self.init_size.height);
        }

        Some(window)
    }
}

impl<B: WindowedBackend, F: FnMut(&mut crate::Terminal<B>) + 'static> ApplicationHandler
    for WindowApp<B, F>
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(window) = self.create_window_and_surface(event_loop) {
            self.window = Some(window);
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                // Push the event so the game loop can process it (save game,
                // confirm dialog, etc.).  Do not call event_loop.exit() here;
                // the game decides when to terminate.
                if let Some(term) = self.terminal.as_mut() {
                    term.backend_mut().push_event(Event::Close);
                }
            }

            WindowEvent::Resized(physical_size) => {
                if let Some(term) = self.terminal.as_mut() {
                    let (cell_w, cell_h) = term.backend().cell_size();
                    let cols = physical_size.width / cell_w;
                    let rows = physical_size.height / cell_h;
                    let new_w = cols * cell_w;
                    let new_h = rows * cell_h;
                    term.backend_mut().resize_surface(new_w, new_h);
                    #[allow(clippy::cast_possible_truncation)]
                    term.backend_mut()
                        .push_event(Event::Resize(cols.max(1) as u16, rows.max(1) as u16));
                }
            }

            // TODO: handle CursorMoved and MouseInput for
            // mouse-to-grid-coordinate conversion (Event::Mouse).
            WindowEvent::ModifiersChanged(mods) => {
                self.current_modifiers = translate_modifiers(mods.state());
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(term) = self.terminal.as_mut()
                    && let Some(e) = translate_key(event, self.current_modifiers)
                {
                    term.backend_mut().push_event(e);
                }
            }

            WindowEvent::RedrawRequested => {
                let Some(term) = self.terminal.as_mut() else {
                    return;
                };
                (self.app_loop)(term);
                if let Err(e) = term.backend_mut().present() {
                    log::error!("frame present failed: {e}");
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::grid::Pos;
    use crate::style::Style;

    fn test_renderer() -> SoftwareRenderer {
        let opts = SoftwareBackend {
            window_title: String::new(),
            font: Some(bitmap_font::vga8x16::FONT),
            cols: 1,
            rows: 1,
            scale: 1,
            #[cfg(feature = "software-tilesets")]
            tilesets: Vec::new(),
        };
        opts.run_headless()
    }

    #[test]
    fn layer0_paints_background() {
        let mut renderer = test_renderer();
        let tile = Tile {
            glyph: ' ',
            style: Style::new().bg(Color::Rgb { r: 255, g: 0, b: 0 }),
            ..Tile::default()
        };
        let diff: Vec<(u8, Pos, &Tile)> = vec![(0, Pos::new(0, 0), &tile)];
        renderer.draw_layers(diff.into_iter());

        let buf = renderer.pixels();
        assert_eq!(buf.len(), 8 * 16);
        assert!(
            buf.iter().all(|&p| p == 0x00FF_0000),
            "all pixels should be red"
        );
    }

    #[test]
    fn layer1_does_not_paint_background() {
        let mut renderer = test_renderer();

        let bg_tile = Tile {
            glyph: ' ',
            style: Style::new().bg(Color::Rgb { r: 255, g: 0, b: 0 }),
            ..Tile::default()
        };
        let space_tile = Tile {
            glyph: ' ',
            style: Style::new().fg(Color::Rgb { r: 0, g: 255, b: 0 }),
            ..Tile::default()
        };
        // draw_layers clears buffer first, so pass all layers in one call.
        renderer.draw_layers(
            [
                (0, Pos::new(0, 0), &bg_tile),
                (1, Pos::new(0, 0), &space_tile),
            ]
            .into_iter(),
        );

        let buf = renderer.pixels();
        assert_eq!(buf.len(), 8 * 16);
        // All pixels should be red (layer 0 bg).  Green fg from layer 1's
        // space tile is ignored because space has no set bits.
        assert!(
            buf.iter().all(|&p| p == 0x00FF_0000),
            "all pixels should be red, not green"
        );
    }

    #[test]
    fn layer1_glyph_overwrites_layer0() {
        let mut renderer = test_renderer();

        let bg = Tile {
            glyph: ' ',
            style: Style::new().bg(Color::Rgb {
                r: 10,
                g: 10,
                b: 10,
            }),
            ..Tile::default()
        };
        let fg = Tile {
            glyph: '@',
            style: Style::new().fg(Color::Rgb { r: 0, g: 255, b: 0 }),
            ..Tile::default()
        };
        // draw_layers clears buffer first, so pass all layers in one call.
        renderer.draw_layers([(0, Pos::new(0, 0), &bg), (1, Pos::new(0, 0), &fg)].into_iter());

        let buf = renderer.pixels();
        assert!(buf.contains(&0x0000_FF00), "some pixels should be green");
        assert!(buf.iter().all(|&p| p == 0x0000_FF00 || p == 0x000A_0A0A));
    }

    #[test]
    fn sub_cell_offset_shifts_glyph() {
        let mut renderer = test_renderer();

        let bg = Tile {
            glyph: ' ',
            style: Style::new().bg(Color::Rgb {
                r: 10,
                g: 10,
                b: 10,
            }),
            ..Tile::default()
        };
        let fg = Tile {
            glyph: '@',
            style: Style::new().fg(Color::Rgb { r: 0, g: 255, b: 0 }),
            dx: 1,
            dy: 0,
            ..Tile::default()
        };
        // draw_layers clears buffer first, so pass all layers in one call.
        renderer.draw_layers([(0, Pos::new(0, 0), &bg), (1, Pos::new(0, 0), &fg)].into_iter());

        let buf = renderer.pixels();
        let has_green = |col: usize| {
            buf.iter()
                .enumerate()
                .any(|(i, &p)| i % 8 == col && p == 0x0000_FF00)
        };
        assert!(!has_green(0), "x=0 should have no green pixels with dx=1");
        assert!(has_green(1), "x=1 should have green pixels with dx=1");
    }

    #[test]
    fn pixel_snapshot_render_scene() {
        // Render a small multi-layer scene and snapshot the pixel output.
        let opts = SoftwareBackendBuilder::new()
            .grid_size(2, 2)
            .scale(1)
            .build()
            .unwrap();
        let mut renderer = opts.run_headless();

        // Layer 0: dark background, ':' at (0,0) in dim blue, '.' at (1,0) in dim gray.
        let bg = Tile {
            glyph: ':',
            style: Style::new()
                .fg(Color::Rgb {
                    r: 60,
                    g: 60,
                    b: 80,
                })
                .bg(Color::Rgb {
                    r: 20,
                    g: 20,
                    b: 30,
                }),
            ..Tile::default()
        };
        let dot = Tile {
            glyph: '.',
            style: Style::new()
                .fg(Color::Rgb {
                    r: 40,
                    g: 40,
                    b: 50,
                })
                .bg(Color::Rgb {
                    r: 20,
                    g: 20,
                    b: 30,
                }),
            ..Tile::default()
        };
        let entity = Tile {
            glyph: '@',
            style: Style::new()
                .fg(Color::Rgb { r: 0, g: 255, b: 0 })
                .bg(Color::Rgb {
                    r: 10,
                    g: 10,
                    b: 10,
                }),
            dx: 1,
            dy: 0,
            ..Tile::default()
        };
        // Single draw_layers call (clears buffer first).
        renderer.draw_layers(
            [
                (0, Pos::new(0, 0), &bg),
                (0, Pos::new(1, 0), &dot),
                (1, Pos::new(0, 0), &entity),
            ]
            .into_iter(),
        );

        // Snapshot the pixel buffer.
        // The buffer is 2 cells wide (16px) x 2 cells tall (32px) = 512 u32s.
        let pixels = renderer.pixels();
        assert_eq!(pixels.len(), 2 * 8 * 2 * 16); // cols * glyph_w * rows * glyph_h

        // Snapshot a debug representation: groups of 16 pixels per row (one pixel row across 2 cells).
        let row_strs: Vec<String> = pixels
            .chunks(16)
            .take(32)
            .map(|row| {
                row.iter()
                    .map(|p| format!("{p:08x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        let snapshot = row_strs.join("\n");

        insta::assert_snapshot!("pixel_snapshot_render_scene", snapshot);
    }
}
