//! [`write_cast`]: exports captured frames as asciicast v3 newline-delimited JSON.

use crate::OwnedCell;
use crate::frame_recorder::CapturedFrame;
use retroglyph_core::color::Style;
use retroglyph_core::color::sgr::push_sgr;
use retroglyph_core::grid::{HasSize, Size};
use retroglyph_core::tile::TileFlags;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Write};

/// Exports `frames` as [asciicast v3](https://docs.asciinema.org/manual/asciicast/v3/)
/// newline-delimited JSON, written to `writer`.
///
/// `frames` comes from [`FrameRecorder`](crate::FrameRecorder) or the `pty` feature's capture
/// source. Writes a header line (`{"version": 3, "term": {"cols", "rows"}}`) sized to `size`, then one
/// `[interval_seconds, "o", ansi_text]` event line per captured frame: `interval_seconds` is the
/// time since the previous event (since [`FrameRecorder::new`](crate::FrameRecorder::new) for the
/// first), and `ansi_text` repositions the cursor to
/// each changed cell (`\x1b[{row};{col}H`, grouping horizontally-adjacent runs to avoid a move
/// per cell) and writes its glyph, switching [SGR](retroglyph_core::color::sgr) state only when
/// the style actually changes -- exactly what a real terminal session emitting that frame would
/// have sent, and exactly what an asciicast player (`asciinema-player`) or converter (`agg`,
/// `svg-term`) expects to replay.
///
/// Hand-rolled (via `serde_json`, not the `asciicast`/`asciicast-rs` crates) because this crate
/// only ever *writes* v3, never parses any version -- a parser's surface would be pure dead
/// weight here.
///
/// No `timestamp` header field is emitted: it would be wall-clock-dependent, and this exists for
/// deterministic, `insta`-snapshottable captures.
///
/// # Known limitation
///
/// Buffers and serializes every frame at once rather than streaming events to `writer` as they're
/// captured, so a session's full cost (memory in [`FrameRecorder`](crate::FrameRecorder), then
/// this function's formatting work) is paid up front. Matches
/// [`FrameRecorder`](crate::FrameRecorder)'s own whole-session-in-memory limitation; fine for
/// docs/demo-length captures.
///
/// # Errors
///
/// Returns an error if `writer` fails to write.
///
/// # Examples
///
/// ```
/// use retroglyph_core::backend::{DrawCell, Headless, Output};
/// use retroglyph_core::color::Style;
/// use retroglyph_core::grid::Pos;
/// use retroglyph_core::tile::Tile;
/// use retroglyph_recorder::{write_cast, FrameRecorder};
///
/// let mut recorder = FrameRecorder::new(Headless::new(3, 1));
/// let tile = Tile::new('a', Style::default());
/// recorder
///     .draw_layers(std::iter::once(DrawCell::new(Pos::new(0, 0), &tile)))
///     .unwrap();
///
/// let mut out = Vec::new();
/// write_cast(&mut out, recorder.inner().size(), &recorder.frames()).unwrap();
/// let text = String::from_utf8(out).unwrap();
/// assert!(text.contains("\"version\":3"));
/// assert!(text.contains("\"o\""));
/// ```
pub fn write_cast(writer: &mut impl Write, size: Size, frames: &[CapturedFrame]) -> io::Result<()> {
    let header = serde_json::json!({
        "version": 3,
        "term": { "cols": size.width(), "rows": size.height() },
    });
    writeln!(writer, "{header}")?;

    let mut previous_at = std::time::Duration::ZERO;
    // Persists across frames, not reset per-frame: this mirrors what a real terminal session
    // emitting these events would see -- SGR state is a property of the terminal's pen, not of
    // any single write.
    let mut current_style: Option<Style> = None;
    for frame in frames {
        let interval = frame.at.saturating_sub(previous_at).as_secs_f64();
        previous_at = frame.at;
        let ansi = render_frame(frame, &mut current_style);
        let line = serde_json::to_string(&(interval, "o", ansi))?;
        writeln!(writer, "{line}")?;
    }
    Ok(())
}

/// Renders one frame's changed cells as raw ANSI: cursor moves plus SGR-styled text, in row-major
/// order. `current_style` carries SGR state in from (and out to) the previous frame so a run that
/// happens to already match the active style emits no redundant escape.
fn render_frame(frame: &CapturedFrame, current_style: &mut Option<Style>) -> String {
    // Last write wins for a position hit more than once in the same `draw_layers` call (multiple
    // layers landing on the same cell): a text export has no layer compositing of its own to fall
    // back on, so it shows whatever the stream's own last word on that cell was.
    let mut by_pos: BTreeMap<(u16, u16), &OwnedCell> = BTreeMap::new();
    for cell in &frame.cells {
        by_pos.insert((cell.pos.y, cell.pos.x), cell);
    }

    let mut out = String::new();
    let mut cursor_row = None;
    let mut cursor_col = None;
    for ((row, col), cell) in &by_pos {
        if cursor_row != Some(*row) || cursor_col != Some(*col) {
            let _ = write!(out, "\x1b[{};{}H", row + 1, col + 1);
        }
        let (text, is_spacer) = display_text(cell);
        let style = if is_spacer {
            Style::default()
        } else {
            cell.tile.style()
        };
        if *current_style != Some(style) {
            out.push_str("\x1b[0m");
            push_sgr(&mut out, style);
            *current_style = Some(style);
        }
        out.push_str(&text);
        cursor_row = Some(*row);
        cursor_col = Some(col + 1);
    }
    out
}

