//! Configuration, builder, and error types for the wgpu backend.
//!
//! [`WgpuBackendBuilder`] gathers grid size, integer scale, a [`FontChain`], and (with the
//! `tilesets` feature) any PNG sprite tilesets, then [`build`](WgpuBackendBuilder::build) produces
//! a [`WgpuRenderer`]. The renderer is created without a GPU device; the device, surface, and every
//! GPU resource are created lazily when the windowing loop calls
//! [`Presenter::init_surface`](retroglyph_window::Presenter::init_surface).

use crate::WgpuRenderer;
use retroglyph_window::atlas::{GlyphAtlas, MAX_SLOTS};
use retroglyph_window::font::FontChain;
#[cfg(feature = "tilesets")]
use retroglyph_window::tileset::TilesetOptions;
use std::fmt;

/// Errors from configuring the wgpu backend.
///
/// Every variant is a configuration mistake caught before any GPU work happens; failures from the
/// device itself are [`SurfaceError`](crate::SurfaceError) instead.
#[derive(Debug)]
#[non_exhaustive]
pub enum WgpuBackendError {
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
    /// A registered tileset failed to decode.
    #[cfg(feature = "tilesets")]
    Tileset(retroglyph_window::tileset::TilesetError),
}

impl fmt::Display for WgpuBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFont => write!(
                f,
                "no bitmap font provided; supply one via WgpuBackendBuilder::font() or enable the \
                 `default-font` feature"
            ),
            Self::MixedGlyphSizes => write!(
                f,
                "every font in a chain must have the same glyph width and height; a grid has one \
                 cell size"
            ),
            Self::FontChainTooLarge => write!(
                f,
                "a font chain may hold at most {MAX_SLOTS} glyphs in total; the atlas addresses a \
                 glyph by a 16-bit slot"
            ),
            Self::ZeroScale => write!(f, "scale must be non-zero"),
            Self::ZeroGrid => write!(f, "grid columns and rows must both be non-zero"),
            Self::SurfaceTooLarge => write!(
                f,
                "grid_size and scale combine to a surface over u32::MAX pixels wide or tall"
            ),
            #[cfg(feature = "tilesets")]
            Self::Tileset(e) => write!(f, "tileset error: {e}"),
        }
    }
}

impl std::error::Error for WgpuBackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(feature = "tilesets")]
            Self::Tileset(e) => Some(e),
            _ => None,
        }
    }
}

/// Builder for the wgpu backend.
///
/// # Examples
///
/// ```no_run
/// use retroglyph_core::color::Style;
/// use retroglyph_wgpu::WgpuBackendBuilder;
/// use retroglyph_window::winit::{WindowConfig, run_windowed};
///
/// let renderer = WgpuBackendBuilder::new()
///     .grid_size(80, 25)
///     .scale(2)
///     .build()
///     .expect("wgpu backend init failed");
/// let config = WindowConfig::fit(&renderer, "My Game", None, true);
/// run_windowed(config, renderer, move |term| {
///     term.draw(|s| s.print((0, 0), "Hello from retroglyph-wgpu!", Style::default()))
///         .ok();
/// })
/// .expect("event loop failed");
/// ```
#[derive(Debug, Clone)]
pub struct WgpuBackendBuilder {
    fonts: Option<FontChain<'static>>,
    /// Registered tilesets, decoded into a sprite atlas at [`build`](Self::build) time.
    #[cfg(feature = "tilesets")]
    tilesets: Vec<TilesetOptions>,
    cols: u16,
    rows: u16,
    scale: u16,
}

