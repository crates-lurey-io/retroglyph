//! Bitmap glyph fonts and CP437 mapping, shared by retroglyph's graphical backends.
//!
//! A [`BitmapFont`] holds a static 1-bit-per-pixel glyph table. Each glyph is stored as
//! `glyph_height` bytes, one byte per row, MSB = leftmost pixel. For the standard 8-pixel-wide
//! VGA format one byte covers all 8 pixels of a row; wider fonts would need two bytes per row,
//! but that is not yet supported.
//!
//! This module is the dependency-free glyph-source layer both `retroglyph-software` (CPU
//! rasterizer) and `retroglyph-gl` (GPU atlas) build on, so their text output stays
//! pixel-identical. It lives here (rather than in a standalone crate) because both consumers
//! already depend on `retroglyph-window` for [`Presenter`](crate::presenter::Presenter), and it needs none of
//! winit: it is available with `default-features = false`. Enable the `default-font` feature for
//! the embedded Unscii 16 font ([`unscii16::FONT`]); leave it off to supply your own via
//! [`BitmapFont::new`].
//!
//! # Future work
//!
//! - **Expanded glyph cache:** For wider fonts (>8px), consider pre-computing expanded scanlines
//!   to avoid per-frame bit extraction. Currently the bit extraction loop is not a bottleneck
//!   for 8px-wide fonts at typical grid sizes, but wider fonts (10px, 16px) would benefit from
//!   caching.
//!
//! - **Wider glyphs:** To support glyphs wider than 8px, change [`BitmapFont::rows`] to return
//!   `ceil(glyph_width / 8)` bytes per row and update the consumers' bit extraction to index
//!   across bytes. Tracked in retroglyph issue #164; deferred until a second, non-8px-wide font
//!   is actually needed.

// ── BitmapFont ─────────────────────────────────────────────────────────────

/// A 1-bit-per-pixel bitmap glyph font.
///
/// `Copy` because it is just a static reference plus a few small fields.
#[derive(Debug, Clone, Copy)]
pub struct BitmapFont {
    /// Glyph bitmap data: `glyph_count * glyph_height` bytes.
    data: &'static [u8],
    /// Width of each glyph in pixels (≤ 8 for single-byte rows).
    glyph_width: u8,
    /// Height of each glyph in pixels; also bytes per glyph.
    glyph_height: u8,
    /// Total number of glyphs stored in `data`.
    glyph_count: u16,
    /// The `char` -> glyph-index table used by [`glyph_index`](Self::glyph_index), or `None` to
    /// use the built-in CP437 mapping.
    ///
    /// A font built with [`with_charset`](Self::with_charset) declares its own repertoire
    /// instead of being routed through the CP437 table every other font shares: this is what
    /// lets a [`FontChain`] extend coverage past CP437 (e.g. quadrants, sextants, braille)
    /// rather than every font in the chain answering the identical CP437 question.
    charset: Option<&'static [(char, u8)]>,
}

impl BitmapFont {
    /// Constructs a bitmap font from a static byte slice, mapped through the built-in CP437
    /// `char` encoding.
    ///
    /// `data` must contain exactly `glyph_count * glyph_height` bytes.
    #[must_use]
    pub const fn new(
        data: &'static [u8],
        glyph_width: u8,
        glyph_height: u8,
        glyph_count: u16,
    ) -> Self {
        Self {
            data,
            glyph_width,
            glyph_height,
            glyph_count,
            charset: None,
        }
    }

    /// Constructs a bitmap font from a static byte slice, mapped through an explicit
    /// `char` -> glyph-index `charset` instead of the built-in CP437 encoding.
    ///
    /// This is how a font extends coverage past CP437: [`glyph_index`](Self::glyph_index) looks a
    /// `char` up in `charset` instead of the CP437 table, so a font built this way can answer for
    /// codepoints (quadrants, sextants, braille, ...) that CP437 has no mapping for at all.
    /// `data` must contain exactly `glyph_count * glyph_height` bytes.
    ///
    /// `charset` is scanned linearly, so it is meant for the focused repertoire a font actually
    /// declares (a few dozen block or marker glyphs), not for a second general-purpose encoding
    /// table.
    #[must_use]
    pub const fn with_charset(
        data: &'static [u8],
        glyph_width: u8,
        glyph_height: u8,
        glyph_count: u16,
        charset: &'static [(char, u8)],
    ) -> Self {
        Self {
            data,
            glyph_width,
            glyph_height,
            glyph_count,
            charset: Some(charset),
        }
    }

    /// Returns the row bytes for glyph `index`.
    ///
    /// Each byte is one row; bit 7 (MSB) is the leftmost pixel.
    ///
    /// # Panics
    ///
    /// Panics if `index as u16 >= self.glyph_count`.
    #[must_use]
    pub fn rows(&self, index: u8) -> &[u8] {
        assert!(
            u16::from(index) < self.glyph_count,
            "glyph index {index} out of range ({})",
            self.glyph_count,
        );
        let h = usize::from(self.glyph_height);
        let start = usize::from(index) * h;
        &self.data[start..start + h]
    }

    /// Iterates the set ("on") pixels of glyph `index` as `(x, y)` coordinates, row-major from the
    /// top: `x` in `0..glyph_width`, `y` in `0..glyph_height`.
    ///
    /// This is the single place the 1-bit format's MSB-first bit order lives (pixel `x` of a row
    /// is bit `glyph_width - 1 - x` of that row's byte), so consumers (the GL atlas builder, the
    /// software rasterizer's glyph blit) decode through it instead of each re-deriving the shift
    /// and risking disagreement. It is also the one seam that has to change for wider-than-8px
    /// glyphs (multi-byte rows, #164): today a row is a single byte (`glyph_width <= 8`), so its
    /// bits are read straight out of that byte.
    ///
    /// A `glyph_width` above 8 is out of this format's contract (rows are one byte, so only bits
    /// 0..8 exist); it is clamped to 8 here rather than shifting past the byte's width, so columns
    /// 8.. of an oversized font are simply never yielded instead of panicking.
    ///
    /// # Panics
    ///
    /// Panics if `index as u16 >= self.glyph_count` (via [`rows`](Self::rows)).
    #[must_use = "iterators are lazy and do nothing unless consumed"]
    pub fn glyph_pixels(&self, index: u8) -> impl Iterator<Item = (u8, u8)> + '_ {
        let width = self.glyph_width.min(8);
        self.rows(index)
            .iter()
            .enumerate()
            .flat_map(move |(y, &row)| {
                #[allow(clippy::cast_possible_truncation)]
                let y = y as u8;
                (0..width)
                    .filter_map(move |x| ((row >> (width - 1 - x)) & 1 == 1).then_some((x, y)))
            })
    }

    /// The width of each glyph in pixels (≤ 8 for single-byte rows).
    #[must_use]
    pub const fn glyph_width(&self) -> u8 {
        self.glyph_width
    }

    /// The height of each glyph in pixels; also bytes per glyph.
    #[must_use]
    pub const fn glyph_height(&self) -> u8 {
        self.glyph_height
    }

    /// The total number of glyphs stored in this font.
    ///
    /// Glyph indices `0..glyph_count()` are valid arguments to [`rows`](Self::rows). A GPU
    /// backend uses this to size its glyph atlas (one texture-array layer per glyph).
    #[must_use]
    pub const fn glyph_count(&self) -> u16 {
        self.glyph_count
    }

    /// Maps a Unicode `char` to a glyph index in this font, or `None` if this font does not
    /// cover `ch`.
    ///
    /// If this font was built with [`with_charset`](Self::with_charset), `ch` is looked up in
    /// that explicit table; otherwise it goes through the built-in CP437 mapping. A miss is
    /// either `ch` not being in this font's repertoire at all, or its mapped index falling
    /// outside this font's `glyph_count` (e.g. a font built with fewer than 256 glyphs).
    ///
    /// A returned index is always `< glyph_count()`, so it is always a valid argument to
    /// [`rows`](Self::rows) and [`glyph_pixels`](Self::glyph_pixels).
    ///
    /// Substituting something drawable for a miss is [`FontChain::resolve`]'s job, not this
    /// one's: a font cannot answer for a character it has no glyph for, and pretending otherwise
    /// is what hides a chain's later fonts from ever being consulted.
    #[must_use]
    pub const fn glyph_index(&self, ch: char) -> Option<u8> {
        if let Some(table) = self.charset {
            let mut i = 0;
            while i < table.len() {
                let (table_ch, index) = table[i];
                if table_ch == ch && (index as u16) < self.glyph_count {
                    return Some(index);
                }
                i += 1;
            }
            return None;
        }
        match try_unicode_to_cp437(ch) {
            Some(index) if (index as u16) < self.glyph_count => Some(index),
            _ => None,
        }
    }
}

// Two `BitmapFont`s are equal when they point at the same static data and
// share the same dimensions.  Comparing the full 4 KB slice on every draw
// call would be wasteful, so we compare the data pointer instead.
impl PartialEq for BitmapFont {
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.data.as_ptr(), other.data.as_ptr())
            && self.glyph_width == other.glyph_width
            && self.glyph_height == other.glyph_height
            && self.glyph_count == other.glyph_count
    }
}

impl Eq for BitmapFont {}

// ── Font chain ──────────────────────────────────────────────────────────────

/// A glyph resolved from a [`FontChain`]: the glyph index plus the specific [`BitmapFont`] it
/// came from, since each font in a chain owns its own bitmap data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedGlyph {
    font: BitmapFont,
    font_index: usize,
    index: u8,
    notdef: bool,
}

impl ResolvedGlyph {
    /// The font this glyph was resolved from.
    #[must_use]
    pub const fn font(&self) -> BitmapFont {
        self.font
    }

    /// The position of [`font`](Self::font) within the chain that resolved it: `0` is the
    /// primary font, `1..` the fallbacks in order.
    ///
    /// A GPU backend packs every font in the chain into one atlas and addresses a glyph by a flat
    /// slot, so it needs the font's position (a stable index into [`FontChain::fonts`]) rather
    /// than the font value, which carries no identity of its own.
    #[must_use]
    pub const fn font_index(&self) -> usize {
        self.font_index
    }

    /// The glyph index within [`font`](Self::font), always `< font().glyph_count()`.
    #[must_use]
    pub const fn index(&self) -> u8 {
        self.index
    }

    /// Whether this is the substituted "no glyph" box rather than a glyph for the character that
    /// was asked for: `true` when no font in the chain covered that character and
    /// [`FontChain::resolve`] fell back to the solid block.
    #[must_use]
    pub const fn is_notdef(&self) -> bool {
        self.notdef
    }

    /// Returns the row bytes for this glyph; see [`BitmapFont::rows`].
    #[must_use]
    pub fn rows(&self) -> &[u8] {
        self.font.rows(self.index)
    }
}

/// The glyph source a backend draws from: a primary [`BitmapFont`] plus an ordered list of
/// fallback fonts.
///
/// This is the only character-to-glyph path the bundled pixel backends have. A single font is a
/// chain of one (`FontChain::from(font)`), so `SoftwareBackendBuilder::font` and
/// `GlBackendBuilder::font` both take an `impl Into<FontChain<'static>>` and there is no second,
/// chain-blind route that could quietly ignore a font's declared repertoire.
///
/// [`resolve`](Self::resolve) tries the primary font first, then each fallback in order, and only
/// if every font misses substitutes the solid block (`'█'`) from the first font in the chain that
/// has one. This lets a caller layer, say, an ASCII or partial-coverage primary font with one or
/// more broader fallback fonts, so a char missing from the primary doesn't automatically become a
/// solid block if some other font in the chain actually has it.
///
/// This type ships **no bundled fallback font data**: every font in the chain, primary or
/// fallback, is supplied by the caller. Bundling a ready-to-use Latin-1/Extended or sub-cell
/// (quadrant/sextant/braille) fallback font is a natural follow-up now that this mechanism is
/// reachable end to end, but is out of scope here.
///
/// A fallback font only extends the chain's repertoire if it declares coverage for the
/// characters it is meant to answer for. A [`BitmapFont::new`] font is always resolved through
/// the built-in CP437 table, so stacking several CP437 fonts in a chain never reaches past CP437:
/// every font in the chain answers the identical question. To actually extend coverage (e.g.
/// quadrants, sextants, braille, none of which CP437 has a mapping for), build the fallback font
/// with [`BitmapFont::with_charset`] and an explicit table covering those codepoints. Until a
/// chain does, `retroglyph_core::symbols`'s `quantize_quadrant`/`quantize_sextant` glyphs render
/// as a solid block on the pixel backends; see those functions' docs.
///
/// # Examples
///
/// ```
/// use retroglyph_window::font::{BitmapFont, FontChain};
///
/// static ASCII: [u8; 128 * 16] = [0; 128 * 16];
/// static QUADRANTS: [u8; 3 * 16] = [0; 3 * 16];
/// const QUADRANT_CHARSET: [(char, u8); 3] = [('▘', 0), ('▝', 1), ('▖', 2)];
///
/// const PRIMARY: BitmapFont = BitmapFont::new(&ASCII, 8, 16, 128);
/// const SUBCELL: BitmapFont = BitmapFont::with_charset(&QUADRANTS, 8, 16, 3, &QUADRANT_CHARSET);
/// static FALLBACKS: [BitmapFont; 1] = [SUBCELL];
///
/// let chain = FontChain::new(PRIMARY, &FALLBACKS);
/// let quadrant = chain.resolve('▘').expect("covered by the fallback font");
/// assert_eq!(quadrant.font_index(), 1);
/// assert!(!quadrant.is_notdef());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontChain<'a> {
    primary: BitmapFont,
    fallbacks: &'a [BitmapFont],
}

