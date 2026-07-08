//! CPU rasterization backend: renders grid cells into a pixel buffer and
//! blits it to a window surface via `softbuffer`.
//!
//! # Architecture
//!
//! [`SoftwareBackend`] holds configuration only (font, grid size, scale); it
//! does not implement [`Backend`]. Call
//! [`run_headless`](SoftwareBackend::run_headless) to build a
//! [`SoftwareRenderer`], which does the actual rendering work:
//!
//! ```text
//! SoftwareBackend (config: font, grid size, scale)
//!   |  .run_headless()
//!   v
//! SoftwareRenderer
//!   implements retroglyph_core::Backend
//!   implements retroglyph_window::Presenter
//!   |                                |
//!   |                                v
//!   |                     wrapped in retroglyph_window::WindowBackend,
//!   |                     driven by a windowing loop (retroglyph-window's
//!   |                     winit integration, or any other source of
//!   |                     raw window handles)
//!   v                                |
//! Terminal<SoftwareRenderer>          v
//! (headless / pixel tests,   softbuffer::Surface -> OS window
//!  inspect via .pixels())
//! ```
//!
//! This crate does not depend on winit. [`SoftwareRenderer`] implements
//! [`Presenter`](retroglyph_window::Presenter) against raw window handles
//! ([`WindowHandle`]), so anything that produces those (winit via
//! `retroglyph-window`, or another windowing library) can drive it.
//! `retroglyph-window`'s [`WindowBackend`](retroglyph_window::WindowBackend)
//! wraps a `Presenter` to provide the full [`Backend`] for windowed use,
//! owning the input event queue that this crate does not.
//!
//! For headless use (in-memory rendering, pixel-level tests) skip windowing
//! entirely: [`SoftwareRenderer`] implements [`Backend`] directly, so
//! `Terminal<SoftwareRenderer>` works without a window, and
//! [`pixels`](SoftwareRenderer::pixels) gives direct access to the rendered
//! buffer.

pub mod bitmap_font;
pub mod config;

#[cfg(feature = "tilesets")]
pub mod sprite_cache;
#[cfg(feature = "tilesets")]
pub mod tileset;

// Platform-specific window surface. Both modules expose a `WindowSurface` with
// the same `new`/`resize`/`present` API and their own `SurfaceError`, so the
// renderer below drives either without `cfg` in its body. This is the same
// module-swap pattern std uses for `std::sys`.
#[cfg(not(target_arch = "wasm32"))]
#[path = "surface_native.rs"]
mod surface;
#[cfg(target_arch = "wasm32")]
#[path = "surface_wasm.rs"]
mod surface;

pub use surface::SurfaceError;
use surface::WindowSurface;

use retroglyph_core::backend::Backend;
use retroglyph_core::color::{AnsiColor, Color};

pub use bitmap_font::BitmapFont;
pub use config::{SoftwareBackend, SoftwareBackendBuilder, SoftwareBackendError};

#[cfg(feature = "tilesets")]
use alpha_blend::rgba::U8x4Rgba;
use bitmap_font::BitmapFont as Font;
use grixy::buf::GridBuf;
use grixy::ops::GridWrite;
use grixy::ops::layout::RowMajor;
use retroglyph_core::event::Event;
use retroglyph_core::grid::{Pos, Size};
use retroglyph_core::style::CellModifier;
use retroglyph_core::tile::Tile;
use retroglyph_window::WindowHandle;
#[cfg(feature = "tilesets")]
use sprite_cache::{Sprite, SpriteCache};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

// ── Public types ──────────────────────────────────────────────────────────────

