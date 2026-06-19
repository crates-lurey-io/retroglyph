//! Configuration, error, and builder types for the software rendering backend.

use super::bitmap_font::BitmapFont;
use std::fmt;

/// Errors that can occur when initializing or running the software backend.
#[derive(Debug)]
pub enum SoftwareBackendError {
    /// Failed to create the OS window.
    WindowCreation(winit::error::OsError),
    /// The winit event loop failed.
    EventLoop(winit::error::EventLoopError),
    /// The softbuffer surface returned an error.
    Softbuffer(softbuffer::SoftBufferError),
    /// No font was provided and the `software-default-font` feature is not enabled.
    NoFont,
}

impl fmt::Display for SoftwareBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowCreation(e) => write!(f, "window creation failed: {e}"),
            Self::EventLoop(e) => write!(f, "event loop error: {e}"),
            Self::Softbuffer(e) => write!(f, "softbuffer error: {e}"),
            Self::NoFont => write!(
                f,
                "no bitmap font provided; supply one via \
                 SoftwareBackendBuilder::font() or enable the \
                 `software-default-font` feature"
            ),
        }
    }
}

impl std::error::Error for SoftwareBackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::WindowCreation(e) => Some(e),
            Self::EventLoop(e) => Some(e),
            Self::Softbuffer(e) => Some(e),
            Self::NoFont => None,
        }
    }
}

/// Configuration for the software rendering backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareBackendOptions {
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
}

impl Default for SoftwareBackendOptions {
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
        }
    }
}

/// Builder for [`super::SoftwareBackend`].
///
/// # Examples
///
/// ```ignore
/// use rg::backend::software::SoftwareBackendBuilder;
///
/// // With the `software-default-font` feature the embedded VGA 8×16 font is
/// // used automatically.  To supply your own 8×16 bitmap font:
/// //
/// //   use rg::backend::software::bitmap_font::BitmapFont;
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
    options: SoftwareBackendOptions,
}

impl SoftwareBackendBuilder {
    /// Creates a builder with default options.
    ///
    /// When the `software-default-font` feature is enabled the IBM VGA 8×16
    /// font is pre-selected; otherwise you must call [`font`](Self::font).
    #[must_use]
    pub fn new() -> Self {
        Self {
            options: SoftwareBackendOptions::default(),
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

    /// Validates options and returns the backend configuration.
    ///
    /// Call [`SoftwareBackend::run`](super::SoftwareBackend::run) on the
    /// result to open the window.
    ///
    /// # Errors
    ///
    /// Returns [`SoftwareBackendError::NoFont`] if no font was set and the
    /// `software-default-font` feature is not enabled.
    pub fn build(self) -> Result<super::SoftwareBackend, SoftwareBackendError> {
        super::SoftwareBackend::new(self.options)
    }
}

impl Default for SoftwareBackendBuilder {
    fn default() -> Self {
        Self::new()
    }
}