impl From<BitmapFont> for FontChain<'static> {
    fn from(font: BitmapFont) -> Self {
        Self::new(font, &[])
    }
}

impl<'a> FontChain<'a> {
    /// Constructs a chain from a primary font and an ordered list of fallback fonts.
    #[must_use]
    pub const fn new(primary: BitmapFont, fallbacks: &'a [BitmapFont]) -> Self {
        Self { primary, fallbacks }
    }

    /// The fonts in resolution order: the primary font first, then each fallback.
    ///
    /// The position of a font in this iterator is its [`ResolvedGlyph::font_index`].
    pub fn fonts(&self) -> impl Iterator<Item = &BitmapFont> {
        core::iter::once(&self.primary).chain(self.fallbacks.iter())
    }

    /// The number of fonts in the chain (always at least one).
    #[must_use]
    pub const fn font_count(&self) -> usize {
        1 + self.fallbacks.len()
    }

    /// The glyph cell size (`(width, height)` in unscaled pixels) shared by every font in the
    /// chain, or `None` if the fonts disagree.
    ///
    /// A grid has one cell size, so a chain whose fonts don't agree on theirs has no single
    /// answer for how big a cell is; backends reject such a chain at build time rather than
    /// picking one font's size and letting the others overflow or under-fill their cells.
    #[must_use]
    pub fn glyph_size(&self) -> Option<(u8, u8)> {
        let size = (self.primary.glyph_width, self.primary.glyph_height);
        self.fallbacks
            .iter()
            .all(|f| (f.glyph_width, f.glyph_height) == size)
            .then_some(size)
    }

    /// Resolves `ch` to a drawable glyph, trying the primary font first, then each fallback font
    /// in order.
    ///
    /// If no font covers `ch`, this substitutes the solid block (`'█'`) from the first font in
    /// the chain that covers *it*, flagged as [`ResolvedGlyph::is_notdef`]. `None` means the
    /// chain cannot draw `ch` at all, not even a substitute box, and the caller should draw
    /// nothing: a chain of narrow `with_charset` fonts (say, braille only) legitimately has no
    /// solid block to fall back to.
    ///
    /// A returned glyph is always in range for its font, so [`ResolvedGlyph::rows`] and
    /// [`BitmapFont::glyph_pixels`] cannot panic on it.
    #[must_use]
    pub fn resolve(&self, ch: char) -> Option<ResolvedGlyph> {
        self.lookup(ch, false).or_else(|| self.lookup(NOTDEF, true))
    }

    /// The first font in the chain covering `ch`, tagged with `notdef`.
    fn lookup(&self, ch: char, notdef: bool) -> Option<ResolvedGlyph> {
        self.fonts()
            .enumerate()
            .find_map(|(font_index, font)| {
                font.glyph_index(ch).map(|index| (font_index, font, index))
            })
            .map(|(font_index, font, index)| ResolvedGlyph {
                font: *font,
                font_index,
                index,
                notdef,
            })
    }
}

// ── Default embedded font ──────────────────────────────────────────────────

/// The Unscii 16 font, embedded when the `default-font` feature is enabled.
///
/// 256 glyphs laid out in CP437 order (matching `unicode_to_cp437`), each
/// 16 bytes (1 bit per pixel, MSB = leftmost). Source: unscii's
/// public-domain/CC0 `unscii-16.hex` (<https://github.com/viznut/unscii>),
/// re-laid-out from Unicode codepoints to CP437 glyph indices.
///
/// Four CP437 codepoints that plain `unscii-16.hex` doesn't cover (U+2302
/// HOUSE, U+263C WHITE SUN WITH RAYS, U+2310 REVERSED NOT SIGN, U+2219
/// BULLET OPERATOR) are filled in with original pixel art or mechanical
/// transforms of neighboring unscii glyphs (e.g. REVERSED NOT SIGN is a
/// horizontal mirror of unscii's own NOT SIGN) rather than pulling in
/// unscii's GPL-licensed `-full` variant (which adds GNU Unifont glyphs).
/// `unicode_to_cp437` carries matching reverse-mapping arms for all four
/// (`☼` already had one; the `⌂`/`⌐`/`∙` arms are new here) so all four
/// are reachable through the normal char-to-glyph path, not just by raw
/// glyph index.
#[cfg(feature = "default-font")]
pub mod unscii16 {
    use super::BitmapFont;

    /// A [`BitmapFont`] backed by the embedded Unscii 16 glyph data.
    pub const FONT: BitmapFont = BitmapFont::new(&DATA, 8, 16, 256);

