//! [`TextInput`]: a single-line, `TextInputState`-driven editable text field.
use retroglyph_core::text::{split_at_width, width_usize};
use retroglyph_core::{Color, Style};

use super::StatefulWidget;
use crate::Surface;
use crate::TextInputState;
use crate::Theme;
use crate::text::truncate as truncate_to_cols;

/// A single-line editable text field: the stateless drawing half of [`TextInputState`], the
/// same split [`List`](super::List) has with [`ListState`](crate::ListState).
///
/// Draws `state.value()` (or `mask`-repeated characters over it, or `placeholder` when empty and
/// unfocused... there's no focus concept here, see below), scrolled horizontally by
/// `state.scroll()` and clipped to `surface.area()`'s width, with a caret cell at the cursor's
/// display column. Neither scrolling nor the caret's column is byte- or char-based: both go
/// through `retroglyph_core::text::width_usize`, the same display-width measurement
/// [`truncate`](crate::text::truncate) uses, so a value containing a double-width character
/// (CJK, most emoji) still puts the caret in the right screen column.
///
/// This widget does not call [`TextInputState::ensure_visible`]: like
/// [`List`](super::List)/[`ListState::ensure_visible`](crate::ListState::ensure_visible), that's
/// the caller's job, once per frame, with the actual current field width (which can change on
/// resize).
///
/// The caret always renders as an inverted-color cell (`caret_style`), not by driving a real
/// terminal cursor via a backend's `Cursor` facet: `render` only has a [`Surface`], not a
/// `Backend`, and a cell-drawn caret renders identically -- including in headless snapshot tests
/// -- on every backend. An app that wants a blinking, backend-native caret instead can position
/// one itself from `state.cursor()`/`state.scroll()` alongside this widget.
///
/// No focus handling, validation, submission, or layout: an app decides which field is focused
/// (only routing input to it while it is) and what happens on Enter. No IME/text composition and
/// no multi-line editing -- see `docs/ROADMAP.md`.
///
/// # Examples
///
/// ```
/// use retroglyph_core::{Grid, Rect};
/// use retroglyph_widgets::{StatefulWidget, Surface, TextInput, TextInputState};
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
    /// An empty-placeholder, unmasked text input in the default style.
    #[must_use]
    pub fn new() -> Self {
        Self {
            placeholder: None,
            mask: None,
            style: Style::new().fg(Color::Rgb {
                r: 170,
                g: 175,
                b: 190,
            }),
            placeholder_style: Style::new().fg(Color::Rgb {
                r: 90,
                g: 95,
                b: 110,
            }),
            caret_style: Style::new()
                .fg(Color::Rgb {
                    r: 16,
                    g: 16,
                    b: 24,
                })
                .bg(Color::Rgb {
                    r: 90,
                    g: 170,
                    b: 250,
                }),
        }
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
    /// display width, not the mask's -- masking only ever substitutes one fixed-width glyph, so
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
            // what remains to the field width -- the same `split_at_width`/`truncate` split
            // `retroglyph-widgets::text` documents for exactly this "windowed" reason.
            let (_, visible) = split_at_width(value, state.scroll());
            let visible = self
                .mask
                .map_or_else(|| visible.to_owned(), |mask| masked(visible, mask));
            (visible, self.style)
        };
        let text = truncate_to_cols(&text, usize::from(width));
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
    use retroglyph_core::{Grid, Pos, Rect};

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
        // matching -- column 2 would be wrong if this used byte/char counting for a value with a
        // wider first character.
        assert_eq!(grid[Pos::new(1, 0)].glyph(), 'あ');
        assert_ne!(
            grid[Pos::new(1, 0)].style().background(),
            grid[Pos::new(0, 0)].style().background()
        );
    }
}
