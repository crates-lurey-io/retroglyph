//! [`Paragraph`]: word-wrapped text, implementing both [`Widget`] and
//! [`Measure`] so a caller can size a pane to fit before rendering.
//!
//! Always available: wrapping falls back to a `char`-boundary-safe,
//! ASCII-whitespace word wrap with no grapheme-cluster correctness (a
//! combining mark, ZWJ sequence, or wide CJK run may land on the wrong
//! side of a wrap point). Enabling the `egc` feature upgrades this same
//! type to route through [`retroglyph_core::layout::TextLayout`] instead,
//! which handles grapheme clusters and hard newlines correctly; there is
//! no second, egc-only `Paragraph` type to migrate to.
#[cfg(not(feature = "egc"))]
use alloc::string::{String, ToString as _};
#[cfg(not(feature = "egc"))]
use alloc::vec;
#[cfg(not(feature = "egc"))]
use alloc::vec::Vec;

use retroglyph_core::color::Style;
#[cfg(feature = "egc")]
use retroglyph_core::grid::HasSize;
#[cfg(feature = "egc")]
use retroglyph_core::grid::Rect;
#[cfg(feature = "egc")]
use retroglyph_core::layout::TextLayout;
#[cfg(feature = "egc")]
use retroglyph_core::text::{Line, Span};

use super::{Measure, Widget};
use crate::Surface;

/// Word-wrapped text in a single [`Style`].
///
/// `Paragraph::new(text)` wraps `text` to whatever width it is rendered at
/// (via [`Widget::render`]), or reports the height it would need at a
/// given width without rendering (via [`Measure::height_for`]) so a caller
/// can size its pane to fit instead of guessing a fixed height. `style`
/// defaults to [`Style::new()`]; set it with [`Paragraph::style`].
///
/// Without the `egc` feature, wrapping is `char`-boundary-safe and breaks
/// on ASCII whitespace only: no grapheme-cluster segmentation, so a
/// combining mark or wide CJK run can land on either side of a wrap point.
/// Enabling `egc` upgrades wrapping to [`retroglyph_core::layout::TextLayout`],
/// which is grapheme-cluster-aware; every other text-bearing widget in this
/// crate (`List`, `Table`, `Log`) already has this same gap regardless of
/// `egc`.
///
/// Unlike [`super::BoxBorder`], [`super::Gauge`], [`super::StatBar`],
/// [`super::Table`], and [`super::Button`], `Paragraph` has no `theme()`/
/// `theme_on()` pair: word-wrapped text has no single semantic
/// [`Theme`](crate::Theme) role to map onto, so callers set `style` directly.
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::{Grid, Rect};
/// use retroglyph_widgets::{Measure, Paragraph, Surface, Widget};
///
/// let p = Paragraph::new("the quick brown fox jumps");
/// let height = p.height_for(10); // rows needed to wrap at 10 columns
///
/// let area = Rect::new(0, 0, 10, height);
/// let mut grid = Grid::new(10, height);
/// p.render(&mut Surface::new(&mut grid, area, 0));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Paragraph<'a> {
    text: &'a str,
    style: Style,
}

impl<'a> Paragraph<'a> {
    /// Text to be word-wrapped, in the default style.
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            style: Style::new(),
        }
    }

    /// Set the text's style.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    #[cfg(feature = "egc")]
    fn line(&self) -> Line {
        Line::from(Span::styled(self.text, self.style))
    }
}

/// Greedy, `char`-boundary-safe word wrap used when the `egc` feature is off.
///
/// Breaks on ASCII space (`' '`, consumed at the break point, not placed) and on `'\n'` (a hard
/// break, regardless of width); an overlong word with no space to break at is force-broken at a
/// `char` boundary once it would exceed `max_width`. Zero-width `char`s (e.g. combining marks)
/// are dropped rather than measured, since without `egc` there is no grapheme-cluster pass to
/// attach them to the `char` before them. This mirrors
/// [`retroglyph_core::layout::TextLayout`]'s own wrap algorithm one abstraction level down
/// (`char` instead of grapheme cluster), so plain-ASCII input wraps identically either way.
#[cfg(not(feature = "egc"))]
fn wrap(text: &str, max_width: u16) -> Vec<String> {
    use retroglyph_core::text::char_width;

    let mut lines: Vec<String> = vec![String::new()];
    let mut col: u16 = 0;

    for ch in text.chars() {
        if ch == '\n' {
            lines.push(String::new());
            col = 0;
            continue;
        }

        let cw = char_width(ch);
        if cw == 0 {
            continue; // zero-width char with no grapheme pass to attach it to; drop it.
        }

        if col + cw > max_width && col > 0 {
            let current = lines.last_mut().expect("always at least one line");
            if let Some(space_idx) = current.rfind(' ') {
                let remainder = current[space_idx + 1..].to_string();
                current.truncate(space_idx); // also drops the space itself
                col = remainder.chars().map(char_width).sum();
                lines.push(remainder);
            } else {
                // No space on the line: force-break (overlong word).
                lines.push(String::new());
                col = 0;
                if ch == ' ' {
                    // Would just be leading whitespace on the new line.
                    continue;
                }
            }
        }

        lines.last_mut().expect("always at least one line").push(ch);
        col += cw;
    }

    lines
}

