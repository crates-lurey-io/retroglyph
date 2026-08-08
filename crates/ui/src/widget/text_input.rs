//! [`TextInput`]: a single-line, `TextInputState`-driven editable text field.
use alloc::borrow::ToOwned as _;
use alloc::string::String;

use retroglyph_core::color::{Color, Style};
use retroglyph_core::grid::Size;
use retroglyph_core::text::{split_at_width, width, width_usize};

use super::{MinSize, StatefulWidget};
use crate::Surface;
use crate::interact::Density;
use crate::state::TextInputState;
use crate::text::truncate as truncate_to_cols;
use crate::theme::Theme;

/// A single-line editable text field: the stateless drawing half of [`TextInputState`], the
/// same split [`List`](super::List) has with [`ListState`](crate::state::ListState).
///
/// Draws `state.value()` (masked with `mask` if set, or `placeholder` while `state.value()` is
/// empty), scrolled horizontally by `state.scroll()` and clipped to `surface.area()`'s width,
/// with a caret cell at the cursor's display column. Neither scrolling nor the caret's column is
/// byte- or char-based: both go
/// through `retroglyph_core::text::width_usize`, the same display-width measurement
/// [`truncate`](crate::text::truncate) uses, so a value containing a double-width character
/// (CJK, most emoji) still puts the caret in the right screen column.
///
/// This widget does not call [`TextInputState::ensure_visible`]: like
/// [`List`](super::List)/[`ListState::ensure_visible`](crate::state::ListState::ensure_visible), that's
/// the caller's job, once per frame, with the actual current field width (which can change on
/// resize).
///
/// The caret always renders as an inverted-color cell (`caret_style`), not by driving a real
/// terminal cursor via a backend's `Cursor` facet: `render` only has a [`Surface`], not a
/// `Backend`, and a cell-drawn caret renders identically (including in headless snapshot tests)
/// on every backend. An app that wants a blinking, backend-native caret instead can position
/// one itself from `state.cursor()`/`state.scroll()` alongside this widget.
///
/// This widget draws one field, nothing more: which field is focused (and therefore routed
/// input), what Enter does, validation, and layout are all the app's job. IME/text composition
/// and multi-line editing are out of scope for this crate entirely.
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::{Grid, Rect};
/// use retroglyph_ui::state::TextInputState;
/// use retroglyph_ui::widget::{StatefulWidget, TextInput};
/// use retroglyph_ui::Surface;
///
/// let mut state = TextInputState::new();
/// state.set_value("hello");
///
/// let area = Rect::new(0, 0, 10, 1);
/// let mut grid = Grid::new(10, 1);
/// TextInput::new().render(&mut Surface::new(&mut grid, area, 0), &mut state);
/// ```
#[derive(Clone, Copy, Debug)]
pub struct TextInput<'a> {
    placeholder: Option<&'a str>,
    mask: Option<char>,
    style: Style,
    placeholder_style: Style,
    caret_style: Style,
}

impl<'a> TextInput<'a> {
    /// An empty-placeholder, unmasked text input, styled from [`Theme::DARK`] (as if
    /// [`TextInput::theme`] had been called); call [`TextInput::theme`]/[`TextInput::theme_on`]
    /// for a different [`Theme`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            placeholder: None,
            mask: None,
            style: Style::new(),
            placeholder_style: Style::new(),
            caret_style: Style::new(),
        }
        .theme(Theme::DARK)
    }

    /// Text shown, in [`placeholder_style`](Self::placeholder_style), when `state.value()` is
    /// empty.
    #[must_use]
    pub const fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = Some(placeholder);
        self
    }

    /// Render every character of `state.value()` as `mask` instead of its real glyph, e.g. `'*'`
    /// for a password field. Column math (scrolling, caret position) still uses the real value's
    /// display width, not the mask's: masking only ever substitutes one fixed-width glyph, so
    /// this is exact as long as `mask` itself is a single-column character.
    #[must_use]
    pub const fn mask(mut self, mask: char) -> Self {
        self.mask = Some(mask);
        self
    }

    /// Set the style of the value/placeholder text (`placeholder` uses
    /// [`placeholder_style`](Self::placeholder_style) instead of this while shown).
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the style of the placeholder text, shown in place of the value while it's empty.
    #[must_use]
    pub const fn placeholder_style(mut self, style: Style) -> Self {
        self.placeholder_style = style;
        self
    }

    /// Set the style of the caret cell.
    #[must_use]
    pub const fn caret_style(mut self, style: Style) -> Self {
        self.caret_style = style;
        self
    }

    /// Applies `theme`'s named roles: `style` becomes `theme.fg` on `theme.panel_bg`,
    /// `placeholder_style` becomes `theme.dim` on the same background, and `caret_style` becomes
    /// `theme.bg` on `theme.accent`. See [`List::theme`](super::List::theme) for why an explicit
    /// background is baked in rather than left unset.
    #[must_use]
    pub fn theme(self, theme: Theme) -> Self {
        self.theme_on(theme, theme.panel_bg)
    }

    /// Same as [`TextInput::theme`], but text is drawn on `bg` instead of `theme.panel_bg`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.style = Style::new().fg(theme.fg).bg(bg);
        self.placeholder_style = Style::new().fg(theme.dim).bg(bg);
        self.caret_style = Style::new().fg(theme.bg).bg(theme.accent);
        self
    }
}

