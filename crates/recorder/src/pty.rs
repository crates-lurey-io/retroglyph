//! [`capture_pty`]: a real-pseudo-terminal capture source, behind the `pty` feature.
//!
//! Promotes the technique `examples/tests/support::capture_pty_until` already used (test-only,
//! synchronizing on a ready marker) into a public, reusable capture source that feeds the same
//! [`write_cast`](crate::write_cast) path [`FrameRecorder`](crate::FrameRecorder) does. This is
//! the mechanism retroglyph#461's "supersede the `vhs` tape plan" bet rests on: a real
//! `portable-pty` pseudo-terminal plus a real `vt100` VT-parser aims for the fidelity `vhs` gets
//! from a real terminal emulator, without one in this crate's own dependency tree.
//!
//! [`PtySession`] is the spawn/write/read/lifecycle plumbing this shares with
//! `examples/tests/support`'s marker/settle-driven test harness (retroglyph#1320): opening the
//! PTY, spawning the child, and draining its output on a background thread are identical in both
//! places, but what each side does with that output differs -- `capture_pty` below polls on a
//! fixed interval and diffs settled frames, while the test harness waits for a ready-marker
//! substring, writes input, then waits for a caller's settle callback before quitting, with much
//! richer panic diagnostics. `PtySession` only owns the part that's genuinely identical; neither
//! loop shape is imposed on the other.
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
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the child's accumulated PTY output is re-parsed and diffed against the previous
/// poll's screen. Fast enough that a docs-length capture (seconds, not minutes) doesn't visibly
/// coarsen the recorded pacing; see this module's docs for what a coarser interval costs.
const POLL_INTERVAL: Duration = Duration::from_millis(16);

/// A live, real-pseudo-terminal-backed child process.
///
/// The spawn/write/read/lifecycle plumbing [`capture_pty`] and `examples/tests/support`'s
/// marker/settle-driven test harness both need, factored out so each keeps its own distinct
/// polling loop on top of it. See this module's docs for why the two loops aren't shared too.
pub struct PtySession {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    // Kept alive for the session's duration even though nothing reads from it directly once
    // `reader`/`writer` are split off it below: dropping a `portable_pty` master closes the PTY
    // out from under a still-running child.
    _master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Option<Box<dyn Write + Send>>,
    output: Arc<Mutex<Vec<u8>>>,
    reader_done: Arc<Mutex<bool>>,
    reader_handle: Option<std::thread::JoinHandle<()>>,
}

