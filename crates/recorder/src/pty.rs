//! [`capture_pty`]: a real-pseudo-terminal capture source, behind the `pty` feature.
//!
//! Promotes the technique `examples/tests/support::capture_pty_until` already used (test-only,
//! synchronizing on a ready marker) into a public, reusable capture source that feeds the same
//! [`write_cast`](crate::write_cast) path [`FrameRecorder`](crate::FrameRecorder) does. This is
//! the mechanism retroglyph#461's "supersede the `vhs` tape plan" bet rests on: a real
//! `portable-pty` pseudo-terminal plus a real `vt100` VT-parser aims for the fidelity `vhs` gets
//! from a real terminal emulator, without one in this crate's own dependency tree.
//!
//! This is a fresh implementation of the technique, not a shared dependency with
//! `examples/tests/support` (there is no library target under `examples/tests/` for either side
//! to depend on): `capture_pty_until`'s ready-marker/settle-callback shape is tuned for
//! synchronizing a snapshot test with a specific keystroke landing, while this needs "capture N
//! settled frames with timing" instead. Migrating `examples/tests/support` onto this is a
//! reasonable follow-up, not done here.
//!
//! # Fidelity vs. the `TestHarness`-driven source
//!
//! Frames here come from diffing successive [`vt100::Screen`] polls of a real child process's PTY
//! output, not from `Output::draw_layers`'s own diff stream -- so timing reflects real scheduling
//! (the child's own draw cadence, kernel PTY buffering, this capture's poll interval) rather than
//! `FrameRecorder`'s frame-exact clock, and a poll interval coarser than the source's redraw rate
//! can miss or coalesce short-lived intermediate frames a `TestHarness`-driven capture would
//! record individually. Both still converge on identical [`write_cast`](crate::write_cast) output
//! for a *settled* scripted session (see this crate's `pty_matches_frame_recorder` test): the
//! difference is in what "a frame" means for a live, real-terminal source, not in the exported
//! format.

use crate::CapturedFrame;
use crate::owned_cell::OwnedCell;
use retroglyph_core::color::{Color, Style};
use retroglyph_core::grid::{Pos, Size};
use retroglyph_core::tile::Tile;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the child's accumulated PTY output is re-parsed and diffed against the previous
/// poll's screen. Fast enough that a docs-length capture (seconds, not minutes) doesn't visibly
/// coarsen the recorded pacing; see this module's docs for what a coarser interval costs.
const POLL_INTERVAL: Duration = Duration::from_millis(16);

