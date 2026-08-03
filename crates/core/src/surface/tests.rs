use crate::color::Style;
use crate::color::{Color, Tint};
use crate::grid::{Grid, Offset, Pos, Rect};
use crate::text::Line;
use crate::tile::Tile;

use super::{Layer, Surface};

fn screen(grid: &mut Grid) -> Surface<'_> {
    let area = Rect::new(0, 0, grid.width(), grid.height());
    Surface::new(grid, area, 0)
}

/// Renders a `'w' x 'h'` block of `ch` at this surface's own local top-left corner, i.e.
/// the way a widget written against `local_area()`/`width()`/`height()` (rather than
/// `area()`'s absolute `left()`/`top()`) is supposed to place itself: correctly regardless
/// of where this surface's own `area` sits on the underlying grid.
fn render_local_top_left(surface: &mut Surface<'_>, ch: char) {
    let local = surface.local_area();
    surface.put((local.left(), local.top()), ch, Style::default());
}

#[test]
fn local_area_is_always_zero_origin_regardless_of_where_area_sits() {
    let mut grid = Grid::new(10, 10);
    let mut surface = screen(&mut grid);
    let scoped = surface.scope(Rect::new(3, 4, 5, 6));

    assert_eq!(scoped.area(), Rect::new(3, 4, 5, 6));
    assert_eq!(scoped.local_area(), Rect::new(0, 0, 5, 6));
}

#[test]
fn a_widget_placed_via_local_area_lands_the_same_way_at_the_origin_and_at_an_offset() {
    // At the grid origin, local and absolute coordinates coincide: this is the case #697
    // warned degenerates into never exercising the mismatch.
    let mut grid_origin = Grid::new(4, 4);
    render_local_top_left(&mut screen(&mut grid_origin), 'X');

    // Scoped away from the origin: a widget built on `local_area()`/`put`'s local coordinate
    // space should draw identically relative to its own area, unlike one built on
    // `area().left()`/`area().top()`, which would silently miss here.
    let mut grid_offset = Grid::new(10, 10);
    {
        let mut surface = screen(&mut grid_offset);
        let mut scoped = surface.scope(Rect::new(3, 3, 4, 4));
        render_local_top_left(&mut scoped, 'X');
    }

    assert_eq!(grid_origin[Pos::new(0, 0)].glyph(), 'X');
    assert_eq!(grid_offset[Pos::new(3, 3)].glyph(), 'X');
}

#[test]
fn blit_reads_the_sources_layer_0_even_when_this_surface_is_on_a_different_layer() {
    // The retroglyph#824 regression: `src` (a standalone, layer-0-only `Grid`, like
    // `BoxStyle::render`'s output) must still land when the destination surface is on a
    // layer other than 0.
    let mut src = Grid::new(2, 2);
    src.put_tile(0, (0, 0), Tile::new('x', Style::default()));

    let mut dst = Grid::new(4, 4);
    let mut surface = Surface::new(&mut dst, Rect::new(0, 0, 4, 4), 0);
    surface.on_layer(3).blit(&src, 1, 1);

    assert_eq!(dst.tile(3, (1, 1)).map(Tile::glyph), Some('x'));
    // Never touched layer 0 (always allocated, but untouched cells stay their default).
    assert_eq!(dst.tile(0, (1, 1)).map(Tile::glyph), Some(' '));
}

#[test]
fn blit_stamps_a_grid_at_a_local_offset() {
    let mut src = Grid::new(2, 2);
    src.put_tile(0, (0, 0), Tile::new('x', Style::default()));
    src.put_tile(0, (1, 1), Tile::new('y', Style::default()));

    let mut dst = Grid::new(5, 5);
    screen(&mut dst).blit(&src, 2, 1);

    assert_eq!(dst[Pos::new(2, 1)].glyph(), 'x');
    assert_eq!(dst[Pos::new(3, 2)].glyph(), 'y');
    // Untouched cells stay whatever the destination started with.
    assert_eq!(dst[Pos::new(0, 0)].glyph(), ' ');
}

#[test]
fn blit_clips_to_this_surfaces_clip_instead_of_skipping_the_whole_call() {
    let mut src = Grid::new(3, 3);
    for y in 0..3 {
        for x in 0..3 {
            src.put_tile(0, (x, y), Tile::new('#', Style::default()));
        }
    }

    let mut dst = Grid::new(5, 5);
    {
        let mut surface = screen(&mut dst);
        // Clip to a 2x2 window starting at (1, 1): only the top-left quadrant of `src`'s
        // footprint at (0, 0) is visible.
        surface.clip(Rect::new(1, 1, 2, 2)).blit(&src, 0, 0);
    }

    assert_eq!(dst[Pos::new(1, 1)].glyph(), '#');
    assert_eq!(dst[Pos::new(2, 2)].glyph(), '#');
    // Outside the clip: never written, even though it's inside `src`'s own footprint.
    assert_eq!(dst[Pos::new(2, 0)].glyph(), ' ');
    assert_eq!(dst[Pos::new(0, 2)].glyph(), ' ');
}

#[test]
fn blit_is_a_no_op_for_a_zero_sized_grid() {
    let src = Grid::new(1, 0);
    let mut dst = Grid::new(4, 4);
    screen(&mut dst).blit(&src, 0, 0);

    assert_eq!(dst[Pos::new(0, 0)].glyph(), ' ');
}

#[test]
fn blit_is_a_no_op_when_the_whole_footprint_is_cropped_before_the_translated_origin() {
    let mut src = Grid::new(2, 2);
    src.put_tile(0, (0, 0), Tile::new('x', Style::default()));

    let mut dst = Grid::new(4, 4);
    {
        let mut surface = screen(&mut dst);
        // Translating by +100 columns shifts `(0, 0)` to local `-100`: the whole 2-wide
        // footprint falls left of the origin, so `crop_left` (100) reaches past `w` (2).
        let mut view = surface.translate((100, 0));
        view.blit(&src, 0, 0);
    }

    assert_eq!(dst[Pos::new(0, 0)].glyph(), ' ');
}

#[test]
fn blit_is_a_no_op_when_the_shifted_x_origin_overflows_u16() {
    let mut src = Grid::new(2, 2);
    src.put_tile(0, (0, 0), Tile::new('x', Style::default()));

    let mut dst = Grid::new(4, 4);
    {
        let mut surface = screen(&mut dst);
        // Translating by -100_000 pushes the shifted local x past `u16::MAX`, which
        // `u16::try_from` refuses (distinct from the ordinary negative-crop path above).
        let mut view = surface.translate((-100_000, 0));
        view.blit(&src, 0, 0);
    }

    assert_eq!(dst[Pos::new(0, 0)].glyph(), ' ');
}

#[test]
fn blit_is_a_no_op_when_the_shifted_y_origin_overflows_u16() {
    let mut src = Grid::new(2, 2);
    src.put_tile(0, (0, 0), Tile::new('x', Style::default()));

    let mut dst = Grid::new(4, 4);
    {
        let mut surface = screen(&mut dst);
        // Same as the x-overflow case above, but on the y axis, which `blit` checks
        // separately (only after the x shift already succeeded).
        let mut view = surface.translate((0, -100_000));
        view.blit(&src, 0, 0);
    }

    assert_eq!(dst[Pos::new(0, 0)].glyph(), ' ');
}

#[test]
fn blit_is_a_no_op_when_the_destination_footprint_misses_the_clip_entirely() {
    let mut src = Grid::new(2, 2);
    src.put_tile(0, (0, 0), Tile::new('x', Style::default()));

    let mut dst = Grid::new(5, 5);
    {
        let mut surface = screen(&mut dst);
        // Clipped to the top-left cell only; the blit lands at (3, 3), whose footprint
        // doesn't overlap the clip at all (unlike the partial-overlap case tested above).
        let mut clipped = surface.clip(Rect::new(0, 0, 1, 1));
        clipped.blit(&src, 3, 3);
    }

    assert_eq!(dst[Pos::new(3, 3)].glyph(), ' ');
}

