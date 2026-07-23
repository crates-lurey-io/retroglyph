//! Configuration, builder, and error types for the GL backend.
//!
//! [`GlBackendBuilder`] gathers grid size, integer scale, and a [`BitmapFont`], then
//! [`build`](GlBackendBuilder::build) produces a [`GlRenderer`]. The renderer is
//! created without a GL context; the context and GPU resources are created lazily when the
//! windowing loop calls
//! [`Presenter::init_surface`](retroglyph_window::Presenter::init_surface).

use crate::GlRenderer;
use retroglyph_font::BitmapFont;
use std::fmt;

/// Errors from configuring the GL backend.
#[derive(Debug)]
pub enum GlBackendError {
    /// No font was provided and the `default-font` feature is not enabled.
    NoFont,
    /// `scale` was set to `0`, which would produce a zero-size surface.
    ZeroScale,
    /// The grid was configured with a zero column or row count.
    ZeroGrid,
}

impl fmt::Display for GlBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFont => write!(
                f,
                "no bitmap font provided; supply one via GlBackendBuilder::font() or enable the \
                 `default-font` feature"
            ),
            Self::ZeroScale => write!(f, "scale must be non-zero"),
            Self::ZeroGrid => write!(f, "grid columns and rows must both be non-zero"),
        }
    }
}

impl std::error::Error for GlBackendError {}

/// Builder for the GL backend.
///
/// # Examples
///
/// ```ignore
/// use retroglyph_gl::GlBackendBuilder;
/// use retroglyph_window::winit::{WindowConfig, run_app};
///
/// let renderer = GlBackendBuilder::new()
///     .grid_size(80, 25)
///     .scale(2)
///     .build()
///     .expect("gl backend init failed");
/// let config = WindowConfig::fit(&renderer, "My Game", None);
/// // run_app(config, renderer, app)?;
/// ```
#[derive(Debug, Clone)]
pub struct GlBackendBuilder {
    font: Option<BitmapFont>,
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
            font: None,
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

    /// Sets the bitmap font (overrides the `default-font` embedded font).
    #[must_use]
    pub const fn font(mut self, font: BitmapFont) -> Self {
        self.font = Some(font);
        self
    }

    /// Builds the [`GlRenderer`].
    ///
    /// The renderer holds no GL context yet; the context is created when the windowing loop calls
    /// [`Presenter::init_surface`](retroglyph_window::Presenter::init_surface).
    ///
    /// # Errors
    ///
    /// Returns [`GlBackendError::NoFont`] if no font was set and the `default-font` feature is
    /// disabled, [`GlBackendError::ZeroScale`] if `scale` is 0, or [`GlBackendError::ZeroGrid`] if
    /// either grid dimension is 0.
    pub fn build(self) -> Result<GlRenderer, GlBackendError> {
        if self.scale == 0 {
            return Err(GlBackendError::ZeroScale);
        }
        if self.cols == 0 || self.rows == 0 {
            return Err(GlBackendError::ZeroGrid);
        }
        let font = self.resolve_font()?;
        Ok(GlRenderer::new(font, self.cols, self.rows, self.scale))
    }

    /// Resolves the font: the explicitly set one, else the embedded default (if the feature is on),
    /// else [`GlBackendError::NoFont`].
    // The `Result` is not always-`Ok`: without `default-font` the fallback arm returns `Err`.
    // clippy only sees one feature configuration at a time, so silence its feature-blind
    // `unnecessary_wraps`/`const` suggestions here.
    #[allow(clippy::unnecessary_wraps, clippy::missing_const_for_fn)]
    fn resolve_font(&self) -> Result<BitmapFont, GlBackendError> {
        if let Some(font) = self.font {
            return Ok(font);
        }
        #[cfg(feature = "default-font")]
        {
            Ok(retroglyph_font::unscii16::FONT)
        }
        #[cfg(not(feature = "default-font"))]
        {
            Err(GlBackendError::NoFont)
        }
    }
}
