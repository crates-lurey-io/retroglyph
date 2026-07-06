//! Compose [`Grid`] values before drawing them.
//!
//! [`join_h`] and [`join_v`] concatenate several `Grid`s into one,
//! side-by-side or stacked, via [`Grid::blit`]. `Grid` is constructible
//! without a [`Backend`]/[`Terminal`], so composing widget output ahead of
//! drawing it means composing `Grid`s directly, with no separate
//! cell/buffer type.
use retroglyph_core::{Backend, Grid, Rect, Terminal};

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
        out.blit(0, g, Rect::new(0, 0, g.width(), g.height()), x_offset, 0);
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
        out.blit(0, g, Rect::new(0, 0, g.width(), g.height()), 0, y_offset);
        y_offset = y_offset.saturating_add(g.height());
    }
    out
}

/// Stamp `grid`'s layer 0 onto `term`, with its top-left cell at `(x, y)`.
///
/// A thin convenience over [`Grid::blit`] (via
/// [`Terminal::grid_mut`](retroglyph_core::Terminal::grid_mut)) so callers
/// composing widget output with [`join_h`]/[`join_v`] don't need to reach
/// into the terminal's grid by hand for the final copy.
pub fn blit_into<B: Backend>(term: &mut Terminal<B>, grid: &Grid, x: u16, y: u16) {
    let rect = Rect::new(0, 0, grid.width(), grid.height());
    term.grid_mut().blit(0, grid, rect, x, y);
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Color, Headless, Style, Terminal, Tile};

    use super::*;

    #[test]
    fn join_h_concatenates_and_pads_shorter_grids() {
        let mut a = Grid::new(2, 3);
        a.put(0, 0, Tile::new('a', Style::default()));
        let mut b = Grid::new(2, 1);
        b.put(0, 0, Tile::new('b', Style::default()));

        let joined = join_h(&[a, b]);
        assert_eq!((joined.width(), joined.height()), (4, 3));
        assert_eq!(joined.get(0, 0).glyph(), 'a');
        assert_eq!(joined.get(2, 0).glyph(), 'b');
        // b is only 1 row tall; row 1 under it was never written.
        assert_eq!(joined.get(2, 1).glyph(), ' ');
    }

    #[test]
    fn join_v_stacks_and_pads_narrower_grids() {
        let mut a = Grid::new(3, 1);
        a.put(0, 0, Tile::new('a', Style::default()));
        let mut b = Grid::new(1, 1);
        b.put(0, 0, Tile::new('b', Style::default()));

        let joined = join_v(&[a, b]);
        assert_eq!((joined.width(), joined.height()), (3, 2));
        assert_eq!(joined.get(0, 0).glyph(), 'a');
        assert_eq!(joined.get(0, 1).glyph(), 'b');
        // b is only 1 column wide; the rest of its row was never written.
        assert_eq!(joined.get(1, 1).glyph(), ' ');
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
        a.put(0, 0, Tile::new('a', Style::default())); // layer 0
        a.put_tile(1, 0, 0, Tile::new('z', Style::default())); // layer 1

        let joined = join_h(&[a]);
        assert_eq!(joined.get(0, 0).glyph(), 'a');
        assert_eq!(joined.get_tile(1, 0, 0), None); // layer 1 was never allocated
    }

    #[test]
    fn blit_into_stamps_a_grid_onto_a_terminal_at_an_offset() {
        let mut grid = Grid::new(2, 2);
        grid.put(0, 0, Tile::new('x', Style::new().fg(Color::GREEN)));
        grid.put(1, 1, Tile::new('y', Style::default()));

        let mut term = Terminal::new(Headless::new(5, 5));
        blit_into(&mut term, &grid, 2, 1);

        assert_eq!(term.grid().get(2, 1).glyph(), 'x');
        assert_eq!(term.grid().get(3, 2).glyph(), 'y');
        // Untouched cells stay whatever the terminal started with.
        assert_eq!(term.grid().get(0, 0).glyph(), ' ');
    }
}