#[test]
fn put_span_takes_any_as_ref_str_row() {
    use alloc::string::String;
    use alloc::vec::Vec;

    let mut grid = Grid::new(4, 4);
    // A footprint computed at runtime: owned rows, no borrowing pass over them.
    let rows: Vec<String> = (0..2)
        .map(|row| {
            (0..2)
                .map(|col| if (row, col) == (0, 0) { 'C' } else { ' ' })
                .collect()
        })
        .collect();

    assert_eq!(
        screen(&mut grid).put_span((0, 0), &rows, Style::default()),
        Some(())
    );
    assert_eq!(grid[Pos::new(0, 0)].span(), (2, 2));
}

#[test]
fn put_span_reports_why_a_span_did_not_draw() {
    let mut grid = Grid::new(4, 4);
    let area = Rect::new(0, 0, 2, 2);
    let mut surface = Surface::new(&mut grid, area, 0);
    let style = Style::default();

    assert_eq!(surface.put_span((0, 0), &[] as &[&str], style), None);
    assert_eq!(surface.put_span((0, 0), &[""], style), None);
    // Ragged rows are refused by the grid, and that answer is passed through.
    assert_eq!(surface.put_span((0, 0), &["ab", "c"], style), None);
    // Fits the grid, but leaves the surface's own area.
    assert_eq!(surface.put_span((1, 1), &["ab"], style), None);
    assert_eq!(surface.put_span((0, 0), &["ab"], style), Some(()));
}

#[test]
fn put_span_refuses_an_axis_wider_than_255_cells() {
    let mut grid = Grid::new(300, 2);
    let mut surface = screen(&mut grid);
    let row = "a".repeat(256);

    // Fits the surface's own clip (300 columns), but 256 columns is one past what a span's
    // footprint can represent (`Tile` stores each span dimension in a `u8`).
    assert_eq!(
        surface.put_span((0, 0), &[row.as_str()], Style::default()),
        None
    );
    assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
}

#[test]
fn put_span_refuses_and_writes_nothing_when_a_later_row_is_longer_than_the_first() {
    let mut grid = Grid::new(4, 4);
    let mut surface = screen(&mut grid);

    // `span_fits` measures the footprint against the first row ("ab", 2 cols) and it fits;
    // `Grid::write_span` is what actually rejects the ragged second row ("abc", 3 cols), so
    // the refusal happens after the fits check has already passed, not before it.
    assert_eq!(
        surface.put_span((0, 0), &["ab", "abc"], Style::default()),
        None
    );
    assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    assert_eq!(grid[Pos::new(0, 1)].glyph(), ' ');
}

#[test]
fn put_span_uniform_writes_the_anchor_once_and_fills_the_rest() {
    let mut grid = Grid::new(4, 4);
    assert_eq!(
        screen(&mut grid).put_span_uniform((1, 1), (2, 2), 'C', '.', Style::default()),
        Some(())
    );

    assert_eq!(grid[Pos::new(1, 1)].glyph(), 'C');
    assert_eq!(grid[Pos::new(1, 1)].span(), (2, 2));
    assert_eq!(grid[Pos::new(2, 2)].glyph(), '.');
    assert_eq!(grid.span_owner(0, 2, 2), Some(Pos::new(1, 1)));
}

#[test]
fn put_span_uniform_writes_to_this_surfaces_layer() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        surface
            .on_layer(2)
            .put_span_uniform((0, 0), (2, 1), 'C', ' ', Style::default())
            .expect("span write");
    }

    assert_eq!(grid.span_owner(2, 1, 0), Some(Pos::new(0, 0)));
    assert_eq!(grid.span_owner(0, 1, 0), None);
}

#[test]
fn put_span_uniform_refuses_a_footprint_that_leaves_the_surfaces_area() {
    let mut grid = Grid::new(4, 4);
    let area = Rect::new(0, 0, 2, 2);
    let mut surface = Surface::new(&mut grid, area, 0);
    let style = Style::default();

    // Both fit the grid; neither fits the area.
    assert_eq!(
        surface.put_span_uniform((1, 0), (2, 1), 'C', ' ', style),
        None
    );
    assert_eq!(
        surface.put_span_uniform((0, 1), (1, 2), 'C', ' ', style),
        None
    );
    assert_eq!(
        surface.put_span_uniform((0, 0), (0, 1), 'C', ' ', style),
        None
    );
    assert_eq!(
        surface.put_span_uniform((0, 0), (2, 2), 'C', ' ', style),
        Some(())
    );
}

#[test]
fn styled_surface_forwards_both_span_calls() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        let mut styled = surface.with_style(Style::new().fg(Color::RED));
        styled.put_span((0, 0), &["ab"]).expect("span write");
        styled
            .put_span_uniform((0, 1), (2, 1), 'C', ' ')
            .expect("span write");
    }

    assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Color::RED);
    assert_eq!(grid[Pos::new(0, 1)].style().foreground(), Color::RED);
    assert_eq!(grid[Pos::new(0, 1)].span(), (2, 1));
}

#[test]
fn styled_surface_forwards_put_print_fill_rect_and_put_offset() {
    let style = Style::new().fg(Color::RED);
    let mut grid = Grid::new(6, 4);
    {
        let mut surface = screen(&mut grid);
        let mut styled = surface.with_style(style);
        assert_eq!(styled.style(), style);

        styled.put((0, 0), 'a');
        styled.print((1, 0), "bc");
        styled.fill_rect(Rect::new(0, 1, 2, 1), '#');
        styled.put_offset((0, 2), Offset::new(3, -3), 'x');
        // `surface()` reaches back to the underlying `Surface` for a call `StyledSurface`
        // doesn't expose, e.g. `print_line`'s per-span styles.
        styled.surface().print_line((0, 3), &Line::from("d"));
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), 'a');
    assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Color::RED);
    assert_eq!(grid[Pos::new(1, 0)].glyph(), 'b');
    assert_eq!(grid[Pos::new(2, 0)].glyph(), 'c');
    assert_eq!(grid[Pos::new(0, 1)].glyph(), '#');
    assert_eq!(grid[Pos::new(1, 1)].glyph(), '#');
    let tile = grid.tile(0, Pos::new(0, 2)).unwrap();
    assert_eq!((tile.dx(), tile.dy()), (3, -3));
    assert_eq!(grid[Pos::new(0, 3)].glyph(), 'd');
}

#[test]
fn with_tint_applies_to_the_cell_it_writes() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        surface
            .with_tint(Tint::multiply(128, 64, 32))
            .put((1, 1), '@', Style::default());
    }

    assert_eq!(grid[Pos::new(1, 1)].glyph(), '@');
    assert_eq!(grid.tint(0, 1, 1), Tint::multiply(128, 64, 32));
}

#[test]
fn an_untinted_surface_leaves_the_side_table_alone() {
    let mut grid = Grid::new(4, 4);
    screen(&mut grid).put((1, 1), '@', Style::default());

    assert_eq!(grid.tint(0, 1, 1), Tint::None);
}

/// `fill_rect`'s batch fast path only applies when the surface is untinted: a tinted surface
/// must fall back to the per-cell loop, or every cell `fill_rect` touches would silently lose
/// its tint.
#[test]
fn with_tint_applies_to_every_cell_fill_rect_touches() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        surface.with_tint(Tint::multiply(128, 64, 32)).fill_rect(
            Rect::new(1, 1, 2, 2),
            '#',
            Style::default(),
        );
    }

    for (x, y) in [(1, 1), (2, 1), (1, 2), (2, 2)] {
        assert_eq!(grid[Pos::new(x, y)].glyph(), '#');
        assert_eq!(grid.tint(0, x, y), Tint::multiply(128, 64, 32));
    }
}