/// The text `render_frame` writes for `cell`, and whether it's a wide-glyph spacer (rendered
/// blank, with no style of its own -- matching
/// [`Headless::display_glyph`](retroglyph_core::backend::Headless)'s convention for the same
/// case).
fn display_text(cell: &OwnedCell) -> (String, bool) {
    let is_spacer = cell.tile.flags().contains(TileFlags::WIDE_CHAR_SPACER);
    if is_spacer {
        return (" ".to_owned(), true);
    }
    let text = cell
        .grapheme
        .clone()
        .unwrap_or_else(|| cell.tile.glyph().to_string());
    (text, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FrameRecorder;
    use retroglyph_core::backend::{DrawCell, Headless, Output};
    use retroglyph_core::color::Color;
    use retroglyph_core::grid::Pos;
    use retroglyph_core::tile::Tile;

    #[test]
    fn header_carries_the_backend_size() {
        let recorder = FrameRecorder::new(Headless::new(7, 4));
        let mut out = Vec::new();
        write_cast(&mut out, recorder.inner().size(), &recorder.frames()).unwrap();
        let text = String::from_utf8(out).unwrap();
        let header: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(header["version"], 3);
        assert_eq!(header["term"]["cols"], 7);
        assert_eq!(header["term"]["rows"], 4);
    }

    #[test]
    fn one_event_line_per_captured_frame() {
        let mut recorder = FrameRecorder::new(Headless::new(3, 1));
        let tile = Tile::new('a', Style::default());
        recorder
            .draw_layers(std::iter::once(DrawCell::new(Pos::new(0, 0), &tile)))
            .unwrap();
        recorder
            .draw_layers(std::iter::once(DrawCell::new(Pos::new(1, 0), &tile)))
            .unwrap();

        let mut out = Vec::new();
        write_cast(&mut out, recorder.inner().size(), &recorder.frames()).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 3); // header + 2 events
    }

    #[test]
    fn emits_sgr_for_a_styled_cell() {
        let mut recorder = FrameRecorder::new(Headless::new(3, 1));
        let tile = Tile::new('a', Style::new().fg(Color::RED));
        recorder
            .draw_layers(std::iter::once(DrawCell::new(Pos::new(0, 0), &tile)))
            .unwrap();

        let mut out = Vec::new();
        write_cast(&mut out, recorder.inner().size(), &recorder.frames()).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\\u001b[0m\\u001b[31m") || text.contains("\\u001b[31m"));
    }

    /// Snapshot of a small, multi-frame, multi-style capture -- built directly from
    /// [`CapturedFrame`]s with fixed `at` durations (rather than a live [`FrameRecorder`], whose
    /// timestamps come from the wall clock) so the exported `.cast` text is fully deterministic
    /// and diffable across runs.
    #[test]
    fn write_cast_snapshot() {
        use retroglyph_core::grid::Pos;
        use std::time::Duration;

        let cell = |x: u16, y: u16, glyph: char, style: Style| OwnedCell {
            layer: 0,
            pos: Pos::new(x, y),
            tile: Tile::new(glyph, style),
            grapheme: None,
            tint: retroglyph_core::color::Tint::None,
        };
        let frames = vec![
            CapturedFrame {
                at: Duration::from_millis(0),
                cells: vec![
                    cell(0, 0, 'H', Style::new().fg(Color::GREEN)),
                    cell(1, 0, 'i', Style::new().fg(Color::GREEN)),
                ],
            },
            CapturedFrame {
                at: Duration::from_millis(250),
                cells: vec![cell(0, 1, '!', Style::default())],
            },
        ];

        let mut out = Vec::new();
        write_cast(&mut out, Size::new(10, 2), &frames).unwrap();
        insta::assert_snapshot!(String::from_utf8(out).unwrap());
    }

    #[test]
    fn first_event_interval_is_time_since_the_recorder_was_created() {
        let mut recorder = FrameRecorder::new(Headless::new(3, 1));
        let tile = Tile::new('a', Style::default());
        recorder
            .draw_layers(std::iter::once(DrawCell::new(Pos::new(0, 0), &tile)))
            .unwrap();

        let mut out = Vec::new();
        write_cast(&mut out, recorder.inner().size(), &recorder.frames()).unwrap();
        let text = String::from_utf8(out).unwrap();
        let event: serde_json::Value = serde_json::from_str(text.lines().nth(1).unwrap()).unwrap();
        // Should be a small, non-negative elapsed time -- not exactly zero, since some real time
        // passes between `FrameRecorder::new` and the first `draw_layers` call.
        let interval = event[0].as_f64().unwrap();
        assert!((0.0..1.0).contains(&interval), "interval was {interval}");
    }
}
