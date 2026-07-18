//! [`Button`]: a clickable label, styled from an already-resolved [`Response`].
use retroglyph_core::{Backend, Color, Rect, Style, Terminal};

use super::Widget;
use crate::Response;
use crate::Theme;
use crate::draw::fill_rect;
use crate::text::truncate as truncate_to_cols;

/// A filled, centered `label`, styled by a [`Response`] the caller already resolved via
/// [`Interaction::interact`](crate::Interaction::interact).
///
/// `Button` is pure presentation, not a new source of truth: it never calls `interact` itself and
/// has no `Id` type parameter, unlike `Interaction<Id>`. The app still owns the `Interaction<Id>`
/// context and decides the button's id/[`Sense`](crate::Sense) -- the same division of labor as
/// every other widget here (state lives outside; the widget only reads it), applied to the
/// `interact` module's own doctest pattern ("draw the button, using `response.hovered()`/
/// `focused()` to pick a style") instead of leaving every call site to hand-roll it:
///
/// ```
/// use retroglyph_core::{Backend, Headless, Rect, Terminal};
/// use retroglyph_widgets::{Button, Interaction, Sense, Widget};
///
/// #[derive(Clone, Copy, PartialEq, Eq)]
/// enum Id {
///     Save,
/// }
///
/// let mut term = Terminal::new(Headless::new(20, 10));
/// let mut interaction = Interaction::<Id>::new();
/// interaction.begin_frame();
/// let area = Rect::new(0, 0, 10, 1);
/// let response = interaction.interact(area, Id::Save, Sense::click());
/// Button::new("Save", response).render(area, &mut term);
/// interaction.end_frame();
/// ```
///
/// Precedence when more than one [`Response`] flag is set at once:
/// [`pressed`](Response::pressed) &gt; [`hovered`](Response::hovered) &gt;
/// [`focused`](Response::focused) &gt; the default `style` -- matching the conventional
/// `:active` &gt; `:hover` &gt; `:focus` ordering, so a press always reads as pressed even while
/// still hovered, and a keyboard-focused-but-not-hovered button still shows something distinct
/// from idle.
///
/// `style`, `hovered_style`, `pressed_style`, and `focused_style` each default to a fixed
/// palette; set them with [`Button::style`]/[`Button::hovered_style`]/[`Button::pressed_style`]/
/// [`Button::focused_style`].
#[derive(Clone, Copy, Debug)]
pub struct Button<'a> {
    label: &'a str,
    response: Response,
    style: Style,
    hovered_style: Style,
    pressed_style: Style,
    focused_style: Style,
}

impl<'a> Button<'a> {
    /// A button labeled `label`, styled from `response`.
    #[must_use]
    pub fn new(label: &'a str, response: Response) -> Self {
        Self {
            label,
            response,
            style: Style::new()
                .fg(Color::Rgb {
                    r: 170,
                    g: 175,
                    b: 190,
                })
                .bg(Color::Rgb {
                    r: 45,
                    g: 48,
                    b: 58,
                }),
            hovered_style: Style::new().fg(Color::BRIGHT_WHITE).bg(Color::Rgb {
                r: 60,
                g: 65,
                b: 80,
            }),
            pressed_style: Style::new().fg(Color::BRIGHT_WHITE).bg(Color::Rgb {
                r: 40,
                g: 60,
                b: 90,
            }),
            focused_style: Style::new().fg(Color::BRIGHT_WHITE).bg(Color::Rgb {
                r: 55,
                g: 55,
                b: 70,
            }),
        }
    }

    /// Set the default (idle) style.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the style used while [`Response::hovered`] is `true`.
    #[must_use]
    pub const fn hovered_style(mut self, style: Style) -> Self {
        self.hovered_style = style;
        self
    }

    /// Set the style used while [`Response::pressed`] is `true`.
    #[must_use]
    pub const fn pressed_style(mut self, style: Style) -> Self {
        self.pressed_style = style;
        self
    }

    /// Set the style used while [`Response::focused`] is `true` (and neither pressed nor
    /// hovered).
    #[must_use]
    pub const fn focused_style(mut self, style: Style) -> Self {
        self.focused_style = style;
        self
    }

    /// Applies `theme`'s named roles to all four of this button's states: idle becomes
    /// `theme.fg` on `theme.panel_bg`; hovered/pressed swap in `theme.hover_bg`/`theme.press_bg`
    /// for the background; focused becomes `theme.accent` on `theme.panel_bg`. The same mapping
    /// `09_widgets_dashboard`'s "Ping" button hand-threads today.
    ///
    /// Call before any manual `_style` override you want to keep.
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.style = Style::new().fg(theme.fg).bg(theme.panel_bg);
        self.hovered_style = Style::new().fg(theme.fg).bg(theme.hover_bg);
        self.pressed_style = Style::new().fg(theme.fg).bg(theme.press_bg);
        self.focused_style = Style::new().fg(theme.accent).bg(theme.panel_bg);
        self
    }

    /// The style this button draws with this frame, per the
    /// pressed &gt; hovered &gt; focused &gt; default precedence documented on [`Button`].
    const fn resolved_style(&self) -> Style {
        if self.response.pressed() {
            self.pressed_style
        } else if self.response.hovered() {
            self.hovered_style
        } else if self.response.focused() {
            self.focused_style
        } else {
            self.style
        }
    }
}

