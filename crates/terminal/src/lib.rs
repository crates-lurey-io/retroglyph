//! Shared rendering seam for retroglyph's terminal-family backends.
//!
//! [`TerminalRenderer`] is a generic ANSI/SGR cell-diff renderer: it converts
//! [`Tile`] content into standard ANSI/CSI escape sequences (cursor movement,
//! `SetForegroundColor`/`SetBackgroundColor`/SGR attributes, synchronized
//! update markers) and writes them to any [`std::io::Write`] sink. It has no
//! opinion about *where* those bytes end up or *how* input arrives; that is
//! the job of the implementor crates:
//!
//! - [`retroglyph-crossterm`](https://docs.rs/retroglyph-crossterm) drives a
//!   real TTY: raw mode, alternate screen, the kitty keyboard protocol, and
//!   `crossterm::event` polling. It writes this renderer's output straight to
//!   `stdout`.
//! - `retroglyph-terminal-wasm` drives a browser terminal emulator (e.g.
//!   xterm.js) from WASM: no TTY, no polling, output collected into a
//!   `String` for JS to pull each frame, input pushed in from JS callbacks.
//!
//! # Why a separate crate from `retroglyph-window`'s `Presenter` seam
//!
//! `retroglyph-window`'s `Presenter` seam exists because every windowed
//! backend (software, future wgpu/GL) shares one runtime driver: the winit
//! event loop. Splitting the seam out of the driver lets each renderer avoid
//! depending on winit's frequent major bumps.
//!
//! Crossterm and the wasm/xterm.js driver share no such runtime: crossterm
//! owns a blocking poll loop against a real TTY, the wasm driver is pushed
//! into by JS with no polling loop at all. There is nothing to factor out of
//! *that*. What genuinely is shared between them is the ANSI/SGR cell-diff
//! renderer, so that -- and only that -- is what lives in this crate. See
//! `docs/design/018-terminal-family-split.md` for the full rationale.
//!
//! # `no_std`
//!
//! This crate always requires `std` (an `impl std::io::Write` sink), unlike
//! `retroglyph-core`, which supports `no_std`.

use retroglyph_core::color::Color;
use retroglyph_core::grid::Pos;
use retroglyph_core::style::CellModifier;
use retroglyph_core::tile::Tile;
use std::io::{self, Write};

/// Converts a [`Color`] to a standard ANSI/CSI `SetForegroundColor` (`38;...`)
/// or `SetBackgroundColor` (`48;...`) parameter sequence, written to `out`.
///
/// `base` is `38` for foreground, `48` for background (the standard SGR
/// prefix codes); `reset` is `39`/`49`, used for [`Color::Default`].
fn write_sgr_color<W: Write>(out: &mut W, color: Color, base: u8, reset: u8) -> io::Result<()> {
    match color {
        Color::Default => write!(out, "\x1b[{reset}m"),
        Color::Ansi(ansi) => {
            // Standard/bright ANSI codes are offsets from the SGR base:
            // foreground 30-37/90-97, background 40-47/100-107. `base` here
            // is 38/48 (the "extended color" introducer), so the plain ANSI
            // path uses its own literal base instead.
            let (plain_base, bright_base) = if base == 38 { (30, 90) } else { (40, 100) };
            let index = ansi.to_index();
            let code = if index < 8 {
                plain_base + index
            } else {
                bright_base + (index - 8)
            };
            write!(out, "\x1b[{code}m")
        }
        Color::Indexed(index) => write!(out, "\x1b[{base};5;{index}m"),
        Color::Rgb { r, g, b } => write!(out, "\x1b[{base};2;{r};{g};{b}m"),
    }
}

/// Writes the SGR attribute-reset-and-reapply sequence for `modifiers`.
///
/// Unlike colors, terminal attributes have no single "set to this exact
/// state" escape; each attribute is toggled independently. We always emit a
/// full reset (`\x1b[0m`) followed by every attribute in `modifiers`, which
/// costs a few more bytes than diffing attribute-by-attribute but avoids
/// having to track which of the 8 independent toggle bits changed.
fn write_sgr_attributes<W: Write>(out: &mut W, modifiers: CellModifier) -> io::Result<()> {
    write!(out, "\x1b[0m")?;
    if modifiers.contains(CellModifier::BOLD) {
        write!(out, "\x1b[1m")?;
    }
    if modifiers.contains(CellModifier::DIM) {
        write!(out, "\x1b[2m")?;
    }
    if modifiers.contains(CellModifier::ITALIC) {
        write!(out, "\x1b[3m")?;
    }
    if modifiers.contains(CellModifier::UNDERLINE) {
        write!(out, "\x1b[4m")?;
    }
    if modifiers.contains(CellModifier::BLINK) {
        write!(out, "\x1b[5m")?;
    }
    if modifiers.contains(CellModifier::REVERSE) {
        write!(out, "\x1b[7m")?;
    }
    if modifiers.contains(CellModifier::HIDDEN) {
        write!(out, "\x1b[8m")?;
    }
    if modifiers.contains(CellModifier::STRIKETHROUGH) {
        write!(out, "\x1b[9m")?;
    }
    Ok(())
}

