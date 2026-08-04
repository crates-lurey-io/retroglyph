//! Compose [`Grid`] values before drawing them.
//!
//! [`join_h`] and [`join_v`] concatenate several `Grid`s into one,
//! side-by-side or stacked, via [`Grid::blit`]. `Grid` is constructible
//! without a [`Backend`](retroglyph_core::Backend)/[`Terminal`](retroglyph_core::Terminal), so
//! composing widget output ahead of drawing it means composing `Grid`s directly, with no
//! separate cell/buffer type.
use retroglyph_core::Grid;

/// Concatenate `grids` left-to-right into one [`Grid`] (layer 0 only).
///
/// The result's width is the sum of the input widths; its height is the
/// tallest input. Each grid is placed top-aligned; cells below a shorter
/// grid are left untouched (empty, per [`Grid::new`]'s default tiles). For
/// an empty slice, returns a 1-wide, 0-tall grid: [`Grid::new`] panics on a
/// width of zero (it divides by width internally), so a 1×0 grid is as
/// close to "empty" as an actual `Grid` can represent.
#[must_use]
pub fn join_h(grids: &[Grid]) -> Grid {
    if grids.is_empty() {
        return Grid::new(1, 0);
    }
    let width = grids
        .iter()
        .fold(0u16, |acc, g| acc.saturating_add(g.width()));
    let height = grids.iter().map(Grid::height).max().unwrap_or(0);
    let mut out = Grid::new(width, height);

    let mut x_offset = 0u16;
    for g in grids {
        out.blit(0, g, g.size().to_rect(), x_offset, 0);
        x_offset = x_offset.saturating_add(g.width());
    }
    out
}

/// Stack `grids` top-to-bottom into one [`Grid`] (layer 0 only).
///
/// The result's height is the sum of the input heights; its width is the
/// widest input. Each grid is placed left-aligned; cells past a narrower
/// grid's width are left untouched (empty, per [`Grid::new`]'s default
/// tiles). For an empty slice, returns a 1-wide, 0-tall grid: see [`join_h`]
/// for why a zero-width grid isn't representable.
#[must_use]
pub fn join_v(grids: &[Grid]) -> Grid {
    if grids.is_empty() {
        return Grid::new(1, 0);
    }
    let width = grids.iter().map(Grid::width).max().unwrap_or(0);
    let height = grids
        .iter()
        .fold(0u16, |acc, g| acc.saturating_add(g.height()));
    let mut out = Grid::new(width, height);

    let mut y_offset = 0u16;
    for g in grids {
        out.blit(0, g, g.size().to_rect(), 0, y_offset);
        y_offset = y_offset.saturating_add(g.height());
    }
    out
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Pos, Style, Tile};

    use super::*;

    #[test]
    fn join_h_concatenates_and_pads_shorter_grids() {
        let mut a = Grid::new(2, 3);
        a.put_tile(0, (0, 0), Tile::new('a', Style::default()));
        let mut b = Grid::new(2, 1);
        b.put_tile(0, (0, 0), Tile::new('b', Style::default()));

        let joined = join_h(&[a, b]);
        assert_eq!((joined.width(), joined.height()), (4, 3));
        assert_eq!(joined[Pos::new(0, 0)].glyph(), 'a');
        assert_eq!(joined[Pos::new(2, 0)].glyph(), 'b');
        // b is only 1 row tall; row 1 under it was never written.
        assert_eq!(joined[Pos::new(2, 1)].glyph(), ' ');
    }

    #[test]
    fn join_v_stacks_and_pads_narrower_grids() {
        let mut a = Grid::new(3, 1);
        a.put_tile(0, (0, 0), Tile::new('a', Style::default()));
        let mut b = Grid::new(1, 1);
        b.put_tile(0, (0, 0), Tile::new('b', Style::default()));

        let joined = join_v(&[a, b]);
        assert_eq!((joined.width(), joined.height()), (3, 2));
        assert_eq!(joined[Pos::new(0, 0)].glyph(), 'a');
        assert_eq!(joined[Pos::new(0, 1)].glyph(), 'b');
        // b is only 1 column wide; the rest of its row was never written.
        assert_eq!(joined[Pos::new(1, 1)].glyph(), ' ');
    }

    #[test]
    fn join_empty_slice_is_essentially_empty() {
        // Grid::new(0, _) always panics (it divides by width internally),
        // so a 1-wide, 0-tall grid is the closest representable "empty".
        let joined = join_h(&[]);
        assert_eq!((joined.width(), joined.height()), (1, 0));
        let joined = join_v(&[]);
        assert_eq!((joined.width(), joined.height()), (1, 0));
    }

    #[test]
    fn join_only_copies_layer_zero() {
        let mut a = Grid::new(1, 1);
        a.put_tile(0, (0, 0), Tile::new('a', Style::default())); // layer 0
        a.put_tile(1, (0, 0), Tile::new('z', Style::default())); // layer 1

        let joined = join_h(&[a]);
        assert_eq!(joined[Pos::new(0, 0)].glyph(), 'a');
        assert_eq!(joined.tile(1, (0, 0)), None); // layer 1 was never allocated
    }
}
