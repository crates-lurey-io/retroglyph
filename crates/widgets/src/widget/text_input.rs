//! [`TextInput`]: a single-line, [`TextInputState`]-driven text field.
use unicode_width::UnicodeWidthChar;

use retroglyph_core::{Backend, Color, Rect, Style, Terminal};

use super::StatefulWidget;
use crate::TextInputState;
use crate::Theme;
use crate::draw::fill_rect;
use crate::text::truncate as truncate_to_cols;

/// A single-line text field, styled and drawn from an externally owned [`TextInputState`].
///
/// `TextInput` is pure presentation, the same division of labor as every other widget in this
/// crate (see `crates/widgets/AGENTS.md`): it never mutates the caller's keyboard/mouse events
/// into text itself -- `new()` takes no value, since the value lives entirely in
/// [`TextInputState`], and interpreting `KeyEvent`s into `state.insert`/`state.backspace`/etc.
/// calls is the caller's job, exactly like [`FocusRing`](crate::FocusRing) leaves key-to-action
/// mapping to the caller:
///
/// ```
/// use retroglyph_core::{Backend, Headless, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, Rect, Terminal};
/// use retroglyph_widgets::{StatefulWidget, TextInput, TextInputState};
///
/// fn apply(state: &mut TextInputState, key: KeyEvent) {
///     match key.code {
///         KeyCode::Char(c) => state.insert(c),
///         KeyCode::Backspace => state.backspace(),
///         KeyCode::Delete => state.delete(),
///         KeyCode::Left => state.move_left(),
///         KeyCode::Right => state.move_right(),
///         KeyCode::Home => state.move_home(),
///         KeyCode::End => state.move_end(),
///         _ => {}
///     }
/// }
///
/// let mut state = TextInputState::new();
/// let press = |code| KeyEvent {
///     code,
///     kind: KeyEventKind::Press,
///     modifiers: KeyModifiers::NONE,
/// };
/// apply(&mut state, press(KeyCode::Char('h')));
/// apply(&mut state, press(KeyCode::Char('i')));
/// apply(&mut state, press(KeyCode::Left));
/// apply(&mut state, press(KeyCode::Char('!')));
/// assert_eq!(state.value(), "h!i");
///
/// // Same "caller calls ensure_* before render" contract as `List`/`ListState::ensure_visible`.
/// let area = Rect::new(0, 0, 10, 1);
/// state.ensure_cursor_visible(area.width());
///
/// let mut term = Terminal::new(Headless::new(10, 1));
/// TextInput::new().render(area, &mut term, &mut state);
/// ```
///
/// `render` never scrolls `state` itself -- like [`List`](super::List), it draws whatever window
/// `state.offset()` already names, so call [`state.ensure_cursor_visible(area.width())`
/// ](TextInputState::ensure_cursor_visible) before rendering to keep the cursor on-screen.
///
/// `style`, `caret_style`, and `placeholder_style` each default to a fixed palette; set them with
/// [`TextInput::style`]/[`TextInput::caret_style`]/[`TextInput::placeholder_style`], or apply a
/// [`Theme`] with [`TextInput::theme`].
#[derive(Clone, Copy, Debug)]
pub struct TextInput<'a> {
    placeholder: &'a str,
    style: Style,
    caret_style: Style,
    placeholder_style: Style,
}

impl<'a> TextInput<'a> {
    /// A text field in the default style, with no placeholder. The value lives in
    /// [`TextInputState`], not here -- pass the state to [`render`](StatefulWidget::render).
    #[must_use]
    pub fn new() -> Self {
        let style = Style::new()
            .fg(Color::Rgb {
                r: 170,
                g: 175,
                b: 190,
            })
            .bg(Color::Rgb {
                r: 45,
                g: 48,
                b: 58,
            });
        Self {
            placeholder: "",
            caret_style: Style::new().fg(style.background()).bg(style.foreground()),
            placeholder_style: Style::new()
                .fg(Color::Rgb {
                    r: 110,
                    g: 112,
                    b: 130,
                })
                .bg(style.background()),
            style,
        }
    }

