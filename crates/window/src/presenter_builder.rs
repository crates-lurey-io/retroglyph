//! [`PresenterBuilder`](crate::presenter_builder::PresenterBuilder), the shape shared by the
//! software/GL/wgpu backend builders.

use crate::font::FontChain;
use crate::presenter::Presenter;
#[cfg(feature = "tilesets")]
use crate::tileset::TilesetOptions;

/// The builder shape `retroglyph-software`, `retroglyph-gl`, and `retroglyph-wgpu` all implement.
///
/// `SoftwareBackendBuilder`, `GlBackendBuilder`, and `WgpuBackendBuilder` are otherwise unrelated
/// types: each crate defines its own builder because each backend owns its own renderer and error
/// type, but the configuration surface (grid size, scale, fonts, tilesets) is identical across all
/// three. This trait names that shared surface so a caller (a demo gallery, a test harness) can be
/// generic over "a windowed backend's builder" instead of writing one near-identical function per
/// backend crate (retroglyph#1192).
///
/// A backend crate implements this by forwarding to its existing inherent methods; see
/// `SoftwareBackendBuilder`, `GlBackendBuilder`, and `WgpuBackendBuilder` for the impls.
pub trait PresenterBuilder: Sized {
    /// The renderer this builder produces.
    type Presenter: Presenter + 'static;
    /// The error [`build_presenter`](Self::build_presenter) can fail with.
    type Error: core::error::Error;

    /// Creates a builder with the backend's default configuration.
    fn new() -> Self;

    /// Sets the grid dimensions in cells.
    #[must_use]
    fn grid_size(self, cols: u16, rows: u16) -> Self;

    /// Sets the integer pixel scale (each glyph pixel becomes `scale`x`scale` physical pixels).
    #[must_use]
    fn scale(self, scale: u16) -> Self;

    /// Sets the fonts glyphs are resolved through.
    #[must_use]
    fn font(self, fonts: impl Into<FontChain<'static>>) -> Self;

    /// Registers a PNG sprite tileset. Available only with the `tilesets` feature.
    #[cfg(feature = "tilesets")]
    #[must_use]
    fn tileset(self, opts: TilesetOptions) -> Self;

    /// Builds the configured [`Presenter`](Self::Presenter).
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if the configuration is invalid (see the backend crate's own error
    /// type for the exact conditions: missing font, mismatched glyph sizes, zero scale/grid, an
    /// oversized surface, or a tileset that failed to load).
    fn build_presenter(self) -> Result<Self::Presenter, Self::Error>;
}