/// Runs `bin` (with `args`) under a real pseudo-terminal sized `cols`x`rows`, and captures its
/// settled frames until it exits or `timeout` elapses.
///
/// Polls the accumulated PTY output through a [`vt100::Parser`] every `POLL_INTERVAL`, diffing
/// each poll's [`vt100::Screen`] cell-by-cell against the previous one; a poll with no visible
/// change produces no [`CapturedFrame`], the same convention
/// [`FrameRecorder`](crate::FrameRecorder) uses for an empty `draw_layers` call.
///
/// # Errors
///
/// Returns an error if the PTY can't be opened, `bin` can't be spawned, or the reader thread
/// panics.
///
/// # Panics
///
/// Panics if the internal output buffer's lock is poisoned, which only happens if the reader
/// thread itself panics while holding it.
pub fn capture_pty(
    bin: &Path,
    args: &[&str],
    cols: u16,
    rows: u16,
    timeout: Duration,
) -> std::io::Result<(Size, Vec<CapturedFrame>)> {
    let pty = portable_pty::native_pty_system();
    let pair = pty
        .openpty(portable_pty::PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| std::io::Error::other(error.to_string()))?;

    let mut cmd = portable_pty::CommandBuilder::new(bin);
    cmd.env("TERM", "xterm-256color");
    for arg in args {
        cmd.arg(arg);
    }
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let output = Arc::new(Mutex::new(Vec::new()));
    let output_clone = Arc::clone(&output);
    let reader_handle = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => output_clone.lock().unwrap().extend_from_slice(&buf[..n]),
            }
        }
    });

    let started = Instant::now();
    let mut parser = vt100::Parser::new(rows, cols, 0);
    let mut parsed = 0usize;
    let mut previous: Option<Vec<Vec<(String, Style)>>> = None;
    let mut frames = Vec::new();

    loop {
        std::thread::sleep(POLL_INTERVAL);
        {
            let raw = output.lock().unwrap();
            parser.process(&raw[parsed..]);
            parsed = raw.len();
        }

        let screen = parser.screen();
        let current = snapshot(screen, cols, rows);
        if let Some(cells) = diff(previous.as_ref(), &current) {
            frames.push(CapturedFrame {
                at: started.elapsed(),
                cells,
            });
        }
        previous = Some(current);

        if matches!(child.try_wait(), Ok(Some(_))) || started.elapsed() > timeout {
            break;
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    // Drop the write half so the reader thread's `read` unblocks with EOF once the child (and
    // any process still holding the slave open) has actually gone away.
    drop(pair.master.take_writer());
    reader_handle
        .join()
        .map_err(|_| std::io::Error::other("PTY reader thread panicked"))?;

    Ok((Size::new(cols, rows), frames))
}

/// One `(glyph+grapheme, style)` pair per visible cell, row-major -- a plain, comparable snapshot
/// of a [`vt100::Screen`], independent of `vt100`'s own cell representation.
fn snapshot(screen: &vt100::Screen, cols: u16, rows: u16) -> Vec<Vec<(String, Style)>> {
    (0..rows)
        .map(|row| {
            (0..cols)
                .map(|col| {
                    let Some(cell) = screen.cell(row, col) else {
                        return (String::new(), Style::default());
                    };
                    let style = Style::new()
                        .fg(convert_color(cell.fgcolor()))
                        .bg(convert_color(cell.bgcolor()));
                    (cell.contents().to_owned(), style)
                })
                .collect()
        })
        .collect()
}

/// Every cell that differs between `previous` and `current`, as [`OwnedCell`]s -- `None` if
/// nothing changed (mirrors [`FrameRecorder`](crate::FrameRecorder)'s "no frame for an empty
/// `draw_layers`" convention).
fn diff(
    previous: Option<&Vec<Vec<(String, Style)>>>,
    current: &[Vec<(String, Style)>],
) -> Option<Vec<OwnedCell>> {
    let mut cells = Vec::new();
    for (y, row) in current.iter().enumerate() {
        for (x, (contents, style)) in row.iter().enumerate() {
            let changed = previous.is_none_or(|prev| prev[y][x] != (contents.clone(), *style));
            if !changed {
                continue;
            }
            let glyph = contents.chars().next().unwrap_or(' ');
            let grapheme = if contents.chars().count() > 1 {
                Some(contents.clone())
            } else {
                None
            };
            #[allow(clippy::cast_possible_truncation)]
            let pos = Pos::new(x as u16, y as u16);
            cells.push(OwnedCell {
                layer: 0,
                pos,
                tile: Tile::new(glyph, *style),
                grapheme,
                tint: retroglyph_core::color::Tint::None,
            });
        }
    }
    if cells.is_empty() { None } else { Some(cells) }
}

/// Maps a `vt100` color to this workspace's own [`Color`]. `vt100::Color::Idx` collapses to
/// [`Color::Indexed`] uniformly (rather than distinguishing the first 16 indices as
/// [`Color::Ansi`]): the SGR this crate's own [`push_sgr`](retroglyph_core::color::sgr::push_sgr)
/// emits for `Indexed` (`38;5;n`/`48;5;n`) renders identically to the 16-color form for those
/// indices in every terminal that matters here, so the distinction isn't worth carrying through.
const fn convert_color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Default,
        vt100::Color::Idx(index) => Color::Indexed(index),
        vt100::Color::Rgb(r, g, b) => Color::rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_and_diff_report_no_change_between_identical_snapshots() {
        let a = vec![vec![("x".to_owned(), Style::default())]];
        let b = a.clone();
        assert!(diff(Some(&a), &b).is_none());
    }

    #[test]
    fn diff_reports_a_changed_cell() {
        let a = vec![vec![("x".to_owned(), Style::default())]];
        let b = vec![vec![("y".to_owned(), Style::default())]];
        let cells = diff(Some(&a), &b).expect("expected a diff");
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].tile.glyph(), 'y');
    }

    #[test]
    fn first_diff_treats_every_cell_as_changed() {
        let current = vec![vec![
            ("x".to_owned(), Style::default()),
            ("y".to_owned(), Style::default()),
        ]];
        let cells = diff(None, &current).expect("expected a diff");
        assert_eq!(cells.len(), 2);
    }
}
