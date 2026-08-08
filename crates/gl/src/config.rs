//! Configuration, builder, and error types for the GL backend.
//!
//! [`GlBackendBuilder`] gathers grid size, integer scale, a [`FontChain`], and (with the
//! `tilesets` feature) any PNG sprite tilesets, then [`build`](GlBackendBuilder::build) produces a
//! [`GlRenderer`]. The renderer is created without a GL context; the context and GPU resources are
//! created lazily when the windowing loop calls
//! [`Presenter::init_surface`](retroglyph_window::presenter::Presenter::init_surface).

use crate::GlRenderer;
use retroglyph_window::atlas::GlyphAtlas;
use retroglyph_window::font::FontChain;
#[cfg(feature = "tilesets")]
use retroglyph_window::tileset::TilesetOptions;
use std::fmt;

/// Errors from configuring the GL backend.
#[derive(Debug)]
#[non_exhaustive]
pub enum GlBackendError {
    /// No font was provided and the `default-font` feature is not enabled.
    NoFont,
    /// The fonts in the configured [`FontChain`] disagree on their glyph size.
    MixedGlyphSizes,
    /// The configured [`FontChain`] has more glyphs in total than the atlas can address.
    FontChainTooLarge,
    /// `scale` was set to `0`, which would produce a zero-size surface.
    ZeroScale,
    /// The grid was configured with a zero column or row count.
    ZeroGrid,
    /// `cols`, `rows`, and `scale` combine to a surface wider or taller than `u32::MAX` physical
    /// pixels.
    SurfaceTooLarge,
    /// A registered tileset failed to decode (issue #366).
    #[cfg(feature = "tilesets")]
    Tileset(retroglyph_window::tileset::TilesetError),
}

impl fmt::Display for GlBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFont => write!(
                f,
                "no bitmap font provided; supply one via GlBackendBuilder::font() or enable the \
                 `default-font` feature"
            ),
            Self::MixedGlyphSizes => write!(
                f,
                "every font in a chain must have the same glyph width and height; a grid has one \
                 cell size"
            ),
            Self::FontChainTooLarge => write!(
                f,
                "a font chain may hold at most {} glyphs in total; the atlas addresses a glyph by \
                 a 16-bit slot",
                u32::from(u16::MAX) + 1
            ),
            Self::ZeroScale => write!(f, "scale must be non-zero"),
            Self::ZeroGrid => write!(f, "grid columns and rows must both be non-zero"),
            Self::SurfaceTooLarge => {
                write!(
                    f,
                    "grid_size and scale combine to a surface over u32::MAX pixels wide or tall"
                )
            }
            #[cfg(feature = "tilesets")]
            Self::Tileset(e) => write!(f, "tileset error: {e}"),
        }
    }
}

impl std::error::Error for GlBackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(feature = "tilesets")]
            Self::Tileset(e) => Some(e),
            _ => None,
        }
    }
}

/// Builder for the GL backend.
///
/// # Examples
///
/// ```no_run
/// # #[cfg(not(target_arch = "wasm32"))]
/// # fn main() {
/// use retroglyph_core::color::Style;
/// use retroglyph_gl::GlBackendBuilder;
/// use retroglyph_window::winit::{WindowConfig, run_windowed};
///
/// let renderer = GlBackendBuilder::new()
///     .grid_size(80, 25)
///     .scale(2)
///     .build()
///     .expect("gl backend init failed");
/// let config = WindowConfig::fit(&renderer, "My Game", None, true);
/// run_windowed(config, renderer, move |term| {
///     term.draw(|s| s.print((0, 0), "Hello from retroglyph-gl!", Style::default()))
///         .ok();
/// })
/// .expect("event loop failed");
/// # }
/// # #[cfg(target_arch = "wasm32")]
/// # fn main() {}
/// ```
#[derive(Debug, Clone)]
pub struct GlBackendBuilder {
    fonts: Option<FontChain<'static>>,
    /// Registered tilesets, decoded into a sprite atlas at [`build`](Self::build) time (issue #366).
    #[cfg(feature = "tilesets")]
    tilesets: Vec<TilesetOptions>,
    cols: u16,
    rows: u16,
    scale: u16,
}

