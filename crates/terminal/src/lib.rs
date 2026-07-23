//! ANSI/SGR cell-diff renderer shared by retroglyph's terminal-family
//! backends.
//!
//! [`TerminalRenderer`] converts [`Tile`] content into standard ANSI/CSI
//! escape sequences (cursor movement,
//! `SetForegroundColor`/`SetBackgroundColor`/SGR attributes, synchronized
//! update markers) and writes them to any [`std::io::Write`] sink. It has no
//! opinion about where those bytes end up or how input arrives; two crates
//! plug it into a concrete environment:
//!
//! ```text
//!                     +-----------------------+
//!                     |     TerminalRenderer   |
//!                     |  (this crate: Tile ->  |
//!                     |   ANSI/SGR escape      |
//!                     |   sequences)           |
//!                     +-----------------------+
//!                        ^                 ^
//!                        |                 |
//!            std::io::Write          std::io::Write
//!               (String buffer)         (stdout)
//!                        |                 |
//!  +-----------------------------+   +-----------------------------+
//!  | retroglyph-terminal-wasm    |   | retroglyph-crossterm        |
//!  | pushed JS key/resize events |   | raw mode, alternate screen, |
//!  | -> String pulled by JS each |   | kitty keyboard protocol,    |
//!  | frame (xterm.js renders it) |   | crossterm::event polling    |
//!  +-----------------------------+   +-----------------------------+
//! ```
//!
//! - [`retroglyph-crossterm`](https://docs.rs/retroglyph-crossterm) drives a
//!   real TTY: raw mode, alternate screen, the kitty keyboard protocol, and
//!   `crossterm::event` polling. It writes this renderer's output straight to
//!   `stdout`.
//! - [`retroglyph-terminal-wasm`](https://docs.rs/retroglyph-terminal-wasm)
//!   drives a browser terminal emulator (e.g. xterm.js) from WASM: no TTY, no
//!   polling, output collected into a `String` for JS to pull each frame,
//!   input pushed in from JS callbacks.
//!
//! # Why not part of `retroglyph-window`
//!
//! `retroglyph-window` splits input (winit event loop) from output
//! (`Presenter`) because every windowed backend shares one runtime driver:
//! the winit event loop. That split lets renderer crates avoid depending on
//! winit's frequent major-version bumps.
//!
//! Crossterm and the wasm/xterm.js driver share no such runtime: crossterm
//! owns a blocking poll loop against a real TTY, and the wasm driver is
//! pushed into by JS with no polling loop at all. What they do share is the
//! ANSI/SGR cell-diff renderer, so that is what lives in this crate.
//!
//! # `no_std`
//!
//! This crate always requires `std` (an `impl std::io::Write` sink), unlike
//! `retroglyph-core`, which supports `no_std`.
//!
//! # RGB color fallback on 256-color terminals
//!
//! [`Color::Rgb`] tiles are written out verbatim as a 24-bit truecolor SGR
//! sequence (`38;2;r;g;b` / `48;2;r;g;b`, one of the codes this crate's
//! internal SGR-color writer emits). This crate does **not** quantize RGB down to the 256-color or 16-color ANSI
//! palettes, and neither does `retroglyph-core`: there is no
//! `Color::to_indexed()`-style guarantee anywhere in this workspace. The
//! bytes this renderer emits are the same regardless of what the receiving
//! terminal actually supports.
//!
//! This mirrors `crossterm`'s own `SetForegroundColor`/`SetBackgroundColor`
//! behavior (and that of most Rust terminal-UI crates): truecolor codes are
//! written unconditionally, and it is left to the terminal emulator (or a
//! multiplexer like `tmux`/`screen` sitting in between) to interpret or
//! degrade them. In practice:
//!
//! - Terminals that advertise truecolor support (`$COLORTERM=truecolor` or
//!   `24bit`) render the exact color.
//! - Many terminals and multiplexers that only support the 256-color palette
//!   (`$TERM=*-256color`) approximate the requested RGB to the nearest
//!   palette entry themselves, since terminal implementations commonly
//!   downsample unrecognized-depth SGR sequences rather than drop them.
//! - A minority of older/limited terminals may render truecolor sequences
//!   incorrectly (wrong color, or no color at all) if they don't recognize
//!   the extended `;2;` SGR form.
//!
//! Callers that need a specific, correct color on a known-limited terminal
//! should use [`Color::Indexed`] or [`Color::Ansi`] explicitly instead of
//! [`Color::Rgb`]; both are passed through untranslated (`38;5;n` / plain ANSI
//! codes) and have no ambiguity across terminal color depths. There is
//! currently no capability-detection step in this crate (or
//! `retroglyph-crossterm`) that would let it choose automatically -- adding
//! one would require querying/guessing terminal color depth (`$COLORTERM`,
//! `$TERM`, or a runtime query), which is out of scope for this shared
//! renderer and left to callers or a future crate.

