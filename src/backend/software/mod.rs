//! Software rendering backend: winit window + softbuffer pixel blitting.
//!
//! # Architecture
//!
//! [`SoftwareBackend`] is a pure-config type (font, grid size, scale). It does
//! **not** implement [`Backend`] directly.  Call [`run`](SoftwareBackend::run)
//! to open a window and spawn the game loop on a background thread, or
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

pub use bitmap_font::BitmapFont;
pub use config::{SoftwareBackend, SoftwareBackendBuilder, SoftwareBackendError};

use crate::backend::Backend;
use crate::color::{AnsiColor, Color};
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
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

// ── Public types ──────────────────────────────────────────────────────────────

/// The running half of the software backend.
///
/// A running software renderer, produced by [`SoftwareBackend::run`] or
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
    ctx: RenderContext,
    #[cfg(feature = "software-tilesets")]
    sprite_cache: Arc<SpriteCache>,
}

struct RenderContext {
    event_rx: mpsc::Receiver<Event>,
    frame_tx: mpsc::SyncSender<Vec<u32>>,
    pixel_buf: GridBuf<u32, Vec<u32>, RowMajor>,
    alive: Arc<AtomicBool>,
}

impl SoftwareRenderer {
    /// Creates a new renderer with the given channels and buffer dimensions.
    pub(crate) fn create(
        options: SoftwareBackend,
        event_rx: mpsc::Receiver<Event>,
        frame_tx: mpsc::SyncSender<Vec<u32>>,
        buf_w: usize,
        buf_h: usize,
        #[cfg(feature = "software-tilesets")] sprite_cache: Arc<SpriteCache>,
    ) -> Self {
        Self {
            options,
            ctx: RenderContext {
                event_rx,
                frame_tx,
                pixel_buf: GridBuf::from_buffer(vec![0u32; buf_w * buf_h], buf_w),
                alive: Arc::new(AtomicBool::new(true)),
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
}

// ── Run (windowed) ────────────────────────────────────────────────────────────

impl SoftwareBackend {
    /// Opens a window and runs the game loop.
    ///
    /// Consumes this config; spawns `app_loop` on a background thread with a
    /// [`Terminal`](crate::Terminal) wrapping a [`SoftwareRenderer`].  Blocks
    /// the calling (main) thread inside the `winit` event loop until the
    /// window is closed.
    ///
    /// The closure receives `&mut Terminal<SoftwareRenderer>` and is called
    /// on every tick.  Return from the closure to continue; the loop only
    /// stops when the window is closed.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use rg::backend::software::SoftwareBackendBuilder;
    /// use rg::event::{Event, KeyCode};
    /// use std::time::Duration;
    ///
    /// SoftwareBackendBuilder::new()
    ///     .title("Demo")
    ///     .grid_size(80, 25)
    ///     .scale(2)
    ///     .build()
    ///     .unwrap()
    ///     .run(|term| {
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
    pub fn run<F>(self, app_loop: F) -> Result<(), SoftwareBackendError>
    where
        F: FnMut(&mut crate::Terminal<SoftwareRenderer>) + Send + 'static,
    {
        let font = self
            .font
            .as_ref()
            .expect("run() requires a font; supply one via SoftwareBackendBuilder::font()");

        let cell_w = u32::from(font.glyph_width) * u32::from(self.scale);
        let cell_h = u32::from(font.glyph_height) * u32::from(self.scale);
        let win_w = u32::from(self.cols) * cell_w;
        let win_h = u32::from(self.rows) * cell_h;

        #[cfg(feature = "software-tilesets")]
        let sprite_cache = if self.tilesets.is_empty() {
            Arc::new(SpriteCache::new())
        } else {
            let mut cache = SpriteCache::new();
            for opts in &self.tilesets {
                cache.load(opts).map_err(SoftwareBackendError::Tileset)?;
            }
            Arc::new(cache)
        };

        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let (frame_tx, frame_rx) = mpsc::sync_channel::<Vec<u32>>(1);

        let renderer = SoftwareRenderer::create(
            self.clone(),
            event_rx,
            frame_tx,
            win_w as usize,
            win_h as usize,
            #[cfg(feature = "software-tilesets")]
            sprite_cache,
        );

        let mut app_loop = app_loop;
        std::thread::spawn(move || {
            let mut terminal = crate::Terminal::new(renderer);
            loop {
                app_loop(&mut terminal);
                if !terminal.backend().is_connected() {
                    break;
                }
            }
        });

        let event_loop = EventLoop::new().map_err(SoftwareBackendError::EventLoop)?;
        let mut window_app = WindowApp {
            title: self.window_title,
            event_tx,
            frame_rx,
            last_frame: Vec::new(),
            window: None,
            context: None,
            surface: None,
            win_w,
            win_h,
            cell_w,
            cell_h,
        };

        event_loop
            .run_app(&mut window_app)
            .map_err(SoftwareBackendError::EventLoop)
    }

    /// Creates a headless renderer that renders into an internal buffer
    /// without opening a window.
    ///
    /// Unlike [`run`](Self::run), this does not block — it returns a
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
    /// use rg::backend::software::SoftwareBackendBuilder;
    /// use rg::tile::Tile;
    /// use rg::style::Style;
    /// use rg::grid::Pos;
    /// use rg::Color;
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
        let font = self.font.as_ref().expect(
            "run_headless() requires a font; supply one via SoftwareBackendBuilder::font()",
        );

        let cell_w = usize::from(font.glyph_width) * usize::from(self.scale);
        let cell_h = usize::from(font.glyph_height) * usize::from(self.scale);
        let buf_w = usize::from(self.cols) * cell_w;
        let buf_h = usize::from(self.rows) * cell_h;

        #[cfg(feature = "software-tilesets")]
        let sprite_cache = if self.tilesets.is_empty() {
            Arc::new(SpriteCache::new())
        } else {
            let mut cache = SpriteCache::new();
            for opts in &self.tilesets {
                cache
                    .load(opts)
                    .unwrap_or_else(|e| panic!("tileset loading failed in run_headless: {e}"));
            }
            Arc::new(cache)
        };

        let (_event_tx, event_rx) = mpsc::channel();
        let (frame_tx, _frame_rx) = mpsc::sync_channel(1);

        SoftwareRenderer::create(
            self,
            event_rx,
            frame_tx,
            buf_w,
            buf_h,
            #[cfg(feature = "software-tilesets")]
            sprite_cache,
        )
    }
}

// ── Backend impl (game / headless thread) ─────────────────────────────────────

impl Backend for SoftwareRenderer {
    fn draw<'a, I>(&mut self, content: I)
    where
        I: Iterator<Item = (Pos, &'a Tile)>,
    {
        let font = self.options.font.as_ref().unwrap();
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
    }

    fn draw_layers<'a, I>(&mut self, content: I)
    where
        I: Iterator<Item = (u8, Pos, &'a Tile)>,
    {
        // Clear the entire buffer before redrawing.  Pixel-based backends
        // get the full frame (see `needs_full_frame`), so this wipes any
        // orphaned pixels from sub-cell offset spill in the previous frame.
        self.ctx.pixel_buf.clear();

        let font = self.options.font.as_ref().unwrap();
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
                let bg = resolve_bg_color(tile.style.bg);
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
    }

    fn flush(&mut self) {
        let buf: Vec<u32> = self.ctx.pixel_buf.as_ref().to_vec();
        match self.ctx.frame_tx.try_send(buf) {
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.ctx.alive.store(false, Ordering::Release);
            }
            Err(mpsc::TrySendError::Full(_)) | Ok(()) => {
                // TODO: Track frame drops via a shared metrics system
                // once designed (see ADR 012: Backend Metrics).
            }
        }
    }

    fn is_connected(&self) -> bool {
        self.ctx.alive.load(Ordering::Acquire)
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
        let font = self.options.font.as_ref().unwrap();
        let cell_w = usize::from(font.glyph_width) * usize::from(self.options.scale);
        let cell_h = usize::from(font.glyph_height) * usize::from(self.options.scale);
        let new_w = usize::from(size.width) * cell_w;
        let new_h = usize::from(size.height) * cell_h;
        self.ctx.pixel_buf.resize(new_w, new_h);
    }

    fn clear(&mut self) {
        self.ctx.pixel_buf.clear();
    }

    fn needs_full_frame(&self) -> bool {
        true
    }

    fn poll_event(&mut self, timeout: Duration) -> Option<Event> {
        if timeout == Duration::ZERO {
            self.ctx.event_rx.try_recv().ok()
        } else {
            self.ctx.event_rx.recv_timeout(timeout).ok()
        }
    }

    fn set_cursor_visible(&mut self, _visible: bool) {
        // No hardware cursor in software mode.
    }

    fn set_cursor_position(&mut self, _position: Pos) {
        // No hardware cursor in software mode.
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

    let mut fg = resolve_fg_color(cell.style().fg);
    let mut bg = resolve_bg_color(cell.style().bg);

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
        resolve_bg_color(tile.style.bg)
    } else {
        resolve_fg_color(tile.style.fg)
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

fn resolve_fg_color(color: Color) -> u32 {
    match color {
        Color::Default => 0x00d4_d4d4,
        Color::Rgb { r, g, b } => (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
        Color::Ansi(a) => ansi_to_rgb(a),
        Color::Indexed(idx) => indexed_to_rgb(idx),
    }
}

fn resolve_bg_color(color: Color) -> u32 {
    match color {
        Color::Default => 0x0000_0000,
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
fn translate_key(input: winit::event::KeyEvent) -> Option<Event> {
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

    Some(Event::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
    }))
}

// ── winit ApplicationHandler (main thread) ────────────────────────────────────

struct WindowApp {
    title: String,
    event_tx: mpsc::Sender<Event>,
    frame_rx: mpsc::Receiver<Vec<u32>>,
    last_frame: Vec<u32>,
    window: Option<Arc<Window>>,
    #[allow(dead_code)]
    context: Option<softbuffer::Context<Arc<Window>>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    win_w: u32,
    win_h: u32,
    cell_w: u32,
    cell_h: u32,
}

impl ApplicationHandler for WindowApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(winit::dpi::PhysicalSize::new(self.win_w, self.win_h));

        let window = Arc::new(match event_loop.create_window(attrs) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("rg software backend: window creation failed: {e}");
                event_loop.exit();
                return;
            }
        });

        let context = match softbuffer::Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("rg software backend: softbuffer context failed: {e}");
                event_loop.exit();
                return;
            }
        };

        let mut surface = match softbuffer::Surface::new(&context, window.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rg software backend: softbuffer surface failed: {e}");
                event_loop.exit();
                return;
            }
        };