impl<B: Backend> Widget<B> for Button<'_> {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        if area.width() == 0 || area.height() == 0 {
            return;
        }

        let style = self.resolved_style();
        fill_rect(term, area, ' ', style);

        let text = truncate_to_cols(self.label, area.width_usize());
        let text_width = text.chars().count() as u16;
        let x = area.left() + (area.width().saturating_sub(text_width)) / 2;
        let y = area.top() + area.height() / 2;

        term.reset_style()
            .fg(style.foreground())
            .bg(style.background());
        term.print(x, y, &text);
        term.reset_style();
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{
        Event, Headless, KeyModifiers, MouseButton, MouseEvent, MouseEventKind, Pos,
    };

    use super::*;
    use crate::{Interaction, Sense};

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Id {
        Save,
    }

    #[test]
    fn draws_the_label_centered_in_the_idle_style() {
        let area = Rect::new(0, 0, 7, 1);
        let mut term = Terminal::new(Headless::new(7, 1));
        Button::new("Go", Response::default()).render(area, &mut term);

        // "Go" (2 cols) centered in width 7 starts at column (7-2)/2 = 2.
        assert_eq!(term.grid().get(2, 0).glyph(), 'G');
        assert_eq!(term.grid().get(3, 0).glyph(), 'o');
    }

    #[test]
    fn fills_the_whole_area_with_the_background() {
        let area = Rect::new(0, 0, 7, 1);
        let mut term = Terminal::new(Headless::new(7, 1));
        Button::new("Go", Response::default()).render(area, &mut term);

        let idle_bg = Style::new()
            .fg(Color::Rgb {
                r: 170,
                g: 175,
                b: 190,
            })
            .bg(Color::Rgb {
                r: 45,
                g: 48,
                b: 58,
            })
            .background();
        assert_eq!(term.grid().get(0, 0).style().background(), idle_bg);
        assert_eq!(term.grid().get(6, 0).style().background(), idle_bg);
    }

    #[test]
    fn pressed_takes_precedence_over_hovered() {
        let response = Response {
            hovered: true,
            pressed: true,
            ..Response::default()
        };
        let button = Button::new("Go", response);
        assert_eq!(
            button.resolved_style().background(),
            button.pressed_style.background()
        );
    }

    #[test]
    fn hovered_takes_precedence_over_focused() {
        let response = Response {
            hovered: true,
            focused: true,
            ..Response::default()
        };
        let button = Button::new("Go", response);
        assert_eq!(
            button.resolved_style().background(),
            button.hovered_style.background()
        );
    }

    #[test]
    fn focused_only_shows_when_not_pressed_or_hovered() {
        let response = Response {
            focused: true,
            ..Response::default()
        };
        let button = Button::new("Go", response);
        assert_eq!(
            button.resolved_style().background(),
            button.focused_style.background()
        );
    }

    #[test]
    fn idle_by_default() {
        let button = Button::new("Go", Response::default());
        assert_eq!(
            button.resolved_style().background(),
            button.style.background()
        );
    }

    #[test]
    fn style_knobs_can_be_overridden() {
        let custom = Style::new().fg(Color::RED).bg(Color::GREEN);
        let response = Response {
            pressed: true,
            ..Response::default()
        };
        let button = Button::new("Go", response).pressed_style(custom);
        assert_eq!(button.resolved_style().background(), Color::GREEN);
    }

    #[test]
    fn integrates_with_interaction_and_reflects_a_real_click() {
        let mut interaction = Interaction::<Id>::new();
        let area = Rect::new(0, 0, 7, 1);

        interaction.begin_frame();
        let _ = interaction.interact(area, Id::Save, Sense::click());
        interaction.end_frame();

        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        interaction.begin_frame();
        let response = interaction.interact(area, Id::Save, Sense::click());
        interaction.end_frame();
        assert!(response.clicked());

        // The synthetic down+up pair above lands in one `handle_event` batch (see
        // `Interaction`'s doc comment on this exact edge case), so `pressed` is still `true` on
        // the same frame `clicked` resolves -- `Button` renders with `pressed_style` here, not
        // idle. Confirms end-to-end wiring (a real click drives a real style pick), not just that
        // `resolved_style` matches its own precedence rules in isolation (the other tests above).
        let button = Button::new("Go", response);
        assert_eq!(
            button.resolved_style().background(),
            button.pressed_style.background()
        );

        let mut term = Terminal::new(Headless::new(7, 1));
        button.render(area, &mut term);
    }

    #[test]
    fn zero_size_is_a_no_op() {
        let area = Rect::new(0, 0, 0, 1);
        let mut term = Terminal::new(Headless::new(1, 1));
        Button::new("Go", Response::default()).render(area, &mut term);
        assert_eq!(term.grid().get(0, 0).glyph(), ' ');
    }

    #[test]
    fn theme_maps_named_roles_onto_every_state() {
        use crate::Theme;

        let response = Response {
            hovered: true,
            ..Response::default()
        };
        let button = Button::new("Go", response).theme(Theme::DARK);

        assert_eq!(button.style.foreground(), Theme::DARK.fg);
        assert_eq!(button.style.background(), Theme::DARK.panel_bg);
        assert_eq!(button.hovered_style.background(), Theme::DARK.hover_bg);
        assert_eq!(button.pressed_style.background(), Theme::DARK.press_bg);
        assert_eq!(button.focused_style.foreground(), Theme::DARK.accent);
        assert_eq!(button.resolved_style().background(), Theme::DARK.hover_bg);
    }
}