// Compile the code blocks in this crate's own README as doctests so its quick start is
// type-checked on every test run and cannot silently rot. The `cfg(doctest)` gate keeps this out
// of the rendered crate documentation -- see `retroglyph-crossterm`'s matching include for the
// same pattern applied to the workspace root README.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

use retroglyph_core::color::Color;
use retroglyph_core::grid::Pos;
use retroglyph_core::tile::Tile;
use std::io::{self, Write};

/// Writes a [`Color`]'s SGR parameter list (no `\x1b[`/`m` wrapper) for `SetForegroundColor`
/// (`38;...`) or `SetBackgroundColor` (`48;...`) to `out`.
///
/// `base` is `38` for foreground, `48` for background (the standard SGR prefix codes); `reset`
/// is `39`/`49`, used for [`Color::Default`]. This is the shared parameter-building block behind
/// both [`write_sgr_color`] (a single complete escape sequence) and the combined-fg/bg path in
/// [`TerminalRenderer::draw`], which concatenates two calls' output with `;` into one sequence.
fn write_sgr_params<W: Write>(out: &mut W, color: Color, base: u8, reset: u8) -> io::Result<()> {
    match color {
        Color::Default => write!(out, "{reset}"),
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
            write!(out, "{code}")
        }
        Color::Indexed(index) => write!(out, "{base};5;{index}"),
        // No quantization: passed straight through as a 24-bit truecolor SGR
        // sequence. See the crate-level "RGB color fallback on 256-color
        // terminals" doc section for the contract this leaves callers with.
        Color::Rgb { r, g, b } => write!(out, "{base};2;{r};{g};{b}"),
    }
}

/// Converts a [`Color`] to a standard ANSI/CSI `SetForegroundColor` (`38;...`)
/// or `SetBackgroundColor` (`48;...`) escape sequence, written to `out`.
///
/// `base` is `38` for foreground, `48` for background (the standard SGR
/// prefix codes); `reset` is `39`/`49`, used for [`Color::Default`].
fn write_sgr_color<W: Write>(out: &mut W, color: Color, base: u8, reset: u8) -> io::Result<()> {
    write!(out, "\x1b[")?;
    write_sgr_params(out, color, base, reset)?;
    write!(out, "m")
}

/// A generic ANSI/SGR cell-diff renderer.
///
/// Converts [`Tile`] content into standard ANSI/CSI escape sequences and
/// writes them to a caller-supplied [`std::io::Write`] sink `W`. Tracks
/// cursor position and the last-emitted foreground/background/attribute
/// state across calls to [`draw`](Self::draw) so it only emits the escape
/// codes needed to move to changed cells and change state.
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
    cursor_x: Option<u16>,
    cursor_y: Option<u16>,
    plain: bool,
}

impl<W: Write> TerminalRenderer<W> {
    /// Creates a new renderer writing to `writer`.
    pub const fn new(writer: W) -> Self {
        Self {
            writer,
            last_fg: None,
            last_bg: None,
            cursor_x: None,
            cursor_y: None,
            plain: false,
        }
    }

    /// Creates a new renderer writing to `writer`, with plain-mode set explicitly.
    ///
    /// See [`set_plain_mode`](Self::set_plain_mode) for what plain mode changes; prefer
    /// [`TerminalRenderer::auto`] when `W` implements [`std::io::IsTerminal`] and the mode should
    /// be picked automatically instead of hardcoded.
    pub const fn with_plain_mode(writer: W, plain: bool) -> Self {
        Self {
            writer,
            last_fg: None,
            last_bg: None,
            cursor_x: None,
            cursor_y: None,
            plain,
        }
    }

    /// Returns whether plain mode is enabled. See [`set_plain_mode`](Self::set_plain_mode).
    pub const fn plain_mode(&self) -> bool {
        self.plain
    }

