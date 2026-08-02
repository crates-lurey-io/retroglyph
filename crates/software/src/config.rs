//! Configuration, builder, and error types for the software rendering backend.
//!
//! The main type is [`SoftwareBackend`], which holds grid and font
//! configuration.  Construct it via [`SoftwareBackendBuilder`], then call
//! [`into_renderer`](SoftwareBackend::into_renderer) to produce a
//! [`SoftwareRenderer`](crate::SoftwareRenderer). Hand that renderer to
//! `retroglyph_window::winit::run_windowed` to open a window, or use it
//! directly for in-memory rendering.

use retroglyph_window::font::FontChain;
#[cfg(feature = "tilesets")]
use retroglyph_window::tileset::TilesetOptions;
use std::fmt;

/// Errors that can occur when configuring the software backend.
///
/// Windowing errors (window creation, event loop) are not represented here:
/// this crate builds renderers, and the loop (`retroglyph-window` or
/// another windowing integration) reports its own errors.
#[derive(Debug)]
#[non_exhaustive]
pub enum SoftwareBackendError {
    /// No font was provided and the `default-font` feature is not enabled.
    NoFont,
    /// The fonts in the configured [`FontChain`] disagree on their glyph size.
    MixedGlyphSizes,
    /// `scale` was set to `0`, which would produce a zero-size pixel buffer.
    ZeroScale,
    /// The grid was configured with a zero column or row count, which would produce a zero-size
    /// pixel buffer.
    ZeroGrid,
    /// Tileset loading failed.
    #[cfg(feature = "tilesets")]
    Tileset(retroglyph_window::tileset::TilesetError),
}

impl fmt::Display for SoftwareBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFont => write!(
                f,
                "no bitmap font provided; supply one via \
                 SoftwareBackendBuilder::font() or enable the \
                 `default-font` feature"
            ),
            Self::MixedGlyphSizes => write!(
                f,
                "every font in a chain must have the same glyph width and height; a grid has one \
                 cell size"
            ),
            Self::ZeroScale => write!(
                f,
                "scale must be non-zero; a scale of 0 would produce a zero-size pixel buffer"
            ),
            Self::ZeroGrid => write!(f, "grid columns and rows must both be non-zero"),
            #[cfg(feature = "tilesets")]
            Self::Tileset(_) => write!(f, "tileset load failed"),
        }
    }
}

impl std::error::Error for SoftwareBackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoFont | Self::MixedGlyphSizes | Self::ZeroScale | Self::ZeroGrid => None,
            #[cfg(feature = "tilesets")]
            Self::Tileset(e) => Some(e),
        }
    }
}

/// Configuration and entry point for the software rendering backend.
///
/// Construct this via [`SoftwareBackendBuilder`], then call
/// [`into_renderer`](SoftwareBackend::into_renderer) to obtain a
/// [`SoftwareRenderer`](crate::SoftwareRenderer) (which implements
/// [`Backend`](retroglyph_core::backend::Backend) for in-memory use, and
/// `retroglyph_window::Presenter` for windowed use).
///
/// # Examples
///
/// Windowed mode (requires `default-font` feature; the loop comes
/// from `retroglyph-window`):
///
/// ```no_run
/// use retroglyph_software::SoftwareBackendBuilder;
/// use retroglyph_window::winit::{WindowConfig, run_windowed};
/// use retroglyph_core::event::{Event, KeyCode};
/// use retroglyph_core::Style;
/// use std::time::Duration;
///
/// let renderer = SoftwareBackendBuilder::new()
///     .grid_size(80, 25)
///     .scale(2)
///     .build()
///     .expect("backend init failed")
///     .into_renderer()
///     .expect("renderer init failed");
///
/// let config = WindowConfig::fit(&renderer, "My Game", None, true);
/// run_windowed(config, renderer, move |term| {
///     term.draw(|s| s.print((0, 0), "Hello from rg!", Style::default())).ok();
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
/// ```
/// use retroglyph_software::{SoftwareBackendBuilder, SoftwareRenderer};
/// use retroglyph_core::style::Style;
/// use retroglyph_core::grid::Pos;
/// use retroglyph_core::{Color, DrawCell, Output};
///
/// let opts = SoftwareBackendBuilder::new()
///     .grid_size(1, 1)
///     .scale(1)
///     .build()
///     .unwrap();
///
/// let mut renderer: SoftwareRenderer = opts.into_renderer().unwrap();
///
/// // Draw a red cell on layer 0.
/// use retroglyph_core::tile::Tile;
/// let tile = Tile::new(' ', Style::new().bg(Color::Rgb { r: 255, g: 0, b: 0 }));
/// renderer
///     .draw_layers([DrawCell::on_layer(0, Pos::new(0, 0), &tile)].into_iter())
///     .unwrap();
///
/// let pixels = renderer.pixels();
/// assert!(pixels.iter().all(|&p| p == 0x00FF_0000));
/// ```
///
/// See the `demo` example for a complete runnable program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareBackend {
    /// The chain of bitmap fonts glyphs are resolved through.
    ///
    /// `None` only when `default-font` is disabled and no font has
    /// been supplied via [`SoftwareBackendBuilder::font`].
    ///
    /// Crate-private: the only way to reach
    /// [`into_renderer`](super::SoftwareBackend::into_renderer) with a `SoftwareBackend` is
    /// through [`SoftwareBackendBuilder`], which validates this invariant in
    /// [`build`](SoftwareBackendBuilder::build). Use [`fonts`](SoftwareBackend::fonts) to read it
    /// back from outside the crate.
    pub(crate) fonts: Option<FontChain<'static>>,
    /// Grid width in cells.
    pub cols: u16,
    /// Grid height in cells.
    pub rows: u16,
    /// Pixel-scale factor applied to each font pixel.
    ///
    /// A scale of 2 renders each 1-bit font pixel as a 2×2 block, making
    /// the Unscii 16 font display at 16×32 pixels per cell. Default is 1.
    pub scale: u8,
    /// Registered tileset options, loaded at
    /// [`into_renderer`](SoftwareBackend::into_renderer) time.
    #[cfg(feature = "tilesets")]
    pub tilesets: Vec<TilesetOptions>,
}

