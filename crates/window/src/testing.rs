//! Glyph-coverage testing helpers, feature-gated (`testing`), no effect on release builds.
//!
//! A character no font in a [`FontChain`](crate::font::FontChain) covers still renders: it falls back to the substituted
//! solid block (or, rarely, draws nothing at all -- see
//! [`FontChain::resolve`](crate::font::FontChain::resolve)). That fallback is a legitimate cell
//! on its own, so a snapshot test of the rendered output cannot tell "this glyph is missing" from
//! "this glyph is a block on purpose": both are a stable, correctly-rendered image
//! (crates-lurey-io/retroglyph#1292). This module is the check that actually asks the question a
//! snapshot can't: does `chain.resolve(ch)` answer with `ch`'s own glyph, or with the fallback?
//!
//! [`diagnostics::warn_notdef_glyph`](crate::diagnostics::warn_notdef_glyph) catches the same gap
//! at render time in a dev build; this module is for asserting it directly in a test, over
//! whatever repertoire a consumer's own UI actually draws, without hand-rolling a
//! resolve-and-check loop or scanning source text for character literals.

use crate::font::FontChain;

/// Whether `ch` renders as its own glyph in `chain`: covered by some font in the chain, not the
/// substituted notdef fallback.
///
/// A character no font in `chain` can draw at all (not even the fallback -- see
/// [`FontChain::resolve`](crate::font::FontChain::resolve)) is also "not covered": it draws
/// nothing, which is no more `ch`'s own glyph than the solid block is.
#[must_use]
pub fn is_glyph_covered(chain: &FontChain<'_>, ch: char) -> bool {
    chain.resolve(ch).is_some_and(|glyph| !glyph.is_notdef())
}

/// Every character in `chars` that does not render as itself in `chain`, in iteration order,
/// duplicates included.
///
/// For a scan over a whole repertoire (a demo's UI text, a game's glyph set) where a test wants
/// to name every offender in one failure rather than stopping at the first, as
/// [`assert_glyphs_covered`] does.
#[must_use]
pub fn uncovered_glyphs(chain: &FontChain<'_>, chars: impl IntoIterator<Item = char>) -> Vec<char> {
    chars
        .into_iter()
        .filter(|&ch| !is_glyph_covered(chain, ch))
        .collect()
}

/// Asserts every character in `chars` renders as itself in `chain`.
///
/// # Panics
///
/// Panics naming every character in `chars` that [`uncovered_glyphs`] reports, if any.
pub fn assert_glyphs_covered(chain: &FontChain<'_>, chars: impl IntoIterator<Item = char>) {
    let missing = uncovered_glyphs(chain, chars);
    assert!(
        missing.is_empty(),
        "chain does not cover (rendered as the fallback instead of their own glyph): {missing:?}"
    );
}

#[cfg(test)]
mod tests {
    use super::{assert_glyphs_covered, is_glyph_covered, uncovered_glyphs};
    use crate::font::{BitmapFont, FontChain};

    static PRIMARY_DATA: [u8; 128 * 16] = [0; 128 * 16];
    const PRIMARY: BitmapFont = BitmapFont::new(&PRIMARY_DATA, 8, 16, 128);

    #[test]
    fn covered_ascii_char_is_covered() {
        let chain = FontChain::from(PRIMARY);
        assert!(is_glyph_covered(&chain, 'A'));
    }

    #[test]
    fn char_outside_the_repertoire_is_not_covered() {
        let chain = FontChain::from(PRIMARY);
        // Past PRIMARY's 128 glyphs, so this falls back to the solid block.
        assert!(!is_glyph_covered(&chain, '\u{00C7}'));
    }

    #[test]
    fn uncovered_glyphs_reports_only_the_misses() {
        let chain = FontChain::from(PRIMARY);
        assert_eq!(
            uncovered_glyphs(&chain, ['A', '\u{00C7}', 'B', '\u{3042}']),
            vec!['\u{00C7}', '\u{3042}']
        );
    }

    #[test]
    fn assert_glyphs_covered_passes_for_a_fully_covered_set() {
        let chain = FontChain::from(PRIMARY);
        assert_glyphs_covered(&chain, ['A', 'B', 'C']);
    }

    #[test]
    #[should_panic(expected = "does not cover")]
    fn assert_glyphs_covered_panics_naming_the_miss() {
        let chain = FontChain::from(PRIMARY);
        assert_glyphs_covered(&chain, ['A', '\u{3042}']);
    }
}