    /// Enables or disables plain mode.
    ///
    /// In plain mode, [`draw`](Self::draw) and the synchronized-update markers stop emitting
    /// ANSI/CSI escape sequences (cursor moves, color/SGR codes, `\x1b[?2026h`/`l`) entirely.
    /// Cell text is written as plain text instead, with row changes turned into `\n` and gaps
    /// between non-adjacent cells on the same row padded with spaces, so a full-grid
    /// [`draw`](Self::draw) call degrades to a readable ASCII rendering of that frame.
    ///
    /// This is modeled on Python's `blessed`, which does the same thing when its output stream
    /// isn't a TTY: piping or redirecting output (`myapp > log.txt`) shouldn't leave a file full
    /// of unreadable escape codes. Because this renderer only ever draws *changed* cells, repeated
    /// [`draw`](Self::draw) calls in plain mode append each frame's diff as more plain text rather
    /// than overwriting previous output in place -- there is no cursor-addressable terminal to
    /// overwrite when the sink is a file or pipe, so this is a lossy degradation intended for
    /// logging/debugging, not for reproducing the exact interactive frame sequence.
    pub const fn set_plain_mode(&mut self, plain: bool) {
        self.plain = plain;
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
    ///
    /// A no-op in [plain mode](Self::set_plain_mode): synchronized updates are themselves a
    /// control code with nothing to synchronize once cell output has already degraded to plain
    /// text.
    pub fn begin_synchronized_update(&mut self) -> io::Result<()> {
        if self.plain {
            return Ok(());
        }
        write!(self.writer, "\x1b[?2026h")
    }

    /// Ends a synchronized update (`\x1b[?2026l`). See
    /// [`begin_synchronized_update`](Self::begin_synchronized_update).
    ///
    /// # Errors
    ///
    /// Returns an error if the writer fails.
    ///
    /// A no-op in [plain mode](Self::set_plain_mode); see
    /// [`begin_synchronized_update`](Self::begin_synchronized_update).
    pub fn end_synchronized_update(&mut self) -> io::Result<()> {
        if self.plain {
            return Ok(());
        }
        write!(self.writer, "\x1b[?2026l")
    }

    /// Draws changed cells, emitting only the escape sequences needed to
    /// move the cursor and change color/attribute state versus what was last
    /// drawn.
    ///
    /// Mirrors [`Output::draw`](retroglyph_core::backend::Output::draw)'s
    /// contract: `content` is a stream of `(Pos, &Tile, Option<&str>)` items
    /// to render, the last being the tile's full grapheme text when it has
    /// one. Does not flush; call [`flush`](Self::flush) after.
    ///
    /// # Errors
    ///
    /// Returns an error if the writer fails.
    #[allow(clippy::similar_names)]
    pub fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (Pos, &'a Tile, Option<&'a str>)>,
    {
        for (pos, cell, extra) in content {
            #[cfg(not(feature = "egc"))]
            let _ = extra;

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

            if self.plain {
                // Row change: newline(s) instead of a cursor-move escape. Advancing rows emits
                // one `\n` per skipped row so blank rows still show up as blank lines; a
                // same-or-backward row repeat (a new frame's diff starting over) just starts a
                // fresh line, since there's no cursor-addressable terminal to overwrite in place.
                let start_col = match self.cursor_y {
                    Some(y) if pos.y == y => self.cursor_x.unwrap_or(0),
                    Some(y) if pos.y > y => {
                        for _ in 0..(pos.y - y) {
                            writeln!(self.writer)?;
                        }
                        0
                    }
                    Some(_) => {
                        writeln!(self.writer)?;
                        0
                    }
                    None => 0,
                };
                // Pad gaps between non-adjacent cells on the same row with spaces so columns
                // still line up; a fresh row starts padding from column 0.
                for _ in start_col..pos.x {
                    write!(self.writer, " ")?;
                }
            } else {
                // Only emit a cursor move when the cursor isn't already at the
                // right position (adjacent cells advance the cursor by printing).
                let needs_move = self.cursor_y != Some(pos.y) || self.cursor_x != Some(pos.x);
                if needs_move {
                    // CSI row;col H is 1-indexed.
                    write!(self.writer, "\x1b[{};{}H", pos.y + 1, pos.x + 1)?;
                }

                let fg_changed = self.last_fg != Some(fg);
                let bg_changed = self.last_bg != Some(bg);

                // When both channels change in the same cell transition, combine them into a
                // single SGR sequence (`\x1b[38;...;48;...m`) instead of two: same visual
                // effect, half the CSI-introducer/terminator overhead. Only one of the two
                // actually changing still gets a single-channel sequence, so an unchanged
                // channel is never re-emitted.
                if fg_changed && bg_changed {
                    write!(self.writer, "\x1b[")?;
                    write_sgr_params(&mut self.writer, fg, 38, 39)?;
                    write!(self.writer, ";")?;
                    write_sgr_params(&mut self.writer, bg, 48, 49)?;
                    write!(self.writer, "m")?;
                    self.last_fg = Some(fg);
                    self.last_bg = Some(bg);
                } else if fg_changed {
                    write_sgr_color(&mut self.writer, fg, 38, 39)?;
                    self.last_fg = Some(fg);
                } else if bg_changed {
                    write_sgr_color(&mut self.writer, bg, 48, 49)?;
                    self.last_bg = Some(bg);
                }
            }

            #[allow(unused_assignments)]
            let mut cell_width: u16 = 1;
            #[cfg(feature = "egc")]
            {
                // Print the full EGC if present; otherwise the primary glyph.
                let mut glyph_buf = [0u8; 4];
                let s: &str = match extra {
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

impl<W: Write + io::IsTerminal> TerminalRenderer<W> {
    /// Creates a new renderer writing to `writer`, auto-detecting plain mode from whether
    /// `writer` is a TTY.
    ///
    /// Equivalent to `TerminalRenderer::with_plain_mode(writer, !writer.is_terminal())`. `W`
    /// must implement [`std::io::IsTerminal`] for this to be callable -- `std::io::Stdout`,
    /// `std::io::Stdin`, `std::io::Stderr`, `std::fs::File`, and their `*Lock` variants all do;
    /// an in-memory sink like `Vec<u8>` does not, so use
    /// [`with_plain_mode`](Self::with_plain_mode) directly for those.
    ///
    /// This mirrors how `retroglyph-crossterm` would typically wire up pipe-safe output: check
    /// once at startup whether the real destination is an interactive terminal, and fall back to
    /// plain text for everything else (files, pipes, `> log.txt` redirection, CI runners).
    pub fn auto(writer: W) -> Self {
        let plain = !writer.is_terminal();
        Self::with_plain_mode(writer, plain)
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
            .draw(core::iter::once((Pos { x: 0, y: 0 }, tile, None)))
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
        // Both fg and bg are unset on the very first draw, so they're combined into a single
        // SGR sequence rather than two separate `\x1b[39m\x1b[49m` escapes.
        assert!(out.contains("\x1b[39;49m"), "output: {out:?}");
    }

    #[test]
    fn ansi_color_maps_to_standard_sgr_range() {
        let style = Style::new().fg(Color::Ansi(AnsiColor::Red));
        let tile = Tile::new('X', style);
        let out = render_one(&tile);
        // Red is index 1 -> plain base 30 + 1 = 31. Background is still default (49), and both
        // channels changed on this first draw, so they're combined into one sequence.
        assert!(out.contains("\x1b[31;49m"), "output: {out:?}");
    }

    #[test]
    fn bright_ansi_color_maps_to_bright_sgr_range() {
        let style = Style::new().fg(Color::Ansi(AnsiColor::BrightRed));
        let tile = Tile::new('X', style);
        let out = render_one(&tile);
        // BrightRed is index 9 -> bright base 90 + (9-8) = 91, combined with the default bg.
        assert!(out.contains("\x1b[91;49m"), "output: {out:?}");
    }

    #[test]
    fn rgb_color_uses_extended_sgr() {
        let style = Style::new().fg(Color::Rgb { r: 1, g: 2, b: 3 });
        let tile = Tile::new('X', style);
        let out = render_one(&tile);
        assert!(out.contains("\x1b[38;2;1;2;3;49m"), "output: {out:?}");
    }

    #[test]
    fn indexed_color_uses_extended_sgr() {
        let style = Style::new().fg(Color::Indexed(200));
        let tile = Tile::new('X', style);
        let out = render_one(&tile);
        assert!(out.contains("\x1b[38;5;200;49m"), "output: {out:?}");
    }

    #[test]
    fn rgb_color_is_passed_through_without_quantization() {
        // Regression test for the documented RGB fallback contract: an RGB
        // value that doesn't land on any of the 256-color palette's exact
        // entries (e.g. a 6x6x6 cube step or a grayscale ramp step) is still
        // emitted verbatim as a 24-bit truecolor sequence, not snapped to the
        // nearest indexed color.
        let style = Style::new().fg(Color::Rgb {
            r: 91,
            g: 142,
            b: 217,
        });
        let tile = Tile::new('X', style);
        let out = render_one(&tile);
        assert!(out.contains("\x1b[38;2;91;142;217;49m"), "output: {out:?}");
        assert!(
            !out.contains("38;5;"),
            "expected no indexed fallback, got: {out:?}"
        );
    }

    #[test]
    fn combines_fg_and_bg_into_single_sequence_when_both_change() {
        // Both channels change relative to the previous cell's state, so they should be
        // coalesced into one `\x1b[38;...;48;...m` sequence instead of two separate escapes.
        let old = Tile::new(
            'A',
            Style::new()
                .fg(Color::Rgb { r: 1, g: 2, b: 3 })
                .bg(Color::Rgb { r: 4, g: 5, b: 6 }),
        );
        let new = Tile::new(
            'B',
            Style::new()
                .fg(Color::Rgb {
                    r: 10,
                    g: 20,
                    b: 30,
                })
                .bg(Color::Rgb {
                    r: 40,
                    g: 50,
                    b: 60,
                }),
        );
        let mut renderer = TerminalRenderer::new(Vec::new());
        renderer
            .draw(core::iter::once((Pos { x: 0, y: 0 }, &old, None)))
            .unwrap();
        renderer
            .draw(core::iter::once((Pos { x: 0, y: 0 }, &new, None)))
            .unwrap();
        renderer.flush().unwrap();
        let out = String::from_utf8(renderer.into_writer()).unwrap();
        assert!(
            out.contains("\x1b[38;2;10;20;30;48;2;40;50;60m"),
            "output: {out:?}"
        );
        assert!(
            !out.contains("\x1b[48;2;40;50;60m"),
            "bg should not be emitted as a separate sequence, got: {out:?}"
        );
    }

    #[test]
    fn only_fg_change_emits_single_channel_sequence() {
        // Background is unchanged between the two draws, so only the fg escape should be
        // emitted -- no combined sequence, and no redundant bg re-emission.
        let bg = Color::Rgb { r: 4, g: 5, b: 6 };
        let old = Tile::new('A', Style::new().fg(Color::Rgb { r: 1, g: 2, b: 3 }).bg(bg));
        let new = Tile::new(
            'B',
            Style::new()
                .fg(Color::Rgb {
                    r: 10,
                    g: 20,
                    b: 30,
                })
                .bg(bg),
        );
        let mut renderer = TerminalRenderer::new(Vec::new());
        renderer
            .draw(core::iter::once((Pos { x: 0, y: 0 }, &old, None)))
            .unwrap();
        renderer
            .draw(core::iter::once((Pos { x: 0, y: 0 }, &new, None)))
            .unwrap();
        renderer.flush().unwrap();
        let out = String::from_utf8(renderer.into_writer()).unwrap();
        // Only the second draw's fg escape should appear after the first draw's combined one;
        // check the second draw doesn't re-emit a bg sequence.
        let second_draw_start = out.rfind("\x1b[1;1H").unwrap();
        let second_draw = &out[second_draw_start..];
        assert!(
            second_draw.contains("\x1b[38;2;10;20;30m"),
            "output: {second_draw:?}"
        );
        assert!(
            !second_draw.contains("48;2;4;5;6"),
            "output: {second_draw:?}"
        );
    }

    #[test]
    fn adjacent_cells_skip_redundant_move() {
        let tile_a = Tile::new('A', Style::default());
        let tile_b = Tile::new('B', Style::default());
        let mut renderer = TerminalRenderer::new(Vec::new());
        renderer
            .draw(
                [
                    (Pos { x: 0, y: 0 }, &tile_a, None),
                    (Pos { x: 1, y: 0 }, &tile_b, None),
                ]
                .into_iter(),
            )
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
            .draw(core::iter::once((Pos { x: 0, y: 0 }, &tile, None)))
            .unwrap();
        renderer.reset_state();
        renderer
            .draw(core::iter::once((Pos { x: 0, y: 0 }, &tile, None)))
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

    #[test]
    fn plain_mode_strips_escape_codes() {
        let style = Style::new().fg(Color::Ansi(AnsiColor::Red));
        let tile = Tile::new('X', style);
        let mut renderer = TerminalRenderer::with_plain_mode(Vec::new(), true);
        renderer
            .draw(core::iter::once((Pos { x: 0, y: 0 }, &tile, None)))
            .unwrap();
        renderer.flush().unwrap();
        let out = String::from_utf8(renderer.into_writer()).unwrap();
        assert_eq!(out, "X");
    }

    #[test]
    fn plain_mode_renders_full_grid_as_readable_ascii() {
        // A 2-row, gapped grid: row 0 has 'A' at col 0 and 'B' at col 2 (a
        // gap at col 1); row 1 has 'C' at col 0. Plain mode should turn this
        // into readable text: gaps become spaces, row changes become '\n'.
        let a = Tile::new('A', Style::default());
        let b = Tile::new('B', Style::default());
        let c = Tile::new('C', Style::default());
        let mut renderer = TerminalRenderer::with_plain_mode(Vec::new(), true);
        renderer
            .draw(
                [
                    (Pos { x: 0, y: 0 }, &a, None),
                    (Pos { x: 2, y: 0 }, &b, None),
                    (Pos { x: 0, y: 1 }, &c, None),
                ]
                .into_iter(),
            )
            .unwrap();
        renderer.flush().unwrap();
        let out = String::from_utf8(renderer.into_writer()).unwrap();
        assert_eq!(out, "A B\nC");
        assert!(!out.contains('\x1b'), "output: {out:?}");
    }

    #[test]
    fn plain_mode_skips_blank_rows() {
        let a = Tile::new('A', Style::default());
        let b = Tile::new('B', Style::default());
        let mut renderer = TerminalRenderer::with_plain_mode(Vec::new(), true);
        renderer
            .draw(
                [
                    (Pos { x: 0, y: 0 }, &a, None),
                    (Pos { x: 0, y: 2 }, &b, None),
                ]
                .into_iter(),
            )
            .unwrap();
        renderer.flush().unwrap();
        let out = String::from_utf8(renderer.into_writer()).unwrap();
        assert_eq!(out, "A\n\nB");
    }

    #[test]
    fn plain_mode_suppresses_synchronized_update_markers() {
        let mut renderer = TerminalRenderer::with_plain_mode(Vec::new(), true);
        renderer.begin_synchronized_update().unwrap();
        renderer.end_synchronized_update().unwrap();
        renderer.flush().unwrap();
        let out = String::from_utf8(renderer.into_writer()).unwrap();
        assert_eq!(out, "");
    }

    #[test]
    fn plain_mode_setter_and_getter_round_trip() {
        let mut renderer = TerminalRenderer::new(Vec::new());
        assert!(!renderer.plain_mode());
        renderer.set_plain_mode(true);
        assert!(renderer.plain_mode());
    }

    #[test]
    fn auto_detects_plain_mode_from_non_terminal_writer() {
        // `Vec<u8>` isn't a TTY-capable writer, but `std::fs::File` implements
        // `IsTerminal`, and a regular file is never a terminal.
        let path = std::env::temp_dir().join(format!(
            "retroglyph-terminal-auto-plain-mode-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let file = std::fs::File::create(&path).unwrap();
        let renderer = TerminalRenderer::auto(file);
        assert!(renderer.plain_mode());
        drop(renderer);
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn draw_prints_full_grapheme_when_provided() {
        // The tile's `glyph` is just the primary codepoint ('e'); the full
        // combining-mark cluster only reaches the renderer via the third
        // `draw` item, not the tile itself (see `Grid::grapheme`).
        let tile = Tile::new('e', Style::default());
        let mut renderer = TerminalRenderer::new(Vec::new());
        renderer
            .draw(core::iter::once((
                Pos { x: 0, y: 0 },
                &tile,
                Some("e\u{0301}"),
            )))
            .unwrap();
        renderer.flush().unwrap();
        let out = String::from_utf8(renderer.into_writer()).unwrap();
        assert!(out.contains("e\u{0301}"), "output: {out:?}");
    }
}