impl SoftwareBackend {
    /// Returns the configured font chain, if any.
    ///
    /// `None` only when `default-font` is disabled and no font was supplied
    /// via [`SoftwareBackendBuilder::font`]; in that case
    /// [`SoftwareBackendBuilder::build`] fails with [`SoftwareBackendError::NoFont`]
    /// before a `SoftwareBackend` can be constructed at all.
    #[must_use]
    pub const fn fonts(&self) -> Option<&FontChain<'static>> {
        self.fonts.as_ref()
    }

    /// Builder-internal defaults. Not exposed as `impl Default`: the only
    /// supported way to obtain a `SoftwareBackend` is through
    /// [`SoftwareBackendBuilder`], so that its `font` invariant is always
    /// validated by [`SoftwareBackendBuilder::build`].
    const fn defaults() -> Self {
        Self {
            #[cfg(feature = "default-font")]
            fonts: Some(FontChain::new(retroglyph_window::font::unscii16::FONT, &[])),
            #[cfg(not(feature = "default-font"))]
            fonts: None,
            cols: 80,
            rows: 25,
            scale: 1,
            #[cfg(feature = "tilesets")]
            tilesets: Vec::new(),
        }
    }
}

/// Builder for [`SoftwareBackend`].
///
/// # Examples
///
/// ```
/// use retroglyph_software::SoftwareBackendBuilder;
///
/// // With the `default-font` feature the embedded Unscii 16 font is
/// // used automatically.  To supply your own 8×16 bitmap font:
/// //
/// //   use retroglyph_window::font::BitmapFont;
/// //   let my_font = BitmapFont::new(include_bytes!("my_font.bin"), 8, 16, 256);
/// //   SoftwareBackendBuilder::new().font(my_font)...
///
/// let backend = SoftwareBackendBuilder::new()
///     .grid_size(80, 25)
///     .build()
///     .expect("backend init failed");
/// ```
#[derive(Debug)]
pub struct SoftwareBackendBuilder {
    options: SoftwareBackend,
}