    /// Unscii 16 glyph bitmaps: 256 CP437-ordered glyphs, 16 bytes each.
    ///
    /// Each byte is one row of 8 pixels; bit 7 (MSB) is the leftmost pixel.
    #[rustfmt::skip]
    static DATA: [u8; 4096] = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x00
        0x00, 0x00, 0x7e, 0x81, 0x81, 0xa5, 0x81, 0x81, 0xbd, 0x99, 0x81, 0x81, 0x7e, 0x00, 0x00, 0x00, // 0x01
        0x00, 0x00, 0x7e, 0xff, 0xff, 0xdb, 0xff, 0xff, 0xc3, 0xe7, 0xff, 0xff, 0x7e, 0x00, 0x00, 0x00, // 0x02
        0x00, 0x00, 0x00, 0x6c, 0xfe, 0xfe, 0xfe, 0xfe, 0x7c, 0x7c, 0x38, 0x10, 0x00, 0x00, 0x00, 0x00, // 0x03
        0x00, 0x00, 0x00, 0x10, 0x38, 0x38, 0x7c, 0xfe, 0x7c, 0x38, 0x38, 0x10, 0x00, 0x00, 0x00, 0x00, // 0x04
        0x00, 0x00, 0x00, 0x10, 0x38, 0x38, 0x54, 0xfe, 0xfe, 0x54, 0x10, 0x38, 0x00, 0x00, 0x00, 0x00, // 0x05
        0x00, 0x00, 0x00, 0x10, 0x38, 0x7c, 0xfe, 0xfe, 0xfe, 0x38, 0x38, 0x7c, 0x00, 0x00, 0x00, 0x00, // 0x06
        0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x3c, 0x3c, 0x3c, 0x3c, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00, // 0x07
        0xff, 0xff, 0xff, 0xff, 0xe7, 0xe7, 0xc3, 0xc3, 0xc3, 0xc3, 0xe7, 0xe7, 0xff, 0xff, 0xff, 0xff, // 0x08
        0x00, 0x00, 0x3c, 0x3c, 0x66, 0x66, 0x42, 0x42, 0x42, 0x42, 0x66, 0x66, 0x3c, 0x3c, 0x00, 0x00, // 0x09
        0xff, 0xff, 0xc3, 0xc3, 0x99, 0x99, 0xbd, 0xbd, 0xbd, 0xbd, 0x99, 0x99, 0xc3, 0xc3, 0xff, 0xff, // 0x0a
        0x00, 0x00, 0x00, 0x1e, 0x0e, 0x1a, 0x78, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0x78, 0x00, 0x00, 0x00, // 0x0b
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x18, 0x7e, 0x18, // 0x0c
        0x00, 0x00, 0x00, 0x18, 0x1c, 0x1e, 0x1b, 0x18, 0x18, 0x78, 0xf8, 0x70, 0x00, 0x00, 0x00, 0x00, // 0x0d
        0x00, 0x00, 0x00, 0x7f, 0x63, 0x63, 0x63, 0x63, 0x63, 0x67, 0xe7, 0xe6, 0xc0, 0x00, 0x00, 0x00, // 0x0e
        0x00, 0x00, 0x00, 0x24, 0x18, 0xbd, 0x7e, 0x7e, 0xbd, 0x18, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x0f
        0x00, 0x00, 0x00, 0x00, 0xc0, 0xf0, 0xfc, 0xff, 0xfc, 0xf0, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x10
        0x00, 0x00, 0x00, 0x00, 0x03, 0x0f, 0x3f, 0xff, 0x3f, 0x0f, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x11
        0x18, 0x3c, 0x7e, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7e, 0x3c, 0x18, // 0x12
        0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00, 0x66, 0x66, 0x00, 0x00, // 0x13
        0x00, 0x00, 0x3e, 0x7a, 0x7a, 0x7a, 0x7a, 0x3a, 0x1a, 0x1a, 0x1a, 0x1a, 0x1a, 0x00, 0x00, 0x00, // 0x14
        0x00, 0x3c, 0x66, 0x60, 0x30, 0x38, 0x6c, 0x66, 0x36, 0x1c, 0x0c, 0x06, 0x66, 0x3c, 0x00, 0x00, // 0x15
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7e, 0x7e, 0x7e, 0x7e, 0x7e, 0x7e, 0x00, 0x00, // 0x16
        0x18, 0x3c, 0x7e, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7e, 0x3c, 0x18, 0xff, // 0x17
        0x18, 0x3c, 0x7e, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0x18
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7e, 0x3c, 0x18, // 0x19
        0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x0c, 0xfe, 0xfe, 0x0c, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x1a
        0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x30, 0x7f, 0x7f, 0x30, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x1b
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xfe, 0x06, 0x06, 0x06, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x1c
        0x00, 0x00, 0x00, 0x00, 0x00, 0x24, 0x66, 0xff, 0xff, 0x66, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x1d
        0x18, 0x18, 0x18, 0x18, 0x3c, 0x3c, 0x3c, 0x3c, 0x7e, 0x7e, 0x7e, 0x7e, 0xff, 0xff, 0xff, 0xff, // 0x1e
        0xff, 0xff, 0xff, 0xff, 0x7e, 0x7e, 0x7e, 0x7e, 0x3c, 0x3c, 0x3c, 0x3c, 0x18, 0x18, 0x18, 0x18, // 0x1f
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x20
        0x00, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00, // 0x21
        0x00, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x22
        0x00, 0x00, 0x6c, 0x6c, 0x6c, 0xfe, 0x6c, 0x6c, 0x6c, 0xfe, 0x6c, 0x6c, 0x6c, 0x00, 0x00, 0x00, // 0x23
        0x00, 0x18, 0x18, 0x3c, 0x66, 0x60, 0x30, 0x18, 0x0c, 0x06, 0x66, 0x3c, 0x18, 0x18, 0x00, 0x00, // 0x24
        0x00, 0x00, 0x06, 0xc6, 0xcc, 0xcc, 0x18, 0x18, 0x30, 0x30, 0x66, 0x66, 0xc6, 0xc0, 0x00, 0x00, // 0x25
        0x00, 0x00, 0x38, 0x6c, 0x6c, 0x38, 0x30, 0x7a, 0xde, 0xcc, 0xcc, 0xcc, 0x76, 0x00, 0x00, 0x00, // 0x26
        0x00, 0x18, 0x18, 0x18, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x27
        0x00, 0x0c, 0x18, 0x18, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x18, 0x18, 0x0c, 0x00, 0x00, // 0x28
        0x00, 0x30, 0x18, 0x18, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x18, 0x18, 0x30, 0x00, 0x00, // 0x29
        0x00, 0x00, 0x00, 0x00, 0x66, 0x66, 0x3c, 0xff, 0x3c, 0x66, 0x66, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x2a
        0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x18, 0x7e, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x2b
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x38, 0x18, 0x18, 0x30, 0x60, 0x00, // 0x2c
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x2d
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, // 0x2e
        0x03, 0x03, 0x06, 0x06, 0x0c, 0x0c, 0x18, 0x18, 0x30, 0x30, 0x60, 0x60, 0xc0, 0xc0, 0x00, 0x00, // 0x2f
        0x00, 0x00, 0x38, 0x6c, 0xc6, 0xc6, 0xce, 0xd6, 0xe6, 0xc6, 0xc6, 0x6c, 0x38, 0x00, 0x00, 0x00, // 0x30
        0x00, 0x00, 0x18, 0x38, 0x78, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7e, 0x00, 0x00, 0x00, // 0x31
        0x00, 0x00, 0x3c, 0x66, 0x66, 0x06, 0x06, 0x0c, 0x18, 0x30, 0x60, 0x60, 0x7e, 0x00, 0x00, 0x00, // 0x32
        0x00, 0x00, 0x3c, 0x66, 0x66, 0x06, 0x06, 0x1c, 0x06, 0x06, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x33
        0x00, 0x00, 0x0c, 0x1c, 0x3c, 0x6c, 0xcc, 0xcc, 0xfe, 0x0c, 0x0c, 0x0c, 0x0c, 0x00, 0x00, 0x00, // 0x34
        0x00, 0x00, 0x7e, 0x60, 0x60, 0x60, 0x7c, 0x06, 0x06, 0x06, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x35
        0x00, 0x00, 0x1c, 0x30, 0x60, 0x60, 0x7c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x36
        0x00, 0x00, 0x7e, 0x06, 0x06, 0x06, 0x0c, 0x0c, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, // 0x37
        0x00, 0x00, 0x3c, 0x66, 0x66, 0x66, 0x76, 0x3c, 0x6e, 0x66, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x38
        0x00, 0x00, 0x3c, 0x66, 0x66, 0x66, 0x66, 0x3e, 0x06, 0x06, 0x06, 0x0c, 0x38, 0x00, 0x00, 0x00, // 0x39
        0x00, 0x00, 0x00, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, // 0x3a
        0x00, 0x00, 0x00, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00, 0x38, 0x18, 0x18, 0x30, 0x60, 0x00, // 0x3b
        0x00, 0x00, 0x00, 0x06, 0x0c, 0x18, 0x30, 0x60, 0x30, 0x18, 0x0c, 0x06, 0x00, 0x00, 0x00, 0x00, // 0x3c
        0x00, 0x00, 0x00, 0x00, 0x00, 0x7e, 0x00, 0x00, 0x00, 0x7e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x3d
        0x00, 0x00, 0x00, 0x60, 0x30, 0x18, 0x0c, 0x06, 0x0c, 0x18, 0x30, 0x60, 0x00, 0x00, 0x00, 0x00, // 0x3e
        0x00, 0x3c, 0x66, 0x66, 0x06, 0x0c, 0x18, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00, // 0x3f
        0x00, 0x00, 0x7c, 0xc6, 0xc6, 0xc6, 0xde, 0xde, 0xde, 0xdc, 0xc0, 0xc0, 0x7c, 0x00, 0x00, 0x00, // 0x40
        0x00, 0x00, 0x18, 0x3c, 0x66, 0x66, 0x66, 0x7e, 0x66, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00, // 0x41
        0x00, 0x00, 0x7c, 0x66, 0x66, 0x66, 0x6c, 0x78, 0x6c, 0x66, 0x66, 0x66, 0x7c, 0x00, 0x00, 0x00, // 0x42
        0x00, 0x00, 0x3c, 0x66, 0x66, 0x60, 0x60, 0x60, 0x60, 0x60, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x43
        0x00, 0x00, 0x78, 0x6c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x6c, 0x78, 0x00, 0x00, 0x00, // 0x44
        0x00, 0x00, 0x7e, 0x60, 0x60, 0x60, 0x60, 0x7c, 0x60, 0x60, 0x60, 0x60, 0x7e, 0x00, 0x00, 0x00, // 0x45
        0x00, 0x00, 0x7e, 0x60, 0x60, 0x60, 0x7c, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x00, 0x00, 0x00, // 0x46
        0x00, 0x00, 0x3c, 0x66, 0x66, 0x60, 0x60, 0x6e, 0x66, 0x66, 0x66, 0x66, 0x3e, 0x00, 0x00, 0x00, // 0x47
        0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x7e, 0x66, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00, // 0x48
        0x00, 0x00, 0x7e, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7e, 0x00, 0x00, 0x00, // 0x49
        0x00, 0x00, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x4a
        0x00, 0x00, 0xc6, 0xc6, 0xcc, 0xcc, 0xd8, 0xf0, 0xd8, 0xcc, 0xcc, 0xc6, 0xc6, 0x00, 0x00, 0x00, // 0x4b
        0x00, 0x00, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x7e, 0x00, 0x00, 0x00, // 0x4c
        0x00, 0x00, 0xc6, 0xee, 0xee, 0xfe, 0xd6, 0xd6, 0xc6, 0xc6, 0xc6, 0xc6, 0xc6, 0x00, 0x00, 0x00, // 0x4d
        0x00, 0x00, 0xc6, 0xc6, 0xe6, 0xe6, 0xf6, 0xfe, 0xde, 0xce, 0xce, 0xc6, 0xc6, 0x00, 0x00, 0x00, // 0x4e
        0x00, 0x00, 0x3c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x4f
        0x00, 0x00, 0x7c, 0x66, 0x66, 0x66, 0x66, 0x7c, 0x60, 0x60, 0x60, 0x60, 0x60, 0x00, 0x00, 0x00, // 0x50
        0x00, 0x00, 0x3c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x0c, 0x06, 0x00, // 0x51
        0x00, 0x00, 0x7c, 0x66, 0x66, 0x66, 0x66, 0x7c, 0x6c, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00, // 0x52
        0x00, 0x00, 0x3c, 0x66, 0x66, 0x60, 0x30, 0x18, 0x0c, 0x06, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x53
        0x00, 0x00, 0x7e, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, // 0x54
        0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x55
        0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x3c, 0x18, 0x18, 0x00, 0x00, 0x00, // 0x56
        0x00, 0x00, 0xc6, 0xc6, 0xc6, 0xc6, 0xc6, 0xd6, 0xd6, 0xfe, 0xee, 0xee, 0xc6, 0x00, 0x00, 0x00, // 0x57
        0x00, 0x00, 0xc3, 0xc3, 0x66, 0x3c, 0x18, 0x18, 0x18, 0x3c, 0x66, 0xc3, 0xc3, 0x00, 0x00, 0x00, // 0x58
        0x00, 0x00, 0xc3, 0xc3, 0x66, 0x66, 0x3c, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, // 0x59
        0x00, 0x00, 0x7e, 0x06, 0x06, 0x0c, 0x0c, 0x18, 0x30, 0x30, 0x60, 0x60, 0x7e, 0x00, 0x00, 0x00, // 0x5a
        0x00, 0x3c, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3c, 0x00, 0x00, // 0x5b
        0xc0, 0xc0, 0x60, 0x60, 0x30, 0x30, 0x18, 0x18, 0x0c, 0x0c, 0x06, 0x06, 0x03, 0x03, 0x00, 0x00, // 0x5c
        0x00, 0x3c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x3c, 0x00, 0x00, // 0x5d
        0x00, 0x10, 0x38, 0x6c, 0x6c, 0xc6, 0xc6, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x5e
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, // 0x5f
        0x00, 0x18, 0x18, 0x0c, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x60
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3c, 0x06, 0x3e, 0x66, 0x66, 0x66, 0x3e, 0x00, 0x00, 0x00, // 0x61
        0x00, 0x00, 0x60, 0x60, 0x60, 0x60, 0x7c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x7c, 0x00, 0x00, 0x00, // 0x62
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3c, 0x66, 0x60, 0x60, 0x60, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x63
        0x00, 0x00, 0x06, 0x06, 0x06, 0x06, 0x3e, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3e, 0x00, 0x00, 0x00, // 0x64
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3c, 0x66, 0x66, 0x7e, 0x60, 0x60, 0x3c, 0x00, 0x00, 0x00, // 0x65
        0x00, 0x00, 0x1e, 0x30, 0x30, 0x30, 0x7e, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x00, 0x00, 0x00, // 0x66
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3e, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3e, 0x06, 0x06, 0x7c, // 0x67
        0x00, 0x00, 0x60, 0x60, 0x60, 0x60, 0x7c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00, // 0x68
        0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x78, 0x18, 0x18, 0x18, 0x18, 0x18, 0x1e, 0x00, 0x00, 0x00, // 0x69
        0x00, 0x00, 0x0c, 0x0c, 0x00, 0x00, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x78, // 0x6a
        0x00, 0x00, 0x60, 0x60, 0x60, 0x60, 0x66, 0x66, 0x6c, 0x78, 0x6c, 0x66, 0x66, 0x00, 0x00, 0x00, // 0x6b
        0x00, 0x00, 0x78, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x1e, 0x00, 0x00, 0x00, // 0x6c
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xcc, 0xfe, 0xd6, 0xd6, 0xd6, 0xd6, 0xc6, 0x00, 0x00, 0x00, // 0x6d
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00, // 0x6e
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x6f
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x7c, 0x60, 0x60, 0x60, // 0x70
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3e, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3e, 0x06, 0x06, 0x06, // 0x71
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7c, 0x66, 0x66, 0x60, 0x60, 0x60, 0x60, 0x00, 0x00, 0x00, // 0x72
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3e, 0x60, 0x60, 0x3c, 0x06, 0x06, 0x7c, 0x00, 0x00, 0x00, // 0x73
        0x00, 0x00, 0x00, 0x30, 0x30, 0x30, 0x7e, 0x30, 0x30, 0x30, 0x30, 0x30, 0x1e, 0x00, 0x00, 0x00, // 0x74
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3e, 0x00, 0x00, 0x00, // 0x75
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x18, 0x00, 0x00, 0x00, // 0x76
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc6, 0xc6, 0xd6, 0xd6, 0xd6, 0x7c, 0x6c, 0x00, 0x00, 0x00, // 0x77
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc6, 0xc6, 0x6c, 0x38, 0x6c, 0xc6, 0xc6, 0x00, 0x00, 0x00, // 0x78
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3e, 0x06, 0x06, 0x3c, // 0x79
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7e, 0x06, 0x0c, 0x18, 0x30, 0x60, 0x7e, 0x00, 0x00, 0x00, // 0x7a
        0x00, 0x0e, 0x18, 0x18, 0x18, 0x18, 0x18, 0xf0, 0x18, 0x18, 0x18, 0x18, 0x18, 0x0e, 0x00, 0x00, // 0x7b
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, // 0x7c
        0x00, 0xe0, 0x30, 0x30, 0x30, 0x30, 0x30, 0x1e, 0x30, 0x30, 0x30, 0x30, 0x30, 0xe0, 0x00, 0x00, // 0x7d
        0x00, 0x72, 0xd6, 0x9c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x7e
        0x00, 0x18, 0x3c, 0x7e, 0xff, 0xc3, 0xc3, 0xc3, 0xdb, 0xdb, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x7f
        0x00, 0x00, 0x3c, 0x66, 0x66, 0x60, 0x60, 0x60, 0x60, 0x60, 0x66, 0x66, 0x3c, 0x0c, 0x06, 0x1c, // 0x80
        0x00, 0x00, 0x66, 0x66, 0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3e, 0x00, 0x00, 0x00, // 0x81
        0x00, 0x00, 0x0c, 0x18, 0x00, 0x00, 0x3c, 0x66, 0x66, 0x7e, 0x60, 0x60, 0x3c, 0x00, 0x00, 0x00, // 0x82
        0x00, 0x18, 0x3c, 0x66, 0x00, 0x00, 0x3c, 0x06, 0x3e, 0x66, 0x66, 0x66, 0x3e, 0x00, 0x00, 0x00, // 0x83
        0x00, 0x00, 0x66, 0x66, 0x00, 0x00, 0x3c, 0x06, 0x3e, 0x66, 0x66, 0x66, 0x3e, 0x00, 0x00, 0x00, // 0x84
        0x00, 0x00, 0x30, 0x18, 0x00, 0x00, 0x3c, 0x06, 0x3e, 0x66, 0x66, 0x66, 0x3e, 0x00, 0x00, 0x00, // 0x85
        0x00, 0x00, 0x3c, 0x66, 0x3c, 0x00, 0x3c, 0x06, 0x3e, 0x66, 0x66, 0x66, 0x3e, 0x00, 0x00, 0x00, // 0x86
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3c, 0x66, 0x60, 0x60, 0x60, 0x66, 0x3c, 0x0c, 0x06, 0x1c, // 0x87
        0x00, 0x18, 0x3c, 0x66, 0x00, 0x00, 0x3c, 0x66, 0x66, 0x7e, 0x60, 0x60, 0x3c, 0x00, 0x00, 0x00, // 0x88
        0x00, 0x00, 0x66, 0x66, 0x00, 0x00, 0x3c, 0x66, 0x66, 0x7e, 0x60, 0x60, 0x3c, 0x00, 0x00, 0x00, // 0x89
        0x00, 0x00, 0x30, 0x18, 0x00, 0x00, 0x3c, 0x66, 0x66, 0x7e, 0x60, 0x60, 0x3c, 0x00, 0x00, 0x00, // 0x8a
        0x00, 0x00, 0x66, 0x66, 0x00, 0x00, 0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3c, 0x00, 0x00, 0x00, // 0x8b
        0x00, 0x18, 0x3c, 0x66, 0x00, 0x00, 0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3c, 0x00, 0x00, 0x00, // 0x8c
        0x00, 0x00, 0x30, 0x18, 0x00, 0x00, 0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3c, 0x00, 0x00, 0x00, // 0x8d
        0x66, 0x66, 0x00, 0x18, 0x3c, 0x66, 0x66, 0x66, 0x7e, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00, // 0x8e
        0x3c, 0x66, 0x3c, 0x00, 0x18, 0x3c, 0x66, 0x66, 0x7e, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00, // 0x8f
        0x0c, 0x18, 0x00, 0x7e, 0x60, 0x60, 0x60, 0x7c, 0x60, 0x60, 0x60, 0x60, 0x7e, 0x00, 0x00, 0x00, // 0x90
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7e, 0x1b, 0x1b, 0x7f, 0xd8, 0xd8, 0x77, 0x00, 0x00, 0x00, // 0x91
        0x00, 0x00, 0x3f, 0x7c, 0xfc, 0xcc, 0xcc, 0xfe, 0xcc, 0xcc, 0xcc, 0xcc, 0xcf, 0x00, 0x00, 0x00, // 0x92
        0x00, 0x18, 0x3c, 0x66, 0x00, 0x00, 0x3c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x93
        0x00, 0x00, 0x66, 0x66, 0x00, 0x00, 0x3c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x94
        0x00, 0x00, 0x30, 0x18, 0x00, 0x00, 0x3c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x95
        0x00, 0x18, 0x3c, 0x66, 0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3e, 0x00, 0x00, 0x00, // 0x96
        0x00, 0x00, 0x30, 0x18, 0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3e, 0x00, 0x00, 0x00, // 0x97
        0x00, 0x00, 0x66, 0x66, 0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3e, 0x06, 0x06, 0x3c, // 0x98
        0x66, 0x66, 0x00, 0x3c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x99
        0x66, 0x66, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0x9a
        0x00, 0x18, 0x18, 0x18, 0x3c, 0x66, 0x60, 0x60, 0x66, 0x3c, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, // 0x9b
        0x00, 0x00, 0x38, 0x6c, 0x6c, 0x60, 0x60, 0xf0, 0x60, 0x60, 0x66, 0x66, 0xfc, 0x00, 0x00, 0x00, // 0x9c
        0x00, 0x00, 0xc3, 0xc3, 0x66, 0x66, 0x3c, 0x18, 0x7e, 0x18, 0x7e, 0x18, 0x18, 0x00, 0x00, 0x00, // 0x9d
        0x00, 0xfc, 0x66, 0x66, 0x7c, 0x62, 0x66, 0x6f, 0x66, 0x66, 0x66, 0xf3, 0x00, 0x00, 0x00, 0x00, // 0x9e
        0x00, 0x0e, 0x1b, 0x18, 0x18, 0x18, 0x18, 0x7e, 0x18, 0x18, 0x18, 0x18, 0xd8, 0x70, 0x00, 0x00, // 0x9f
        0x00, 0x00, 0x0c, 0x18, 0x00, 0x00, 0x3c, 0x06, 0x3e, 0x66, 0x66, 0x66, 0x3e, 0x00, 0x00, 0x00, // 0xa0
        0x00, 0x00, 0x0c, 0x18, 0x00, 0x00, 0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3c, 0x00, 0x00, 0x00, // 0xa1
        0x00, 0x00, 0x0c, 0x18, 0x00, 0x00, 0x3c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0xa2
        0x00, 0x00, 0x0c, 0x18, 0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3e, 0x00, 0x00, 0x00, // 0xa3
        0x00, 0x00, 0x76, 0xdc, 0x00, 0x00, 0x7c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00, // 0xa4
        0x76, 0xdc, 0x00, 0xc6, 0xc6, 0xe6, 0xf6, 0xde, 0xce, 0xc6, 0xc6, 0xc6, 0xc6, 0x00, 0x00, 0x00, // 0xa5
        0x00, 0x3c, 0x06, 0x06, 0x3e, 0x66, 0x66, 0x3e, 0x00, 0x7e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xa6
        0x00, 0x00, 0x00, 0x3c, 0x66, 0x66, 0x66, 0x3c, 0x00, 0x7e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xa7
        0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x30, 0x30, 0x60, 0x60, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, // 0xa8
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7f, 0x60, 0x60, 0x60, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xa9
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xfe, 0x06, 0x06, 0x06, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xaa
        0x00, 0x40, 0xc6, 0x46, 0x4c, 0x4c, 0x18, 0x18, 0x30, 0x30, 0x6c, 0x62, 0xc4, 0xc8, 0x0e, 0x00, // 0xab
        0x00, 0x40, 0xc6, 0x46, 0x4c, 0x4c, 0x18, 0x18, 0x30, 0x30, 0x62, 0x66, 0xca, 0xcf, 0x02, 0x00, // 0xac
        0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, // 0xad
        0x00, 0x00, 0x00, 0x00, 0x00, 0x33, 0x66, 0xcc, 0x66, 0x33, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xae
        0x00, 0x00, 0x00, 0x00, 0x00, 0xcc, 0x66, 0x33, 0x66, 0xcc, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xaf
        0x22, 0x88, 0x22, 0x88, 0x22, 0x88, 0x22, 0x88, 0x22, 0x88, 0x22, 0x88, 0x22, 0x88, 0x22, 0x88, // 0xb0
        0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, // 0xb1
        0xdd, 0x77, 0xdd, 0x77, 0xdd, 0x77, 0xdd, 0x77, 0xdd, 0x77, 0xdd, 0x77, 0xdd, 0x77, 0xdd, 0x77, // 0xb2
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0xb3
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0xf8, 0xf8, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0xb4
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0xf8, 0x18, 0xf8, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0xb5
        0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0xec, 0xec, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, // 0xb6
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xfc, 0xfc, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, // 0xb7
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf8, 0x18, 0xf8, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0xb8
        0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0xec, 0x0c, 0xec, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, // 0xb9
        0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, // 0xba
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xfc, 0x0c, 0xec, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, // 0xbb
        0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0xec, 0x0c, 0xfc, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xbc
        0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0xfc, 0xfc, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xbd
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0xf8, 0x18, 0xf8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xbe
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf8, 0xf8, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0xbf
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x1f, 0x1f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xc0
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xc1
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0xc2
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x1f, 0x1f, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0xc3
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xc4
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0xff, 0xff, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0xc5
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x1f, 0x18, 0x1f, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0xc6
        0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6f, 0x6f, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, // 0xc7
        0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6f, 0x60, 0x7f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xc8
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7f, 0x60, 0x6f, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, // 0xc9
        0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0xef, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xca
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0xef, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, // 0xcb
        0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6f, 0x60, 0x6f, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, // 0xcc
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xcd
        0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0xef, 0x00, 0xef, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, // 0xce
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0xff, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xcf
        0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xd0
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0xff, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0xd1
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, // 0xd2
        0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x7f, 0x7f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xd3
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x1f, 0x18, 0x1f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xd4
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1f, 0x18, 0x1f, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0xd5
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7f, 0x7f, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, // 0xd6
        0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0xff, 0xff, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, 0x6c, // 0xd7
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0xff, 0x18, 0xff, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0xd8
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0xf8, 0xf8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xd9
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1f, 0x1f, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0xda
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // 0xdb
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // 0xdc
        0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, // 0xdd
        0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, // 0xde
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xdf
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x76, 0xce, 0xc6, 0xc6, 0xc6, 0xce, 0x76, 0x00, 0x00, 0x00, // 0xe0
        0x00, 0x00, 0x78, 0xcc, 0xcc, 0xcc, 0xd8, 0xcc, 0xc6, 0xc6, 0xc6, 0xc6, 0xcc, 0x00, 0x00, 0x00, // 0xe1
        0x00, 0x00, 0x7e, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x00, 0x00, 0x00, // 0xe2
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00, // 0xe3
        0x00, 0x00, 0xfe, 0xc0, 0x60, 0x30, 0x18, 0x0c, 0x18, 0x30, 0x60, 0xc0, 0xfe, 0x00, 0x00, 0x00, // 0xe4
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7e, 0xcc, 0xc6, 0xc6, 0xc6, 0xc6, 0x7c, 0x00, 0x00, 0x00, // 0xe5
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x7c, 0x60, 0x60, 0xc0, // 0xe6
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7e, 0x18, 0x18, 0x18, 0x18, 0x18, 0x0c, 0x00, 0x00, 0x00, // 0xe7
        0x10, 0x10, 0x10, 0x7c, 0xd6, 0xd6, 0xd6, 0xd6, 0xd6, 0x7c, 0x10, 0x10, 0x10, 0x00, 0x00, 0x00, // 0xe8
        0x00, 0x00, 0x7c, 0xc6, 0xc6, 0xc6, 0xc6, 0xfe, 0xc6, 0xc6, 0xc6, 0xc6, 0x7c, 0x00, 0x00, 0x00, // 0xe9
        0x00, 0x00, 0x7c, 0xc6, 0xc6, 0xc6, 0xc6, 0xc6, 0xc6, 0xee, 0x6c, 0x6c, 0xee, 0x00, 0x00, 0x00, // 0xea
        0x00, 0x00, 0xfe, 0xc0, 0xc0, 0x60, 0x30, 0x18, 0x7c, 0xc6, 0xc6, 0xc6, 0x7c, 0x00, 0x00, 0x00, // 0xeb
        0x00, 0x00, 0x00, 0x00, 0x00, 0x76, 0xdb, 0xdb, 0xdb, 0x6e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xec
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7c, 0xc0, 0xdc, 0xd6, 0xd6, 0xd6, 0x7c, 0x10, 0x10, 0x00, // 0xed
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3e, 0x60, 0x60, 0x3c, 0x60, 0x60, 0x3e, 0x00, 0x00, 0x00, // 0xee
        0x00, 0x00, 0x00, 0x3c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00, 0x00, // 0xef
        0x00, 0x00, 0x00, 0x00, 0x7e, 0x00, 0x00, 0x7e, 0x00, 0x00, 0x7e, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xf0
        0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x18, 0x7e, 0x18, 0x18, 0x18, 0x00, 0x7e, 0x00, 0x00, 0x00, // 0xf1
        0x00, 0x00, 0x00, 0x00, 0x30, 0x18, 0x0c, 0x06, 0x0c, 0x18, 0x30, 0x00, 0x7e, 0x00, 0x00, 0x00, // 0xf2
        0x00, 0x00, 0x00, 0x00, 0x0c, 0x18, 0x30, 0x60, 0x30, 0x18, 0x0c, 0x00, 0x7e, 0x00, 0x00, 0x00, // 0xf3
        0x00, 0x00, 0x0e, 0x1b, 0x1b, 0x1b, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // 0xf4
        0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0xd8, 0xd8, 0xd8, 0x70, 0x00, 0x00, 0x00, // 0xf5
        0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00, 0x7e, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xf6
        0x00, 0x00, 0x00, 0x00, 0x72, 0xd6, 0x9c, 0x00, 0x72, 0xd6, 0x9c, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xf7
        0x00, 0x3c, 0x66, 0x66, 0x3c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xf8
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xf9
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xfa
        0x00, 0x03, 0x03, 0x06, 0x06, 0x06, 0x06, 0x06, 0xcc, 0xcc, 0x6c, 0x38, 0x18, 0x00, 0x00, 0x00, // 0xfb
        0x00, 0x00, 0x00, 0x78, 0x6c, 0x6c, 0x6c, 0x6c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xfc
        0x00, 0x38, 0x6c, 0x0c, 0x18, 0x30, 0x60, 0x7c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xfd
        0x00, 0x00, 0x7e, 0x7e, 0x7e, 0x7e, 0x7e, 0x7e, 0x7e, 0x7e, 0x7e, 0x7e, 0x7e, 0x7e, 0x00, 0x00, // 0xfe
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0xff
    ];
}

