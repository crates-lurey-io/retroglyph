//! Configuration, builder, and error types for the software rendering backend.
//!
//! The main type is [`SoftwareBackend`], which holds grid and font
//! configuration.  Construct it via [`SoftwareBackendBuilder`], then call
//! [`run_headless`](SoftwareBackend::run_headless) to produce a
//! [`SoftwareRenderer`](crate::SoftwareRenderer). Hand that renderer to
//! `retroglyph_window::winit::run_windowed` to open a window, or use it
//! directly for in-memory rendering.

use super::bitmap_font::BitmapFont;
#[cfg(feature = "software-tilesets")]
use super::tileset::TilesetOptions;
use std::fmt;

/// Errors that can occur when configuring the software backend.
///
/// Windowing errors (window creation, event loop) are not represented here:
/// this crate builds renderers, and the loop -- `retroglyph-window` or
/// another windowing integration -- reports its own errors.
#[derive(Debug)]
pub enum SoftwareBackendError {
    /// No font was provided and the `software-default-font` feature is not enabled.
    NoFont,
    /// Tileset loading failed.
    #[cfg(feature = "software-tilesets")]
    Tileset(super::tileset::TilesetError),
}

impl fmt::Display for SoftwareBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFont => write!(
                f,
                "no bitmap font provided; supply one via \
                 SoftwareBackendBuilder::font() or enable the \
                 `software-default-font` feature"
            ),
            #[cfg(feature = "software-tilesets")]
            Self::Tileset(e) => write!(f, "tileset error: {e}"),
        }
    }
}

impl std::error::Error for SoftwareBackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoFont => None,
            #[cfg(feature = "software-tilesets")]
            Self::Tileset(e) => Some(e),
        }
    }
}

/// Configuration and entry point for the software rendering backend.
///
/// Construct this via [`SoftwareBackendBuilder`], then call
/// [`run_headless`](SoftwareBackend::run_headless) to obtain a
/// [`SoftwareRenderer`](crate::SoftwareRenderer) (which implements
/// [`Backend`](retroglyph_core::backend::Backend) for in-memory use, and
/// `retroglyph_window::Presenter` for windowed use).
///
/// # Examples
///
/// Windowed mode (requires `software-default-font` feature; the loop comes
/// from `retroglyph-window`):
///
/// ```ignore
/// use retroglyph_software::SoftwareBackendBuilder;
/// use retroglyph_window::winit::{WindowConfig, run_windowed};
/// use retroglyph_core::event::{Event, KeyCode};
/// use std::time::Duration;
///
/// let renderer = SoftwareBackendBuilder::new()
///     .grid_size(80, 25)
///     .scale(2)
///     .build()
///     .expect("backend init failed")
///     .run_headless();
///
/// let config = WindowConfig::fit(&renderer, "My Game", None);
/// run_windowed(config, renderer, move |term| {
///     term.clear();
///     term.print(0, 0, "Hello from rg!");
///     term.present();
///
///     if let Some(event) = term.poll(Duration::from_millis(16)) {
///         match event {
///             Event::Key(k) if k.code == KeyCode::Escape => std::process::exit(0),
///             Event::Close => std::process::exit(0),
///             _ => {}
///         }
///     }
/// }).expect("event loop failed");
/// ```
///
/// Headless mode (useful for testing):
///
/// ```ignore
/// use retroglyph_software::{SoftwareBackendBuilder, SoftwareRenderer};
/// use retroglyph_core::style::Style;
/// use retroglyph_core::grid::Pos;
/// use retroglyph_core::Color;
///
/// let opts = SoftwareBackendBuilder::new()
///     .grid_size(1, 1)
///     .scale(1)
///     .build()
///     .unwrap();
///
/// let mut renderer: SoftwareRenderer = opts.run_headless();
///
/// // Draw a red cell on layer 0.
/// use retroglyph_core::tile::Tile;
/// renderer.draw_layers(
///     [(0, Pos::new(0, 0), &Tile {
///         glyph: ' ',
///         style: Style::new().bg(Color::Rgb { r: 255, g: 0, b: 0 }),
///         ..Tile::default()
///     })].into_iter(),
/// );
///
/// let pixels = renderer.pixels();
/// assert!(pixels.iter().all(|&p| p == 0x00FF_0000));
/// ```
///
/// See the `demo` example for a complete runnable program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareBackend {
    /// Title shown in the window's title bar.
    pub window_title: String,
    /// Bitmap font used to render glyphs.
    ///
    /// `None` only when `software-default-font` is disabled and no font has
    /// been supplied via [`SoftwareBackendBuilder::font`].
    pub font: Option<BitmapFont>,
    /// Grid width in cells.
    pub cols: u16,
    /// Grid height in cells.
    pub rows: u16,
    /// Pixel-scale factor applied to each font pixel.
    ///
    /// A scale of 2 renders each 1-bit font pixel as a 2×2 block, making
    /// the VGA 8×16 font display at 16×32 pixels per cell. Default is 1.
    pub scale: u8,
    /// Registered tileset options, loaded at [`run_headless`](SoftwareBackend::run_headless) time.
    #[cfg(feature = "software-tilesets")]
    pub tilesets: Vec<TilesetOptions>,
    /// Target frame rate cap in frames per second.
    ///
    /// `None` (the default) runs unbounded: the event loop re-renders as fast
    /// as the backend allows. Set to e.g. `Some(60)` to cap at 60 fps by
    /// sleeping in `about_to_wait` until the next frame deadline. On WASM
    /// this has no effect; `requestAnimationFrame` drives the loop at the
    /// display refresh rate regardless.
    pub target_fps: Option<u32>,
}

