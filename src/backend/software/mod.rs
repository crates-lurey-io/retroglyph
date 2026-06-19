//! Software rendering backend: winit window + softbuffer pixel blitting.
//!
//! # Threading model
//!
//! `winit` requires the main thread on macOS.  [`SoftwareBackend::run`] takes
//! over the main thread to drive the `winit` event loop and spawns the
//! caller's game loop on a background thread.
//!
//! Two `std::sync::mpsc` channels bridge the threads:
//! - **event channel** (main → game): translated [`Event`]s.
//! - **frame channel** (game → main): rendered `Vec<u32>` pixel buffers.

pub mod bitmap_font;
pub mod config;

pub use bitmap_font::BitmapFont;
pub use config::{SoftwareBackendBuilder, SoftwareBackendError, SoftwareBackendOptions};

use crate::backend::Backend;
use crate::cell::Cell;
use crate::color::{AnsiColor, Color};
use crate::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crate::grid::{Pos, Size};
use bitmap_font::BitmapFont as Font;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

// ── Public backend type ────────────────────────────────────────────────────────

/// Software rendering backend.
///
/// Create via [`SoftwareBackendBuilder`], then call [`run`](Self::run) to open
/// a window.  The closure you pass to `run` receives a `Terminal<SoftwareBackend>`
/// on a background thread.
pub struct SoftwareBackend {
    options: SoftwareBackendOptions,
    /// Populated only after the game thread is spawned inside `run()`.
    inner: Option<GameInner>,
}

struct GameInner {
    event_rx: mpsc::Receiver<Event>,
    frame_tx: mpsc::SyncSender<Vec<u32>>,
    /// Pixel buffer (0x00RRGGBB), updated by `draw()` and sent on `flush()`.
    pixel_buf: Vec<u32>,
}

impl SoftwareBackend {
    /// Validates `options` and returns a configuration-mode backend.
    ///
    /// # Errors
    ///
    /// Returns [`SoftwareBackendError::NoFont`] when no font is set.
    pub(crate) fn new(options: SoftwareBackendOptions) -> Result<Self, SoftwareBackendError> {
        if options.font.is_none() {
            return Err(SoftwareBackendError::NoFont);
        }
        Ok(Self {
            options,
            inner: None,
        })
    }