impl Default for GlBackendBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl GlBackendBuilder {
    /// A new builder with an 80x25 grid at scale 1 and no font yet (the `default-font` feature
    /// supplies one at [`build`](Self::build) time if none is set).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            fonts: None,
            #[cfg(feature = "tilesets")]
            tilesets: Vec::new(),
            cols: 80,
            rows: 25,
            scale: 1,
        }
    }

    /// Sets the grid size in cells.
    #[must_use]
    pub const fn grid_size(mut self, cols: u16, rows: u16) -> Self {
        self.cols = cols;
        self.rows = rows;
        self
    }

    /// Sets the integer pixel scale (each glyph pixel becomes `scale`x`scale` physical pixels).
    #[must_use]
    pub const fn scale(mut self, scale: u16) -> Self {
        self.scale = scale;
        self
    }

    /// Sets the fonts glyphs are resolved through, overriding the `default-font` embedded font.
    ///
    /// Takes either a single [`BitmapFont`](retroglyph_window::font::BitmapFont) or a whole
    /// [`FontChain`], since a lone font is a chain of one. A chain is how a grid draws characters
    /// CP437 has no mapping for (quadrants, sextants, braille): the extra coverage comes from a
    /// fallback font built with
    /// [`BitmapFont::with_charset`](retroglyph_window::font::BitmapFont::with_charset), every font
    /// in the chain is packed into the same glyph atlas, and the glyph is drawn from that atlas,
    /// so it takes the cell's foreground color like any other glyph (unlike a tileset sprite,
    /// which carries its own colors).
    ///
    /// Every font in a chain must agree on its glyph size, since that is the grid's cell size, and
    /// the chain's glyphs must fit the atlas's 16-bit slot space; otherwise
    /// [`build`](Self::build) fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use retroglyph_gl::GlBackendBuilder;
    /// use retroglyph_window::font::{BitmapFont, FontChain, unscii16};
    ///
    /// // A fallback font declaring the quadrant glyphs CP437 has no mapping for.
    /// static QUADRANTS: [u8; 3 * 16] = [0; 3 * 16];
    /// const CHARSET: [(char, u8); 3] = [('▘', 0), ('▝', 1), ('▖', 2)];
    /// static FALLBACKS: [BitmapFont; 1] =
    ///     [BitmapFont::with_charset(&QUADRANTS, 8, 16, 3, &CHARSET)];
    ///
    /// let renderer = GlBackendBuilder::new()
    ///     .font(FontChain::new(unscii16::FONT, &FALLBACKS))
    ///     .build()
    ///     .expect("gl backend init failed");
    /// ```
    #[must_use]
    pub fn font(mut self, fonts: impl Into<FontChain<'static>>) -> Self {
        self.fonts = Some(fonts.into());
        self
    }

    /// Registers a PNG sprite tileset (issue #366). Glyphs a tileset maps override the bitmap font
    /// for those codepoints; register multiple and later ones win on codepoint collision. Build
    /// the options with [`TilesetOptions::builder`](retroglyph_window::tileset::TilesetOptions::builder).
    ///
    /// Available only with the `tilesets` feature.
    #[cfg(feature = "tilesets")]
    #[must_use]
    pub fn tileset(mut self, opts: TilesetOptions) -> Self {
        self.tilesets.push(opts);
        self
    }

    /// Builds the [`GlRenderer`].
    ///
    /// The renderer holds no GL context yet; the context is created when the windowing loop calls
    /// [`Presenter::init_surface`](retroglyph_window::presenter::Presenter::init_surface).
    ///
    /// # Errors
    ///
    /// Returns [`GlBackendError::NoFont`] if no font was set and the `default-font` feature is
    /// disabled, [`GlBackendError::MixedGlyphSizes`] if the configured chain's fonts disagree on
    /// their glyph size, [`GlBackendError::FontChainTooLarge`] if the chain's glyphs overflow the
    /// atlas's slot space, [`GlBackendError::ZeroScale`] if `scale` is 0,
    /// [`GlBackendError::ZeroGrid`] if either grid dimension is 0, or
    /// [`GlBackendError::SurfaceTooLarge`] if `cols`/`rows`/`scale` combine to a surface wider or
    /// taller than `u32::MAX` physical pixels.
    pub fn build(self) -> Result<GlRenderer, GlBackendError> {
        if self.scale == 0 {
            return Err(GlBackendError::ZeroScale);
        }
        if self.cols == 0 || self.rows == 0 {
            return Err(GlBackendError::ZeroGrid);
        }
        let fonts = self.resolve_fonts()?;
        let Some(glyph_size) = fonts.glyph_size() else {
            return Err(GlBackendError::MixedGlyphSizes);
        };
        // `CellGeometry::surface_size` multiplies as plain `u32`; check for overflow here, in
        // `u64`, before it can happen there (`scale` is `u16`, so the product is not
        // overflow-free by construction).
        let cell_w = u64::from(glyph_size.0) * u64::from(self.scale);
        let cell_h = u64::from(glyph_size.1) * u64::from(self.scale);
        let surface_w = u64::from(self.cols) * cell_w;
        let surface_h = u64::from(self.rows) * cell_h;
        if surface_w > u64::from(u32::MAX) || surface_h > u64::from(u32::MAX) {
            return Err(GlBackendError::SurfaceTooLarge);
        }
        let glyphs = GlyphAtlas::new(fonts, glyph_size);
        if glyphs.slot_count() > retroglyph_window::atlas::MAX_SLOTS {
            return Err(GlBackendError::FontChainTooLarge);
        }
        #[cfg_attr(not(feature = "tilesets"), allow(unused_mut))]
        let mut renderer = GlRenderer::new(glyphs, self.cols, self.rows, self.scale);
        #[cfg(feature = "tilesets")]
        {
            let cache = retroglyph_window::sprite_cache::SpriteCache::from_tilesets(&self.tilesets)
                .map_err(GlBackendError::Tileset)?;
            if let Some(set) = crate::sprites::SpriteSet::from_cache(&cache) {
                renderer.set_sprites(set);
            }
        }
        Ok(renderer)
    }

    /// Resolves the font chain: the explicitly set one, else the embedded default (if the feature
    /// is on), else [`GlBackendError::NoFont`].
    // The `Result` is not always-`Ok`: without `default-font` the fallback arm returns `Err`.
    // clippy only sees one feature configuration at a time, so silence its feature-blind
    // `unnecessary_wraps`/`const` suggestions here.
    #[allow(clippy::unnecessary_wraps, clippy::missing_const_for_fn)]
    fn resolve_fonts(&self) -> Result<FontChain<'static>, GlBackendError> {
        if let Some(fonts) = self.fonts {
            return Ok(fonts);
        }
        #[cfg(feature = "default-font")]
        {
            Ok(FontChain::from(retroglyph_window::font::unscii16::FONT))
        }
        #[cfg(not(feature = "default-font"))]
        {
            Err(GlBackendError::NoFont)
        }
    }
}