// ── Generated block/braille fallback font ──────────────────────────────────

/// Generated fallback [`BitmapFont`]s, embedded when the `legacy-computing` feature is enabled.
///
/// Covers the 10 quadrant block characters, the 60 addressable Unicode "Symbols for Legacy
/// Computing" sextant characters, and the full 256-glyph Braille Patterns block
/// (U+2800–U+28FF). None of these are covered by CP437 (and so not by [`unscii16`] either): quadrants and
/// sextants exist to give [`retroglyph_core::symbols::quantize_quadrant`] and
/// [`retroglyph_core::symbols::quantize_sextant`] a font that actually renders their glyphs as
/// blocks instead of a CP437 solid-block substitute, and braille is a common terminal-UI density
/// trick with no CP437 equivalent at all. All three repertoires are pure geometry (rectangular
/// quadrants, banded sextants, a 2x4 dot grid), so both fonts below are computed at compile time
/// by a `const fn` rather than transcribed from an external font file; there is no font asset
/// backing this module and no `image`/build-script dependency.
///
/// This is two [`BitmapFont`]s ([`legacy_computing::blocks::FONT`] and
/// [`legacy_computing::braille::FONT`]), not one: a [`BitmapFont`] addresses its glyphs with a
/// `u8` index (see [`BitmapFont::rows`], [`BitmapFont::glyph_index`]), capping any single font at
/// 256 glyphs. Braille alone needs the full 256, so folding quadrants and sextants into the same
/// font would silently wrap their indices mod 256 and collide with braille glyphs. Splitting at
/// the geometry boundary (block elements vs. braille) keeps every font within that limit with
/// room to spare (70 for [`legacy_computing::blocks::FONT`]) instead of splitting mid-repertoire.
///
/// Add either or both as [`FontChain`] fallbacks alongside a primary CP437 font (e.g.
/// [`unscii16`]) to extend a chain's coverage past CP437:
///
/// ```
/// # #[cfg(feature = "legacy-computing")]
/// # {
/// use retroglyph_window::font::{FontChain, legacy_computing, unscii16};
///
/// static FALLBACKS: [retroglyph_window::font::BitmapFont; 2] =
///     [legacy_computing::blocks::FONT, legacy_computing::braille::FONT];
/// let chain = FontChain::new(unscii16::FONT, &FALLBACKS);
/// let quadrant = chain.resolve('▘').expect("covered by legacy_computing::blocks");
/// assert_eq!(quadrant.font_index(), 1);
/// let braille = chain.resolve('\u{2837}').expect("covered by legacy_computing::braille");
/// assert_eq!(braille.font_index(), 2);
/// # }
/// ```
#[cfg(feature = "legacy-computing")]
pub mod legacy_computing {
    /// The 10 quadrant block glyphs and 60 addressable sextant glyphs CP437 has no mapping for.
    ///
    /// See [`super::legacy_computing`]'s module docs for why this is a separate [`BitmapFont`]
    /// from [`super::legacy_computing::braille`] rather than one combined font.
    ///
    /// [`BitmapFont`]: crate::font::BitmapFont
    pub mod blocks {
        use crate::font::BitmapFont;