/// `fill_rect`'s batch fast path only applies to a single-column glyph: a wide glyph must
/// fall back to the same per-cell `put` loop used before this method had a fast path, not the
/// batch `Tile::new` write (which carries no wide-char bookkeeping at all).
#[test]
fn fill_rect_with_a_wide_glyph_falls_back_to_the_put_loop() {
    let rect = Rect::new(0, 0, 6, 1);

    let mut via_fill_rect = Grid::new(8, 1);
    screen(&mut via_fill_rect).fill_rect(rect, '\u{6f22}', Style::default());

    let mut via_put = Grid::new(8, 1);
    {
        let mut surface = screen(&mut via_put);
        for y in rect.top()..rect.bottom() {
            for x in rect.left()..rect.right() {
                surface.put((x, y), '\u{6f22}', Style::default());
            }
        }
    }

    for x in 0..8 {
        assert_eq!(
            via_fill_rect[Pos::new(x, 0)],
            via_put[Pos::new(x, 0)],
            "cell ({x}, 0)"
        );
    }
}

/// `fill_rect`, `clear`, and `clear_region` all route through `Grid::fill_region`, which must
/// clear any span the fill partially overwrites the same way the per-cell loop it replaced
/// did, or the surviving span's anchor would keep claiming cells the fill just overwrote.
#[test]
fn fill_rect_clears_a_span_it_partially_overwrites() {
    use crate::tile::TileFlags;

    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        surface
            .put_span((0, 0), &["C=", "[]"], Style::default())
            .expect("2x2 span fits in a 4x4 grid");
        surface.fill_rect(Rect::new(1, 0, 3, 3), '#', Style::default());
    }

    assert!(
        !grid
            .tile(0, (0, 0))
            .unwrap()
            .flags()
            .contains(TileFlags::SPAN_ANCHOR)
    );
}

#[test]
fn clear_region_clears_a_span_it_partially_overwrites() {
    use crate::tile::TileFlags;

    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        surface
            .put_span((0, 0), &["C=", "[]"], Style::default())
            .expect("2x2 span fits in a 4x4 grid");
        surface.clear_region(Rect::new(1, 0, 3, 3));
    }

    assert!(
        !grid
            .tile(0, (0, 0))
            .unwrap()
            .flags()
            .contains(TileFlags::SPAN_ANCHOR)
    );
}

#[test]
fn fill_rect_with_a_zero_sized_rect_is_a_no_op() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        surface.fill_rect(Rect::new(1, 1, 0, 3), '#', Style::default());
        surface.fill_rect(Rect::new(1, 1, 3, 0), '#', Style::default());
    }

    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(grid[Pos::new(x, y)].glyph(), ' ', "cell ({x}, {y})");
        }
    }
}

#[test]
fn clear_region_with_a_zero_sized_rect_is_a_no_op() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        surface.fill_rect(Rect::new(0, 0, 4, 4), '#', Style::default());
        surface.clear_region(Rect::new(1, 1, 0, 2));
        surface.clear_region(Rect::new(1, 1, 2, 0));
    }

    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(grid[Pos::new(x, y)].glyph(), '#', "cell ({x}, {y})");
        }
    }
}

#[test]
fn with_tint_lands_on_the_span_anchor_only() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        surface
            .with_tint(Tint::multiply(200, 200, 200))
            .put_span((0, 0), &["ab", "cd"], Style::default())
            .expect("span write");
    }

    // A pixel backend draws the whole footprint from the anchor, so that is the only cell
    // with a sprite to recolour.
    assert_eq!(grid.tint(0, 0, 0), Tint::multiply(200, 200, 200));
    assert_eq!(grid.tint(0, 1, 0), Tint::None);
    assert_eq!(grid.tint(0, 1, 1), Tint::None);
}

#[test]
fn with_tint_applies_to_a_uniform_span_anchor() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        surface
            .with_tint(Tint::mix(255, 0, 0, 128))
            .put_span_uniform((1, 1), (2, 2), 'C', '.', Style::default())
            .expect("span write");
    }

    assert_eq!(grid.tint(0, 1, 1), Tint::mix(255, 0, 0, 128));
    assert_eq!(grid.tint(0, 2, 2), Tint::None);
}

#[test]
fn with_tint_is_not_applied_to_a_refused_span() {
    let mut grid = Grid::new(4, 4);
    let area = Rect::new(0, 0, 2, 2);
    {
        let mut surface = Surface::new(&mut grid, area, 0);
        // Fits the grid, leaves the area: nothing is written, so nothing is tinted.
        assert_eq!(
            surface
                .with_tint(Tint::multiply(1, 2, 3))
                .put_span((1, 1), &["ab"], Style::default()),
            None
        );
    }

    assert_eq!(grid.tint(0, 1, 1), Tint::None);
}

#[test]
fn with_tint_survives_clip_and_on_layer() {
    let mut grid = Grid::new(8, 4);
    {
        let mut surface = screen(&mut grid);
        let mut tinted = surface.with_tint(Tint::multiply(9, 9, 9));
        assert_eq!(tinted.tint(), Tint::multiply(9, 9, 9));
        assert_eq!(
            tinted.clip(Rect::new(0, 0, 4, 4)).tint(),
            Tint::multiply(9, 9, 9)
        );
        assert_eq!(tinted.on_layer(2).tint(), Tint::multiply(9, 9, 9));

        tinted.on_layer(2).put((1, 1), '@', Style::default());
    }

    assert_eq!(grid.tint(2, 1, 1), Tint::multiply(9, 9, 9));
}

#[test]
fn with_tint_replaces_rather_than_composes() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        let mut outer = surface.with_tint(Tint::multiply(128, 128, 128));
        // Unlike `clip`, a nested tint substitutes: two tints have no meaningful product.
        outer
            .with_tint(Tint::mix(255, 0, 0, 64))
            .put((0, 0), '@', Style::default());
    }

    assert_eq!(grid.tint(0, 0, 0), Tint::mix(255, 0, 0, 64));
}

#[test]
fn clip_narrows_the_visible_rect_but_not_the_area() {
    let mut grid = Grid::new(8, 4);
    let area = Rect::new(0, 0, 8, 4);
    let mut surface = screen(&mut grid);
    let sub = surface.clip(Rect::new(2, 1, 4, 2));

    assert_eq!(sub.clip_rect(), Rect::new(2, 1, 4, 2));
    // `area` reports what the surface represents, unaffected by `clip`.
    assert_eq!(sub.area(), area);
    assert_eq!(sub.width(), 8);
    assert_eq!(sub.height(), 4);
}

#[test]
fn clip_keeps_the_layer() {
    let mut grid = Grid::new(4, 4);
    let mut surface = screen(&mut grid);
    let mut layer1 = surface.on_layer(1);

    assert_eq!(layer1.clip(Rect::new(0, 0, 2, 2)).layer(), 1);
}

#[test]
fn layer_variants_order_low_to_high() {
    assert!(Layer::World < Layer::Hud);
    assert!(Layer::Hud < Layer::Overlay);
    assert!(Layer::Overlay < Layer::Debug);
}

#[test]
fn layer_as_u8_matches_the_documented_grid_layer_ids() {
    assert_eq!(Layer::World.as_u8(), 0);
    assert_eq!(Layer::Hud.as_u8(), 1);
    assert_eq!(Layer::Overlay.as_u8(), 2);
    assert_eq!(Layer::Debug.as_u8(), 3);
}

#[test]
fn layer_default_is_world() {
    assert_eq!(Layer::default(), Layer::World);
}

