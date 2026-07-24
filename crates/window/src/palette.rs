//! Canonical default colors shared by the graphical backends.
//!
//! `retroglyph-core`'s [`Color::resolve_rgb`](retroglyph_core::color::Color::resolve_rgb) already
//! resolves palette/RGB colors to `(r, g, b)`; what it can't supply is the value a
//! [`Color::Default`](retroglyph_core::color::Color::Default) foreground/background should fall
//! back to, which is a rendering-policy choice, not a core color-model one. Both graphical backends
//! (`retroglyph-gl`, `retroglyph-software`) resolve `Color::Default` to the same pair, so it lives
//! here once instead of being re-hardcoded (in two different representations) per backend, where it
//! could silently drift.

/// Foreground for [`Color::Default`](retroglyph_core::color::Color::Default): a light grey,
/// matching a typical terminal's default text color.
pub const DEFAULT_FG: (u8, u8, u8) = (0xD4, 0xD4, 0xD4);

/// Background for [`Color::Default`](retroglyph_core::color::Color::Default): black.
pub const DEFAULT_BG: (u8, u8, u8) = (0x00, 0x00, 0x00);