impl Default for TextInput<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl MinSize for TextInput<'_> {
    /// [`placeholder`](TextInput::placeholder)'s display width (`0` with no placeholder set)
    /// plus 2 columns of padding on each side, matching [`Button`](super::Button)'s own padding,
    /// one row tall, floored at `density`'s [`min_target_size`](Density::min_target_size) so an
    /// empty field still claims a usable click/tap target.
    fn min_size(&self, density: Density) -> Size {
        let content_width = self.placeholder.map_or(0, width).saturating_add(4);
        Size::new(content_width, 1).max(density.min_target_size())
    }
}

impl StatefulWidget for TextInput<'_> {
    type State = TextInputState;

    fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State) {
        let width = surface.width();
        if width == 0 {
            return;
        }

        let value = state.value();
        let caret_col =
            width_usize(&value[..state.cursor()]).saturating_sub(usize::from(state.scroll()));

        let (text, style) = if value.is_empty() {
            self.placeholder.map_or_else(
                || (String::new(), self.style),
                |placeholder| (placeholder.to_owned(), self.placeholder_style),
            )
        } else {
            // Skip the scrolled-past prefix (by display width, not bytes) before truncating
            // what remains to the field width, the same `split_at_width`/`truncate` split
            // `retroglyph-ui::text` documents for exactly this "windowed" reason.
            let (_, visible) = split_at_width(value, state.scroll());
            let visible = self
                .mask
                .map_or_else(|| visible.to_owned(), |mask| masked(visible, mask));
            (visible, self.style)
        };
        let text = truncate_to_cols(&text, width);
        surface.print((0, 0), text, style);

        if caret_col < usize::from(width) {
            #[allow(clippy::cast_possible_truncation)] // caret_col < width, a u16
            let x = caret_col as u16;
            // Re-derive the glyph from what was actually just printed (placeholder, masked, or
            // real text) rather than re-reading `value`, so the caret cell inverts whatever's
            // underneath it instead of clobbering a placeholder character with a blank.
            let glyph = glyph_at_column(text, caret_col).unwrap_or(' ');
            surface.put((x, 0), glyph, self.caret_style);
        }
    }
}

/// Replace every character in `s` with `mask`, preserving its display width for scroll/caret
/// math: single-column masks (the common case, `'*'`) keep the same byte-for-column
/// correspondence as the real value.
fn masked(s: &str, mask: char) -> String {
    s.chars().map(|_| mask).collect()
}

/// The character at display column `col` in `s`, or `None` past its last column.
fn glyph_at_column(s: &str, col: usize) -> Option<char> {
    #[allow(clippy::cast_possible_truncation)] // caller already clamped col < a surface's u16 width
    let col = col.min(usize::from(u16::MAX)) as u16;
    let (_, rest) = split_at_width(s, col);
    rest.chars().next()
}

#[cfg(test)]
mod tests {
    use retroglyph_core::grid::{Grid, HasSize as _, Pos, Rect};

    use super::*;

    #[test]
    fn renders_the_value_and_a_caret_at_the_end() {
        let area = Rect::new(0, 0, 10, 1);
        let mut grid = Grid::new(10, 1);
        let mut state = TextInputState::new();
        state.set_value("hi");

        TextInput::new().render(&mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'h');
        assert_eq!(grid[Pos::new(1, 0)].glyph(), 'i');
        // Cursor is at the end (byte 2): the caret cell is the space just past "hi".
        assert_eq!(grid[Pos::new(2, 0)].glyph(), ' ');
        assert_ne!(
            grid[Pos::new(2, 0)].style().background(),
            grid[Pos::new(0, 0)].style().background()
        );
    }