    /// Set the text shown (in [`placeholder_style`](Self::placeholder_style)) when the field's
    /// value is empty.
    #[must_use]
    pub const fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Set the style of the value text and the field's background.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the style of the caret cell. Drawn as a plain inverse-video cell -- no blink, no
    /// backend hardware cursor -- so it renders identically across every backend.
    #[must_use]
    pub const fn caret_style(mut self, style: Style) -> Self {
        self.caret_style = style;
        self
    }

    /// Set the style of the placeholder text shown when the value is empty.
    #[must_use]
    pub const fn placeholder_style(mut self, style: Style) -> Self {
        self.placeholder_style = style;
        self
    }

    /// Applies `theme`'s named roles: `style` becomes `theme.fg` on `theme.panel_bg`,
    /// `caret_style` swaps those two (an inverse cell against the field), and
    /// `placeholder_style` becomes `theme.dim` on `theme.panel_bg`. The same mapping
    /// [`Button::theme`](super::Button)/[`List::theme`](super::List) use for their own styles.
    ///
    /// Call before any manual `_style` override you want to keep.
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.style = Style::new().fg(theme.fg).bg(theme.panel_bg);
        self.caret_style = Style::new().fg(theme.panel_bg).bg(theme.fg);
        self.placeholder_style = Style::new().fg(theme.dim).bg(theme.panel_bg);
        self
    }
}