impl Default for SoftwareBackend {
    fn default() -> Self {
        Self {
            window_title: String::from("rg application"),
            #[cfg(feature = "software-default-font")]
            font: Some(super::bitmap_font::vga8x16::FONT),
            #[cfg(not(feature = "software-default-font"))]
            font: None,
            cols: 80,
            rows: 25,
            scale: 1,
            #[cfg(feature = "software-tilesets")]
            tilesets: Vec::new(),
            target_fps: None,
        }
    }
}

/// Builder for [`SoftwareBackend`].
///
/// # Examples
///
/// ```ignore
/// use retroglyph_software::SoftwareBackendBuilder;
///
/// // With the `software-default-font` feature the embedded VGA 8×16 font is
/// // used automatically.  To supply your own 8×16 bitmap font:
/// //
/// //   use retroglyph_software::bitmap_font::BitmapFont;
/// //   let my_font = BitmapFont::new(include_bytes!("my_font.bin"), 8, 16, 256);
/// //   SoftwareBackendBuilder::new().font(my_font)...
///
/// let backend = SoftwareBackendBuilder::new()
///     .title("My Game")
///     .grid_size(80, 25)
///     .build()
///     .expect("backend init failed");
/// ```
pub struct SoftwareBackendBuilder {
    options: SoftwareBackend,
}

impl SoftwareBackendBuilder {
    /// Creates a builder with default options.
    ///
    /// When the `software-default-font` feature is enabled the IBM VGA 8×16
    /// font is pre-selected; otherwise you must call [`font`](Self::font).
    #[must_use]
    pub fn new() -> Self {
        Self {
            options: SoftwareBackend::default(),
        }
    }

    /// Sets the window title.
    #[must_use]
    pub fn title(mut self, title: &str) -> Self {
        self.options.window_title = title.to_string();
        self
    }

    /// Sets the grid dimensions in cells.
    #[must_use]
    pub const fn grid_size(mut self, cols: u16, rows: u16) -> Self {
        self.options.cols = cols;
        self.options.rows = rows;
        self
    }

    /// Pixel-scale factor for the font.
    ///
    /// Each 1-bit font pixel becomes a `scale`×`scale` block.  For the VGA
    /// 8×16 font a scale of 2 gives 16×32 pixel cells — much more readable
    /// on modern displays.
    #[must_use]
    pub const fn scale(mut self, scale: u8) -> Self {
        self.options.scale = scale;
        self
    }

    /// Overrides the bitmap font.
    ///
    /// The cell pixel size is derived from [`BitmapFont::glyph_width`] and
    /// [`BitmapFont::glyph_height`] multiplied by [`scale`](Self::scale).
    #[must_use]
    pub const fn font(mut self, font: BitmapFont) -> Self {
        self.options.font = Some(font);
        self
    }

    /// Registers a tileset for loading when the backend starts.
    ///
    /// Multiple tilesets can be registered; they are all loaded when
    /// [`run_headless`](SoftwareBackend::run_headless) is called. Later
    /// registrations win on codepoint collision.
    ///
    /// Available only when the `software-tilesets` feature is enabled.
    #[cfg(feature = "software-tilesets")]
    #[must_use]
    pub fn tileset(mut self, opts: TilesetOptions) -> Self {
        self.options.tilesets.push(opts);
        self
    }

    /// Sets a target frame rate cap in frames per second.
    ///
    /// When set, `about_to_wait` sleeps until the next frame deadline instead
    /// of rendering as fast as possible. Useful for CPU-friendly demos that
    /// don't need maximum throughput. Pass `0` to disable the cap (same as
    /// not calling this method). On WASM this has no effect.
    #[must_use]
    pub const fn target_fps(mut self, fps: u32) -> Self {
        self.options.target_fps = if fps == 0 { None } else { Some(fps) };
        self
    }

    /// Validates options and returns the backend configuration.
    ///
    /// Call [`run_headless`](SoftwareBackend::run_headless) on the result to
    /// obtain the renderer (hand it to `retroglyph_window::winit::run_windowed`
    /// to open a window).
    ///
    /// # Errors
    ///
    /// Returns [`SoftwareBackendError::NoFont`] if no font was set and the
    /// `software-default-font` feature is not enabled.
    pub fn build(self) -> Result<SoftwareBackend, SoftwareBackendError> {
        if self.options.font.is_none() {
            return Err(SoftwareBackendError::NoFont);
        }
        Ok(self.options)
    }
}

impl Default for SoftwareBackendBuilder {
    fn default() -> Self {
        Self::new()
    }
}
