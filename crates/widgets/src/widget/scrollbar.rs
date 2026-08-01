//! [`Scrollbar`]: a vertical track+thumb indicator.
use retroglyph_core::{Color, Frame, Rect, Style};

use super::{AnimatedWidget, InteractiveWidget, Widget};
use crate::Response;
use crate::ScrollState;
use crate::Sense;
use crate::Surface;
use crate::Theme;
use crate::draw::{offset_for_pos, thumb_geometry};

/// A vertical scrollbar (typically one cell wide) covering `total_len`
/// items in a `visible_len`-row viewport.
///
/// `offset` defaults to `0`; `track_style`/`thumb_style` default to
/// [`Style::new()`]. Set whichever a caller needs via
/// [`Scrollbar::offset`]/[`Scrollbar::track_style`]/[`Scrollbar::thumb_style`].
///
/// `track_style` fills the whole strip, then [`crate::draw::thumb_geometry`]'s
/// span (if any) is redrawn with `thumb_style` on top. Draws just the plain
/// track, with no thumb, if there's nothing to scroll: see
/// [`crate::draw::thumb_geometry`].
///
/// As a plain [`Widget`], `Scrollbar` is purely a display: `offset` is whatever the caller last
/// set, unaffected by the pointer. [`InteractiveWidget`]'s `type State = ScrollState` makes it
/// draggable and wheel-scrollable instead: `sense()` is
/// <code>[Sense::drag]() | [Sense::CLICK](crate::Sense::CLICK) |
/// [Sense::SCROLL](crate::Sense::SCROLL)</code>, and its `render` reads the thumb's position from
/// `state.integer_offset()`, drives a drag via [`offset_for_pos`]/[`ScrollState::update_drag`]/
/// [`ScrollState::end_drag`] using [`Response::pointer_pos`], and applies wheel input via
/// [`ScrollState::apply`].
///
/// # Examples
///
/// ```
/// use retroglyph_core::{Grid, Rect};
/// use retroglyph_widgets::{Scrollbar, Surface, Widget};
///
/// let area = Rect::new(0, 0, 1, 10);
/// let mut grid = Grid::new(1, 10);
/// let scrollbar = Scrollbar::new(100, 10).offset(20);
/// Widget::render(&scrollbar, &mut Surface::new(&mut grid, area, 0));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Scrollbar {
    total_len: usize,
    visible_len: usize,
    offset: usize,
    track_style: Style,
    thumb_style: Style,
}

impl Scrollbar {
    /// A scrollbar covering `total_len` items in a `visible_len`-row
    /// viewport, starting at offset `0` in the default style.
    #[must_use]
    pub fn new(total_len: usize, visible_len: usize) -> Self {
        Self {
            total_len,
            visible_len,
            offset: 0,
            track_style: Style::new(),
            thumb_style: Style::new(),
        }
    }

    /// Set the scroll offset the thumb is drawn at.
    #[must_use]
    pub const fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Set the track's style.
    #[must_use]
    pub const fn track_style(mut self, style: Style) -> Self {
        self.track_style = style;
        self
    }

    /// Set the thumb's style.
    #[must_use]
    pub const fn thumb_style(mut self, style: Style) -> Self {
        self.thumb_style = style;
        self
    }

    /// Applies `theme`'s named roles to this scrollbar: `track_style` becomes `theme.panel_bg`
    /// (the same surface the scrolled content sits on), and `thumb_style` becomes `theme.border`:
    /// a subtle divider-like color rather than `theme.accent`, so a themed scrollbar doesn't
    /// compete with an actually-selected/focused control for attention.
    ///
    /// Call before any manual [`Scrollbar::track_style`]/[`Scrollbar::thumb_style`] override you
    /// want to keep.
    #[must_use]
    pub fn theme(self, theme: Theme) -> Self {
        self.theme_on(theme, theme.panel_bg)
    }

    /// Same as [`Scrollbar::theme`], but `track_style` is drawn on `bg` instead of
    /// `theme.panel_bg`: for a scrollbar drawn directly on a backdrop other than a themed
    /// [`super::Panel`]/[`super::Modal`]'s fill. [`Scrollbar::theme`] is exactly
    /// `theme_on(theme, theme.panel_bg)`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.track_style = Style::new().bg(bg);
        self.thumb_style = Style::new().bg(theme.border);
        self
    }
}