        /// Number of quadrant block glyphs (the 10 not already covered by CP437).
        const QUADRANT_COUNT: usize = 10;
        /// Number of sextant glyphs (the 60 addressable masks not already covered by CP437).
        const SEXTANT_COUNT: usize = 60;
        /// Number of `retroglyph_core::symbols::bar` eighth-fraction glyphs not already covered
        /// by CP437 (`ONE_EIGHTH`, `ONE_QUARTER`, `THREE_EIGHTHS`, `FIVE_EIGHTHS`,
        /// `THREE_QUARTERS`, `SEVEN_EIGHTHS`; `HALF` and `FULL` are CP437's own `▄`/`█`).
        const BAR_COUNT: usize = 6;
        /// Number of `retroglyph_core::symbols::block` eighth-fraction glyphs not already
        /// covered by CP437 (`ONE_EIGHTH`, `ONE_QUARTER`, `THREE_EIGHTHS`, `FIVE_EIGHTHS`,
        /// `THREE_QUARTERS`, `SEVEN_EIGHTHS`; `HALF` and `FULL` are CP437's own `▌`/`█`).
        const BLOCK_COUNT: usize = 6;
        /// Total glyph count: quadrants, then sextants, then bar levels, then block levels, in
        /// that index order.
        const TOTAL: usize = QUADRANT_COUNT + SEXTANT_COUNT + BAR_COUNT + BLOCK_COUNT;

        /// A [`BitmapFont`] backed by the generated quadrant/sextant/bar/block glyph data.
        ///
        /// Built with [`BitmapFont::with_charset`] (not [`BitmapFont::new`]): none of these
        /// codepoints are in the CP437 table this crate's default mapping uses, so this font
        /// declares its own explicit `char` -> glyph-index table instead.
        #[allow(clippy::cast_possible_truncation)]
        pub const FONT: BitmapFont = BitmapFont::with_charset(&DATA, 8, 16, TOTAL as u16, &CHARSET);

        /// The 10 quadrant block glyphs not already covered by CP437, as `(mask, char)` pairs.
        ///
        /// `mask` is a 4-bit pattern, bit 0 = top-left, bit 1 = top-right, bit 2 = bottom-left,
        /// bit 3 = bottom-right (matching `retroglyph_core::symbols::QUADRANTS`'s own bit
        /// order), skipping the 6 masks CP437 already serves (`0` space, `3` `▀`, `5` `▌`,
        /// `10` `▐`, `12` `▄`, `15` `█`).
        #[rustfmt::skip]
        const QUADRANTS: [(u8, char); QUADRANT_COUNT] = [
            (1, '▘'), (2, '▝'), (4, '▖'), (6, '▞'), (7, '▛'),
            (8, '▗'), (9, '▚'), (11, '▜'), (13, '▙'), (14, '▟'),
        ];

        /// `retroglyph_core::symbols::bar`'s 6 eighth-fraction levels CP437 doesn't cover, as
        /// `(eighths, char)` pairs: a bottom-anchored vertical fill, `eighths` rows out of 8
        /// filled from the bottom of the cell (matching `bar::NINE_LEVELS`'s ordering).
        #[rustfmt::skip]
        const BAR_LEVELS: [(u8, char); BAR_COUNT] = [
            (1, '▁'), (2, '▂'), (3, '▃'), (5, '▅'), (6, '▆'), (7, '▇'),
        ];

        /// `retroglyph_core::symbols::block`'s 6 eighth-fraction levels CP437 doesn't cover, as
        /// `(eighths, char)` pairs: a left-anchored horizontal fill, `eighths` columns out of 8
        /// filled from the left of the cell.
        #[rustfmt::skip]
        const BLOCK_LEVELS: [(u8, char); BLOCK_COUNT] = [
            (1, '▏'), (2, '▎'), (3, '▍'), (5, '▋'), (6, '▊'), (7, '▉'),
        ];

        /// The 60 addressable sextant masks, in ascending order: every 6-bit pattern `1..=62`
        /// (`0` and `63` would be space/full-block, already CP437) except `21` and `42` (a fully
        /// filled left/right column respectively, CP437's own `▌`/`▐`, which have no
        /// codepoint of their own in the Sextants block).
        ///
        /// Bit order: 0 = top-left, 1 = top-right, 2 = mid-left, 3 = mid-right, 4 = bottom-left,
        /// 5 = bottom-right (matching `retroglyph_core::symbols::SEXTANTS`'s own bit order).
        const fn sextant_masks() -> [u8; SEXTANT_COUNT] {
            let mut masks = [0u8; SEXTANT_COUNT];
            let mut m: u16 = 1;
            let mut i = 0;
            while m <= 62 {
                if m != 21 && m != 42 {
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        masks[i] = m as u8;
                    }
                    i += 1;
                }
                m += 1;
            }
            masks
        }

        /// Maps a sextant `mask` (`1..=62`, excluding `21`/`42`) to its codepoint in the
        /// Symbols for Legacy Computing block.
        ///
        /// Sextant codepoints are not `0x1FB00 + mask`: masks `21` and `42` are gaps (see
        /// [`QUADRANTS`], they're CP437's `▌`/`▐` instead), so every mask above each gap
        /// shifts its codepoint down by one relative to a naive offset. Mask `1` -> U+1FB00;
        /// mask `22` (one gap below it, at `21`) -> U+1FB00 + 21 - 1 = U+1FB14.
        const fn sextant_codepoint(mask: u8) -> u32 {
            let gaps_below = if mask > 21 { 1 } else { 0 } + if mask > 42 { 1 } else { 0 };
            0x1_FB00 + (mask as u32 - 1) - gaps_below
        }

        /// Sets pixel `(x, y)` of glyph `index` in `data` (a full `[u8; TOTAL * 16]` glyph
        /// table).
        const fn set_pixel(data: &mut [u8; TOTAL * 16], index: usize, x: u8, y: u8) {
            let row = index * 16 + y as usize;
            data[row] |= 1 << (7 - x);
        }

        /// Computes the full glyph bitmap table: quadrants, then sextants, then bar levels,
        /// then block levels, matching [`CHARSET`]'s glyph-index order.
        const fn build_data() -> [u8; TOTAL * 16] {
            let mut data = [0u8; TOTAL * 16];

            // Quadrants: each glyph is one quarter-rectangle of the 8x16 cell (mx=4, my=8
            // split).
            let mut qi = 0;
            while qi < QUADRANT_COUNT {
                let (mask, _) = QUADRANTS[qi];
                let mut y = 0u8;
                while y < 16 {
                    let mut x = 0u8;
                    while x < 8 {
                        let bit = if x < 4 {
                            if y < 8 { 0 } else { 2 }
                        } else if y < 8 {
                            1
                        } else {
                            3
                        };
                        if (mask >> bit) & 1 == 1 {
                            set_pixel(&mut data, qi, x, y);
                        }
                        x += 1;
                    }
                    y += 1;
                }
                qi += 1;
            }

            // Sextants: 2 columns (mx=4) x 3 row bands (y=0,5,11,16, uneven, to avoid a 1px
            // seam between vertically stacked filled cells).
            let masks = sextant_masks();
            let mut si = 0;
            while si < SEXTANT_COUNT {
                let mask = masks[si];
                let index = QUADRANT_COUNT + si;
                let mut y = 0u8;
                while y < 16 {
                    let row = if y < 5 {
                        0
                    } else if y < 11 {
                        1
                    } else {
                        2
                    };
                    let mut x = 0u8;
                    while x < 8 {
                        let col: usize = if x >= 4 { 1 } else { 0 };
                        let bit = row * 2 + col;
                        if (mask >> bit) & 1 == 1 {
                            set_pixel(&mut data, index, x, y);
                        }
                        x += 1;
                    }
                    y += 1;
                }
                si += 1;
            }

            // Bar levels: bottom-anchored, `eighths` rows out of 16 (2px per eighth) filled
            // from the bottom of the cell.
            let mut bi = 0;
            while bi < BAR_COUNT {
                let (eighths, _) = BAR_LEVELS[bi];
                let index = QUADRANT_COUNT + SEXTANT_COUNT + bi;
                let fill_from = 16 - eighths * 2;
                let mut y = fill_from;
                while y < 16 {
                    let mut x = 0u8;
                    while x < 8 {
                        set_pixel(&mut data, index, x, y);
                        x += 1;
                    }
                    y += 1;
                }
                bi += 1;
            }

            // Block levels: left-anchored, `eighths` columns out of 8 (1px per eighth) filled
            // from the left of the cell.
            let mut bli = 0;
            while bli < BLOCK_COUNT {
                let (eighths, _) = BLOCK_LEVELS[bli];
                let index = QUADRANT_COUNT + SEXTANT_COUNT + BAR_COUNT + bli;
                let mut y = 0u8;
                while y < 16 {
                    let mut x = 0u8;
                    while x < eighths {
                        set_pixel(&mut data, index, x, y);
                        x += 1;
                    }
                    y += 1;
                }
                bli += 1;
            }

            data
        }

        /// Computes the `char` -> glyph-index charset table, matching [`build_data`]'s glyph
        /// order.
        const fn build_charset() -> [(char, u8); TOTAL] {
            let mut charset = [('\0', 0u8); TOTAL];

            let mut qi = 0;
            while qi < QUADRANT_COUNT {
                let (_, ch) = QUADRANTS[qi];
                #[allow(clippy::cast_possible_truncation)]
                {
                    charset[qi] = (ch, qi as u8);
                }
                qi += 1;
            }

            let masks = sextant_masks();
            let mut si = 0;
            while si < SEXTANT_COUNT {
                let cp = sextant_codepoint(masks[si]);
                let Some(ch) = char::from_u32(cp) else {
                    panic!("sextant codepoint is not a valid char")
                };
                let index = QUADRANT_COUNT + si;
                #[allow(clippy::cast_possible_truncation)]
                {
                    charset[index] = (ch, index as u8);
                }
                si += 1;
            }

            let mut bi = 0;
            while bi < BAR_COUNT {
                let (_, ch) = BAR_LEVELS[bi];
                let index = QUADRANT_COUNT + SEXTANT_COUNT + bi;
                #[allow(clippy::cast_possible_truncation)]
                {
                    charset[index] = (ch, index as u8);
                }
                bi += 1;
            }

            let mut bli = 0;
            while bli < BLOCK_COUNT {
                let (_, ch) = BLOCK_LEVELS[bli];
                let index = QUADRANT_COUNT + SEXTANT_COUNT + BAR_COUNT + bli;
                #[allow(clippy::cast_possible_truncation)]
                {
                    charset[index] = (ch, index as u8);
                }
                bli += 1;
            }

            charset
        }

        /// Glyph bitmap data for [`FONT`]: `TOTAL` glyphs, 16 bytes each, computed at compile
        /// time.
        static DATA: [u8; TOTAL * 16] = build_data();

        /// The `char` -> glyph-index table for [`FONT`], computed at compile time.
        static CHARSET: [(char, u8); TOTAL] = build_charset();

        #[cfg(test)]
        mod tests {
            use super::{
                BAR_LEVELS, BLOCK_LEVELS, CHARSET, FONT, QUADRANTS, SEXTANT_COUNT, TOTAL,
                sextant_codepoint, sextant_masks,
            };
            use crate::font::FontChain;
            use std::collections::HashSet;

            #[test]
            fn total_glyph_count_matches_quadrants_plus_sextants_plus_bar_plus_block() {
                assert_eq!(TOTAL, 10 + 60 + 6 + 6);
                assert_eq!(FONT.glyph_count(), u16::try_from(TOTAL).unwrap());
            }