#[test]
fn layer_into_u8_matches_as_u8() {
    assert_eq!(u8::from(Layer::Overlay), Layer::Overlay.as_u8());
}

#[test]
fn on_tier_matches_on_layer_with_the_tiers_u8() {
    let mut grid = Grid::new(4, 4);
    let mut surface = screen(&mut grid);

    assert_eq!(surface.on_tier(Layer::Overlay).layer(), 2);
    assert_eq!(
        surface.on_layer(2).layer(),
        surface.on_tier(Layer::Overlay).layer()
    );
}

#[test]
fn on_tier_writes_land_on_the_tiers_grid_layer() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        surface
            .on_tier(Layer::Overlay)
            .put((0, 0), '@', Style::default());
    }

    assert_eq!(grid.tile(2, Pos::new(0, 0)).map(Tile::glyph), Some('@'));
    // Untouched on lower tiers: `on_tier` switches layers, it doesn't also write there.
    // Layer 0 is always allocated (empty), layer 1 was never written so stays unallocated.
    assert_eq!(grid.tile(0, Pos::new(0, 0)).map(Tile::glyph), Some(' '));
    assert_eq!(grid.tile(1, Pos::new(0, 0)), None);
}

#[test]
#[cfg(feature = "egc")]
fn a_wide_char_at_the_clip_edge_does_not_write_its_spacer_outside_the_clip() {
    let mut grid = Grid::new(8, 1);
    let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 8, 1), 0);

    // Clip is columns 0..4; the wide char's primary cell (column 3) is inside the clip, but
    // its spacer would land at column 4, outside it. The whole write is refused.
    surface
        .clip(Rect::new(0, 0, 4, 1))
        .put((3, 0), '\u{6f22}', Style::default());

    assert_eq!(grid.tile(0, Pos::new(3, 0)).map(Tile::glyph), Some(' '));
    assert!(
        !grid
            .tile(0, Pos::new(4, 0))
            .is_some_and(|t| t.flags().contains(crate::tile::TileFlags::WIDE_CHAR_SPACER)),
        "spacer must not be written outside the clip"
    );
}

/// The `not(egc)` twin of the test above: `put`'s `not(egc)` path (unlike `write_grapheme_at`,
/// used when `egc` is enabled) has no check that a wide char's spacer cell is also inside this
/// surface's clip before writing, so the spacer still escapes here. Documented rather than
/// asserted as fixed (see retroglyph#1007's follow-up) so a future fix has a red test to turn
/// green instead of this gap silently staying uncovered under `--no-default-features`.
#[test]
#[cfg(not(feature = "egc"))]
fn a_wide_char_at_the_clip_edge_writes_its_spacer_outside_the_clip_without_egc() {
    let mut grid = Grid::new(8, 1);
    let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 8, 1), 0);

    // Same setup as the `egc` test above: clip is columns 0..4, the wide char's primary cell
    // (column 3) is inside the clip, but its spacer would land at column 4, outside it.
    surface
        .clip(Rect::new(0, 0, 4, 1))
        .put((3, 0), '\u{6f22}', Style::default());

    assert_eq!(
        grid.tile(0, Pos::new(3, 0)).map(Tile::glyph),
        Some('\u{6f22}')
    );
    assert!(
        grid.tile(0, Pos::new(4, 0))
            .is_some_and(|t| t.flags().contains(crate::tile::TileFlags::WIDE_CHAR_SPACER)),
        "not(egc) put still writes the spacer past the clip edge"
    );
}

#[test]
fn clip_intersects_rather_than_replaces_so_it_cannot_widen() {
    let mut grid = Grid::new(8, 4);
    let area = Rect::new(2, 1, 4, 2);
    let mut surface = Surface::new(&mut grid, area, 0);

    // A rect reaching outside the surface's own clip only ever tightens it.
    assert_eq!(surface.clip(Rect::new(0, 0, 8, 4)).clip_rect(), area);
    assert_eq!(
        surface.clip(Rect::new(0, 0, 4, 4)).clip_rect(),
        Rect::new(2, 1, 2, 2)
    );
}

#[test]
fn clip_does_not_widen_the_visible_region_after_scope_narrows_it() {
    let mut grid = Grid::new(8, 4);
    let mut surface = screen(&mut grid);
    let mut scoped = surface.scope(Rect::new(1, 1, 2, 2));

    // `clip` on an already-scoped surface can only tighten what was already visible, even
    // when asked for a rect that reaches back out past it.
    assert_eq!(
        scoped.clip(Rect::new(0, 0, 8, 4)).clip_rect(),
        Rect::new(1, 1, 2, 2)
    );
}

#[test]
fn scope_sets_the_area_and_intersects_the_clip() {
    let mut grid = Grid::new(8, 4);
    let mut surface = screen(&mut grid);
    let mut clipped = surface.clip(Rect::new(0, 0, 4, 4));

    // `scope` widens `area` to a rect the parent's clip does not fully cover...
    let scoped = clipped.scope(Rect::new(2, 0, 4, 4));
    assert_eq!(scoped.area(), Rect::new(2, 0, 4, 4));
    // ...but the visible region still cannot exceed the parent's own clip.
    assert_eq!(scoped.clip_rect(), Rect::new(2, 0, 2, 4));
}

#[test]
fn scope_inside_a_clipped_surface_cannot_widen_the_visible_region() {
    let mut grid = Grid::new(8, 4);
    let mut surface = screen(&mut grid);
    let mut clipped = surface.clip(Rect::new(2, 1, 2, 2));

    // Even a `scope` rect that reaches well outside the parent's clip only ever tightens
    // what is visible; `area` still becomes exactly the requested rect.
    let scoped = clipped.scope(Rect::new(0, 0, 8, 4));
    assert_eq!(scoped.area(), Rect::new(0, 0, 8, 4));
    assert_eq!(scoped.clip_rect(), Rect::new(2, 1, 2, 2));
}

#[test]
fn scope_writes_outside_the_inherited_clip_are_dropped() {
    let mut grid = Grid::new(8, 4);
    {
        let mut surface = screen(&mut grid);
        let mut clipped = surface.clip(Rect::new(0, 0, 4, 4));
        let mut scoped = clipped.scope(Rect::new(0, 0, 8, 4));
        // Inside the requested `area`, outside the inherited clip.
        scoped.put((5, 0), 'a', Style::default());
        scoped.put((1, 0), 'b', Style::default());
    }

    assert_eq!(grid[Pos::new(5, 0)].glyph(), ' ');
    assert_eq!(grid[Pos::new(1, 0)].glyph(), 'b');
}

#[test]
fn clip_writes_outside_the_sub_rect_are_dropped() {
    let mut grid = Grid::new(4, 2);
    {
        let mut surface = screen(&mut grid);
        let mut top = surface.clip(Rect::new(0, 0, 4, 1));
        top.put((1, 0), 'a', Style::default());
        // Inside the surface's own area, outside the clip.
        top.put((1, 1), 'b', Style::default());
    }

    assert_eq!(grid[Pos::new(1, 0)].glyph(), 'a');
    assert_eq!(grid[Pos::new(1, 1)].glyph(), ' ');
}

#[test]
fn clip_to_one_row_drops_print_overflow_instead_of_wrapping_it() {
    let mut grid = Grid::new(4, 2);
    {
        let mut surface = screen(&mut grid);
        surface
            .clip(Rect::new(0, 0, 4, 1))
            .print((0, 0), "abcdef", Style::default());
    }

    assert_eq!(grid[Pos::new(3, 0)].glyph(), 'd');
    // "ef" wrapped onto row 1, which the clip excludes.
    assert_eq!(grid[Pos::new(0, 1)].glyph(), ' ');
}