    #[test]
    fn shows_the_placeholder_only_while_empty() {
        let area = Rect::new(0, 0, 10, 1);
        let mut grid = Grid::new(10, 1);
        let mut state = TextInputState::new();

        TextInput::new()
            .placeholder("name")
            .render(&mut Surface::new(&mut grid, area, 0), &mut state);
        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'n');

        let mut grid = Grid::new(10, 1);
        state.insert('x');
        TextInput::new()
            .placeholder("name")
            .render(&mut Surface::new(&mut grid, area, 0), &mut state);
        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'x');
    }

    #[test]
    fn mask_hides_the_real_characters() {
        let area = Rect::new(0, 0, 10, 1);
        let mut grid = Grid::new(10, 1);
        let mut state = TextInputState::new();
        state.set_value("secret");

        TextInput::new()
            .mask('*')
            .render(&mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].glyph(), '*');
        assert_eq!(grid[Pos::new(5, 0)].glyph(), '*');
    }

    #[test]
    fn scroll_offset_shifts_the_visible_window() {
        let area = Rect::new(0, 0, 5, 1);
        let mut grid = Grid::new(5, 1);
        let mut state = TextInputState::new();
        state.set_value("hello world");
        state.ensure_visible(5);

        TextInput::new().render(&mut Surface::new(&mut grid, area, 0), &mut state);

        // Scrolled to keep the end-of-value cursor in view: last 5 columns of "hello world".
        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'o');
        assert_eq!(grid[Pos::new(1, 0)].glyph(), 'r');
    }

    #[test]
    fn caret_lands_on_the_correct_column_with_wide_characters() {
        let area = Rect::new(0, 0, 10, 1);
        let mut grid = Grid::new(10, 1);
        let mut state = TextInputState::new();
        state.set_value("aあ"); // 'a' (1 col) + 'あ' (2 cols)
        state.move_home();
        state.move_right(); // cursor after 'a', before the 2-wide 'あ'

        TextInput::new().render(&mut Surface::new(&mut grid, area, 0), &mut state);

        // Caret is at column 1 (display width of "a"), not column 1 by char count coincidentally
        // matching: column 2 would be wrong if this used byte/char counting for a value with a
        // wider first character.
        assert_eq!(grid[Pos::new(1, 0)].glyph(), 'あ');
        assert_ne!(
            grid[Pos::new(1, 0)].style().background(),
            grid[Pos::new(0, 0)].style().background()
        );
    }

    #[test]
    fn text_input_caret_does_not_write_past_the_field() {
        // retroglyph#712: `ensure_visible` used to leave `scroll` inside a wide character's own
        // cell (scroll == 1 for "ああ"), which `split_at_width` can't honor, so the caret's
        // spacer cell was printed one column past the field's own 4-wide area, clobbering a
        // neighbor drawn at column 4. The same mechanism as #9.
        let mut grid = Grid::new(6, 1);
        let mut state = TextInputState::new();
        state.set_value("ああ"); // two 2-column characters, 4 columns
        state.ensure_visible(4);

        // A neighbor widget occupies columns 4..6, to the right of the 4-wide text field.
        Surface::new(&mut grid, Rect::new(0, 0, 6, 1), 0).print((4, 0), "Z", Style::default());

        TextInput::new().render(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 4, 1), 0),
            &mut state,
        );

        assert_eq!(grid[Pos::new(4, 0)].glyph(), 'Z');
    }

    #[test]
    fn min_size_floors_an_empty_placeholder_at_the_density_minimum() {
        // No placeholder set: content width is 0 + padding 4 = 4 cols, under either density's
        // 6-cell floor.
        let size = TextInput::new().min_size(Density::Mouse);
        assert_eq!(size, Density::Mouse.min_target_size());
    }

    #[test]
    fn min_size_pads_the_placeholder_by_two_columns_each_side() {
        // "Name" is 4 cols wide; padded to 8, past the 6-cell floor either density sets, so the
        // placeholder's own width should win over `min_target_size`.
        let size = TextInput::new().placeholder("Name").min_size(Density::Mouse);
        assert_eq!(size.width(), 8);
        assert_eq!(size.height(), 1);
    }

    #[test]
    fn min_size_grows_taller_for_touch_density() {
        let size = TextInput::new().min_size(Density::Touch);
        assert_eq!(size, Density::Touch.min_target_size());
    }
}