        if let (Some(w), Some(h)) = (NonZeroU32::new(self.win_w), NonZeroU32::new(self.win_h)) {
            let _ = surface.resize(w, h);
        }

        self.context = Some(context);
        self.surface = Some(surface);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                let _ = self.event_tx.send(Event::Close);
                event_loop.exit();
            }

            WindowEvent::Resized(physical_size) => {
                let cols = physical_size.width / self.cell_w;
                let rows = physical_size.height / self.cell_h;
                self.win_w = cols * self.cell_w;
                self.win_h = rows * self.cell_h;
                if let Some(surface) = &mut self.surface {
                    if let (Some(w), Some(h)) =
                        (NonZeroU32::new(self.win_w), NonZeroU32::new(self.win_h))
                    {
                        let _ = surface.resize(w, h);
                    }
                }
                self.last_frame.clear();
                #[allow(clippy::cast_possible_truncation)]
                let _ = self
                    .event_tx
                    .send(Event::Resize(cols.max(1) as u16, rows.max(1) as u16));
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(e) = translate_key(event) {
                    let _ = self.event_tx.send(e);
                }
            }

            WindowEvent::RedrawRequested => {
                while let Ok(buf) = self.frame_rx.try_recv() {
                    self.last_frame = buf;
                }
                if let Some(surface) = self.surface.as_mut() {
                    if let Ok(mut buffer) = surface.buffer_mut() {
                        let src = &self.last_frame;
                        if src.len() == buffer.len() {
                            buffer.copy_from_slice(src);
                        } else {
                            buffer.fill(0);
                        }
                        let _ = buffer.present();
                    }
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

    /// Write a `u32` pixel buffer (0x00RRGGBB) to a PNG file at `path`.
    fn write_png(path: &str, pixels: &[u32], width: u32, height: u32) {
        let img = image::RgbaImage::from_fn(width, height, |x, y| {
            let idx = (y * width + x) as usize;
            let p = pixels[idx];
            let r = ((p >> 16) & 0xFF) as u8;
            let g = ((p >> 8) & 0xFF) as u8;
            let b = (p & 0xFF) as u8;
            image::Rgba([r, g, b, 255])
        });
        img.save(path).expect("failed to write PNG");
    }

    #[test]
    fn sub_cell_offset_does_not_smear() {
        // Verify that the full-frame clear prevents orphaned pixels from
        // sub-cell offset spill across adjacent cells.
        //
        // We go through the full `Terminal::present()` pipeline so the
        // backend's `needs_full_frame()` flag triggers the all-cells path.
        let opts = SoftwareBackendBuilder::new()
            .grid_size(3, 1)
            .scale(1)
            .build()
            .unwrap();
        let mut term = crate::Terminal::new(opts.run_headless());

        // ── Frame 1: layer 0 bg (red) + layer 1 @ at dx=+2 ──
        term.layer(0);
        term.bg(Color::Rgb { r: 128, g: 0, b: 0 });
        for x in 0..3 {
            term.put(x, 0, ' ');
        }

        term.layer(1);
        term.fg(Color::Rgb { r: 0, g: 255, b: 0 });
        term.put_offset(1, 0, 2, 0, '@');

        term.present();

        // ── Frame 2: clear layer 1, put @ at dx=-2 ──
        // Layer 0 stays the same.
        term.layer(1);
        term.clear();
        term.fg(Color::Rgb { r: 0, g: 255, b: 0 });
        term.put_offset(1, 0, -2, 0, '@');

        term.present();

        let buf = term.backend().pixels();
        let cols = 3usize;
        let cell_w = 8usize;
        let buf_w = cols * cell_w; // 24
        assert_eq!(buf.len(), buf_w * 16);

        // Cell (2,0) should have NO green orphaned pixels.
        let cell2_x_start = 2 * cell_w; // 16
        let cell2_x_end = 3 * cell_w; // 24

        let mut orphaned_green: Vec<(usize, usize, u32)> = Vec::new();
        for y in 0..16 {
            for x in cell2_x_start..cell2_x_end {
                let idx = y * buf_w + x;
                if buf[idx] == 0x0000_FF00 {
                    orphaned_green.push((x, y, buf[idx]));
                }
            }
        }

        // Write a debug PNG regardless so we can inspect visually.
        let png_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/snapshots/sub_cell_offset_does_not_smear.png"
        );
        #[allow(clippy::cast_possible_truncation)]
        write_png(png_path, buf, buf_w as u32, 16);

        assert!(
            orphaned_green.is_empty(),
            "Cell (2,0) has {} orphaned green pixels from old @ spill — see {}",
            orphaned_green.len(),
            png_path
        );

        // Also check that cell (1,0) has green pixels (the @ at dx=-2).
        let cell1_x_start = cell_w;
        let cell1_x_end = 2 * cell_w;
        let has_green = (0..16)
            .any(|y| (cell1_x_start..cell1_x_end).any(|x| buf[y * buf_w + x] == 0x0000_FF00));
        assert!(
            has_green,
            "Cell (1,0) should have green pixels from the @ glyph"
        );
    }
}