#[test]
fn print_wraps_at_the_surfaces_own_width_not_at_clip_right() {
    // `area` starts at column 4, so the surface-local wrap column (4) is smaller than the
    // absolute grid column `clip.right()` resolves to (8). Wrapping must use the former.
    let mut grid = Grid::new(8, 2);
    let mut surface = Surface::new(&mut grid, Rect::new(4, 0, 4, 2), 0);
    surface.print((0, 0), "abcdef", Style::default());

    assert_eq!(grid[Pos::new(4, 0)].glyph(), 'a');
    assert_eq!(grid[Pos::new(7, 0)].glyph(), 'd');
    assert_eq!(grid[Pos::new(4, 1)].glyph(), 'e');
    assert_eq!(grid[Pos::new(5, 1)].glyph(), 'f');
}

#[test]
fn print_line_measures_its_span_skip_threshold_against_the_surfaces_own_width() {
    // `area` starts at column 4, so the surface-local skip column (4) is smaller than the
    // absolute grid column `clip.right()` resolves to (8). The second span starts at local
    // column 3, which is inside the 4-wide area, so it must still print.
    use crate::text::Span;
    use alloc::vec;

    let mut grid = Grid::new(8, 1);
    let mut surface = Surface::new(&mut grid, Rect::new(4, 0, 4, 1), 0);
    let line = Line::from(vec![Span::raw("ab"), Span::raw("cd")]);
    surface.print_line((0, 0), &line);

    assert_eq!(grid[Pos::new(4, 0)].glyph(), 'a');
    assert_eq!(grid[Pos::new(5, 0)].glyph(), 'b');
    assert_eq!(grid[Pos::new(6, 0)].glyph(), 'c');
    assert_eq!(grid[Pos::new(7, 0)].glyph(), 'd');
}

#[test]
fn clip_makes_put_span_measure_its_footprint_against_the_sub_rect() {
    let mut grid = Grid::new(4, 3);
    {
        let mut surface = screen(&mut grid);
        // Fits the grid, but reserves a cell on the bottom row the clip excludes.
        surface
            .clip(Rect::new(0, 0, 4, 2))
            .put_span((0, 1), &["ab", "cd"], Style::default());
    }

    assert_eq!(grid[Pos::new(0, 1)].glyph(), ' ');

    let mut surface = screen(&mut grid);
    surface
        .clip(Rect::new(0, 0, 4, 2))
        .put_span((0, 0), &["ab", "cd"], Style::default());

    assert_eq!(grid[Pos::new(0, 0)].span(), (2, 2));
}

#[test]
fn clip_makes_put_span_uniform_measure_its_footprint_against_the_sub_rect() {
    let mut grid = Grid::new(4, 3);
    let style = Style::default();
    {
        let mut surface = screen(&mut grid);
        let mut content = surface.clip(Rect::new(0, 0, 4, 2));
        // Fits the grid, but reserves a cell on the bottom row the clip excludes.
        assert_eq!(
            content.put_span_uniform((0, 1), (2, 2), 'C', '.', style),
            None
        );
        assert_eq!(
            content.put_span_uniform((0, 0), (2, 2), 'C', '.', style),
            Some(())
        );
    }

    assert_eq!(grid[Pos::new(0, 0)].span(), (2, 2));
    assert_eq!(grid[Pos::new(0, 2)].glyph(), ' ');
}

#[test]
fn clip_to_a_disjoint_rect_is_empty_and_drops_every_write() {
    let mut grid = Grid::new(8, 4);
    {
        let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);
        let mut sub = surface.clip(Rect::new(4, 0, 4, 4));
        assert_eq!(sub.clip_rect(), Rect::EMPTY);
        sub.print((0, 0), "abc", Style::default());
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
}

#[test]
fn clip_to_a_zero_width_rect_is_empty_and_drops_every_write() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        let mut sub = surface.clip(Rect::new(1, 1, 0, 2));
        assert_eq!(sub.clip_rect(), Rect::EMPTY);
        sub.put((1, 1), 'X', Style::default());
    }

    assert_eq!(grid[Pos::new(1, 1)].glyph(), ' ');
}

#[test]
fn scope_to_a_zero_height_rect_is_empty_and_drops_every_write() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        let mut sub = surface.scope(Rect::new(1, 1, 2, 0));
        assert_eq!(sub.area(), Rect::new(1, 1, 2, 0));
        assert_eq!(sub.clip_rect(), Rect::EMPTY);
        sub.put((0, 0), 'X', Style::default());
    }

    assert_eq!(grid[Pos::new(1, 1)].glyph(), ' ');
}

#[test]
fn print_with_an_empty_string_is_a_no_op() {
    let mut grid = Grid::new(4, 4);
    screen(&mut grid).print((0, 0), "", Style::default());

    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(grid[Pos::new(x, y)].glyph(), ' ', "cell ({x}, {y})");
        }
    }
}

#[test]
fn print_line_with_an_empty_line_is_a_no_op() {
    let mut grid = Grid::new(4, 4);
    screen(&mut grid).print_line((0, 0), &Line::default());

    assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
}

#[test]
fn print_and_fill_rect_are_no_ops_on_a_surface_with_an_empty_area() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = Surface::new(&mut grid, Rect::EMPTY, 0);
        surface.print((0, 0), "hi", Style::default());
        surface.fill_rect(Rect::new(0, 0, 4, 4), '#', Style::default());
    }

    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(grid[Pos::new(x, y)].glyph(), ' ', "cell ({x}, {y})");
        }
    }
}

#[test]
fn put_signed_drops_a_negative_coordinate() {
    let mut grid = Grid::new(4, 4);
    let mut surface = screen(&mut grid);

    surface.put_signed((-1, 0), 'X', Style::default());
    surface.put_signed((0, -1), 'X', Style::default());

    assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
}

#[test]
fn put_signed_lands_a_valid_coordinate_at_the_area_origin() {
    let mut grid = Grid::new(4, 4);
    let area = Rect::new(1, 1, 2, 2);
    let mut surface = Surface::new(&mut grid, area, 0);

    // (0, 0) relative to the area's own origin is grid position (1, 1).
    surface.put_signed((0, 0), 'X', Style::default());

    assert_eq!(grid[Pos::new(1, 1)].glyph(), 'X');
}

#[test]
fn put_signed_drops_a_coordinate_past_this_surfaces_width_or_height() {
    let mut grid = Grid::new(4, 4);
    let area = Rect::new(0, 0, 2, 2);
    let mut surface = Surface::new(&mut grid, area, 0);

    // Fits the grid, but not this surface's own (relative) width/height.
    surface.put_signed((2, 0), 'X', Style::default());
    surface.put_signed((0, 2), 'X', Style::default());

    assert_eq!(grid[Pos::new(2, 0)].glyph(), ' ');
    assert_eq!(grid[Pos::new(0, 2)].glyph(), ' ');
}

// Not gated behind `egc`: `Tile::new` sets `WIDE_CHAR`/`WIDE_CHAR_SPACER` on every feature
// combination (unlike the clip-edge tests above, this bookkeeping goes through
// `Grid::put_tile` either way, not through the `egc`-only grapheme path).
#[test]
fn put_signed_does_wide_char_bookkeeping_like_put() {
    use crate::tile::TileFlags;

    let mut grid = Grid::new(8, 2);
    {
        let mut surface = screen(&mut grid);

        // A wide char via `put`: primary at (0, 0), spacer at (1, 0).
        surface.put((0, 0), '\u{6f22}', Style::default());
        // Overwriting the primary cell via `put_signed` must clear the orphaned spacer too.
        surface.put_signed((0, 0), 'a', Style::default());
    }
    assert!(
        !grid[Pos::new(1, 0)]
            .flags()
            .contains(TileFlags::WIDE_CHAR_SPACER)
    );

    // Writing a wide char via `put_signed` must set the flags and spacer `put` would.
    screen(&mut grid).put_signed((3, 0), '\u{6f22}', Style::default());
    assert!(grid[Pos::new(3, 0)].flags().contains(TileFlags::WIDE_CHAR));
    assert!(
        grid[Pos::new(4, 0)]
            .flags()
            .contains(TileFlags::WIDE_CHAR_SPACER)
    );
}