#[cfg(feature = "egc")]
impl Measure for Paragraph<'_> {
    fn height_for(&self, width: u16) -> u16 {
        let line = self.line();
        TextLayout::new(&line)
            .rect(Rect::new(0, 0, width, u16::MAX))
            .measure()
            .height()
    }
}

#[cfg(not(feature = "egc"))]
impl Measure for Paragraph<'_> {
    fn height_for(&self, width: u16) -> u16 {
        let lines = wrap(self.text, width);
        #[allow(clippy::cast_possible_truncation)]
        let height = lines.len().min(usize::from(u16::MAX)) as u16;
        height
    }
}

#[cfg(feature = "egc")]
impl Widget for Paragraph<'_> {
    fn render(&self, surface: &mut Surface<'_>) {
        let area = surface.area();
        let line = self.line();
        TextLayout::new(&line).rect(area).render_to_surface(surface);
    }
}

#[cfg(not(feature = "egc"))]
impl Widget for Paragraph<'_> {
    fn render(&self, surface: &mut Surface<'_>) {
        let width = surface.area().width();
        let height = surface.area().height();
        let lines = wrap(self.text, width);
        for (row, line) in lines.into_iter().take(usize::from(height)).enumerate() {
            #[allow(clippy::cast_possible_truncation)] // `row < height`, a `u16`
            surface.print((0, row as u16), &line, self.style);
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;

    use retroglyph_core::grid::{Grid, Pos, Rect};

    use super::*;

    #[test]
    fn height_for_matches_wrapped_line_count() {
        let p = Paragraph::new("the quick brown fox jumps");
        assert_eq!(p.height_for(10), 3); // "the quick" / "brown fox" / "jumps"
        assert_eq!(p.height_for(100), 1);
    }

    #[test]
    fn height_for_respects_hard_newlines() {
        // A naive whitespace-based wrap would flatten this to one paragraph;
        // both wrap paths treat "\n" as a hard break regardless of width.
        let p = Paragraph::new("first\nsecond\nthird");
        assert_eq!(p.height_for(100), 3);
    }

    #[test]
    fn render_draws_one_line_per_wrapped_row() {
        let area = Rect::new(0, 0, 10, 5);
        let mut grid = Grid::new(10, 5);
        Paragraph::new("the quick brown fox jumps").render(&mut Surface::new(&mut grid, area, 0));

        let row0: String = (0..10).map(|x| grid[Pos::new(x, 0)].glyph()).collect();
        let row1: String = (0..10).map(|x| grid[Pos::new(x, 1)].glyph()).collect();
        let row2: String = (0..10).map(|x| grid[Pos::new(x, 2)].glyph()).collect();
        assert!(row0.starts_with("the quick"));
        assert!(row1.starts_with("brown fox"));
        assert!(row2.starts_with("jumps"));
    }

    #[test]
    fn render_stops_at_the_area_bottom() {
        // Only 1 row of height: only the first wrapped line should draw.
        let area = Rect::new(0, 0, 10, 1);
        let mut grid = Grid::new(10, 2);
        Paragraph::new("the quick brown fox jumps").render(&mut Surface::new(&mut grid, area, 0));

        let row1: String = (0..10).map(|x| grid[Pos::new(x, 1)].glyph()).collect();
        assert_eq!(row1.trim(), "");
    }

    #[cfg(feature = "egc")]
    #[test]
    fn paragraph_honours_the_surface_clip() {
        // Clip out rows 1-2: the wrapped remainder ("cccc") must not be drawn there,
        // even though the `egc` render path used to write through `grid_mut()` and
        // bypass the clip entirely.
        let area = Rect::new(0, 0, 10, 3);
        let mut grid = Grid::new(10, 3);
        {
            let mut surface = Surface::new(&mut grid, area, 0);
            let mut clipped = surface.clip(Rect::new(0, 0, 10, 1));
            Paragraph::new("aaaa bbbb cccc").render(&mut clipped);
        }

        let row0: String = (0..10).map(|x| grid[Pos::new(x, 0)].glyph()).collect();
        assert_eq!(row0.trim_end(), "aaaa bbbb");
        for row in 1..3 {
            let r: String = (0..10).map(|x| grid[Pos::new(x, row)].glyph()).collect();
            assert_eq!(r.trim(), "");
        }
    }
}
