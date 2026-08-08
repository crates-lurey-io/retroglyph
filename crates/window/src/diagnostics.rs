//! Shared render-time glyph diagnostics, called identically by both graphical backends so a
//! consumer sees the same message regardless of which one it built with.
//!
//! Unlike [`sprite_cache`](crate::sprite_cache)'s `warn_sprite_needs_span`/`warn_tint_needs_sprite`,
//! this module is not gated behind the `tilesets` feature:
//! [`warn_notdef_glyph`](crate::diagnostics::warn_notdef_glyph) fires from plain
//! [`FontChain`](crate::font::FontChain) resolution, which has nothing to do with sprites and is
//! reachable with no other feature enabled.

use retroglyph_core::dev_only;
use std::collections::BTreeSet;

/// Warns, at most once per character, that `ch` did not resolve to its own glyph anywhere in the
/// chain.
///
/// No font covered it, so [`FontChain::resolve`](crate::font::FontChain::resolve) substituted the
/// solid block (or, for a chain built entirely from
/// [`BitmapFont::with_charset`](crate::font::BitmapFont::with_charset) fonts, some other font's
/// own notdef box) instead of `ch`'s real shape. That substitute is a legitimate cell on its own
/// (a full block is a valid glyph to draw on purpose), so nothing about the rendered output
/// distinguishes "this glyph is missing" from "this glyph is a block" without a diagnostic naming
/// which one happened (crates-lurey-io/retroglyph#1292). Call this from the branch that already
/// knows a resolved glyph was flagged
/// [`ResolvedGlyph::is_notdef`](crate::font::ResolvedGlyph::is_notdef); both graphical backends
/// do, right after their own resolve call, so the diagnostic and the character it names are
/// identical on each.
///
/// `seen` is caller-owned state so a redraw loop reports each offending character once rather
/// than every frame; entries are only ever added.
///
/// Returns whether a warning was emitted, which is always `false` in a build that compiles
/// diagnostics out: the `seen` bookkeeping and the message both sit inside
/// [`dev_only!`](retroglyph_core::dev_only), so a release build does neither. See
/// [`BuildMode`](retroglyph_core::dev::BuildMode).
pub fn warn_notdef_glyph(seen: &mut BTreeSet<char>, ch: char) -> bool {
    dev_only!({
        if !seen.insert(ch) {
            return false;
        }
        #[allow(clippy::cast_lossless)]
        let cp = ch as u32;
        log::warn!(
            "no font in the chain covers {ch:?} (U+{cp:04X}); it rendered as the substituted \
             solid block instead of its own glyph"
        );
        return true;
    });
    false
}

#[cfg(test)]
mod tests {
    use super::warn_notdef_glyph;
    use std::collections::BTreeSet;

    #[test]
    fn warns_once_per_char() {
        let mut seen = BTreeSet::new();
        assert_eq!(
            warn_notdef_glyph(&mut seen, 'あ'),
            retroglyph_core::dev::DEV
        );
        // Second call for the same character is always silent, dev build or not.
        assert!(!warn_notdef_glyph(&mut seen, 'あ'));
    }

    #[test]
    fn distinct_characters_each_warn_once() {
        let mut seen = BTreeSet::new();
        assert_eq!(
            warn_notdef_glyph(&mut seen, 'あ'),
            retroglyph_core::dev::DEV
        );
        assert_eq!(
            warn_notdef_glyph(&mut seen, '\u{2603}'),
            retroglyph_core::dev::DEV
        );
    }
}