#[test]
fn put_offset_applies_the_surfaces_tint() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        surface.with_tint(Tint::multiply(128, 64, 32)).put_offset(
            (1, 1),
            Offset::new(2, -2),
            'X',
            Style::default(),
        );
    }

    assert_eq!(grid.tint(0, 1, 1), Tint::multiply(128, 64, 32));
}

#[test]
fn put_offset_still_carries_the_pixel_offset() {
    let mut grid = Grid::new(4, 4);
    let mut surface = screen(&mut grid);

    surface.put_offset((1, 1), Offset::new(2, -2), 'X', Style::default());

    let tile = grid.tile(0, Pos::new(1, 1)).unwrap();
    assert_eq!((tile.dx(), tile.dy()), (2, -2));
}

#[test]
fn put_offset_on_a_refused_write_leaves_the_targets_glyph_and_offset_alone() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        let mut clipped = surface.clip(Rect::new(0, 0, 2, 2));
        clipped.put((1, 1), 'X', Style::default());
        // (3, 3) is inside the grid but outside the clip: refused entirely (retroglyph#998:
        // this used to still set `dx`/`dy` on a cell the glyph write itself never touched).
        clipped.put_offset((3, 3), Offset::new(5, -5), 'Y', Style::default());
    }

    // Nothing was ever written at the refused write's own target.
    let target = grid.tile(0, Pos::new(3, 3)).unwrap();
    assert_eq!(target.glyph(), ' ');
    assert_eq!((target.dx(), target.dy()), (0, 0));
    // The glyph inside the clip is unaffected.
    assert_eq!(grid[Pos::new(1, 1)].glyph(), 'X');
}

#[test]
fn translate_does_not_change_area_width_or_height() {
    let mut grid = Grid::new(10, 10);
    let mut surface = screen(&mut grid);
    let mut scoped = surface.scope(Rect::new(5, 5, 4, 4));
    let view = scoped.translate((-5, -5));

    assert_eq!(view.area(), Rect::new(5, 5, 4, 4));
    assert_eq!(view.clip_rect(), Rect::new(5, 5, 4, 4));
    assert_eq!(view.width(), 4);
    assert_eq!(view.height(), 4);
}

#[test]
fn clip_translate_does_not_change_area_local_area_width_or_height() {
    let mut grid = Grid::new(10, 10);
    let mut surface = screen(&mut grid);
    let view = surface.clip_translate(Rect::new(5, 5, 4, 4), (-5, -5));

    assert_eq!(view.area(), Rect::new(5, 5, 4, 4));
    assert_eq!(view.local_area(), Rect::new(0, 0, 4, 4));
    assert_eq!(view.width(), 4);
    assert_eq!(view.height(), 4);
}

#[test]
fn translate_saturates_at_i32_max_across_repeated_calls_instead_of_overflowing() {
    let mut grid = Grid::new(4, 4);
    let mut surface = screen(&mut grid);
    // Composing `(i32::MAX - 1, 0)` then `(5, 0)` would overflow a plain `+` past `i32::MAX`;
    // `saturating_add` instead pins the composed origin at `i32::MAX`.
    let mut once = surface.translate((i32::MAX - 1, 0));
    let mut view = once.translate((5, 0));

    // Every coordinate this surface can express (`u16`) minus an origin pinned at `i32::MAX`
    // stays deeply negative, so every write is dropped rather than landing somewhere
    // unexpected, or panicking on the intermediate overflow.
    view.put((0, 0), 'A', Style::default());
    view.put((3, 3), 'B', Style::default());

    assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    assert_eq!(grid[Pos::new(3, 3)].glyph(), ' ');
}

#[test]
fn translate_saturates_at_i32_min_across_repeated_calls_instead_of_overflowing() {
    let mut grid = Grid::new(4, 4);
    let mut surface = screen(&mut grid);
    let mut once = surface.translate((i32::MIN + 1, 0));
    let mut view = once.translate((-5, 0));

    // Pinned at `i32::MIN`: `shift`'s `checked_sub` cannot represent `x - i32::MIN` in an
    // `i32` for any grid coordinate, so it returns `None` and the write is dropped rather
    // than panicking on the subtraction.
    view.put((0, 0), 'A', Style::default());

    assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
}

#[test]
fn translate_composes_with_scope_and_clip_rect() {
    let mut grid = Grid::new(10, 10);
    let mut surface = screen(&mut grid);
    let mut clipped = surface.clip(Rect::new(0, 0, 6, 6));
    // `scope` widens `area` past the parent's clip; `translate` on top must not change
    // either `area` or the still-narrower `clip_rect` it composed with.
    let mut scoped = clipped.scope(Rect::new(4, 4, 4, 4));
    let view = scoped.translate((-4, -4));

    assert_eq!(view.area(), Rect::new(4, 4, 4, 4));
    assert_eq!(view.clip_rect(), Rect::new(4, 4, 2, 2));
}

#[test]
fn translate_shifts_put_by_subtracting_the_origin() {
    let mut grid = Grid::new(10, 10);
    {
        let mut surface = screen(&mut grid);
        let mut view = surface.translate((3, 3));

        // (3, 3) minus the translate origin (3, 3) is (0, 0).
        view.put((3, 3), 'A', Style::default());
        // (2, 3) minus (3, 3) is negative on the x axis: out of bounds, dropped.
        view.put((2, 3), 'B', Style::default());
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), 'A');
    assert_eq!(grid[Pos::new(0, 3)].glyph(), ' ');
}

#[test]
fn translate_composes_with_scope_and_lets_a_negative_signed_coordinate_land() {
    let mut grid = Grid::new(10, 10);
    {
        let mut surface = screen(&mut grid);
        let mut scoped = surface.scope(Rect::new(5, 5, 4, 4));
        let mut view = scoped.translate((-5, -5));

        // -5 minus the translate origin (-5) is 0: the viewport's own local origin, landing
        // at the scoped area's top-left grid cell.
        view.put_signed((-5, -5), 'X', Style::default());
        // -6 minus -5 is still -1: still negative, so still out of bounds.
        view.put_signed((-6, -6), 'Y', Style::default());
    }

    assert_eq!(grid[Pos::new(5, 5)].glyph(), 'X');
    assert_eq!(grid[Pos::new(4, 4)].glyph(), ' ');
}

#[test]
fn translate_composes_with_clip_and_shifts_put_the_same_way_as_put_signed() {
    // A regression test for a `shift` bug: `clip`'s area does not start at the grid's own
    // `(0, 0)` (unlike every other `clip` + `translate` test above), and the translate origin
    // is not simply `-area.left()`, so `put` must re-derive the same absolute cell
    // `put_signed` already did rather than landing on the clipped area's raw absolute
    // coordinates.
    let mut grid = Grid::new(20, 20);
    {
        let mut surface = screen(&mut grid);
        let mut view = surface.clip_translate(Rect::new(5, 5, 10, 10), (45, 45));

        // (50, 50) minus the origin (45, 45) is (5, 5): the clipped area's local (5, 5),
        // landing at absolute grid (10, 10), not at (5, 5), which is what the pre-fix
        // `shift` incorrectly produced by using the clip's raw absolute bounds instead of
        // re-adding the area's own top-left.
        view.put((50, 50), '@', Style::default());
    }

    assert_eq!(grid[Pos::new(10, 10)].glyph(), '@');
    assert_eq!(grid[Pos::new(5, 5)].glyph(), ' ');
}