            #[test]
            fn no_charset_entry_duplicates_a_codepoint_cp437_already_serves() {
                // Space, the 4 CP437 half/quadrant blocks, the full block, and the shade ramp
                // are all already reachable through `unscii16`/CP437; this font must not
                // re-supply them.
                let already_cp437: HashSet<char> = [' ', '▀', '▄', '▌', '▐', '█', '░', '▒', '▓']
                    .into_iter()
                    .collect();
                for &(ch, _) in &CHARSET {
                    assert!(
                        !already_cp437.contains(&ch),
                        "{ch:?} (U+{:04X}) duplicates existing CP437 coverage",
                        ch as u32
                    );
                }
            }

            #[test]
            fn every_charset_character_appears_exactly_once() {
                let mut seen = HashSet::with_capacity(TOTAL);
                for &(ch, _) in &CHARSET {
                    assert!(seen.insert(ch), "{ch:?} appears more than once in CHARSET");
                }
                assert_eq!(seen.len(), TOTAL);
            }

            #[test]
            fn sextant_masks_skip_21_and_42() {
                let masks = sextant_masks();
                assert_eq!(masks.len(), SEXTANT_COUNT);
                assert!(!masks.contains(&21));
                assert!(!masks.contains(&42));
                assert_eq!(masks[0], 1);
                assert_eq!(masks[SEXTANT_COUNT - 1], 62);
            }

            #[test]
            fn sextant_codepoint_shifts_down_after_each_gap() {
                assert_eq!(sextant_codepoint(1), 0x1FB00);
                // One gap below (mask 21) has already been skipped by the time mask 22 is
                // reached.
                assert_eq!(sextant_codepoint(22), 0x1FB00 + 21 - 1);
                // Two gaps below (masks 21 and 42) have been skipped by mask 43.
                assert_eq!(sextant_codepoint(43), 0x1FB00 + 42 - 2);
            }

            /// Round-trips this module's `QUADRANTS` table against
            /// `retroglyph_core::symbols::QUADRANTS`, the table it exists to invert (retroglyph#769).
            /// The two are maintained by hand in separate crates with nothing but a doc-comment
            /// claim tying them together; a wrong bit order or codepoint here would silently
            /// scramble any posterized image rendered through `quantize_quadrant`, invisible to
            /// ordinary code review.
            #[test]
            fn quadrant_table_round_trips_core_subcell_quadrants() {
                // The 6 masks CP437 already serves directly, per `QUADRANTS`'s own doc comment;
                // not present in this module's `QUADRANTS` (which only covers the other 10).
                const CP437_COVERED: [(u8, char); 6] = [
                    (0, ' '),
                    (3, '▀'),
                    (5, '▌'),
                    (10, '▐'),
                    (12, '▄'),
                    (15, '█'),
                ];

                let mut by_mask: [Option<char>; 16] = [None; 16];
                for &(mask, ch) in &CP437_COVERED {
                    by_mask[mask as usize] = Some(ch);
                }
                for &(mask, ch) in &QUADRANTS {
                    by_mask[mask as usize] = Some(ch);
                }

                for (mask, expected) in retroglyph_core::symbols::QUADRANTS.into_iter().enumerate()
                {
                    assert_eq!(
                        by_mask[mask],
                        Some(expected),
                        "mask {mask}: this module's quadrant table disagrees with \
                         retroglyph_core::symbols::QUADRANTS[{mask}] ({expected:?})"
                    );
                }
            }

            /// Round-trips this module's sextant generation (`sextant_masks` plus
            /// `sextant_codepoint`, and the 4 masks CP437 already serves) against
            /// `retroglyph_core::symbols::SEXTANTS`, the table it exists to invert (retroglyph#769).
            /// The Symbols for Legacy Computing block is non-contiguous (which is exactly why
            /// `sextant_codepoint`'s gap-correction exists), so this is the class of table where a
            /// hand-review-only guarantee is weakest.
            #[test]
            fn sextant_table_round_trips_core_subcell_sextants() {
                for (mask, expected) in retroglyph_core::symbols::SEXTANTS.into_iter().enumerate() {
                    let mask = u8::try_from(mask).unwrap();
                    let actual = match mask {
                        0 => ' ',
                        21 => '▌',
                        42 => '▐',
                        63 => '█',
                        _ => char::from_u32(sextant_codepoint(mask))
                            .expect("sextant_codepoint always yields a valid char"),
                    };
                    assert_eq!(
                        actual, expected,
                        "mask {mask}: sextant_codepoint disagrees with \
                         retroglyph_core::symbols::SEXTANTS[{mask}]"
                    );
                }
            }

            /// Every quadrant glyph's set pixels fall in the correct quarter of the 8x16 cell.
            #[test]
            fn quadrant_top_left_mask_only_fills_the_top_left_quarter() {
                let index = FONT.glyph_index('▘').expect("U+2598 is covered");
                for (x, y) in FONT.glyph_pixels(index) {
                    assert!(x < 4 && y < 8, "({x}, {y}) outside the top-left quarter");
                }
            }

            #[test]
            fn font_chain_resolves_quadrant_via_blocks_fallback() {
                static PRIMARY_DATA: [u8; 256 * 16] = [0; 256 * 16];
                const PRIMARY: crate::font::BitmapFont =
                    crate::font::BitmapFont::new(&PRIMARY_DATA, 8, 16, 256);

                static FALLBACKS: [crate::font::BitmapFont; 1] = [FONT];
                let chain = FontChain::new(PRIMARY, &FALLBACKS);

                let quadrant = chain
                    .resolve('▘')
                    .expect("covered by legacy_computing::blocks");
                assert_eq!(quadrant.font_index(), 1);
                assert!(!quadrant.is_notdef());
            }

            /// Every `bar`/`block` eighth-fraction glyph this module generates is reachable by
            /// its own `char` and resolves to a non-empty, non-`notdef` glyph through a
            /// [`FontChain`] (retroglyph#832).
            #[test]
            fn bar_and_block_levels_are_covered_and_non_empty() {
                static PRIMARY_DATA: [u8; 256 * 16] = [0; 256 * 16];
                const PRIMARY: crate::font::BitmapFont =
                    crate::font::BitmapFont::new(&PRIMARY_DATA, 8, 16, 256);

                static FALLBACKS: [crate::font::BitmapFont; 1] = [FONT];
                let chain = FontChain::new(PRIMARY, &FALLBACKS);

                for &(_, ch) in BAR_LEVELS.iter().chain(BLOCK_LEVELS.iter()) {
                    let resolved = chain
                        .resolve(ch)
                        .unwrap_or_else(|| panic!("{ch:?} covered by legacy_computing::blocks"));
                    assert_eq!(resolved.font_index(), 1);
                    assert!(!resolved.is_notdef(), "{ch:?} resolved to notdef");
                    assert!(
                        FONT.glyph_pixels(resolved.index()).count() > 0,
                        "{ch:?} has no set pixels"
                    );
                }
            }

            /// `BAR_LEVELS`' fill grows monotonically with `eighths`: level `n` must be a strict
            /// pixel-count superset of level `n - 1` (bottom-anchored), matching
            /// `bar::NINE_LEVELS`' intended ramp semantics.
            #[test]
            fn bar_levels_fill_monotonically_from_the_bottom() {
                let mut last_count = 0usize;
                for &(eighths, ch) in &BAR_LEVELS {
                    let index = FONT.glyph_index(ch).unwrap();
                    let pixels: Vec<(u8, u8)> = FONT.glyph_pixels(index).collect();
                    assert!(
                        pixels.iter().all(|&(_, y)| y >= 16 - eighths * 2),
                        "{ch:?} has a filled pixel above its {eighths}/8 fill line"
                    );
                    assert_eq!(pixels.len(), usize::from(eighths) * 2 * 8);
                    assert!(pixels.len() > last_count);
                    last_count = pixels.len();
                }
            }

            /// `BLOCK_LEVELS`' fill grows monotonically with `eighths`: level `n` must be a
            /// strict pixel-count superset of level `n - 1` (left-anchored).
            #[test]
            fn block_levels_fill_monotonically_from_the_left() {
                let mut last_count = 0usize;
                for &(eighths, ch) in &BLOCK_LEVELS {
                    let index = FONT.glyph_index(ch).unwrap();
                    let pixels: Vec<(u8, u8)> = FONT.glyph_pixels(index).collect();
                    assert!(
                        pixels.iter().all(|&(x, _)| x < eighths),
                        "{ch:?} has a filled pixel past its {eighths}/8 fill line"
                    );
                    assert_eq!(pixels.len(), usize::from(eighths) * 16);
                    assert!(pixels.len() > last_count);
                    last_count = pixels.len();
                }
            }
        }
    }

    /// The full 256-glyph Braille Patterns block (U+2800–U+28FF) CP437 has no mapping for.
    ///
    /// See [`super::legacy_computing`]'s module docs for why this is a separate [`BitmapFont`]
    /// from [`super::legacy_computing::blocks`] rather than one combined font.
    ///
    /// [`BitmapFont`]: crate::font::BitmapFont
    pub mod braille {
        use crate::font::BitmapFont;

        /// Total glyph count: the full U+2800..=U+28FF block.
        const TOTAL: usize = 256;

        /// A [`BitmapFont`] backed by the generated braille glyph data.
        ///
        /// Built with [`BitmapFont::with_charset`] (not [`BitmapFont::new`]): braille
        /// codepoints are not in the CP437 table this crate's default mapping uses, so this
        /// font declares its own explicit `char` -> glyph-index table instead.
        #[allow(clippy::cast_possible_truncation)]
        pub const FONT: BitmapFont = BitmapFont::with_charset(&DATA, 8, 16, TOTAL as u16, &CHARSET);

        /// Dot-column pixel centers for braille glyphs (`x`), in cell-pixel coordinates.
        const COL_X: [u8; 2] = [1, 4];
        /// Dot-row pixel centers for braille glyphs (`y`), in cell-pixel coordinates.
        const ROW_Y: [u8; 4] = [1, 5, 9, 13];

        /// Maps a braille dot's `(col, row)` position (`col` in `0..2`, `row` in `0..4`) to its
        /// bit index in the U+2800 block's `u8` payload, per historical braille dot numbering:
        /// column 0 is dots 1,2,3,7 (bit indices 0,1,2,6), column 1 is dots 4,5,6,8 (bit indices
        /// 3,4,5,7).
        const fn bit_index(col: usize, row: usize) -> u32 {
            match (col, row) {
                (0, 0) => 0,
                (0, 1) => 1,
                (0, 2) => 2,
                (0, 3) => 6,
                (1, 0) => 3,
                (1, 1) => 4,
                (1, 2) => 5,
                (1, 3) => 7,
                _ => panic!("braille dot position out of range"),
            }
        }

        /// Sets pixel `(x, y)` of glyph `index` in `data` (a full `[u8; TOTAL * 16]` glyph
        /// table).
        const fn set_pixel(data: &mut [u8; TOTAL * 16], index: usize, x: u8, y: u8) {
            let row = index * 16 + y as usize;
            data[row] |= 1 << (7 - x);
        }

        /// Computes the full glyph bitmap table: 2 columns x 4 rows of dots per glyph, each dot
        /// a 3x3 filled square, matching [`CHARSET`]'s glyph-index order.
        const fn build_data() -> [u8; TOTAL * 16] {
            #[allow(clippy::cast_possible_truncation)]
            const TOTAL_U32: u32 = TOTAL as u32;

            let mut data = [0u8; TOTAL * 16];

            let mut bits: u32 = 0;
            while bits < TOTAL_U32 {
                let index = bits as usize;
                let mut col = 0usize;
                while col < 2 {
                    let mut row = 0usize;
                    while row < 4 {
                        let bit = bit_index(col, row);
                        if (bits >> bit) & 1 == 1 {
                            let cx = COL_X[col];
                            let cy = ROW_Y[row];
                            let mut dy: i32 = -1;
                            while dy <= 1 {
                                let mut dx: i32 = -1;
                                while dx <= 1 {
                                    let px = cx as i32 + dx;
                                    let py = cy as i32 + dy;
                                    if px >= 0 && px < 8 && py >= 0 && py < 16 {
                                        #[allow(
                                            clippy::cast_sign_loss,
                                            clippy::cast_possible_truncation
                                        )]
                                        set_pixel(&mut data, index, px as u8, py as u8);
                                    }
                                    dx += 1;
                                }
                                dy += 1;
                            }
                        }
                        row += 1;
                    }
                    col += 1;
                }
                bits += 1;
            }

            data
        }

        /// Computes the `char` -> glyph-index charset table, matching [`build_data`]'s glyph
        /// order: `CHARSET[i] == (char::from_u32(0x2800 + i).unwrap(), i as u8)`.
        const fn build_charset() -> [(char, u8); TOTAL] {
            #[allow(clippy::cast_possible_truncation)]
            const TOTAL_U32: u32 = TOTAL as u32;

            let mut charset = [('\0', 0u8); TOTAL];

            let mut bits: u32 = 0;
            while bits < TOTAL_U32 {
                let cp = 0x2800 + bits;
                let Some(ch) = char::from_u32(cp) else {
                    panic!("braille codepoint is not a valid char")
                };
                let index = bits as usize;
                #[allow(clippy::cast_possible_truncation)]
                {
                    charset[index] = (ch, index as u8);
                }
                bits += 1;
            }

            charset
        }

        /// Glyph bitmap data for [`FONT`]: `TOTAL` glyphs, 16 bytes each, computed at compile
        /// time.
        static DATA: [u8; TOTAL * 16] = build_data();

        /// The `char` -> glyph-index table for [`FONT`], computed at compile time.
        static CHARSET: [(char, u8); TOTAL] = build_charset();

        #[cfg(test)]
        mod tests {
            use super::{CHARSET, FONT, TOTAL};
            use crate::font::FontChain;
            use std::collections::HashSet;

            #[test]
            fn total_glyph_count_is_256() {
                assert_eq!(TOTAL, 256);
                assert_eq!(FONT.glyph_count(), 256);
            }

            #[test]
            fn every_charset_character_appears_exactly_once() {
                let mut seen = HashSet::with_capacity(TOTAL);
                for &(ch, _) in &CHARSET {
                    assert!(seen.insert(ch), "{ch:?} appears more than once in CHARSET");
                }
                assert_eq!(seen.len(), TOTAL);
            }

            #[test]
            fn covers_the_full_u2800_block() {
                for bits in 0u32..u32::try_from(TOTAL).unwrap() {
                    let ch = char::from_u32(0x2800 + bits).unwrap();
                    assert_eq!(CHARSET[bits as usize].0, ch);
                    assert_eq!(CHARSET[bits as usize].1, u8::try_from(bits).unwrap());
                }
            }

            /// Round-trips this module's `CHARSET` against `retroglyph_core::symbols::braille`'s
            /// own `glyph` function, the independent implementation it exists to render
            /// (retroglyph#769): every one of the 256 braille patterns must map to the identical
            /// codepoint through both.
            #[test]
            fn charset_round_trips_core_symbols_braille_glyph() {
                for bits in 0u8..=u8::MAX {
                    let expected = retroglyph_core::symbols::braille::glyph(bits);
                    assert_eq!(
                        CHARSET[bits as usize].0, expected,
                        "pattern {bits:#04x}: this module's CHARSET disagrees with \
                         retroglyph_core::symbols::braille::glyph"
                    );
                }
            }

            #[test]
            fn blank_glyph_is_all_zero_bits() {
                let index = FONT.glyph_index('\u{2800}').expect("U+2800 is covered");
                assert!(FONT.rows(index).iter().all(|&b| b == 0));
            }

            #[test]
            fn full_glyph_has_all_dot_positions_set() {
                let index = FONT.glyph_index('\u{28FF}').expect("U+28FF is covered");
                let pixel_count = FONT.glyph_pixels(index).count();
                // 8 dots, 3x3 each, none clipped by the 8x16 cell at these centers: 8 * 9 = 72
                // lit pixels.
                assert_eq!(pixel_count, 72);
            }

            /// Mirrors `FontChain`'s own doc example: a chain resolving a character none of
            /// CP437 has a mapping for at all through this generated fallback font.
            #[test]
            fn font_chain_resolves_via_braille_fallback() {
                static PRIMARY_DATA: [u8; 256 * 16] = [0; 256 * 16];
                const PRIMARY: crate::font::BitmapFont =
                    crate::font::BitmapFont::new(&PRIMARY_DATA, 8, 16, 256);

                static FALLBACKS: [crate::font::BitmapFont; 1] = [FONT];
                let chain = FontChain::new(PRIMARY, &FALLBACKS);

                // A mid-range braille char: not reachable through CP437 at all.
                let braille = chain
                    .resolve('\u{2837}')
                    .expect("covered by legacy_computing::braille");
                assert_eq!(braille.font_index(), 1);
                assert!(!braille.is_notdef());

                // The primary font's own CP437 coverage answers directly for the solid block;
                // this chain never needs to fall back to `notdef` for it.
                let full_block = chain.resolve('█').expect("CP437 coverage");
                assert_eq!(full_block.font(), PRIMARY);
                assert!(!full_block.is_notdef());
            }
        }
    }
}