impl SoftwareBackendBuilder {
    /// Creates a builder with default options.
    ///
    /// When the `default-font` feature is enabled the Unscii 16
    /// font is pre-selected; otherwise you must call [`font`](Self::font).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            options: SoftwareBackend::defaults(),
        }
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
    /// Each 1-bit font pixel becomes a `scale`×`scale` block. For the VGA
    /// 8×16 font a scale of 2 gives 16×32 pixel cells, more readable
    /// on modern displays.
    #[must_use]
    pub const fn scale(mut self, scale: u8) -> Self {
        self.options.scale = scale;
        self
    }

    /// Overrides the fonts glyphs are resolved through.
    ///
    /// Takes either a single [`BitmapFont`](retroglyph_window::font::BitmapFont) or a whole
    /// [`FontChain`], since a lone font is a chain of one. A chain is how a grid draws characters
    /// CP437 has no mapping for (quadrants, sextants, braille): the extra coverage comes from a
    /// fallback font built with
    /// [`BitmapFont::with_charset`](retroglyph_window::font::BitmapFont::with_charset), and the
    /// glyph is drawn from the bitmap font path, so it takes the cell's foreground color like any
    /// other glyph (unlike a tileset sprite, which carries its own colors).
    ///
    /// The cell pixel size is derived from the chain's glyph size multiplied by
    /// [`scale`](Self::scale); every font in a chain must therefore agree on its glyph size, or
    /// [`build`](Self::build) fails with [`SoftwareBackendError::MixedGlyphSizes`].
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_software::SoftwareBackendBuilder;
    /// use retroglyph_window::font::{BitmapFont, FontChain, unscii16};
    ///
    /// // A fallback font declaring the quadrant glyphs CP437 has no mapping for.
    /// static QUADRANTS: [u8; 3 * 16] = [0; 3 * 16];
    /// const CHARSET: [(char, u8); 3] = [('▘', 0), ('▝', 1), ('▖', 2)];
    /// static FALLBACKS: [BitmapFont; 1] =
    ///     [BitmapFont::with_charset(&QUADRANTS, 8, 16, 3, &CHARSET)];
    ///
    /// let backend = SoftwareBackendBuilder::new()
    ///     .font(FontChain::new(unscii16::FONT, &FALLBACKS))
    ///     .build()
    ///     .expect("backend init failed");
    /// ```
    #[must_use]
    pub fn font(mut self, fonts: impl Into<FontChain<'static>>) -> Self {
        self.options.fonts = Some(fonts.into());
        self
    }

    /// Registers a tileset for loading when the backend starts.
    ///
    /// Multiple tilesets can be registered; they are all loaded when
    /// [`into_renderer`](SoftwareBackend::into_renderer) is called. Later
    /// registrations win on codepoint collision.
    ///
    /// Available only when the `tilesets` feature is enabled.
    #[cfg(feature = "tilesets")]
    #[must_use]
    pub fn tileset(mut self, opts: TilesetOptions) -> Self {
        self.options.tilesets.push(opts);
        self
    }

    /// Validates options and returns the backend configuration.
    ///
    /// Call [`into_renderer`](SoftwareBackend::into_renderer) on the result to
    /// obtain the renderer (hand it to `retroglyph_window::winit::run_windowed`
    /// to open a window).
    ///
    /// # Errors
    ///
    /// Returns [`SoftwareBackendError::NoFont`] if no font was set and the
    /// `default-font` feature is not enabled.
    ///
    /// Returns [`SoftwareBackendError::MixedGlyphSizes`] if the configured chain's fonts disagree
    /// on their glyph size.
    ///
    /// Returns [`SoftwareBackendError::ZeroScale`] if `scale` was set to `0`.
    ///
    /// Returns [`SoftwareBackendError::ZeroGrid`] if `cols` or `rows` was set to `0`.
    pub fn build(self) -> Result<SoftwareBackend, SoftwareBackendError> {
        let Some(fonts) = self.options.fonts else {
            return Err(SoftwareBackendError::NoFont);
        };
        if fonts.glyph_size().is_none() {
            return Err(SoftwareBackendError::MixedGlyphSizes);
        }
        if self.options.scale == 0 {
            return Err(SoftwareBackendError::ZeroScale);
        }
        if self.options.cols == 0 || self.options.rows == 0 {
            return Err(SoftwareBackendError::ZeroGrid);
        }
        Ok(self.options)
    }
}

impl Default for SoftwareBackendBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_window::font::BitmapFont;

    fn test_font() -> BitmapFont {
        static DATA: [u8; 16] = [0; 16];
        BitmapFont::new(&DATA, 8, 16, 1)
    }

    #[test]
    fn build_rejects_zero_scale() {
        let result = SoftwareBackendBuilder::new()
            .font(test_font())
            .scale(0)
            .build();
        assert!(matches!(result, Err(SoftwareBackendError::ZeroScale)));
    }

    #[test]
    fn build_rejects_a_zero_grid_dimension() {
        let result = SoftwareBackendBuilder::new()
            .font(test_font())
            .grid_size(0, 25)
            .build();
        assert!(matches!(result, Err(SoftwareBackendError::ZeroGrid)));
    }

    #[test]
    fn build_accepts_nonzero_scale() {
        let result = SoftwareBackendBuilder::new()
            .font(test_font())
            .scale(2)
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn build_rejects_a_chain_whose_fonts_disagree_on_glyph_size() {
        static SHORT_DATA: [u8; 8] = [0; 8];
        static FALLBACKS: [BitmapFont; 1] = [BitmapFont::new(&SHORT_DATA, 8, 8, 1)];

        let result = SoftwareBackendBuilder::new()
            .font(FontChain::new(test_font(), &FALLBACKS))
            .build();
        assert!(matches!(result, Err(SoftwareBackendError::MixedGlyphSizes)));
    }
}