/// A running software renderer, produced by [`SoftwareBackend::run_headless`].
///
/// Unlike [`SoftwareBackend`] (which is just configuration), this type
/// always has an active rendering context: its pixel buffer is always
/// available, and the `ctx` field is never `None`, so [`Backend`] methods
/// never panic for missing initialisation.
///
/// Call [`pixels`](Self::pixels) to inspect the rendered output, or use
/// [`Backend::draw`] and [`Backend::draw_layers`] to render into it.
pub struct SoftwareRenderer {
    options: SoftwareBackend,
    /// The bitmap font, extracted from `options.font` at construction time.
    /// Always present; the `Option` wrapper in `SoftwareBackend` is only for
    /// the builder validation step.
    font: BitmapFont,
    ctx: RenderContext,
    #[cfg(feature = "tilesets")]
    sprite_cache: Arc<SpriteCache>,
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
        #[cfg(feature = "tilesets")] sprite_cache: Arc<SpriteCache>,
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
            #[cfg(feature = "tilesets")]
            sprite_cache,
        }
    }

    /// Returns a slice of the rendered pixel buffer (`0x00RRGGBB` format).
    ///
    /// The buffer length is `cols * (glyph_width * scale) * rows * (glyph_height * scale)`.
    /// Each `u32` is a packed RGB pixel with the top byte unused.
    ///
    /// This is always available: there is no `Option` wrapper because
    /// `SoftwareRenderer` is guaranteed to have an active rendering context.
    #[must_use]
    pub fn pixels(&self) -> &[u32] {
        self.ctx.pixel_buf.as_ref()
    }

    /// Pushes an event into the internal buffer, to be drained by
    /// [`Backend::poll_event`].
    pub fn push_event(&mut self, event: Event) {
        self.ctx.event_buffer.push_back(event);
    }

    /// Initializes the window surface from a raw window/display handle.
    ///
    /// The concrete surface is platform-specific (softbuffer on native, a
    /// `Canvas2D` context on wasm32); see the `surface` module.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError`] if the platform surface cannot be created.
    pub fn init_surface(&mut self, window: Arc<dyn WindowHandle>) -> Result<(), SurfaceError> {
        self.ctx.window_surface = Some(WindowSurface::new(window)?);
        Ok(())
    }

    /// Resizes the window surface to `width` x `height` pixels. No-op if the
    /// surface has not been initialized via [`init_surface`](Self::init_surface).
    pub fn resize_surface(&mut self, width: u32, height: u32) {
        if let Some(surf) = &mut self.ctx.window_surface {
            surf.resize(width, height);
        }
    }

    /// Presents the pixel buffer to the window surface. No-op in headless
    /// mode (no surface initialized).
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError`] if the platform surface fails to present.
    pub fn present(&mut self) -> Result<(), SurfaceError> {
        match self.ctx.window_surface.as_mut() {
            Some(surface) => surface.present(self.ctx.pixel_buf.as_ref()),
            None => Ok(()), // headless mode, nothing to present
        }
    }
}

// ── Renderer construction ────────────────────────────────────────────────────────────

impl SoftwareBackend {
    /// Creates a headless renderer that renders into an internal buffer
    /// without opening a window.
    ///
    /// This does not block: it returns a [`SoftwareRenderer`] immediately.
    /// The renderer's pixel buffer can be inspected via
    /// [`SoftwareRenderer::pixels`], or the renderer can be handed to
    /// `retroglyph_window::winit::run_windowed` to drive a window.  Flushing
    /// is a no-op (the buffer stays in memory).
    ///
    /// This is primarily useful for testing pixel-level output without
    /// needing a window or event loop.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use retroglyph_software::SoftwareBackendBuilder;
    /// use retroglyph_core::tile::Tile;
    /// use retroglyph_core::style::Style;
    /// use retroglyph_core::grid::Pos;
    /// use retroglyph_core::Color;
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

        #[cfg(feature = "tilesets")]
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
            #[cfg(feature = "tilesets")]
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

        #[cfg(feature = "tilesets")]
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
                #[cfg(feature = "tilesets")]
                sprite_cache,
            );
        }
        Ok(())
    }

    /// Composite the raw layer stream into the pixel buffer.
    ///
    /// Layers arrive layer-major (0 first), so painting them in order gives the
    /// correct z-order. Layer 0 always fills its cell background; a higher layer
    /// fills the background only when its tile is non-empty and its background is
    /// not [`Color::Default`], mirroring the layer-flattening rule so cell and
    /// pixel backends agree. The `is_empty` guard matters because this
    /// receives the full frame (see [`needs_full_frame`](Backend::needs_full_frame)),
    /// including empty higher-layer cells that must not overwrite layer 0.
    ///
    /// Known divergence from cell backends: an occupied space with a
    /// [`Color::Default`] background on a higher layer erases the glyph beneath
    /// it when flattened, but here it leaves the lower glyph's pixels intact.
    /// Repainting the lower background per pixel would require composited
    /// per-cell state this renderer intentionally avoids.
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
        #[cfg(feature = "tilesets")]
        let buf_h = self.ctx.pixel_buf.as_ref().len() / buf_w;

        #[cfg(feature = "tilesets")]
        let sprite_cache = Some(&*self.sprite_cache);

        for (layer_id, pos, tile) in content {
            let px_x = usize::from(pos.x) * cell_w;
            let px_y = usize::from(pos.y) * cell_h;

            // Layer 0 always paints its background. Higher layers paint theirs
            // only when the tile actually contributes an opaque background,
            // matching `Grid::flatten_into`.
            let paints_bg =
                layer_id == 0 || (!tile.is_empty() && tile.style().background() != Color::Default);
            if paints_bg {
                let bg = resolve_color(tile.style().background(), DEFAULT_BG);
                let rect = ixy::Rect::new(px_x, px_y, cell_w, cell_h);
                self.ctx.pixel_buf.fill_rect_solid(rect, bg);
            }

            // Sprite cache dispatch: sprite wins over bitmap font.
            #[cfg(feature = "tilesets")]
            if let Some(sprite) = sprite_cache.and_then(|c| c.get(tile.glyph())) {
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

    fn composites_layers(&self) -> bool {
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

// ── Presenter impl ───────────────────────────────────────────────────────────

// Implemented via full path (no `use retroglyph_window::Presenter`) so the
// trait is never in scope in this file: `SoftwareRenderer` also implements
// `Backend` (for the headless path), and both traits share method names
// (`draw_layers`, `flush`, ...) -- keeping `Presenter` out of scope avoids
// method-resolution ambiguity at every call site below.
impl retroglyph_window::Presenter for SoftwareRenderer {
    type Error = core::convert::Infallible;
    type SurfaceError = SurfaceError;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (Pos, &'a Tile)>,
    {
        <Self as Backend>::draw(self, content)
    }

    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u8, Pos, &'a Tile)>,
    {
        <Self as Backend>::draw_layers(self, content)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        <Self as Backend>::flush(self)
    }

    fn size(&self) -> Size {
        <Self as Backend>::size(self)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        <Self as Backend>::clear(self)
    }

    fn resize(&mut self, size: Size) {
        <Self as Backend>::resize(self, size);
    }

    fn init_surface(&mut self, window: Arc<dyn WindowHandle>) -> Result<(), SurfaceError> {
        Self::init_surface(self, window)
    }

    fn resize_surface(&mut self, width: u32, height: u32) {
        Self::resize_surface(self, width, height);
    }

    fn present(&mut self) -> Result<(), SurfaceError> {
        Self::present(self)
    }

    fn cell_size(&self) -> (u32, u32) {
        (self.ctx.cell_w, self.ctx.cell_h)
    }
}

// ── Grid compositing ──────────────────────────────────────────────────────────

/// Renders one grid cell into `buffer` using 1-bit bitmap glyph data.
///
/// Each set bit in the font row maps to `fg`; each clear bit maps to `bg`.
/// No alpha blending is needed: bitmap fonts are 1-bit.
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
    #[cfg(feature = "tilesets")] sprite_cache: Option<&SpriteCache>,
) {
    let px_x = pos.x as usize * cell_w;
    let px_y = pos.y as usize * cell_h;

    #[cfg(feature = "tilesets")]
    if let Some(sprite) = sprite_cache.and_then(|c| c.get(cell.glyph())) {
        let buf_h = buffer.len() / buf_w;
        blit_sprite(buffer, buf_w, buf_h, px_x, px_y, cell, sprite, scale);
        return;
    }

    let mut fg = resolve_color(cell.style().foreground(), DEFAULT_FG);
    let mut bg = resolve_color(cell.style().background(), DEFAULT_BG);

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

/// Blits a glyph's set bits into `buffer` at `(px_x, px_y)` plus sub-cell
/// offset from `tile.dx`/`tile.dy`. Only the foreground (glyph) pixels are
/// painted; background is left untouched.
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
    if tile.glyph() == ' ' {
        return;
    }

    #[allow(clippy::cast_possible_wrap)]
    let fg = if tile.style().modifiers().contains(CellModifier::REVERSE) {
        resolve_color(tile.style().background(), DEFAULT_BG)
    } else {
        resolve_color(tile.style().foreground(), DEFAULT_FG)
    };

    #[allow(clippy::cast_possible_wrap)]
    let origin_x = px_x as i64 + i64::from(tile.dx()) * scale as i64;
    #[allow(clippy::cast_possible_wrap)]
    let origin_y = px_y as i64 + i64::from(tile.dy()) * scale as i64;

    let glyph_index = font.char_to_index(tile.glyph());
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
#[cfg(feature = "tilesets")]
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
    let origin_x = cell_px_x as i64 + i64::from(tile.dx()) * scale as i64;
    let origin_y = cell_px_y as i64 + i64::from(tile.dy()) * scale as i64;

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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_core::color::Color;
    use retroglyph_core::grid::Pos;
    use retroglyph_core::style::Style;

    fn test_renderer() -> SoftwareRenderer {
        let opts = SoftwareBackend {
            window_title: String::new(),
            font: Some(bitmap_font::vga8x16::FONT),
            cols: 1,
            rows: 1,
            scale: 1,
            #[cfg(feature = "tilesets")]
            tilesets: Vec::new(),
            target_fps: None,
        };
        opts.run_headless()
    }

    #[test]
    fn layer0_paints_background() {
        let mut renderer = test_renderer();
        let tile = Tile::new(' ', Style::new().bg(Color::Rgb { r: 255, g: 0, b: 0 }));
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

        let bg_tile = Tile::new(' ', Style::new().bg(Color::Rgb { r: 255, g: 0, b: 0 }));
        let space_tile = Tile::new(' ', Style::new().fg(Color::Rgb { r: 0, g: 255, b: 0 }));
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

        let bg = Tile::new(
            ' ',
            Style::new().bg(Color::Rgb {
                r: 10,
                g: 10,
                b: 10,
            }),
        );
        let fg = Tile::new('@', Style::new().fg(Color::Rgb { r: 0, g: 255, b: 0 }));
        // draw_layers clears buffer first, so pass all layers in one call.
        renderer.draw_layers([(0, Pos::new(0, 0), &bg), (1, Pos::new(0, 0), &fg)].into_iter());

        let buf = renderer.pixels();
        assert!(buf.contains(&0x0000_FF00), "some pixels should be green");
        assert!(buf.iter().all(|&p| p == 0x0000_FF00 || p == 0x000A_0A0A));
    }

    #[test]
    fn sub_cell_offset_shifts_glyph() {
        let mut renderer = test_renderer();

        let bg = Tile::new(
            ' ',
            Style::new().bg(Color::Rgb {
                r: 10,
                g: 10,
                b: 10,
            }),
        );
        let fg =
            Tile::new('@', Style::new().fg(Color::Rgb { r: 0, g: 255, b: 0 })).with_offset(1, 0);
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
        let bg = Tile::new(
            ':',
            Style::new()
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
        );
        let dot = Tile::new(
            '.',
            Style::new()
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
        );
        let entity = Tile::new(
            '@',
            Style::new()
                .fg(Color::Rgb { r: 0, g: 255, b: 0 })
                .bg(Color::Rgb {
                    r: 10,
                    g: 10,
                    b: 10,
                }),
        )
        .with_offset(1, 0);
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

    #[test]
    fn higher_layer_opaque_background_paints_but_empty_cell_does_not() {
        // 2x1 grid. Layer 0 is a plain dark background across both cells.
        // Layer 1 puts a colored background only at cell (0, 0); cell (1, 0)
        // on layer 1 is left empty and must not disturb layer 0.
        let opts = SoftwareBackendBuilder::new()
            .grid_size(2, 1)
            .scale(1)
            .build()
            .unwrap();
        let mut renderer = opts.run_headless();

        let base = Tile::new(
            ' ',
            Style::new().bg(Color::Rgb {
                r: 20,
                g: 20,
                b: 20,
            }),
        );
        // Layer 1 overlay: an opaque space (non-empty) with a red background.
        let overlay = Tile::new(' ', Style::new().bg(Color::Rgb { r: 200, g: 0, b: 0 }));
        // Layer 1 empty cell (default tile) must be skipped.
        let empty = Tile::default();

        renderer.draw_layers(
            [
                (0, Pos::new(0, 0), &base),
                (0, Pos::new(1, 0), &base),
                (1, Pos::new(0, 0), &overlay),
                (1, Pos::new(1, 0), &empty),
            ]
            .into_iter(),
        );

        let pixels = renderer.pixels();
        let cell_w = 8usize; // glyph width at scale 1
        // Top-left pixel of cell (0, 0): the layer-1 red overlay wins.
        assert_eq!(pixels[0] & 0x00ff_ffff, 0x00c8_0000);
        // Top-left pixel of cell (1, 0): layer-1 cell was empty, so layer 0 shows.
        assert_eq!(pixels[cell_w] & 0x00ff_ffff, 0x0014_1414);
    }
}