    /// Consumes the backend configuration and takes over the main thread.
    ///
    /// Spawns `app_loop` on a background thread with a `Terminal<SoftwareBackend>`.
    /// Blocks the calling (main) thread inside the `winit` event loop until the
    /// window is closed.
    ///
    /// # Panics
    ///
    /// Panics if the font was not set (this is checked during construction via
    /// [`SoftwareBackendBuilder::build`]).
    ///
    /// # Errors
    ///
    /// Returns [`SoftwareBackendError::EventLoop`] if the event loop fails to
    /// start.
    pub fn run<F>(self, app_loop: F) -> Result<(), SoftwareBackendError>
    where
        F: FnMut(&mut crate::Terminal<Self>) + Send + 'static,
    {
        let opts = self.options;
        // Validated by new().
        let font = opts.font.expect("font was None despite new() validation");

        let cell_w = u32::from(font.glyph_width) * u32::from(opts.scale);
        let cell_h = u32::from(font.glyph_height) * u32::from(opts.scale);
        let win_w = u32::from(opts.cols) * cell_w;
        let win_h = u32::from(opts.rows) * cell_h;

        let (event_tx, event_rx) = mpsc::channel::<Event>();
        // Capacity 1: if the window thread hasn't consumed the last frame yet
        // we skip the new one rather than accumulating unbounded memory.
        let (frame_tx, frame_rx) = mpsc::sync_channel::<Vec<u32>>(1);

        let game_backend = Self {
            options: opts.clone(),
            inner: Some(GameInner {
                event_rx,
                frame_tx,
                pixel_buf: vec![0u32; win_w as usize * win_h as usize],
            }),
        };

        let mut app_loop = app_loop;
        std::thread::spawn(move || {
            let mut terminal = crate::Terminal::new(game_backend);
            loop {
                app_loop(&mut terminal);
            }
        });

        let event_loop = EventLoop::new().map_err(SoftwareBackendError::EventLoop)?;
        let mut window_app = WindowApp {
            title: opts.window_title,
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
}

// ── Backend impl (game thread) ─────────────────────────────────────────────────

impl Backend for SoftwareBackend {
    fn draw<'a, I>(&mut self, content: I)
    where
        I: Iterator<Item = (Pos, &'a Cell)>,
    {
        let font = self
            .options
            .font
            .as_ref()
            .expect("draw called outside game thread");
        let scale = usize::from(self.options.scale);
        let cols = self.options.cols;
        let glyph_w = usize::from(font.glyph_width) * scale;
        let glyph_h = usize::from(font.glyph_height) * scale;
        let buf_w = usize::from(cols) * glyph_w;

        let inner = self
            .inner
            .as_mut()
            .expect("draw called outside game thread");

        for (pos, cell) in content {
            blit_cell(
                &mut inner.pixel_buf,
                buf_w,
                pos,
                cell,
                font,
                glyph_w,
                glyph_h,
                scale,
            );
        }
    }

    fn flush(&mut self) {
        let inner = self
            .inner
            .as_ref()
            .expect("flush called outside game thread");
        // Drop the frame silently if the window thread hasn't consumed the
        // previous one yet.
        let _ = inner.frame_tx.try_send(inner.pixel_buf.clone());
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
        if let Some(inner) = &mut self.inner {
            let font = self
                .options
                .font
                .as_ref()
                .expect("font missing despite new() validation");
            let cell_w = usize::from(font.glyph_width) * usize::from(self.options.scale);
            let cell_h = usize::from(font.glyph_height) * usize::from(self.options.scale);
            let new_len =
                usize::from(size.width) * cell_w * usize::from(size.height) * cell_h;
            inner.pixel_buf.resize(new_len, 0);
            inner.pixel_buf.fill(0);
        }
    }

    fn clear(&mut self) {
        if let Some(inner) = &mut self.inner {
            inner.pixel_buf.fill(0);
        }
    }

    fn poll_event(&mut self, timeout: Duration) -> Option<Event> {
        let inner = self
            .inner
            .as_ref()
            .expect("poll_event called outside game thread");
        if timeout == Duration::ZERO {
            inner.event_rx.try_recv().ok()
        } else {
            inner.event_rx.recv_timeout(timeout).ok()
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
#[allow(clippy::cast_possible_truncation, clippy::too_many_arguments)]
fn blit_cell(
    buffer: &mut [u32],
    buf_w: usize,
    pos: Pos,
    cell: &Cell,
    font: &Font,
    cell_w: usize,
    cell_h: usize,
    scale: usize,
) {
    let px_x = pos.x as usize * cell_w;
    let px_y = pos.y as usize * cell_h;

    let mut fg = resolve_fg_color(cell.style().fg);
    let mut bg = resolve_bg_color(cell.style().bg);

    if cell
        .style()
        .modifiers()
        .contains(crate::style::CellModifier::REVERSE)
    {
        core::mem::swap(&mut fg, &mut bg);
    }

    let glyph_index = font.char_to_index(cell.glyph());
    let rows = font.rows(glyph_index);
    let src_w = usize::from(font.glyph_width);

    for (src_y, &mask) in rows.iter().enumerate() {
        for src_x in 0..src_w {
            let bit = (mask >> (src_w - 1 - src_x)) & 1;
            let pixel = if bit != 0 { fg } else { bg };

            // Render each source pixel as a scale×scale block.
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

    // Fill remaining horizontal strip below the glyph rows with background.
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
    // Indices 16–231: 6×6×6 colour cube.
    if idx < 232 {
        let i = idx - 16;
        let b = i % 6;
        let g = (i / 6) % 6;
        let r = i / 36;
        let scale = |v: u8| if v == 0 { 0u32 } else { u32::from(v) * 40 + 55 };
        return (scale(r) << 16) | (scale(g) << 8) | scale(b);
    }
    // Indices 232–255: greyscale ramp.
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

/// Main-thread state for the winit event loop.
struct WindowApp {
    title: String,
    event_tx: mpsc::Sender<Event>,
    frame_rx: mpsc::Receiver<Vec<u32>>,
    /// Last pixel buffer received from the game thread. Used as fallback when
    /// `RedrawRequested` fires before a new frame is ready (e.g. right after
    /// resize), preventing a black flash.
    last_frame: Vec<u32>,
    window: Option<Arc<Window>>,
    /// Kept alive alongside `surface`; both borrow the same `Arc<Window>`.
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
                    if let (Some(w), Some(h)) = (
                        NonZeroU32::new(self.win_w),
                        NonZeroU32::new(self.win_h),
                    ) {
                        let _ = surface.resize(w, h);
                    }
                }
                #[allow(clippy::cast_possible_truncation)]
                // Clear stale frame data so RedrawRequested doesn't blit
                // a partially-copied old frame while the game thread is
                // producing its first frame at the new size.
                self.last_frame.clear();
                #[allow(clippy::cast_possible_truncation)]
                let _ = self.event_tx.send(Event::Resize(
                    cols.max(1) as u16,
                    rows.max(1) as u16,
                ));
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(e) = translate_key(event) {
                    let _ = self.event_tx.send(e);
                }
            }

            WindowEvent::RedrawRequested => {
                // Drain stale frames; keep only the latest one.
                while let Ok(buf) = self.frame_rx.try_recv() {
                    self.last_frame = buf;
                }
                if let Some(surface) = self.surface.as_mut() {
                    if let Ok(mut buffer) = surface.buffer_mut() {
                        let src = &self.last_frame;
                        if src.len() == buffer.len() {
                            buffer.copy_from_slice(src);
                        } else {
                            // Size mismatch: the game thread hasn't produced a
                            // frame for the current surface dimensions yet.
                            // Show black rather than a stride-misaligned blit.
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
