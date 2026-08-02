//! [`BorderType`]: which box-drawing glyph set a bordered widget draws with.
use retroglyph_core::symbols::BorderSet;
use retroglyph_core::symbols::border;

/// The box-drawing glyph set [`BoxBorder`](super::BoxBorder), [`Panel`](super::Panel), and
/// [`Modal`](super::Modal) draw their corners and edges with.
///
/// [`BorderType::Plain`] is the default -- the single-line set every one of these widgets drew
/// before this type existed, so adding it is purely additive. The other three variants exist to
/// make nested or stateful boxes visually distinct at a glance: an outer [`BorderType::Double`]
/// around an inner [`BorderType::Plain`] reads instantly, where two identical single lines don't,
/// and double/thick borders are a legible way to mark an active pane even on a 16-color terminal
/// where [`Theme`](crate::Theme) alone can't carry that distinction.
///
/// Covers four of ratatui's six `BorderType` variants (`Plain`, `Rounded`, `Double`, `Thick`);
/// `QuadrantInside`/`QuadrantOutside` are omitted since nothing here has a use for them yet. Each
/// variant maps directly to a [`BorderSet`](retroglyph_core::symbols::BorderSet) in
/// [`retroglyph_core::symbols::border`], the shared glyph tables every crate draws borders from.
///
/// # Examples
///
/// ```
/// use retroglyph_widgets::{BorderType, BoxBorder};
///
/// let border = BoxBorder::new().border_type(BorderType::Rounded);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BorderType {
    /// Single-line box-drawing glyphs: `┌─┐│└┘`.
    #[default]
    Plain,
    /// Single-line glyphs with rounded corners: `╭─╮│╰╯`.
    Rounded,
    /// Double-line box-drawing glyphs: `╔═╗║╚╝`.
    Double,
    /// Heavy single-line box-drawing glyphs: `┏━┓┃┗┛`.
    Thick,
}

impl BorderType {
    /// This variant's [`BorderSet`](retroglyph_core::symbols::BorderSet): the six corner/edge
    /// glyphs to draw a border with.
    #[must_use]
    pub(crate) const fn glyphs(self) -> BorderSet {
        match self {
            Self::Plain => border::PLAIN,
            Self::Rounded => border::ROUNDED,
            Self::Double => border::DOUBLE,
            Self::Thick => border::THICK,
        }
    }
}