impl Default for WgpuBackendBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl WgpuBackendBuilder {
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
    /// in the chain is packed into the same glyph atlas, and the glyph is drawn from that atlas, so
    /// it takes the cell's foreground color like any other glyph (unlike a tileset sprite, which
    /// carries its own colors).
    ///
    /// Every font in a chain must agree on its glyph size, since that is the grid's cell size, and
    /// the chain's glyphs must fit the atlas's 16-bit slot space; otherwise [`build`](Self::build)
    /// fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use retroglyph_wgpu::WgpuBackendBuilder;
    /// use retroglyph_window::font::{BitmapFont, FontChain, unscii16};
    ///
    /// // A fallback font declaring the quadrant glyphs CP437 has no mapping for.
    /// static QUADRANTS: [u8; 3 * 16] = [0; 3 * 16];
    /// const CHARSET: [(char, u8); 3] = [('▘', 0), ('▝', 1), ('▖', 2)];
    /// static FALLBACKS: [BitmapFont; 1] =
    ///     [BitmapFont::with_charset(&QUADRANTS, 8, 16, 3, &CHARSET)];
    ///
    /// let renderer = WgpuBackendBuilder::new()
    ///     .font(FontChain::new(unscii16::FONT, &FALLBACKS))
    ///     .build()
    ///     .expect("wgpu backend init failed");
    /// ```
    #[must_use]
    pub fn font(mut self, fonts: impl Into<FontChain<'static>>) -> Self {
        self.fonts = Some(fonts.into());
        self
    }

    /// Registers a PNG sprite tileset. Glyphs a tileset maps override the bitmap font for those
    /// codepoints; register multiple and later ones win on codepoint collision. Build the options
    /// with [`TilesetOptions::builder`](retroglyph_window::tileset::TilesetOptions::builder).
    ///
    /// Available only with the `tilesets` feature.
    #[cfg(feature = "tilesets")]
    #[must_use]
    pub fn tileset(mut self, opts: TilesetOptions) -> Self {
        self.tilesets.push(opts);
        self
    }

    /// Builds the [`WgpuRenderer`].
    ///
    /// The renderer holds no GPU device yet; the device and surface are created when the windowing
    /// loop calls [`Presenter::init_surface`](retroglyph_window::Presenter::init_surface).
    ///
    /// # Errors
    ///
    /// Returns [`WgpuBackendError::NoFont`] if no font was set and the `default-font` feature is
    /// disabled, [`WgpuBackendError::MixedGlyphSizes`] if the configured chain's fonts disagree on
    /// their glyph size, [`WgpuBackendError::FontChainTooLarge`] if the chain's glyphs overflow the
    /// atlas's slot space, [`WgpuBackendError::ZeroScale`] if `scale` is 0,
    /// [`WgpuBackendError::ZeroGrid`] if either grid dimension is 0, or
    /// [`WgpuBackendError::SurfaceTooLarge`] if `cols`/`rows`/`scale` combine to a surface wider or
    /// taller than `u32::MAX` physical pixels.
    pub fn build(self) -> Result<WgpuRenderer, WgpuBackendError> {
        if self.scale == 0 {
            return Err(WgpuBackendError::ZeroScale);
        }
        if self.cols == 0 || self.rows == 0 {
            return Err(WgpuBackendError::ZeroGrid);
        }
        let fonts = self.resolve_fonts()?;
        let Some(glyph_size) = fonts.glyph_size() else {
            return Err(WgpuBackendError::MixedGlyphSizes);
        };
        // `CellGeometry::surface_size` multiplies as plain `u32`; check for overflow here, in
        // `u64`, before it can happen there (`scale` is `u16` on this backend, so the product is
        // not overflow-free by construction).
        let cell_w = u64::from(glyph_size.0) * u64::from(self.scale);
        let cell_h = u64::from(glyph_size.1) * u64::from(self.scale);
        let surface_w = u64::from(self.cols) * cell_w;
        let surface_h = u64::from(self.rows) * cell_h;
        if surface_w > u64::from(u32::MAX) || surface_h > u64::from(u32::MAX) {
            return Err(WgpuBackendError::SurfaceTooLarge);
        }
        let glyphs = GlyphAtlas::new(fonts, glyph_size);
        if glyphs.slot_count() > MAX_SLOTS {
            return Err(WgpuBackendError::FontChainTooLarge);
        }
        #[cfg_attr(not(feature = "tilesets"), allow(unused_mut))]
        let mut renderer = WgpuRenderer::new(glyphs, self.cols, self.rows, self.scale);
        #[cfg(feature = "tilesets")]
        {
            let cache = retroglyph_window::sprite_cache::SpriteCache::from_tilesets(&self.tilesets)
                .map_err(WgpuBackendError::Tileset)?;
            if let Some(set) = crate::sprite_set::SpriteSet::from_cache(&cache) {
                renderer.set_sprites(set);
            }
        }
        Ok(renderer)
    }