/// A generic ANSI/SGR cell-diff renderer.
///
/// Converts [`Tile`] content into standard ANSI/CSI escape sequences and
/// writes them to a caller-supplied [`std::io::Write`] sink `W`. Tracks
/// cursor position and the last-emitted foreground/background/attribute
/// state across calls to [`draw`](Self::draw) so it only emits the escape
/// codes needed to move to changed cells and change state, the same
/// cell-diffing strategy `crossterm`-based rendering used before the
/// workspace split (see `docs/design/018-terminal-family-split.md`).
///
/// This type has no knowledge of *how* its output bytes reach a display
/// (stdout, a `String` buffer for JS, a test harness) or *how* input
/// arrives -- it is a pure `Tile` stream -> ANSI bytes transform, reused by
/// every terminal-family [`Backend`](retroglyph_core::backend::Backend)
/// implementor.
#[derive(Debug)]
pub struct TerminalRenderer<W> {
    writer: W,
    last_fg: Option<Color>,
    last_bg: Option<Color>,
    last_attrs: Option<CellModifier>,
    cursor_x: Option<u16>,
    cursor_y: Option<u16>,
}

impl<W: Write> TerminalRenderer<W> {
    /// Creates a new renderer writing to `writer`.
    pub const fn new(writer: W) -> Self {
        Self {
            writer,
            last_fg: None,
            last_bg: None,
            last_attrs: None,
            cursor_x: None,
            cursor_y: None,
        }
    }

    /// Returns a reference to the underlying writer.
    pub const fn writer(&self) -> &W {
        &self.writer
    }

    /// Returns a mutable reference to the underlying writer.
    pub const fn writer_mut(&mut self) -> &mut W {
        &mut self.writer
    }

    /// Consumes the renderer, returning the underlying writer.
    pub fn into_writer(self) -> W {
        self.writer
    }

    /// Resets tracked cursor/style state without touching the writer.
    ///
    /// Call this after an external clear (e.g. `\x1b[2J`) so the next
    /// [`draw`](Self::draw) doesn't skip a `MoveTo`/color/attribute escape
    /// under the assumption the terminal is still in the last-known state.
    pub const fn reset_state(&mut self) {
        self.last_fg = None;
        self.last_bg = None;
        self.last_attrs = None;
        self.cursor_x = None;
        self.cursor_y = None;
    }

    /// Begins a synchronized update (`\x1b[?2026h`).
    ///
    /// Terminals that support this hold rendering until the matching
    /// [`end_synchronized_update`](Self::end_synchronized_update), avoiding
    /// visible tearing mid-frame. Terminals that don't understand the
    /// sequence ignore it.
    ///
    /// # Errors
    ///
    /// Returns an error if the writer fails.
    pub fn begin_synchronized_update(&mut self) -> io::Result<()> {
        write!(self.writer, "\x1b[?2026h")
    }

    /// Ends a synchronized update (`\x1b[?2026l`). See
    /// [`begin_synchronized_update`](Self::begin_synchronized_update).
    ///
    /// # Errors
    ///
    /// Returns an error if the writer fails.
    pub fn end_synchronized_update(&mut self) -> io::Result<()> {
        write!(self.writer, "\x1b[?2026l")
    }