#[test]
fn translate_composes_additively_across_two_calls() {
    let mut grid = Grid::new(10, 10);
    {
        let mut surface = screen(&mut grid);
        let mut once = surface.translate((2, 0));
        let mut twice = once.translate((1, 0));

        // Composed origin is (3, 0): (3, 0) minus (3, 0) is (0, 0).
        twice.put((3, 0), 'A', Style::default());
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), 'A');
}

#[test]
fn translate_shifts_fill_rect_print_and_clear_region_via_put() {
    let mut grid = Grid::new(10, 10);
    {
        let mut surface = screen(&mut grid);
        let mut view = surface.translate((5, 5));
        view.fill_rect(Rect::new(5, 5, 2, 2), '#', Style::default());
        view.print((5, 6), "a", Style::default());
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), '#');
    assert_eq!(grid[Pos::new(1, 1)].glyph(), '#');
    assert_eq!(grid[Pos::new(0, 1)].glyph(), 'a');
}

#[test]
fn translate_shifts_print_wrap_to_the_areas_own_width_not_the_local_offset_width() {
    // retroglyph#991: the wrap threshold used to stay in area-local space while `cx` advanced
    // in translated space, so it fired `origin_offset.0` columns early instead of at the
    // area's own 10-column width.
    let mut grid = Grid::new(10, 4);
    {
        let mut surface = screen(&mut grid);
        let mut view = surface.translate((5, 0));
        view.print((5, 0), "abcdefghij", Style::default());
    }

    // All 10 characters fit on row 0; nothing wraps to row 1.
    assert_eq!(grid[Pos::new(0, 0)].glyph(), 'a');
    assert_eq!(grid[Pos::new(9, 0)].glyph(), 'j');
    assert_eq!(grid[Pos::new(0, 1)].glyph(), ' ');
}

#[test]
fn translate_shifts_print_line_which_still_emits_its_spans() {
    // retroglyph#991: the span-skip threshold had the same area-local-vs-translated mismatch,
    // so on a translated surface `cx >= right` was true immediately and every span was
    // dropped instead of printed.
    use crate::text::Span;
    use alloc::vec;

    let mut grid = Grid::new(4, 1);
    {
        let mut surface = screen(&mut grid);
        let mut view = surface.translate((10, 0));
        let line = Line::from(vec![Span::raw("ab"), Span::raw("cd")]);
        view.print_line((10, 0), &line);
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), 'a');
    assert_eq!(grid[Pos::new(1, 0)].glyph(), 'b');
    assert_eq!(grid[Pos::new(2, 0)].glyph(), 'c');
    assert_eq!(grid[Pos::new(3, 0)].glyph(), 'd');
}

#[test]
fn translate_shifts_clear_region() {
    let mut grid = Grid::new(10, 10);
    {
        let mut surface = screen(&mut grid);
        surface.fill_rect(Rect::new(0, 0, 4, 4), '#', Style::default());
        let mut view = surface.translate((2, 2));
        // Clears grid (0..2, 0..2) once shifted by the translate origin.
        view.clear_region(Rect::new(2, 2, 2, 2));
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    assert_eq!(grid[Pos::new(1, 1)].glyph(), ' ');
    assert_eq!(grid[Pos::new(2, 2)].glyph(), '#');
}

#[test]
fn translate_shifts_put_span_and_put_span_uniform() {
    let mut grid = Grid::new(10, 10);
    {
        let mut surface = screen(&mut grid);
        let mut view = surface.translate((4, 4));
        assert_eq!(view.put_span((4, 4), &["ab"], Style::default()), Some(()));
        assert_eq!(
            view.put_span_uniform((6, 4), (2, 1), 'C', ' ', Style::default()),
            Some(())
        );
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), 'a');
    assert_eq!(grid[Pos::new(2, 0)].glyph(), 'C');
}

#[test]
fn translate_shifts_print_and_still_wraps_at_the_surfaces_own_width() {
    // A 5-char string, unlike `translate_shifts_fill_rect_print_and_clear_region_via_put`'s
    // one-character print, is long enough to actually cross the wrap column at the area's own
    // 4-column width.
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 2), 0);
        let mut view = surface.translate((2, 0));
        view.print((2, 0), "abcde", Style::default());
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), 'a');
    assert_eq!(grid[Pos::new(1, 0)].glyph(), 'b');
    assert_eq!(grid[Pos::new(2, 0)].glyph(), 'c');
    assert_eq!(grid[Pos::new(3, 0)].glyph(), 'd');
    // Wrapped to row 1 at the area's own width (4 columns, retroglyph#991's fix), still
    // shifted by the translate origin rather than one column early or late.
    assert_eq!(grid[Pos::new(0, 1)].glyph(), 'e');
}

#[test]
fn translate_shifts_print_line() {
    use crate::text::Span;
    use alloc::vec;

    let mut grid = Grid::new(6, 4);
    {
        let mut surface = screen(&mut grid);
        let mut view = surface.translate((2, 0));
        let line = Line::from(vec![Span::raw("ab"), Span::raw("cd")]);
        view.print_line((2, 0), &line);
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), 'a');
    assert_eq!(grid[Pos::new(1, 0)].glyph(), 'b');
    assert_eq!(grid[Pos::new(2, 0)].glyph(), 'c');
    assert_eq!(grid[Pos::new(3, 0)].glyph(), 'd');
}

#[test]
fn translate_shifts_print_aligned() {
    let mut grid = Grid::new(8, 1);
    {
        let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 6, 1), 0);
        let mut view = surface.translate((2, 0));
        // Without the translate, centering "hi" in a 6-wide rect lands it at columns 2..4;
        // shifted by (2, 0), it lands 2 columns earlier instead.
        view.print_aligned(
            Rect::new(0, 0, 6, 1),
            "hi",
            crate::layout::HAlign::Center,
            Style::default(),
        );
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), 'h');
    assert_eq!(grid[Pos::new(1, 0)].glyph(), 'i');
}

#[test]
fn translate_composes_with_with_style() {
    let mut grid = Grid::new(10, 10);
    {
        let mut surface = screen(&mut grid);
        let mut view = surface.translate((3, 3));
        let mut styled = view.with_style(Style::new().fg(Color::RED));
        styled.put((3, 3), 'A');
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), 'A');
    assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Color::RED);
}