impl Scrollbar {
    /// The shared drawing routine both [`Widget::render`] and [`InteractiveWidget::render`] use,
    /// parameterized on the offset to draw the thumb at: the display-only [`Widget`] impl passes
    /// `self.offset`, the interactive one passes `state.integer_offset()`.
    fn draw(&self, surface: &mut Surface<'_>, offset: usize) {
        let (w, h) = (surface.width(), surface.height());
        if w == 0 || h == 0 {
            return;
        }

        for y in 0..h {
            for x in 0..w {
                surface.put((x, y), ' ', self.track_style);
            }
        }

        let local = Rect::new(0, 0, w, h);
        let Some((start, len)) = thumb_geometry(local, self.total_len, self.visible_len, offset)
        else {
            return;
        };
        for y in start..(start + len) {
            for x in 0..w {
                surface.put((x, y), ' ', self.thumb_style);
            }
        }
    }

    /// This scrollbar's maximum offset: `0` if everything already fits (`total_len <=
    /// visible_len`), otherwise `total_len - visible_len`, widened to `f32` for
    /// [`ScrollState`]'s fractional-offset math.
    #[allow(clippy::cast_precision_loss)]
    const fn max_offset(&self) -> f32 {
        self.total_len.saturating_sub(self.visible_len) as f32
    }
}

impl Widget for Scrollbar {
    fn render(&self, surface: &mut Surface<'_>) {
        self.draw(surface, self.offset);
    }
}

impl InteractiveWidget for Scrollbar {
    type State = ScrollState;

    /// Draggable (thumb or track, click-to-jump then continues as a smooth drag) and
    /// wheel-scrollable.
    fn sense(&self) -> Sense {
        Sense::drag() | Sense::CLICK | Sense::SCROLL
    }

    fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State, response: Response) {
        let area = surface.area();
        let max_offset = self.max_offset();

        // `ScrollState::update_drag` follows a content-drag convention (dragging up, i.e.
        // decreasing `y`, increases the offset, matching a touch-scroll gesture). A scrollbar
        // thumb needs the opposite: dragging *down* increases the offset, so `y` is negated
        // before it's fed in, flipping that convention to match `offset_for_pos`'s own
        // top-to-bottom-is-increasing mapping.
        if response.pressed() {
            // Click-to-jump: land the thumb under the pointer immediately, then let the drag
            // below continue smoothly from there.
            if let Some(pos) = response.pointer_pos() {
                if let Some(target) = offset_for_pos(area, self.total_len, self.visible_len, pos) {
                    // `target` is bounded to `total_len - visible_len` by `offset_for_pos`, the
                    // same quantity `max_offset` widens to `f32` above.
                    #[allow(clippy::cast_precision_loss)]
                    state.set_offset(target as f32, max_offset);
                }
                state.begin_drag(-f32::from(pos.y));
            }
        }
        if response.held()
            && let Some(pos) = response.pointer_pos()
        {
            state.update_drag(-f32::from(pos.y), max_offset);
        }
        if response.released() {
            state.end_drag();
        }
        state.apply(&response);

        self.draw(surface, state.integer_offset());
    }
}

impl AnimatedWidget for Scrollbar {
    type State = ScrollState;