impl PtySession {
    /// Opens a `cols`x`rows` pseudo-terminal, spawns `bin` (with `args` and `env`, plus
    /// `TERM=xterm-256color`) on its slave side, and starts draining the master side's output on
    /// a background thread into [`output`](Self::output).
    ///
    /// # Errors
    ///
    /// Returns an error if the PTY can't be opened, `bin` can't be spawned, or the reader/writer
    /// handles can't be obtained from the master side.
    ///
    /// # Panics
    ///
    /// Never panics itself; the spawned background reader thread panics only if `output`'s lock
    /// is poisoned, which requires another panic while holding it first.
    pub fn spawn(
        bin: &Path,
        args: &[&str],
        env: &[(&str, &str)],
        cols: u16,
        rows: u16,
    ) -> std::io::Result<Self> {
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
        for (key, value) in env {
            cmd.env(key, value);
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| std::io::Error::other(error.to_string()))?;

        let output = Arc::new(Mutex::new(Vec::new()));
        let output_clone = Arc::clone(&output);
        let reader_done = Arc::new(Mutex::new(false));
        let reader_done_clone = Arc::clone(&reader_done);
        let reader_handle = std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => output_clone.lock().unwrap().extend_from_slice(&buf[..n]),
                }
            }
            *reader_done_clone.lock().unwrap() = true;
            drop(reader);
        });

        Ok(Self {
            child,
            _master: pair.master,
            writer: Some(writer),
            output,
            reader_done,
            reader_handle: Some(reader_handle),
        })
    }

    /// The shared output buffer, appended to by the background reader thread as bytes arrive.
    /// Lock it directly (rather than calling this repeatedly and cloning the whole capture) to
    /// scan or parse only the bytes that arrived since a caller's own last poll.
    ///
    /// Drop every handle returned from this before calling [`finish`](Self::finish), which needs
    /// to be the buffer's sole owner.
    #[must_use]
    pub fn output(&self) -> Arc<Mutex<Vec<u8>>> {
        Arc::clone(&self.output)
    }

    /// Set once the background reader thread's `read` returns EOF or an error: the child has
    /// closed the PTY and no more output will ever arrive. Useful as a non-blocking check before
    /// committing to a wait with its own timeout/diagnostics, which [`finish`](Self::finish)
    /// doesn't provide (it blocks on the reader thread unconditionally).
    #[must_use]
    pub fn reader_done(&self) -> Arc<Mutex<bool>> {
        Arc::clone(&self.reader_done)
    }

    /// Writes `bytes` to the child's PTY input.
    ///
    /// # Errors
    ///
    /// Returns an error if the write half was already closed via
    /// [`close_writer`](Self::close_writer), or if the underlying write fails (e.g. the child has
    /// exited and closed its end of the PTY).
    pub fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.writer.as_mut().map_or_else(
            || Err(std::io::Error::other("PTY writer already closed")),
            |writer| writer.write_all(bytes),
        )
    }

    /// Closes the write half of the PTY, so the child sees EOF the next time it reads its stdin.
    /// Idempotent. [`finish`](Self::finish) always does this first; call it directly only to
    /// close the write half earlier (e.g. before a child that reacts to EOF on its own).
    pub fn close_writer(&mut self) {
        self.writer = None;
    }

    /// Polls whether the child has exited, without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying platform wait call fails.
    pub fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
        self.child.try_wait()
    }

    /// Sends the child a kill signal. Best-effort: a child that has already exited is not an
    /// error here.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }

    /// Blocks until the child exits. Best-effort: an already-reaped child is not an error here.
    pub fn wait(&mut self) {
        let _ = self.child.wait();
    }

    /// Closes the write half (if still open) and blocks until the background reader thread has
    /// fully drained the output and returned, then hands back everything captured.
    ///
    /// # Errors
    ///
    /// Returns an error if the reader thread itself panicked.
    ///
    /// # Panics
    ///
    /// Panics if a handle from [`output`](Self::output) is still held elsewhere when this is
    /// called -- see that method's docs.
    pub fn finish(mut self) -> std::io::Result<Vec<u8>> {
        self.close_writer();
        self.reader_handle
            .take()
            .expect("PtySession::spawn always sets reader_handle")
            .join()
            .map_err(|_| std::io::Error::other("PTY reader thread panicked"))?;
        Ok(Arc::try_unwrap(self.output)
            .expect("PtySession::finish called while an output() handle is still held")
            .into_inner()
            .unwrap())
    }
}

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
    let mut session = PtySession::spawn(bin, args, &[], cols, rows)?;

    let started = Instant::now();
    let mut parser = vt100::Parser::new(rows, cols, 0);
    let mut parsed = 0usize;
    let mut previous: Option<Vec<Vec<(String, Style)>>> = None;
    let mut frames = Vec::new();

    {
        let output = session.output();
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

            if matches!(session.try_wait(), Ok(Some(_))) || started.elapsed() > timeout {
                break;
            }
        }
        // Drops `output`, this block's only handle on it, before `finish` below needs to be its
        // sole owner -- an explicit block rather than relying on the loop's own end, since a
        // local's drop runs at its enclosing block's close, not at its last use.
    }

    session.kill();
    session.wait();
    // The child (and any process still holding the slave open) has now actually gone away, so
    // `finish`'s own writer close unblocks the reader thread's `read` with EOF.
    session.finish()?;

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