    /// Draws changed cells, emitting only the escape sequences needed to
    /// move the cursor and change color/attribute state versus what was last
    /// drawn.
    ///
    /// Mirrors [`Backend::draw`](retroglyph_core::backend::Backend::draw)'s
    /// contract: `content` is a stream of `(Pos, &Tile)` pairs to render.
    /// Does not flush; call [`flush`](Self::flush) after.
    ///
    /// # Errors
    ///
    /// Returns an error if the writer fails.
    #[allow(clippy::similar_names)]
    pub fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (Pos, &'a Tile)>,
    {
        for (pos, cell) in content {
            // Spacer cells are the right half of a wide character. The wide
            // char itself already drew over this position, so skip it.
            #[cfg(feature = "egc")]
            if cell
                .flags()
                .contains(retroglyph_core::tile::TileFlags::WIDE_CHAR_SPACER)
            {
                continue;
            }
            #[cfg(not(feature = "egc"))]
            if cell.glyph() == '\0' {
                continue;
            }

            let fg = cell.style().foreground();
            let bg = cell.style().background();
            let attrs = cell.style().modifiers();

            // Only emit a cursor move when the cursor isn't already at the
            // right position (adjacent cells advance the cursor by printing).
            let needs_move = self.cursor_y != Some(pos.y) || self.cursor_x != Some(pos.x);
            if needs_move {
                // CSI row;col H is 1-indexed.
                write!(self.writer, "\x1b[{};{}H", pos.y + 1, pos.x + 1)?;
            }

            // Attributes first: `write_sgr_attributes` always starts with a
            // full SGR reset (`\x1b[0m`), which also resets fg/bg to the
            // terminal default. Emitting it after the color codes would
            // silently clobber whatever color was just set.
            if self.last_attrs != Some(attrs) {
                write_sgr_attributes(&mut self.writer, attrs)?;
                self.last_attrs = Some(attrs);
                // The reset also invalidates the terminal's actual fg/bg
                // state even when our tracked `last_fg`/`last_bg` value
                // hasn't changed, so force both to be re-emitted below.
                self.last_fg = None;
                self.last_bg = None;
            }

            if self.last_fg != Some(fg) {
                write_sgr_color(&mut self.writer, fg, 38, 39)?;
                self.last_fg = Some(fg);
            }

            if self.last_bg != Some(bg) {
                write_sgr_color(&mut self.writer, bg, 48, 49)?;
                self.last_bg = Some(bg);
            }

            #[allow(unused_assignments)]
            let mut cell_width: u16 = 1;
            #[cfg(feature = "egc")]
            {
                // Print the full EGC if present; otherwise the primary glyph.
                let mut glyph_buf = [0u8; 4];
                let s: &str = match cell.extra() {
                    Some(extra) => extra,
                    None => cell.glyph().encode_utf8(&mut glyph_buf),
                };
                #[allow(clippy::cast_possible_truncation)]
                {
                    cell_width = unicode_width::UnicodeWidthStr::width(s).max(1) as u16;
                }
                write!(self.writer, "{s}")?;
            }
            #[cfg(not(feature = "egc"))]
            {
                #[allow(clippy::cast_possible_truncation)]
                {
                    cell_width =
                        unicode_width::UnicodeWidthChar::width(cell.glyph()).unwrap_or(1) as u16;
                }
                write!(self.writer, "{}", cell.glyph())?;
            }

            // After printing, the terminal cursor advances by the cell's
            // display width. Track that so the next cell can skip the move.
            self.cursor_x = Some(pos.x + cell_width);
            self.cursor_y = Some(pos.y);
        }
        Ok(())
    }

    /// Flushes the underlying writer.
    ///
    /// # Errors
    ///
    /// Returns an error if the writer fails to flush.
    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_core::color::AnsiColor;
    use retroglyph_core::style::Style;

    fn render_one(tile: &Tile) -> String {
        let mut renderer = TerminalRenderer::new(Vec::new());
        renderer
            .draw(core::iter::once((Pos { x: 0, y: 0 }, tile)))
            .unwrap();
        renderer.flush().unwrap();
        String::from_utf8(renderer.into_writer()).unwrap()
    }

    #[test]
    fn moves_cursor_with_1_indexed_csi() {
        let tile = Tile::new('X', Style::default());
        let out = render_one(&tile);
        assert!(out.contains("\x1b[1;1H"), "output: {out:?}");
        assert!(out.contains('X'));
    }

    #[test]
    fn default_color_emits_reset_codes() {
        let tile = Tile::new('X', Style::default());
        let out = render_one(&tile);
        assert!(out.contains("\x1b[39m"), "output: {out:?}");
        assert!(out.contains("\x1b[49m"), "output: {out:?}");
    }

    #[test]
    fn ansi_color_maps_to_standard_sgr_range() {
        let style = Style::new().fg(Color::Ansi(AnsiColor::Red));
        let tile = Tile::new('X', style);
        let out = render_one(&tile);
        // Red is index 1 -> plain base 30 + 1 = 31.
        assert!(out.contains("\x1b[31m"), "output: {out:?}");
    }

    #[test]
    fn bright_ansi_color_maps_to_bright_sgr_range() {
        let style = Style::new().fg(Color::Ansi(AnsiColor::BrightRed));
        let tile = Tile::new('X', style);
        let out = render_one(&tile);
        // BrightRed is index 9 -> bright base 90 + (9-8) = 91.
        assert!(out.contains("\x1b[91m"), "output: {out:?}");
    }

    #[test]
    fn rgb_color_uses_extended_sgr() {
        let style = Style::new().fg(Color::Rgb { r: 1, g: 2, b: 3 });
        let tile = Tile::new('X', style);
        let out = render_one(&tile);
        assert!(out.contains("\x1b[38;2;1;2;3m"), "output: {out:?}");
    }

    #[test]
    fn indexed_color_uses_extended_sgr() {
        let style = Style::new().fg(Color::Indexed(200));
        let tile = Tile::new('X', style);
        let out = render_one(&tile);
        assert!(out.contains("\x1b[38;5;200m"), "output: {out:?}");
    }

    /// Regression test: `write_sgr_attributes` always starts with a full SGR
    /// reset (`\x1b[0m`), which also resets fg/bg to the terminal default.
    /// The color codes for a cell must be emitted *after* the attribute
    /// reset, not before, or a reset clobbers the color that was just set.
    /// This exact ordering bug broke `tests/e2e_snapshots.rs`'s
    /// `hex_battle` snapshot during the retroglyph-terminal extraction: the
    /// first cell drawn (default attributes, non-default color) lost its
    /// background color entirely.
    #[test]
    fn attribute_reset_does_not_clobber_color_on_first_cell() {
        // Style::default() has empty modifiers, which still differs from the
        // renderer's initial `last_attrs` of `None`, so the reset is emitted
        // on this very first cell -- exactly the scenario that regressed.
        let style = Style::new().fg(Color::Rgb { r: 1, g: 2, b: 3 });
        let tile = Tile::new('X', style);
        let mut renderer = TerminalRenderer::new(Vec::new());
        renderer
            .draw(core::iter::once((Pos { x: 0, y: 0 }, &tile)))
            .unwrap();
        renderer.flush().unwrap();
        let out = String::from_utf8(renderer.into_writer()).unwrap();
        // The color escape must appear, and after the last `\x1b[0m` reset
        // in the stream (not before it, where it would be clobbered).
        let reset_pos = out.rfind("\x1b[0m").expect("reset should be emitted");
        let color_pos = out
            .find("\x1b[38;2;1;2;3m")
            .expect("color escape should be emitted");
        assert!(
            color_pos > reset_pos,
            "color escape must come after the attribute reset: {out:?}"
        );
    }

    #[test]
    fn bold_attribute_emitted() {
        let style = Style::new().bold();
        let tile = Tile::new('X', style);
        let out = render_one(&tile);
        assert!(out.contains("\x1b[1m"), "output: {out:?}");
    }

    #[test]
    fn adjacent_cells_skip_redundant_move() {
        let tile_a = Tile::new('A', Style::default());
        let tile_b = Tile::new('B', Style::default());
        let mut renderer = TerminalRenderer::new(Vec::new());
        renderer
            .draw([(Pos { x: 0, y: 0 }, &tile_a), (Pos { x: 1, y: 0 }, &tile_b)].into_iter())
            .unwrap();
        renderer.flush().unwrap();
        let out = String::from_utf8(renderer.into_writer()).unwrap();
        // Only one cursor move: the second cell is adjacent to the first.
        assert_eq!(out.matches('H').count(), 1, "output: {out:?}");
    }

    #[test]
    fn reset_state_clears_tracked_state() {
        let tile = Tile::new('X', Style::default());
        let mut renderer = TerminalRenderer::new(Vec::new());
        renderer
            .draw(core::iter::once((Pos { x: 0, y: 0 }, &tile)))
            .unwrap();
        renderer.reset_state();
        renderer
            .draw(core::iter::once((Pos { x: 0, y: 0 }, &tile)))
            .unwrap();
        renderer.flush().unwrap();
        let out = String::from_utf8(renderer.into_writer()).unwrap();
        // Without reset_state, the second draw call (same pos, same style)
        // would skip the move + color codes entirely.
        assert_eq!(out.matches("\x1b[1;1H").count(), 2, "output: {out:?}");
    }

    #[test]
    fn synchronized_update_markers() {
        let mut renderer = TerminalRenderer::new(Vec::new());
        renderer.begin_synchronized_update().unwrap();
        renderer.end_synchronized_update().unwrap();
        renderer.flush().unwrap();
        let out = String::from_utf8(renderer.into_writer()).unwrap();
        assert_eq!(out, "\x1b[?2026h\x1b[?2026l");
    }
}