    /// Ticks `state`'s momentum/rubber-band physics forward by `frame.delta` (a no-op while
    /// [`ScrollState::dragging`](crate::ScrollState::dragging) is `true`, per [`ScrollState::tick`]'s
    /// own docs), then draws the thumb at the resulting
    /// [`integer_offset`](crate::ScrollState::integer_offset) -- the [`Widget::render`] this type
    /// already has, just with `self.offset` replaced by `state`'s current one. `max_offset` is
    /// `total_len - visible_len` (floored at `0`), matching [`thumb_geometry`]'s own definition, so
    /// the physics and the drawn thumb position are always in terms of the same bound.
    fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State, frame: &Frame) {
        let max_offset = self.total_len.saturating_sub(self.visible_len);
        #[allow(clippy::cast_precision_loss)] // scroll extents stay well under 2^24 items
        state.tick(frame.delta, max_offset as f32);

        self.draw(surface, state.integer_offset());
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // ScrollState offsets under test here are exact 0.0 sentinels, not
// accumulated float results, so exact equality is the correct check, not a bug.
mod tests {
    use retroglyph_core::{Color, Grid, Pos, Rect};

    use super::*;
    use crate::Surface;

    #[test]
    fn draws_a_plain_track_with_no_thumb_when_nothing_to_scroll() {
        let area = Rect::new(0, 0, 1, 5);
        let mut grid = Grid::new(1, 5);
        let track = Style::new().bg(Color::Rgb { r: 1, g: 1, b: 1 });
        let thumb = Style::new().bg(Color::Rgb { r: 2, g: 2, b: 2 });
        let scrollbar = Scrollbar::new(3, 5).track_style(track).thumb_style(thumb);
        Widget::render(&scrollbar, &mut Surface::new(&mut grid, area, 0));
        for y in 0..5 {
            assert_eq!(
                grid[Pos::new(0, y)].style().background(),
                track.background()
            );
        }
    }

    #[test]
    fn draws_the_thumb_over_the_track() {
        let area = Rect::new(0, 0, 1, 10);
        let mut grid = Grid::new(1, 10);
        let track = Style::new().bg(Color::Rgb { r: 1, g: 1, b: 1 });
        let thumb = Style::new().bg(Color::Rgb { r: 2, g: 2, b: 2 });
        let scrollbar = Scrollbar::new(20, 5)
            .offset(0)
            .track_style(track)
            .thumb_style(thumb);
        Widget::render(&scrollbar, &mut Surface::new(&mut grid, area, 0));

        let (start, len) = thumb_geometry(area, 20, 5, 0).unwrap();
        for y in 0..10 {
            let bg = grid[Pos::new(0, y)].style().background();
            if y >= start && y < start + len {
                assert_eq!(bg, thumb.background());
            } else {
                assert_eq!(bg, track.background());
            }
        }
    }

    #[test]
    fn theme_maps_named_roles_onto_track_and_thumb() {
        let area = Rect::new(0, 0, 1, 10);
        let mut grid = Grid::new(1, 10);
        let scrollbar = Scrollbar::new(20, 5).theme(Theme::DARK);
        Widget::render(&scrollbar, &mut Surface::new(&mut grid, area, 0));

        let (start, len) = thumb_geometry(area, 20, 5, 0).unwrap();
        for y in 0..10 {
            let bg = grid[Pos::new(0, y)].style().background();
            if y >= start && y < start + len {
                assert_eq!(bg, Theme::DARK.border);
            } else {
                assert_eq!(bg, Theme::DARK.panel_bg);
            }
        }
    }

    #[test]
    fn theme_on_uses_the_given_backdrop_instead_of_panel_bg() {
        let area = Rect::new(0, 0, 1, 10);
        let mut grid = Grid::new(1, 10);
        let scrollbar = Scrollbar::new(20, 5).theme_on(Theme::DARK, Color::Default);
        Widget::render(&scrollbar, &mut Surface::new(&mut grid, area, 0));

        let (start, len) = thumb_geometry(area, 20, 5, 0).unwrap();
        for y in 0..10 {
            let bg = grid[Pos::new(0, y)].style().background();
            if y >= start && y < start + len {
                assert_eq!(bg, Theme::DARK.border);
            } else {
                assert_eq!(bg, Color::Default);
            }
        }
    }

    #[test]
    fn offset_defaults_to_zero() {
        let area = Rect::new(0, 0, 1, 10);
        let mut grid = Grid::new(1, 10);
        let track = Style::new().bg(Color::Rgb { r: 1, g: 1, b: 1 });
        let thumb = Style::new().bg(Color::Rgb { r: 2, g: 2, b: 2 });
        let scrollbar = Scrollbar::new(20, 5).track_style(track).thumb_style(thumb);
        Widget::render(&scrollbar, &mut Surface::new(&mut grid, area, 0));

        let (start, _) = thumb_geometry(area, 20, 5, 0).unwrap();
        assert_eq!(start, 0);
    }

    fn frame(delta_ms: u64) -> Frame {
        Frame {
            delta: core::time::Duration::from_millis(delta_ms),
            frame: 0,
        }
    }

    #[test]
    fn animated_render_ticks_state_before_drawing_the_thumb() {
        let area = Rect::new(0, 0, 1, 10);
        let mut grid = Grid::new(1, 10);
        let mut state = ScrollState::new();
        state.scroll_by_wheel(4.0); // gives the state some velocity to integrate
        assert_eq!(state.offset(), 0.0, "no physics has run yet");

        AnimatedWidget::render(
            &Scrollbar::new(20, 5),
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
            &frame(100),
        );

        assert!(
            state.offset() > 0.0,
            "tick should have advanced the offset before drawing"
        );

        // The thumb was drawn at the *post-tick* offset, not the stale offset from before the
        // call: proves render() and tick() ran in the order the trait promises, in one call.
        let (start, _) = thumb_geometry(area, 20, 5, state.integer_offset()).unwrap();
        let mut expected = Grid::new(1, 10);
        let scrollbar = Scrollbar::new(20, 5).offset(state.integer_offset());
        Widget::render(&scrollbar, &mut Surface::new(&mut expected, area, 0));
        for y in 0..10 {
            assert_eq!(
                grid[Pos::new(0, y)].style().background(),
                expected[Pos::new(0, y)].style().background(),
                "row {y}, thumb starting at {start}"
            );
        }
    }

    #[test]
    fn animated_render_does_not_tick_while_dragging() {
        let area = Rect::new(0, 0, 1, 10);
        let mut grid = Grid::new(1, 10);
        let mut state = ScrollState::new();
        state.begin_drag(5.0);
        state.scroll_by_wheel(4.0); // ignored while dragging, per ScrollState::scroll_by_wheel

        AnimatedWidget::render(
            &Scrollbar::new(20, 5),
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
            &frame(100),
        );

        assert_eq!(state.offset(), 0.0, "tick is a no-op while dragging");
    }

    #[test]
    fn drag_moves_scroll_state() {
        use retroglyph_core::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        use crate::Interaction;

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Id {
            Bar,
        }

        let area = Rect::new(0, 0, 1, 10); // total=40, visible=10 -> max_offset = 30
        let scrollbar = Scrollbar::new(40, 10);
        let mut state = ScrollState::new();
        let mut interaction = Interaction::<Id>::new();
        let mut grid = Grid::new(1, 10);

        // Frame 1: register the rect for next frame's hit-test.
        interaction.begin_frame();
        let response = interaction.interact(area, Id::Bar, scrollbar.sense());
        InteractiveWidget::render(
            &scrollbar,
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
            response,
        );
        interaction.end_frame();
        assert!((state.offset() - 0.0).abs() < f32::EPSILON);

        // Press at the top of the track: resolves against frame 1's registration.
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(0, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let response = interaction.interact(area, Id::Bar, scrollbar.sense());
        InteractiveWidget::render(
            &scrollbar,
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
            response,
        );
        interaction.end_frame();
        assert!((state.offset() - 0.0).abs() < f32::EPSILON); // clicked at the top: no jump needed

        // Drag down to the bottom of the track while still held.
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(0, 9),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let response = interaction.interact(area, Id::Bar, scrollbar.sense());
        InteractiveWidget::render(
            &scrollbar,
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
            response,
        );
        interaction.end_frame();
        assert!(state.offset() > 0.0); // dragged toward the bottom: offset increased

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            position: Pos::new(0, 9),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let response = interaction.interact(area, Id::Bar, scrollbar.sense());
        InteractiveWidget::render(
            &scrollbar,
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
            response,
        );
        interaction.end_frame();
        assert!(!state.dragging());
    }

    #[test]
    fn wheel_scroll_applies_velocity() {
        let area = Rect::new(0, 0, 1, 10);
        let scrollbar = Scrollbar::new(40, 10);
        let mut state = ScrollState::new();
        let response = Response {
            scroll_delta: 2,
            ..Response::default()
        };
        let mut grid = Grid::new(1, 10);
        InteractiveWidget::render(
            &scrollbar,
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
            response,
        );
        assert!(state.velocity() > 0.0);
    }
}