// ── Unicode → CP437 mapping ────────────────────────────────────────────────

/// The substitute drawn for a character no font in a chain covers: the solid block, whichever
/// glyph index the font that has it stores it at.
///
/// Naming the substitute as a `char` rather than a fixed index is what keeps
/// [`FontChain::resolve`] total: CP437's own `0xDB` is out of range for a font with fewer than
/// 220 glyphs, so an index constant would resolve to a glyph that font does not have.
const NOTDEF: char = '█';

/// Attempts to map a Unicode scalar to its CP437 glyph index.
///
/// ASCII (U+0020–U+007E) maps identically.  Common box-drawing characters,
/// block-elements, and roguelike symbols are mapped explicitly.  Returns
/// `None` for anything else, distinguishing "not in the CP437 table" from a
/// character that legitimately maps to the solid-block glyph (`'█'`) --
/// [`FontChain`] relies on that distinction to keep trying further
/// fonts on a miss instead of stopping at a false-positive solid-block hit.
#[allow(clippy::too_many_lines)]
const fn try_unicode_to_cp437(ch: char) -> Option<u8> {
    // Direct ASCII pass-through (the most common path for roguelikes).
    let u = ch as u32;
    if u < 0x80 {
        #[allow(clippy::cast_possible_truncation)]
        return Some(u as u8);
    }

    // Named mappings for the characters roguelikes actually use.
    match ch {
        // ── Latin-1 accented letters that overlap CP437 ──────────────────
        'Ç' => Some(0x80),
        'ü' => Some(0x81),
        'é' => Some(0x82),
        'â' => Some(0x83),
        'ä' => Some(0x84),
        'à' => Some(0x85),
        'å' => Some(0x86),
        'ç' => Some(0x87),
        'ê' => Some(0x88),
        'ë' => Some(0x89),
        'è' => Some(0x8A),
        'ï' => Some(0x8B),
        'î' => Some(0x8C),
        'ì' => Some(0x8D),
        'Ä' => Some(0x8E),
        'Å' => Some(0x8F),
        'É' => Some(0x90),
        'æ' => Some(0x91),
        'Æ' => Some(0x92),
        'ô' => Some(0x93),
        'ö' => Some(0x94),
        'ò' => Some(0x95),
        'û' => Some(0x96),
        'ù' => Some(0x97),
        'ÿ' => Some(0x98),
        'Ö' => Some(0x99),
        'Ü' => Some(0x9A),
        '¢' => Some(0x9B),
        '£' => Some(0x9C),
        '¥' => Some(0x9D),
        '₧' => Some(0x9E),
        'ƒ' => Some(0x9F),
        'á' => Some(0xA0),
        'í' => Some(0xA1),
        'ó' => Some(0xA2),
        'ú' => Some(0xA3),
        'ñ' => Some(0xA4),
        'Ñ' => Some(0xA5),
        'ª' => Some(0xA6),
        'º' => Some(0xA7),
        '¿' => Some(0xA8),
        '⌐' => Some(0xA9),
        '¬' => Some(0xAA),
        '½' => Some(0xAB),
        '¼' => Some(0xAC),
        '¡' => Some(0xAD),
        '«' => Some(0xAE),
        '»' => Some(0xAF),

        // ── Shade characters ─────────────────────────────────────────────
        '░' => Some(0xB0),
        '▒' => Some(0xB1),
        '▓' => Some(0xB2),

        // ── Single-line box drawing ───────────────────────────────────────
        '│' => Some(0xB3),
        '┤' => Some(0xB4),
        '╡' => Some(0xB5),
        '╢' => Some(0xB6),
        '╖' => Some(0xB7),
        '╕' => Some(0xB8),
        '╣' => Some(0xB9),
        '║' => Some(0xBA),
        '╗' => Some(0xBB),
        '╝' => Some(0xBC),
        '╜' => Some(0xBD),
        '╛' => Some(0xBE),
        '┐' => Some(0xBF),
        '└' => Some(0xC0),
        '┴' => Some(0xC1),
        '┬' => Some(0xC2),
        '├' => Some(0xC3),
        '─' => Some(0xC4),
        '┼' => Some(0xC5),
        '╞' => Some(0xC6),
        '╟' => Some(0xC7),
        '╚' => Some(0xC8),
        '╔' => Some(0xC9),
        '╩' => Some(0xCA),
        '╦' => Some(0xCB),
        '╠' => Some(0xCC),
        '═' => Some(0xCD),
        '╬' => Some(0xCE),
        '╧' => Some(0xCF),
        '╨' => Some(0xD0),
        '╤' => Some(0xD1),
        '╥' => Some(0xD2),
        '╙' => Some(0xD3),
        '╘' => Some(0xD4),
        '╒' => Some(0xD5),
        '╓' => Some(0xD6),
        '╫' => Some(0xD7),
        '╪' => Some(0xD8),
        '┘' => Some(0xD9),
        '┌' => Some(0xDA),

        // ── Block elements ────────────────────────────────────────────────
        '█' => Some(0xDB),
        '▄' => Some(0xDC),
        '▌' => Some(0xDD),
        '▐' => Some(0xDE),
        '▀' => Some(0xDF),

        // ── Greek / math ──────────────────────────────────────────────────
        'α' => Some(0xE0),
        'ß' => Some(0xE1),
        'Γ' => Some(0xE2),
        'π' => Some(0xE3),
        'Σ' => Some(0xE4),
        'σ' => Some(0xE5),
        'µ' | 'μ' => Some(0xE6),
        'τ' => Some(0xE7),
        'Φ' => Some(0xE8),
        'Θ' => Some(0xE9),
        'Ω' => Some(0xEA),
        'δ' => Some(0xEB),
        '∞' => Some(0xEC),
        'φ' => Some(0xED),
        'ε' => Some(0xEE),
        '∩' => Some(0xEF),
        '≡' => Some(0xF0),
        '±' => Some(0xF1),
        '≥' => Some(0xF2),
        '≤' => Some(0xF3),
        '⌠' => Some(0xF4),
        '⌡' => Some(0xF5),
        '÷' => Some(0xF6),
        '≈' => Some(0xF7),
        '°' => Some(0xF8),
        '∙' => Some(0xF9),
        '·' => Some(0xFA),
        '√' => Some(0xFB),
        'ⁿ' => Some(0xFC),
        '²' => Some(0xFD),
        '■' => Some(0xFE),
        '\u{00A0}' => Some(0xFF),

        // ── Roguelike / Unicode symbols ───────────────────────────────────
        '☺' => Some(0x01),
        '•' => Some(0x07),
        '☻' => Some(0x02),
        '♥' => Some(0x03),
        '♦' => Some(0x04),
        '♣' => Some(0x05),
        '♠' => Some(0x06),
        '◘' => Some(0x08),
        '○' => Some(0x09),
        '◙' => Some(0x0A),
        '♂' => Some(0x0B),
        '♀' => Some(0x0C),
        '♪' => Some(0x0D),
        '♫' => Some(0x0E),
        '☼' => Some(0x0F),
        '►' => Some(0x10),
        '◄' => Some(0x11),
        '↕' => Some(0x12),
        '‼' => Some(0x13),
        '¶' => Some(0x14),
        '§' => Some(0x15),
        '▬' => Some(0x16),
        '↨' => Some(0x17),
        '↑' => Some(0x18),
        '↓' => Some(0x19),
        '→' => Some(0x1A),
        '←' => Some(0x1B),
        '∟' => Some(0x1C),
        '↔' => Some(0x1D),
        '▲' => Some(0x1E),
        '▼' => Some(0x1F),
        '⌂' => Some(0x7F),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{BitmapFont, FontChain, try_unicode_to_cp437};

    /// The four codepoints patched into `unscii16`'s `DATA` (see that module's doc comment)
    /// must actually be reachable through the char-to-glyph path, not just present at their
    /// raw glyph index: otherwise they're invisible to anything that goes through
    /// [`FontChain::resolve`]/`Surface::print`, which is every real caller.
    #[test]
    fn patched_glyphs_are_reachable_by_char() {
        assert_eq!(try_unicode_to_cp437('⌂'), Some(0x7F), "U+2302 HOUSE");
        assert_eq!(
            try_unicode_to_cp437('☼'),
            Some(0x0F),
            "U+263C WHITE SUN WITH RAYS"
        );
        assert_eq!(
            try_unicode_to_cp437('⌐'),
            Some(0xA9),
            "U+2310 REVERSED NOT SIGN"
        );
        assert_eq!(
            try_unicode_to_cp437('∙'),
            Some(0xF9),
            "U+2219 BULLET OPERATOR"
        );
        assert_eq!(
            try_unicode_to_cp437('\u{00A0}'),
            Some(0xFF),
            "U+00A0 NO-BREAK SPACE"
        );
        assert_eq!(try_unicode_to_cp437('¬'), Some(0xAA), "U+00AC NOT SIGN");
        assert_eq!(try_unicode_to_cp437('₧'), Some(0x9E), "U+20A7 PESETA SIGN");
        assert_eq!(try_unicode_to_cp437('·'), Some(0xFA), "U+00B7 MIDDLE DOT");
    }

    /// Every entry in [`crate::tileset::CP437_TO_UNICODE`] must round-trip back through
    /// [`try_unicode_to_cp437`] to its own index: the reverse map is supposed to be a clean
    /// inverse of the crate's own canonical forward table.
    #[cfg(feature = "tilesets")]
    #[test]
    fn cp437_to_unicode_round_trips_through_try_unicode_to_cp437() {
        for (i, &ch) in crate::tileset::CP437_TO_UNICODE.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let expected = i as u8;
            assert_eq!(
                try_unicode_to_cp437(ch),
                Some(expected),
                "0x{i:02X} {ch:?} did not round-trip"
            );
        }
    }

    /// A primary font that only covers the ASCII half of CP437 (glyph indices 0..128), so
    /// any character mapping into the extended range (128..256) is a miss for it.
    static PRIMARY_DATA: [u8; 128 * 16] = [0; 128 * 16];
    const PRIMARY: BitmapFont = BitmapFont::new(&PRIMARY_DATA, 8, 16, 128);

    /// A fallback font with full CP437 coverage (glyph indices 0..256).
    static FALLBACK_DATA: [u8; 256 * 16] = [0; 256 * 16];
    const FALLBACK_FONT: BitmapFont = BitmapFont::new(&FALLBACK_DATA, 8, 16, 256);

    #[test]
    fn chain_resolves_char_present_only_in_fallback_font() {
        // 'Ç' maps to CP437 index 0x80, which is out of range for `PRIMARY`
        // (glyph_count == 128) but present in `FALLBACK_FONT` (glyph_count == 256).
        let chain = FontChain::new(PRIMARY, &[FALLBACK_FONT]);
        let resolved = chain.resolve('Ç').expect("covered by the fallback font");
        assert_eq!(resolved.font(), FALLBACK_FONT);
        assert_eq!(resolved.font_index(), 1);
        assert_eq!(resolved.index(), 0x80);
        assert!(!resolved.is_notdef());
    }

    #[test]
    fn chain_falls_back_to_solid_block_when_every_font_misses() {
        // 'あ' (U+3042 HIRAGANA LETTER A) isn't in the CP437 table at all, so both fonts in the
        // chain miss and resolution must substitute the solid block. `PRIMARY` stops at glyph 128
        // and so doesn't have one, which is exactly the case a fixed 0xDB fallback index used to
        // resolve to an out-of-range glyph for.
        let chain = FontChain::new(PRIMARY, &[FALLBACK_FONT]);
        let resolved = chain.resolve('あ').expect("solid block substitute");
        assert_eq!(resolved.font(), FALLBACK_FONT);
        assert_eq!(resolved.index(), 0xDB);
        assert!(resolved.is_notdef());
    }

    #[test]
    fn chain_resolves_nothing_when_no_font_has_a_substitute() {
        // A chain that covers braille and nothing else: an uncovered character has no solid block
        // to fall back to anywhere in the chain, so resolution reports "undrawable" instead of
        // pointing at a glyph the font doesn't have.
        static DATA: [u8; 16] = [0; 16];
        const CHARSET: [(char, u8); 1] = [('\u{2800}', 0)];
        const BRAILLE: BitmapFont = BitmapFont::with_charset(&DATA, 8, 16, 1, &CHARSET);

        let chain = FontChain::new(BRAILLE, &[]);
        assert!(chain.resolve('\u{2800}').is_some());
        assert!(chain.resolve('A').is_none());
    }

    #[test]
    fn single_font_chain_resolves_that_font_directly() {
        let chain = FontChain::from(FALLBACK_FONT);
        assert_eq!(chain.font_count(), 1);
        for ch in ['A', ' ', '█', '│', 'Ç', '☺'] {
            let resolved = chain.resolve(ch).expect("CP437 coverage");
            assert_eq!(resolved.font(), FALLBACK_FONT);
            assert_eq!(resolved.font_index(), 0);
            assert_eq!(resolved.index(), FALLBACK_FONT.glyph_index(ch).unwrap());
        }
    }

    #[test]
    fn glyph_size_is_none_for_a_chain_of_mismatched_fonts() {
        static DATA: [u8; 8] = [0; 8];
        const SHORT: BitmapFont = BitmapFont::new(&DATA, 8, 8, 1);

        assert_eq!(
            FontChain::new(PRIMARY, &[FALLBACK_FONT]).glyph_size(),
            Some((8, 16))
        );
        assert_eq!(FontChain::new(PRIMARY, &[SHORT]).glyph_size(), None);
    }

    #[test]
    fn glyph_pixels_decodes_msb_first_row_major() {
        // Two 8x2 glyphs. Glyph 0: corners of the top row set; glyph 1: full top row plus one
        // interior pixel on the second row.
        static DATA: [u8; 4] = [0b1000_0001, 0b0000_0000, 0b1111_1111, 0b0000_1000];
        let font = BitmapFont::new(&DATA, 8, 2, 2);

        let g0: Vec<(u8, u8)> = font.glyph_pixels(0).collect();
        assert_eq!(
            g0,
            [(0, 0), (7, 0)],
            "MSB is the leftmost pixel; row 0 first"
        );

        let g1: Vec<(u8, u8)> = font.glyph_pixels(1).collect();
        let mut expected: Vec<(u8, u8)> = (0..8).map(|x| (x, 0)).collect();
        expected.push((4, 1)); // bit 3 of 0b0000_1000 -> x = width-1-3 = 4
        assert_eq!(g1, expected);
    }

    #[test]
    fn glyph_pixels_is_parameterized_by_width_not_hardcoded_to_8() {
        // A 5px-wide glyph: set pixels must come from bits (width-1-x), i.e. bit 4 and bit 0, not
        // bit 7 and bit 3. This guards against a consumer re-introducing a hardcoded `7 - x`.
        static DATA: [u8; 1] = [0b0001_0001];
        let font = BitmapFont::new(&DATA, 5, 1, 1);
        let pixels: Vec<(u8, u8)> = font.glyph_pixels(0).collect();
        assert_eq!(pixels, [(0, 0), (4, 0)]);
    }

    #[test]
    fn glyph_pixels_does_not_overflow_the_shift_for_width_above_8() {
        // retroglyph#729: `row >> (width - 1 - x)` used to shift past the byte's width once
        // `glyph_width` exceeded 8, which this 1-bit-per-row format never actually supports.
        static DATA: [u8; 1] = [0b1111_1111];
        let font = BitmapFont::new(&DATA, 12, 1, 1);
        let pixels: Vec<(u8, u8)> = font.glyph_pixels(0).collect();
        assert_eq!(pixels, (0..8).map(|x| (x, 0)).collect::<Vec<_>>());
    }

    /// Reproduces retroglyph#507: a fallback font built with [`BitmapFont::with_charset`] can
    /// declare coverage for a codepoint CP437 has no mapping for at all (here U+2800 BRAILLE
    /// PATTERN BLANK), and a [`FontChain`] resolves it to that font's own distinct glyph
    /// index instead of colliding with CP437's solid-block fallback (`chain.resolve('\u{2588}')`,
    /// i.e. `'█'`).
    #[test]
    fn chain_extends_past_cp437_via_charset_fallback_font() {
        static BRAILLE_DATA: [u8; 16] = [0; 16];
        const BRAILLE_CHARSET: [(char, u8); 1] = [('\u{2800}', 0)];
        const BRAILLE_FONT: BitmapFont =
            BitmapFont::with_charset(&BRAILLE_DATA, 8, 16, 1, &BRAILLE_CHARSET);

        let primary = FALLBACK_FONT; // full CP437 coverage, glyph_count == 256
        let chain = FontChain::new(primary, &[BRAILLE_FONT]);

        let braille = chain.resolve('\u{2800}').expect("charset coverage");
        assert_eq!(braille.font(), BRAILLE_FONT);
        assert_eq!(braille.index(), 0);

        let full_block = chain.resolve('\u{2588}').expect("CP437 coverage"); // '█', index 0xDB
        assert_eq!(full_block.font(), primary);
        assert_eq!(full_block.index(), 0xDB);

        assert_ne!(braille.index(), full_block.index());
    }
}

