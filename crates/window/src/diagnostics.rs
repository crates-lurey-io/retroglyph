//! Shared render-time glyph diagnostics, called identically by every graphical backend so a
//! consumer sees the same message regardless of which one it built with.
//!
//! [`DiagnosticLog`](crate::diagnostics::DiagnosticLog) is the state a renderer carries: one
//! field per backend instead of one `BTreeSet<char>` per diagnostic, so "which diagnostics does a
//! windowed renderer emit" is answered by this struct's methods rather than by grepping every
//! backend for `warned_*` fields.
//! [`DiagnosticLog::notdef_glyph`](crate::diagnostics::DiagnosticLog::notdef_glyph) is not gated
//! behind the `tilesets` feature: it fires from plain [`FontChain`](crate::font::FontChain)
//! resolution, which has nothing to do with sprites and is reachable with no other feature
//! enabled. The sprite-only diagnostics
//! ([`sprite_needs_span`](crate::diagnostics::DiagnosticLog::sprite_needs_span),
//! [`tint_needs_sprite`](crate::diagnostics::DiagnosticLog::tint_needs_sprite)) are.

use retroglyph_core::dev_only;
use std::collections::BTreeSet;

#[cfg(feature = "tilesets")]
use crate::sprite_cache::{warn_sprite_needs_span, warn_tint_needs_sprite};
#[cfg(feature = "tilesets")]
use retroglyph_core::color::Tint;

/// The dedup state behind every render-time diagnostic a windowed renderer can emit.
///
/// Each graphical backend holds one `DiagnosticLog` field (`Default`-constructed) instead of a
/// separate `BTreeSet<char>` per diagnostic, so the diagnostics a backend emits are exactly this
/// struct's methods, not something a reader has to reconstruct by finding every `warned_*` field
/// and the free function it feeds. The methods here are thin wrappers over the free functions in
/// this module and in [`sprite_cache`](crate::sprite_cache), which remain the implementation and
/// stay public for any caller that wants the dedup set itself rather than one owned alongside the
/// others.
///
/// Constructing a `DiagnosticLog` never allocates: every set starts empty, and
/// [`BTreeSet::default`] does not allocate until its first insert, so a release build (where every
/// method's body is compiled out by [`dev_only!`]) pays nothing for carrying this field.
#[derive(Debug, Default)]
pub struct DiagnosticLog {
    notdef: BTreeSet<char>,
    #[cfg(feature = "tilesets")]
    oversized: BTreeSet<char>,
    #[cfg(feature = "tilesets")]
    dropped_tint: BTreeSet<char>,
}

impl DiagnosticLog {
    /// Reports `ch` if it resolved to the fallback notdef glyph rather than its own, at most once
    /// per character. See [`warn_notdef_glyph`] for what this means and why it is worth a
    /// diagnostic.
    pub fn notdef_glyph(&mut self, ch: char) -> bool {
        warn_notdef_glyph(&mut self.notdef, ch)
    }

    /// Reports `glyph`'s sprite as drawn without a span reserving the cells it covers, at most
    /// once per glyph. See [`warn_sprite_needs_span`] for the full contract.
    #[cfg(feature = "tilesets")]
    pub fn sprite_needs_span(&mut self, glyph: char, sprite: (u32, u32), cell: (u32, u32)) -> bool {
        warn_sprite_needs_span(&mut self.oversized, glyph, sprite, cell)
    }

    /// Reports `glyph`'s tint as dropped because it resolved to a bitmap font glyph rather than a
    /// sprite, at most once per glyph. See [`warn_tint_needs_sprite`] for the full contract.
    #[cfg(feature = "tilesets")]
    pub fn tint_needs_sprite(&mut self, glyph: char, tint: Tint) -> bool {
        warn_tint_needs_sprite(&mut self.dropped_tint, glyph, tint)
    }

    /// Whether [`notdef_glyph`](Self::notdef_glyph) has already reported `ch`.
    ///
    /// For a backend's own test suite to assert the dedup behavior directly; production code only
    /// ever needs `notdef_glyph`'s own return value.
    #[must_use]
    pub fn has_reported_notdef(&self, ch: char) -> bool {
        self.notdef.contains(&ch)
    }

    /// Whether [`tint_needs_sprite`](Self::tint_needs_sprite) has already reported `glyph`.
    ///
    /// For a backend's own test suite to assert the dedup behavior directly; production code only
    /// ever needs `tint_needs_sprite`'s own return value.
    #[cfg(feature = "tilesets")]
    #[must_use]
    pub fn has_reported_dropped_tint(&self, glyph: char) -> bool {
        self.dropped_tint.contains(&glyph)
    }

    /// How many distinct glyphs [`tint_needs_sprite`](Self::tint_needs_sprite) has reported so
    /// far.
    ///
    /// For a backend's own test suite to assert dedup collapses repeat reports to one; production
    /// code has no use for the count.
    #[cfg(feature = "tilesets")]
    #[must_use]
    pub fn dropped_tint_report_count(&self) -> usize {
        self.dropped_tint.len()
    }
}

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