impl retroglyph_window::presenter_builder::PresenterBuilder for GlBackendBuilder {
    type Presenter = GlRenderer;
    type Error = GlBackendError;

    fn new() -> Self {
        Self::new()
    }

    fn grid_size(self, cols: u16, rows: u16) -> Self {
        self.grid_size(cols, rows)
    }

    fn scale(self, scale: u16) -> Self {
        self.scale(scale)
    }

    fn font(self, fonts: impl Into<FontChain<'static>>) -> Self {
        self.font(fonts)
    }

    #[cfg(feature = "tilesets")]
    fn tileset(self, opts: TilesetOptions) -> Self {
        self.tileset(opts)
    }

    fn build_presenter(self) -> Result<Self::Presenter, Self::Error> {
        self.build()
    }
}

#[cfg(test)]
mod tests {
    use super::GlBackendBuilder;

    fn test_font() -> retroglyph_window::font::BitmapFont {
        static DATA: [u8; 16] = [0; 16];
        retroglyph_window::font::BitmapFont::new(&DATA, 8, 16, 1)
    }

    /// Exercises `PresenterBuilder`'s methods through the trait, not the inherent ones, so the
    /// forwarding impl itself (not just the methods it forwards to) is under test.
    #[test]
    fn presenter_builder_impl_forwards_to_the_inherent_methods() {
        fn build<B: retroglyph_window::presenter_builder::PresenterBuilder>(
            font: retroglyph_window::font::BitmapFont,
        ) -> Result<B::Presenter, B::Error> {
            B::new()
                .grid_size(4, 2)
                .scale(1)
                .font(font)
                .build_presenter()
        }
        assert!(build::<GlBackendBuilder>(test_font()).is_ok());
    }
}