/// Coverage test for `retroglyph_core::symbols`'s hand-maintained glyph tables against the
/// fullest bundled [`FontChain`] this crate can build (`unscii16` plus every `legacy_computing`
/// fallback font).
///
/// `core::symbols` promises a repertoire that no font is required to actually draw; nothing
/// checked, before this, that any bundled font could render a given entry (retroglyph#769). This
/// only records the gap (asserting each entry is either drawable or a documented exception): the
/// fix (generating the missing eighth-block glyphs and adding "falls back to notdef" doc notes
/// for the rest) is tracked as a follow-up, deliberately out of scope here.
#[cfg(all(test, feature = "default-font", feature = "legacy-computing"))]
mod symbols_coverage {
    use crate::font::{BitmapFont, FontChain, legacy_computing, unscii16};
    use retroglyph_core::symbols::{bar, block, border, line};
    use std::collections::HashSet;

    /// The bundled `unscii16` primary font plus every `legacy_computing` fallback: the fullest
    /// font coverage this crate can build without a caller supplying custom glyph art.
    fn chain() -> FontChain<'static> {
        static FALLBACKS: [BitmapFont; 2] = [
            legacy_computing::blocks::FONT,
            legacy_computing::braille::FONT,
        ];
        FontChain::new(unscii16::FONT, &FALLBACKS)
    }

    /// Every `core::symbols` entry that currently falls back to the notdef substitute through
    /// [`chain`] (or that no font in the chain can draw at all), as found by retroglyph#769's
    /// audit and narrowed by retroglyph#832's fix for the `bar`/`block` eighth-fraction gaps: 4
    /// of `border::ROUNDED`, all 6 of `border::THICK`, and 5 of `line::THICK`.
    ///
    /// None of these are fixed here: `border::ROUNDED`'s corners, `border::THICK`, and
    /// `line::THICK`'s tees/cross need real glyph art rather than a mechanical eighth-block
    /// generator (see their own doc comments in `retroglyph_core::symbols` for the same note).
    /// This list exists so a *regression* (a currently-covered glyph losing coverage) fails
    /// loudly, and so this test starts failing (forcing the list to shrink) the moment a
    /// future change closes any of these gaps.
    fn known_notdef_gaps() -> HashSet<char> {
        [
            // border::ROUNDED: 4 of 6 (the corners; horizontal/vertical are shared with PLAIN).
            border::ROUNDED.top_left,
            border::ROUNDED.top_right,
            border::ROUNDED.bottom_left,
            border::ROUNDED.bottom_right,
            // border::THICK: all 6.
            border::THICK.top_left,
            border::THICK.top_right,
            border::THICK.bottom_left,
            border::THICK.bottom_right,
            border::THICK.horizontal,
            border::THICK.vertical,
            // line::THICK: 5 of 7. `horizontal`/`vertical` are the same glyphs as
            // `border::THICK`'s (already counted above); the 4 tees and the cross are not.
            line::THICK.cross,
            line::THICK.vertical_left,
            line::THICK.vertical_right,
            line::THICK.horizontal_down,
            line::THICK.horizontal_up,
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn every_symbols_entry_resolves_or_is_a_known_gap() {
        let chain = chain();
        let known = known_notdef_gaps();
        let mut still_notdef = HashSet::new();

        let mut check = |ch: char| {
            let resolved = chain.resolve(ch);
            let is_notdef = resolved.is_none_or(|g| g.is_notdef());
            if is_notdef {
                still_notdef.insert(ch);
            }
        };

        for set in [
            border::PLAIN,
            border::ROUNDED,
            border::DOUBLE,
            border::THICK,
        ] {
            check(set.top_left);
            check(set.top_right);
            check(set.bottom_left);
            check(set.bottom_right);
            check(set.horizontal);
            check(set.vertical);
        }

        for set in [line::NORMAL, line::DOUBLE, line::THICK] {
            check(set.horizontal);
            check(set.vertical);
            check(set.cross);
            check(set.vertical_left);
            check(set.vertical_right);
            check(set.horizontal_down);
            check(set.horizontal_up);
        }

        for ch in [
            block::FULL,
            block::SEVEN_EIGHTHS,
            block::THREE_QUARTERS,
            block::FIVE_EIGHTHS,
            block::HALF,
            block::THREE_EIGHTHS,
            block::ONE_QUARTER,
            block::ONE_EIGHTH,
        ] {
            check(ch);
        }

        for ch in bar::NINE_LEVELS {
            check(ch);
        }

        for pattern in 0u8..=u8::MAX {
            check(retroglyph_core::symbols::braille::glyph(pattern));
        }

        for &ch in &still_notdef {
            assert!(
                known.contains(&ch),
                "{ch:?} (U+{:04X}) newly falls back to notdef through the bundled chain; \
                 either fix its font coverage or add it to `known_notdef_gaps`",
                ch as u32
            );
        }
        assert_eq!(
            still_notdef, known,
            "`known_notdef_gaps` is stale: a previously-notdef glyph now resolves through the \
             bundled chain. Shrink the allowlist to match."
        );
    }
}