impl Default for TextInput<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: Backend> StatefulWidget<B> for TextInput<'_> {
    type State = TextInputState;

    fn render(self, area: Rect, term: &mut Terminal<B>, state: &mut Self::State) {
        if area.width() == 0 || area.height() == 0 {
            return;
        }

        let y = area.top();
        fill_rect(term, area, ' ', Style::new().bg(self.style.background()));

        if state.value().is_empty() {
            if !self.placeholder.is_empty() {
                let text = truncate_to_cols(self.placeholder, area.width_usize());
                term.reset_style()
                    .fg(self.placeholder_style.foreground())
                    .bg(self.placeholder_style.background());
                term.print(area.left(), y, &text);
            }
            term.reset_style()
                .fg(self.caret_style.foreground())
                .bg(self.caret_style.background());
            term.put(area.left(), y, ' ');
            term.reset_style();
            return;
        }

        let width = area.width_usize();
        let char_count = state.char_count();
        let cursor = state.cursor();
        let offset = state.offset();
        let max_cols = if cursor == char_count {
            width.saturating_sub(1)
        } else {
            width
        };

        let mut visible = String::new();
        let mut col = 0usize;
        let mut caret_col = None;
        let mut caret_glyph = ' ';
        for (i, ch) in state.value().chars().enumerate().skip(offset) {
            let w = ch.width().unwrap_or(0);
            if i == cursor && col < max_cols {
                caret_col = Some(col);
                caret_glyph = ch;
            }
            if col + w > max_cols {
                break;
            }
            visible.push(ch);
            col += w;
        }
        if cursor == char_count && caret_col.is_none() && col < width {
            caret_col = Some(col);
        }

        term.reset_style()
            .fg(self.style.foreground())
            .bg(self.style.background());
        term.print(area.left(), y, &visible);

        if let Some(caret_col) = caret_col
            && caret_col < width
        {
            let x = area.left() + caret_col as u16;
            term.reset_style()
                .fg(self.caret_style.foreground())
                .bg(self.caret_style.background());
            term.put(x, y, caret_glyph);
        }
        term.reset_style();
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::Headless;

    use super::*;

    #[test]
    fn draws_the_value() {
        let area = Rect::new(0, 0, 10, 1);
        let mut term = Terminal::new(Headless::new(10, 1));
        let mut state = TextInputState::with_value("hi");
        state.set_cursor(0); // keep the caret off the last glyph to check plain-text drawing
        TextInput::new().render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(1, 0).glyph(), 'i');
    }

    #[test]
    fn caret_cell_is_styled_differently_from_a_plain_cell() {
        let area = Rect::new(0, 0, 10, 1);
        let mut term = Terminal::new(Headless::new(10, 1));
        let mut state = TextInputState::with_value("hi");
        state.set_cursor(0);
        TextInput::new().render(area, &mut term, &mut state);

        let caret_bg = term.grid().get(0, 0).style().background();
        let plain_bg = term.grid().get(1, 0).style().background();
        assert_ne!(caret_bg, plain_bg);
    }

    #[test]
    fn placeholder_shown_when_empty_with_caret_at_column_zero() {
        let area = Rect::new(0, 0, 10, 1);
        let mut term = Terminal::new(Headless::new(10, 1));
        let mut state = TextInputState::new();
        TextInput::new()
            .placeholder("name")
            .render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(0, 0).glyph(), ' '); // caret sits over the placeholder's 'n'
        assert_eq!(term.grid().get(1, 0).glyph(), 'a');
        let caret_bg = term.grid().get(0, 0).style().background();
        let placeholder_bg = term.grid().get(1, 0).style().background();
        assert_ne!(caret_bg, placeholder_bg);
    }

    #[test]
    fn horizontal_scroll_shows_the_window_around_the_cursor() {
        let area = Rect::new(0, 0, 4, 1);
        let mut term = Terminal::new(Headless::new(4, 1));
        let mut state = TextInputState::with_value("abcdefgh");
        state.ensure_cursor_visible(area.width());
        TextInput::new().render(area, &mut term, &mut state);

        // Cursor is at char_count == 8; ensure_cursor_visible scrolls the offset to 6 (window
        // [6, 8)), reserving 1 col of the 4-wide area for the end caret: "gh" then the caret.
        assert_eq!(state.offset(), 6);
        assert_eq!(term.grid().get(0, 0).glyph(), 'g');
        assert_eq!(term.grid().get(1, 0).glyph(), 'h');
        let caret_bg = term.grid().get(2, 0).style().background();
        let plain_bg = term.grid().get(0, 0).style().background();
        assert_ne!(caret_bg, plain_bg); // the caret cell at col 2 is styled differently
    }

    #[test]
    fn wide_character_caret_covers_the_primary_cell_only() {
        let area = Rect::new(0, 0, 10, 1);
        let mut term = Terminal::new(Headless::new(10, 1));
        let mut state = TextInputState::with_value("あb");
        state.set_cursor(0); // cursor sits on the wide character
        TextInput::new().render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(0, 0).glyph(), 'あ');
        assert_eq!(term.grid().get(2, 0).glyph(), 'b');
        let caret_bg = term.grid().get(0, 0).style().background();
        let plain_bg = term.grid().get(2, 0).style().background();
        assert_ne!(caret_bg, plain_bg);
    }

    #[test]
    fn zero_size_is_a_no_op() {
        let area = Rect::new(0, 0, 0, 1);
        let mut term = Terminal::new(Headless::new(1, 1));
        let mut state = TextInputState::with_value("hi");
        TextInput::new().render(area, &mut term, &mut state);
        assert_eq!(term.grid().get(0, 0).glyph(), ' ');
    }

    #[test]
    fn theme_maps_named_roles_onto_style_and_caret() {
        use crate::Theme;

        let area = Rect::new(0, 0, 10, 1);
        let mut term = Terminal::new(Headless::new(10, 1));
        let mut state = TextInputState::with_value("hi");
        state.set_cursor(0);
        TextInput::new()
            .theme(Theme::DARK)
            .render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(1, 0).style().foreground(), Theme::DARK.fg);
        assert_eq!(
            term.grid().get(1, 0).style().background(),
            Theme::DARK.panel_bg
        );
        assert_eq!(
            term.grid().get(0, 0).style().foreground(),
            Theme::DARK.panel_bg
        );
        assert_eq!(term.grid().get(0, 0).style().background(), Theme::DARK.fg);
    }
}