    /// Resolves the font chain: the explicitly set one, else the embedded default (if the feature
    /// is on), else [`WgpuBackendError::NoFont`].
    // The `Result` is not always-`Ok`: without `default-font` the fallback arm returns `Err`.
    // clippy only sees one feature configuration at a time, so silence its feature-blind
    // `unnecessary_wraps`/`const` suggestions here.
    #[allow(clippy::unnecessary_wraps, clippy::missing_const_for_fn)]
    fn resolve_fonts(&self) -> Result<FontChain<'static>, WgpuBackendError> {
        if let Some(fonts) = self.fonts {
            return Ok(fonts);
        }
        #[cfg(feature = "default-font")]
        {
            Ok(FontChain::from(retroglyph_window::font::unscii16::FONT))
        }
        #[cfg(not(feature = "default-font"))]
        {
            Err(WgpuBackendError::NoFont)
        }
    }
}

impl retroglyph_window::PresenterBuilder for WgpuBackendBuilder {
    type Presenter = WgpuRenderer;
    type Error = WgpuBackendError;

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
    use super::{WgpuBackendBuilder, WgpuBackendError};

    #[test]
    fn zero_scale_and_zero_grid_are_rejected_before_any_font_work() {
        assert!(matches!(
            WgpuBackendBuilder::new().scale(0).build(),
            Err(WgpuBackendError::ZeroScale)
        ));
        assert!(matches!(
            WgpuBackendBuilder::new().grid_size(0, 10).build(),
            Err(WgpuBackendError::ZeroGrid)
        ));
        assert!(matches!(
            WgpuBackendBuilder::new().grid_size(10, 0).build(),
            Err(WgpuBackendError::ZeroGrid)
        ));
    }

    #[cfg(not(feature = "default-font"))]
    #[test]
    fn a_fontless_build_names_the_two_ways_to_supply_one() {
        let err = WgpuBackendBuilder::new().build().unwrap_err();
        assert!(matches!(err, WgpuBackendError::NoFont));
        let msg = err.to_string();
        assert!(msg.contains("WgpuBackendBuilder::font()"));
        assert!(msg.contains("default-font"));
    }

    #[cfg(feature = "default-font")]
    #[test]
    fn a_grid_and_scale_that_overflow_the_surface_size_are_rejected() {
        // retroglyph#729: `CellGeometry::surface_size` multiplies cols/rows/scale as plain `u32`,
        // which overflows for a `u16` scale this large.
        let result = WgpuBackendBuilder::new()
            .grid_size(u16::MAX, 1)
            .scale(u16::MAX)
            .build();
        assert!(matches!(result, Err(WgpuBackendError::SurfaceTooLarge)));
    }

    #[cfg(feature = "default-font")]
    #[test]
    fn mixed_glyph_sizes_are_rejected() {
        use retroglyph_window::font::{BitmapFont, FontChain, unscii16};

        // A fallback whose glyphs are 8x8, against an 8x16 primary: a grid has one cell size.
        static SMALL: [u8; 8] = [0; 8];
        const CHARSET: [(char, u8); 1] = [('▘', 0)];
        static FALLBACKS: [BitmapFont; 1] = [BitmapFont::with_charset(&SMALL, 8, 8, 1, &CHARSET)];

        let result = WgpuBackendBuilder::new()
            .font(FontChain::new(unscii16::FONT, &FALLBACKS))
            .build();
        assert!(matches!(result, Err(WgpuBackendError::MixedGlyphSizes)));
    }
}