#[test]
fn clear_is_unaffected_by_translate() {
    let mut grid = Grid::new(4, 4);
    {
        let mut surface = screen(&mut grid);
        surface.fill_rect(Rect::new(0, 0, 4, 4), '#', Style::default());
        let mut view = surface.translate((100, 100));
        // `clear` takes no coordinate, so the translate offset does not apply to it: it
        // always clears this surface's own area.
        view.clear();
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    assert_eq!(grid[Pos::new(3, 3)].glyph(), ' ');
}

#[test]
fn clear_only_clears_the_intersection_of_area_and_clip() {
    let mut grid = Grid::new(6, 6);
    {
        let mut surface = screen(&mut grid);
        surface.fill_rect(Rect::new(0, 0, 6, 6), '#', Style::default());
        // `area` is the whole grid; `clip` narrows to a 2x2 window inside it.
        surface.clip(Rect::new(2, 2, 2, 2)).clear();
    }

    // Inside `area \u2229 clip`: cleared.
    assert_eq!(grid[Pos::new(2, 2)].glyph(), ' ');
    assert_eq!(grid[Pos::new(3, 3)].glyph(), ' ');
    // Inside `area`, outside `clip`: untouched.
    assert_eq!(grid[Pos::new(0, 0)].glyph(), '#');
    assert_eq!(grid[Pos::new(5, 5)].glyph(), '#');
}

#[test]
fn clear_on_a_scoped_surface_clears_the_scoped_area_intersected_with_the_parent_clip() {
    let mut grid = Grid::new(6, 6);
    {
        let mut surface = screen(&mut grid);
        surface.fill_rect(Rect::new(0, 0, 6, 6), '#', Style::default());
        let mut clipped = surface.clip(Rect::new(0, 0, 4, 4));
        // `scope` widens `area` to a rect the parent's clip does not fully cover.
        clipped.scope(Rect::new(2, 2, 4, 4)).clear();
    }

    // `area \u2229 clip` == (2, 2, 2, 2): cleared.
    assert_eq!(grid[Pos::new(2, 2)].glyph(), ' ');
    assert_eq!(grid[Pos::new(3, 3)].glyph(), ' ');
    // Inside the scoped `area` but outside the parent's clip: untouched.
    assert_eq!(grid[Pos::new(5, 5)].glyph(), '#');
    // Outside the scoped `area` entirely: untouched.
    assert_eq!(grid[Pos::new(0, 0)].glyph(), '#');
}

#[test]
fn grid_is_the_read_only_counterpart_of_grid_mut() {
    let mut grid = Grid::new(4, 4);
    let mut surface = screen(&mut grid);
    surface.put((1, 1), 'X', Style::default());

    assert_eq!(surface.grid()[Pos::new(1, 1)].glyph(), 'X');
}

#[test]
fn tile_reads_a_written_cell_without_a_mutable_borrow() {
    let mut grid = Grid::new(4, 4);
    let mut surface = screen(&mut grid);
    surface.put((1, 1), 'X', Style::default());

    assert_eq!(surface.tile((1, 1)).map(Tile::glyph), Some('X'));
    assert_eq!(surface.tile((0, 0)).map(Tile::glyph), Some(' '));
    assert_eq!(surface.tile((10, 10)), None);
}

#[test]
fn background_reads_the_styles_background_colour() {
    let mut grid = Grid::new(4, 4);
    let mut surface = screen(&mut grid);
    surface.put((1, 1), 'X', Style::new().bg(Color::RED));

    assert_eq!(surface.background((1, 1)), Some(Color::RED));
    assert_eq!(surface.background((10, 10)), None);
}

#[test]
fn tile_returns_none_on_an_unallocated_layer() {
    let mut grid = Grid::new(4, 4);
    let mut surface = screen(&mut grid);
    let unallocated = surface.on_layer(1);

    // Layer 0 is always allocated (even empty); layer 1 was never written to, so it isn't
    // allocated at all, a third reason `tile` answers `None`, distinct from "nothing was ever
    // written at this cell" and "out of bounds".
    assert_eq!(unallocated.tile((0, 0)), None);
}

#[test]
fn background_returns_none_on_an_unallocated_layer() {
    let mut grid = Grid::new(4, 4);
    let mut surface = screen(&mut grid);
    let unallocated = surface.on_layer(1);

    assert_eq!(unallocated.background((0, 0)), None);
}

#[test]
fn print_aligned_left_aligns_by_default() {
    let mut grid = Grid::new(8, 1);
    {
        let mut surface = screen(&mut grid);
        surface.print_aligned(
            Rect::new(0, 0, 8, 1),
            "hi",
            crate::layout::HAlign::Left,
            Style::default(),
        );
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), 'h');
    assert_eq!(grid[Pos::new(1, 0)].glyph(), 'i');
    assert_eq!(grid[Pos::new(2, 0)].glyph(), ' ');
}

#[test]
fn print_aligned_centers_matching_text_layouts_own_saturating_formula() {
    let mut grid = Grid::new(6, 1);
    {
        let mut surface = screen(&mut grid);
        surface.print_aligned(
            Rect::new(0, 0, 6, 1),
            "hi",
            crate::layout::HAlign::Center,
            Style::default(),
        );
    }

    // (6 - 2) / 2 == 2 columns of left padding, matching `HAlign::Center` in `align.rs`.
    assert_eq!(grid[Pos::new(2, 0)].glyph(), 'h');
    assert_eq!(grid[Pos::new(3, 0)].glyph(), 'i');
}

#[test]
fn print_aligned_right_aligns_flush_to_the_rects_right_edge() {
    let mut grid = Grid::new(6, 1);
    {
        let mut surface = screen(&mut grid);
        surface.print_aligned(
            Rect::new(0, 0, 6, 1),
            "hi",
            crate::layout::HAlign::Right,
            Style::default(),
        );
    }

    assert_eq!(grid[Pos::new(4, 0)].glyph(), 'h');
    assert_eq!(grid[Pos::new(5, 0)].glyph(), 'i');
}

#[test]
fn print_aligned_does_not_panic_or_underflow_on_text_wider_than_the_rect() {
    let mut grid = Grid::new(4, 1);
    {
        let mut surface = screen(&mut grid);
        // "hello" is wider than the 4-column rect on every alignment: this must not panic
        // (a plain `rect.width() - text_width` would underflow) and instead left-aligns and
        // lets `print` clip the overflow.
        surface.print_aligned(
            Rect::new(0, 0, 4, 1),
            "hello",
            crate::layout::HAlign::Center,
            Style::default(),
        );
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), 'h');
    assert_eq!(grid[Pos::new(3, 0)].glyph(), 'l');
}

#[test]
fn print_aligned_clips_to_this_surfaces_own_area_as_well_as_rect() {
    let mut grid = Grid::new(4, 1);
    {
        let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 2, 1), 0);
        // `rect` extends past this surface's own area; the write is still clipped to it.
        surface.print_aligned(
            Rect::new(0, 0, 4, 1),
            "hi",
            crate::layout::HAlign::Right,
            Style::default(),
        );
    }

    assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    assert_eq!(grid[Pos::new(1, 0)].glyph(), ' ');
}

#[test]
fn print_aligned_right_on_an_offset_surface_lands_within_the_visible_columns() {
    let mut grid = Grid::new(8, 1);
    {
        // `area` starts at column 2, not 0: local and absolute coordinates now differ.
        let mut surface = Surface::new(&mut grid, Rect::new(2, 0, 6, 1), 0);
        surface.print_aligned(
            Rect::new(0, 0, 6, 1),
            "hi",
            crate::layout::HAlign::Right,
            Style::default(),
        );
    }

    // `rect` (0, 0, 6, 1) intersected with the surface's own clip (2, 0, 6, 1) leaves
    // columns 2..6 visible; right-aligning "hi" within `rect` puts it at columns 4..6.
    assert_eq!(grid[Pos::new(4, 0)].glyph(), 'h');
    assert_eq!(grid[Pos::new(5, 0)].glyph(), 'i');
}

#[test]
fn print_aligned_center_on_an_offset_surface_lands_within_the_visible_columns() {
    let mut grid = Grid::new(8, 1);
    {
        let mut surface = Surface::new(&mut grid, Rect::new(2, 0, 6, 1), 0);
        surface.print_aligned(
            Rect::new(0, 0, 6, 1),
            "hi",
            crate::layout::HAlign::Center,
            Style::default(),
        );
    }

    // (6 - 2) / 2 == 2 columns of left padding within `rect`, so "hi" lands at absolute
    // columns 2..4, which is inside the surface's own visible columns 2..6.
    assert_eq!(grid[Pos::new(2, 0)].glyph(), 'h');
    assert_eq!(grid[Pos::new(3, 0)].glyph(), 'i');
}

#[test]
fn clip_nests_monotonically() {
    let mut grid = Grid::new(8, 4);
    let mut surface = screen(&mut grid);
    let mut outer = surface.clip(Rect::new(1, 1, 4, 2));
    let inner = outer.clip(Rect::new(0, 0, 8, 4));

    assert_eq!(inner.clip_rect(), Rect::new(1, 1, 4, 2));
}
