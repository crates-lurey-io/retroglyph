//! Shared snapshot-test harness for retroglyph's examples.
//!
//! Every example gets three snapshot artifacts, all driven from its own
//! `Example` impl (no separate "test version" of the example to drift out of
//! sync):
//!
//! - [`headless_snapshot`]: plain text, via [`Headless::format_view`] --
//!   cheap, no subprocess, runs on every target including `wasm32`.
//! - [`png_snapshot`]: a pixel-level render via `SoftwareRenderer`'s headless
//!   mode, encoded as PNG bytes -- pass straight to
//!   `insta::assert_binary_snapshot!(".png", ...)`, which compares the
//!   bytes byte-for-byte against the committed `.snap.png` (this is exact,
//!   not a perceptual/fuzzy image diff, so it can only ever false-positive
//!   on a genuinely non-deterministic render, not on incidental PNG
//!   re-encoding -- we always encode with the same `image` crate call).
//! - [`svg_snapshot`]: spawns the compiled `--features crossterm` example
//!   binary in a real PTY, drives it with real ANSI input, and renders the
//!   resulting `vt100` screen to SVG -- the only one of the three that
//!   exercises the actual terminal I/O path end to end. Both `insta::assert_
//!   snapshot!`-diffed (via [`svg_snapshot`]'s returned string) *and* written
//!   to a plain `.svg` file via [`write_snapshot_file`] for visual review
//!   (opening the raw `.snap` text file directly wouldn't render, since
//!   insta prepends a YAML header that isn't valid SVG).
//!
//! [`Headless::format_view`]: retroglyph_core::backend::Headless::format_view

#![allow(dead_code)] // not every test file uses every helper

use retroglyph_core::app::{App, Flow, Frame};
use retroglyph_core::backend::Backend;
use retroglyph_core::terminal::Terminal;
use retroglyph_examples::Example;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Adapts an [`Example`] into an [`App`](retroglyph_core::app::App), so
/// [`TestHarness`](retroglyph_core::testing::TestHarness) can drive it (retroglyph#1001):
/// `Example::tick`'s `bool` becomes [`Flow::Continue`]/[`Flow::Exit`], the only two states an
/// example ever returns (an example has no equivalent of `App`'s [`Flow::Idle`]).
///
/// Wraps an already-[`init`](Example::init)ed `E` rather than building one itself, so a caller
/// keeps its own handle to the state for post-hoc assertions (hitboxes, the example's own fields)
/// the same way the old hand-rolled `drive` loops did.
pub struct TestApp<E>(pub E);

impl<B: Backend, E: Example> App<B> for TestApp<E> {
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
        if self.0.tick(term, frame) {
            Flow::Continue
        } else {
            Flow::Exit
        }
    }
}

/// Runs `E`'s headless fallback for up to `frames` frames and returns each
/// frame's rendered text, joined by a separator line -- pass straight to
/// `insta::assert_snapshot!`.
#[must_use]
pub fn headless_snapshot<E: Example>(frames: u32) -> String {
    retroglyph_examples::render_headless_frames::<E>(frames).join("\n--- frame ---\n")
}

/// Builds a `SoftwareRenderer` (`cols`x`rows` grid, embedded default font, `scale`, plus
/// whatever `E::configure` adds -- a tileset, most likely), runs `E::init` + one
/// `E::tick`, and PNG-encodes the resulting pixel buffer.
///
/// Threading every example's own [`Example::configure`] through here (rather than
/// building a bare, uncustomized renderer) keeps this snapshot honest: it's built from exactly
/// the same builder `cargo run --example <name> --features software` would use, not a simplified
/// stand-in that could drift from it (e.g. missing a registered tileset, so the PNG never
/// actually exercises sprite rendering at all).
///
/// Requires the `software` feature on `retroglyph-examples` (which pulls in
/// `retroglyph-software/default-font` and `retroglyph-software/tilesets` -- see the Cargo.toml
/// comment).
///
/// # Panics
///
/// Panics if the software backend or PNG encoding fails.
#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[must_use]
pub fn png_snapshot<E: Example>(cols: u16, rows: u16, scale: u16) -> Vec<u8> {
    use retroglyph_core::terminal::Terminal;
    use retroglyph_software::SoftwareBackendBuilder;
    use retroglyph_window::Presenter;

    let builder = E::configure(
        SoftwareBackendBuilder::new()
            .grid_size(cols, rows)
            .scale(scale),
    );
    let renderer = builder
        .build()
        .expect("software backend init")
        .into_renderer()
        .expect("headless renderer init");

    // Read the pixel-buffer geometry before handing `renderer` to `Terminal`
    // (which owns it from here): cols/rows in cells x the presenter's own
    // reported cell size in pixels.
    let (cell_w, cell_h) = renderer.cell_size();
    let width = u32::from(cols) * cell_w;
    let height = u32::from(rows) * cell_h;

    let mut term = Terminal::new(renderer);
    let mut state = E::init(&mut term);
    let frame = Frame {
        delta: retroglyph_examples::HEADLESS_FRAME_DELTA,
        frame: 0,
    };
    state.tick(&mut term, &frame);
    term.present().ok();

    let mut rgb = Vec::with_capacity(term.backend().pixels().len() * 3);
    for &p in term.backend().pixels() {
        rgb.push(((p >> 16) & 0xff) as u8);
        rgb.push(((p >> 8) & 0xff) as u8);
        rgb.push((p & 0xff) as u8);
    }

    encode_png(width, height, &rgb)
}

/// PNG-encodes `width x height` interleaved RGB bytes (as produced by [`png_snapshot`] and
/// `retroglyph_examples::render_perf_overlay_rgb`).
///
/// # Panics
///
/// Panics if `rgb`'s length doesn't match `width * height * 3`, or if PNG encoding fails.
#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[must_use]
pub fn encode_png(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let img: image::RgbImage = image::ImageBuffer::from_raw(width, height, rgb.to_vec())
        .expect("pixel buffer matches dimensions");
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("PNG encode");
    out
}

/// A build directory dedicated to [`build_crossterm_example`]'s output, isolated from the
/// workspace's normal `target/` dir.
///
/// `cargo test --workspace --all-features` (see this crate's `AGENTS.md`)
/// compiles every `[[example]]` -- including with the `software` feature, which
/// [`build_crossterm_example`]'s own doc comment already explains is unusable in a PTY -- into
/// the ordinary `target/debug/examples/` directory *before* any `#[test]` runs. Building the
/// crossterm-only variant back into that same path (as this used to do) means every single
/// `svg_snapshot` test forces a real relink there, fighting the `--all-features` build for the
/// same output file on every `cargo test` invocation, warm cache or not.
///
/// That relink churn isn't just wasted compile time: on macOS, first executing a binary the
/// kernel hasn't seen this exact content at (a fresh relink counts) triggers a synchronous
/// code-signature validation that measured ~1.5-2 real seconds per example here -- almost all of
/// `svg_snapshot`'s wall time for the smaller examples. Routing this crate's crossterm builds to
/// their own `--target-dir` sidesteps both costs at once: they're never touched by the
/// `--all-features` build, so once each example has been built here (and executed once, paying
/// that validation cost a single time ever), every later `cargo test` run -- this one included --
/// finds a byte-identical, already-validated binary and skips straight to the real capture.
#[cfg(not(target_arch = "wasm32"))]
fn crossterm_target_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join("pty-examples")
}

/// Locates `example_name`'s `--features crossterm` binary in [`crossterm_target_dir`] and returns
/// its path.
///
/// This deliberately does *not* build the binary. It used to run `cargo build --examples
/// --features crossterm --target-dir crossterm_target_dir()` itself, once per `svg_snapshot`
/// call -- but every `svg_snapshot` test is its own separate `[[test]]` binary, one per example,
/// and nextest runs all of them as concurrent processes, so every one of those in-test builds
/// raced every other one over the same shared target dir. Cargo's own locking keeps that from
/// corrupting a build, but it does not stop one process from spawning a binary that a concurrent
/// build in another process is mid-relink over, which is a plausible cause of retroglyph#976 (a
/// child that exited in well under a second with zero bytes captured, immediately after a
/// `Blocking waiting for file lock on package cache` line proving another cargo invocation was
/// live at the same moment).
///
/// The `just build-pty-examples` recipe is now the sole builder, and every recipe that runs these
/// tests (`test`, `test-ci`, `test-v`, `check`, `insta`) already depends on it, running once,
/// before nextest starts any test process. That removes the race instead of narrowing it. Running
/// a `[[test]]` binary directly (e.g. `cargo nextest run -p retroglyph-examples --test <name>`)
/// without having run `just build-pty-examples` first will hit the panic below instead of
/// silently self-building.
///
/// # Panics
///
/// Panics if `example_name`'s binary doesn't exist, naming the missing path and the command that
/// builds it.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn build_crossterm_example(example_name: &str) -> PathBuf {
    let bin = crossterm_target_dir()
        .join("debug")
        .join("examples")
        .join(example_name);
    assert!(
        bin.exists(),
        "{} does not exist; run `just build-pty-examples` before running PTY example tests\n\
         (or run `just test`/`just check`/etc., which already depend on it)",
        bin.display()
    );
    bin
}

/// How often the PTY capture loops re-check the child's output. Short enough that the settle wait
/// in [`capture_pty_until`] costs a test a few milliseconds rather than a visible pause, long
/// enough that polling a live FPS overlay's byte stream isn't itself a busy loop.
#[cfg(not(target_arch = "wasm32"))]
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(5);

/// How long the ready-marker wait tolerates the child producing *no new output at all* before
/// declaring it hung.
///
/// This is a liveness check, not a wall-clock budget, and the difference is the whole point. An
/// example whose marker only appears once an animation has finished measures that animation in
/// real elapsed time, and [`FrameClock::advance`](retroglyph_core::frames::FrameClock::advance) caps
/// catch-up at five steps -- so every stretch the child spends descheduled under a loaded runner
/// is animation time it never gets back, and a fixed total budget turns that into an
/// intermittent, load-dependent failure (retroglyph#544). Measuring "has it stopped talking to
/// us" instead still fails fast on a genuinely stuck example while tolerating a merely starved
/// one.
#[cfg(not(target_arch = "wasm32"))]
const READY_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Absolute backstop on the ready-marker wait, for a child that keeps emitting bytes but never
/// reaches its marker (so [`READY_IDLE_TIMEOUT`] never fires).
///
/// Comfortably under nextest's own kill -- `.config/nextest.toml` sets `slow-timeout = { period =
/// "20s", terminate-after = 2 }`, i.e. 40s -- so a test that trips this reports *which* marker it
/// was waiting for instead of being killed from the outside with no such detail.
#[cfg(not(target_arch = "wasm32"))]
const READY_HARD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Spawns `bin` in a PTY sized `rows`x`cols`, waits until the screen contains `ready_marker`,
/// writes `input`, then quits the example and returns all captured output bytes.
///
/// Runs the child with `RG_FPS=0`: the shared driver draws its FPS overlay by default (see
/// `retroglyph_examples::fps`), and a live frame rate is not reproducible, so leaving it on would
/// diff every committed SVG on every run. Use [`capture_pty_with_env`] directly to capture a run
/// with the overlay left alone.
///
/// # Panics
///
/// Panics if the PTY can't be opened, or if `ready_marker` never appears
/// within 10 seconds.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn capture_pty(bin: &Path, input: &[u8], rows: u16, cols: u16, ready_marker: &str) -> Vec<u8> {
    capture_pty_with_env(bin, input, rows, cols, ready_marker, &[("RG_FPS", "0")])
}

/// How much faster than real time [`capture_pty_animated`] runs its example.
///
/// Large enough that the wall-clock cost of the slowest animation in the gallery (`06_layers`, at
/// 4.7s) stops being a meaningful share of the capture, small enough to stay far away from the
/// per-step granularity where a scaled delta could skip visible states.
#[cfg(not(target_arch = "wasm32"))]
const ANIMATION_TIME_SCALE: &str = "20";

/// [`capture_pty`] for an example whose `ready_marker` only appears once an animation has
/// finished, fast-forwarding that animation via `RG_TIME_SCALE`.
///
/// Most examples publish a static marker (a title, a key legend) that is on screen from the first
/// frame. `06_layers` and `08_animation` deliberately do not: both animate to a parked end state
/// and announce *that*, because an animation that loops forever never settles into a single frame
/// a snapshot can pin. That makes their markers a wall-clock wait, and
/// [`FrameClock`](retroglyph_core::frames::FrameClock)'s catch-up cap means the wait cannot be made up
/// after a stall -- see [`READY_IDLE_TIMEOUT`] and retroglyph#544.
///
/// Scaling time keeps the marker's meaning intact ("the animation has settled", still reached by
/// running the example's real logic on the real backend) while removing the fixed floor under how
/// long that takes. The captured frame is the parked one either way, so the committed SVG is
/// unchanged.
///
/// # Panics
///
/// Panics if the PTY can't be opened, or if `ready_marker` never appears.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn capture_pty_animated(
    bin: &Path,
    input: &[u8],
    rows: u16,
    cols: u16,
    ready_marker: &str,
) -> Vec<u8> {
    capture_pty_with_env(
        bin,
        input,
        rows,
        cols,
        ready_marker,
        &[("RG_FPS", "0"), ("RG_TIME_SCALE", ANIMATION_TIME_SCALE)],
    )
}

/// [`capture_pty`] with an explicit child environment instead of its default `RG_FPS=0`.
///
/// Only for captures that send no `input`, or whose `input` has no visible effect: anything that
/// types a key the example reacts to has to say how to tell that key landed, which is what
/// [`capture_pty_until`] is for. See that function for why.
///
/// # Panics
///
/// Panics if the PTY can't be opened, or if `ready_marker` never appears
/// within 10 seconds.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn capture_pty_with_env(
    bin: &Path,
    input: &[u8],
    rows: u16,
    cols: u16,
    ready_marker: &str,
    env: &[(&str, &str)],
) -> Vec<u8> {
    capture_pty_until(bin, input, rows, cols, ready_marker, env, &|_| true)
}

/// Blocks until `ready_marker` appears in `output`, which the caller's reader thread is filling.
///
/// `bin` and `child` exist only to enrich the empty-capture panic below: when the child closes the
/// PTY having printed nothing at all, the bare message used to say only that, with no way to tell
/// "the example panicked on startup" apart from "the harness spawned a half-written binary"
/// (retroglyph#976). Naming the binary's path, size, and mtime alongside the child's exit status
/// turns that guess into an answer.
///
/// # Panics
///
/// Panics if the child closes the PTY without printing `ready_marker`, if it produces no new
/// output at all for [`READY_IDLE_TIMEOUT`], or if it never reaches the marker within
/// [`READY_HARD_TIMEOUT`].
#[cfg(not(target_arch = "wasm32"))]
fn wait_for_marker(
    bin: &Path,
    child: &mut dyn portable_pty::Child,
    output: &std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    reader_done: &std::sync::Arc<std::sync::Mutex<bool>>,
    ready_marker: &str,
) {
    use std::time::Instant;

    let hard_deadline = Instant::now() + READY_HARD_TIMEOUT;
    let mut idle_deadline = Instant::now() + READY_IDLE_TIMEOUT;
    // Scans only the bytes that arrived since the last poll, keeping `ready_marker.len() - 1`
    // bytes of overlap so a marker split across two reads still matches. Rescanning the whole
    // capture every 5ms (as this used to) is not just quadratic in a capture that reaches
    // hundreds of kilobytes -- it does that work while holding the very lock the reader thread
    // needs in order to append, throttling the drain that keeps the child off a full PTY buffer.
    let mut scanned = 0usize;
    let overlap = ready_marker.len().saturating_sub(1);
    loop {
        let (found, len) = {
            let raw = output.lock().unwrap();
            let from = scanned.saturating_sub(overlap);
            (
                String::from_utf8_lossy(&raw[from..]).contains(ready_marker),
                raw.len(),
            )
        };
        if found {
            return;
        }
        if len > scanned {
            scanned = len;
            idle_deadline = Instant::now() + READY_IDLE_TIMEOUT;
        }
        // A child that has closed the PTY is never going to print the marker, so there's no point
        // waiting out either timeout: report it now, and quote what it did manage to print, plus
        // enough about the binary and the child's own exit to tell "it panicked on startup" apart
        // from "the harness spawned a stale/half-written binary" (retroglyph#976) without guessing.
        if *reader_done.lock().unwrap() {
            let metadata = std::fs::metadata(bin);
            let bin_desc = match &metadata {
                Ok(meta) => format!("{} bytes, mtime {:?}", meta.len(), meta.modified().ok()),
                Err(err) => format!("<stat failed: {err}>"),
            };
            let exit_desc = match child.try_wait() {
                Ok(Some(status)) => format!("{status:?}"),
                Ok(None) => "<still running>".to_owned(),
                Err(err) => format!("<wait failed: {err}>"),
            };
            panic!(
                "child exited before printing {ready_marker:?}; bin: {} ({bin_desc}); exit \
                 status: {exit_desc}; captured:\n{}",
                bin.display(),
                String::from_utf8_lossy(&output.lock().unwrap())
            );
        }
        let now = Instant::now();
        assert!(
            now <= idle_deadline,
            "no PTY output for {READY_IDLE_TIMEOUT:?} while waiting for {ready_marker:?}"
        );
        assert!(
            now <= hard_deadline,
            "timed out after {READY_HARD_TIMEOUT:?} waiting for {ready_marker:?} in PTY output"
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// [`capture_pty_with_env`], but waits for `settled` to accept the rendered screen after writing
/// `input` and before sending the quit key.
///
/// **Every capture that sends input the example reacts to needs this.** The quit key is just more
/// bytes in the same PTY input stream, so if the child happens to be descheduled between its first
/// frame and the quit write -- routine on a loaded CI runner -- it reads `input` and `q` in the
/// same frame's [`Terminal::drain_events`]. The shared driver deliberately does not present on the
/// frame an example quits on (see `ExampleApp::update`: the example returns from its event handler
/// without drawing, so the working grid is empty and presenting it would erase the last frame), so
/// whatever `input` changed is applied to state and then never rendered. The capture ends up
/// showing the pre-input screen, and the test fails claiming the key did nothing.
///
/// `settled` receives the screen as [`vt100::Screen::contents`] text and returns whether the key
/// has visibly landed. It is a synchronization point, not the assertion: it buys the child a frame
/// to render the change, and the test still asserts on the returned capture as usual.
///
/// # Panics
///
/// Panics if the PTY can't be opened, if `ready_marker` never appears within 10 seconds, or if
/// `settled` never accepts within 10 seconds of `input` being written.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn capture_pty_until(
    bin: &Path,
    input: &[u8],
    rows: u16,
    cols: u16,
    ready_marker: &str,
    env: &[(&str, &str)],
    settled: &dyn Fn(&str) -> bool,
) -> Vec<u8> {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");

    let mut cmd = CommandBuilder::new(bin);
    cmd.env("TERM", "xterm-256color");
    for (key, value) in env {
        cmd.env(key, value);
    }
    let mut child = pair.slave.spawn_command(cmd).expect("spawn");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("reader");
    let mut writer = pair.master.take_writer().expect("writer");

    let output = Arc::new(Mutex::new(Vec::new()));
    let output_clone = Arc::clone(&output);
    let reader_done = Arc::new(Mutex::new(false));
    let reader_done_clone = Arc::clone(&reader_done);
    // Keep the `JoinHandle` (rather than only the `reader_done` flag below): setting that flag
    // and this closure actually *returning* (dropping `output_clone`, its `Arc` clone) are two
    // separate steps, so polling the flag alone races the still-unwinding thread -- the poll can
    // observe `true` and reach `Arc::try_unwrap` below a moment before `output_clone` is actually
    // dropped, which fails non-deterministically (this is what used to intermittently panic
    // here). `.join()` blocks until the thread function has fully returned, which is the only way
    // to be sure the second `Arc` reference is gone.
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

    // Nothing is typed until the example has drawn a frame. Before that it hasn't enabled raw
    // mode yet, so the line discipline would hold the bytes in its canonical-mode line buffer and
    // echo them straight back into the capture.
    wait_for_marker(bin, child.as_mut(), &output, &reader_done, ready_marker);

    if !input.is_empty() {
        writer.write_all(input).expect("write input");

        // Feeds only the bytes that arrived since the last poll, so a capture that grows to
        // megabytes (the crossterm driver is an unthrottled spin loop, and a visible FPS overlay
        // redraws its readout every frame) stays linear instead of re-parsing from scratch.
        let mut screen = vt100::Parser::new(rows, cols, 0);
        let mut parsed = 0usize;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            {
                let raw = output.lock().unwrap();
                screen.process(&raw[parsed..]);
                parsed = raw.len();
            }
            if settled(&screen.screen().contents()) {
                break;
            }
            assert!(
                Instant::now() <= deadline,
                "timed out waiting for {input:?} to land on screen; screen was:\n{}",
                screen.screen().contents()
            );
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    // Only pokes the child if it hasn't already quit on its own: an example whose own quit key
    // differs from `q` (sent as part of `input` above, e.g. Escape) can already have exited by
    // this point, and writing to a PTY whose slave side is fully closed fails with `EIO` on
    // macOS -- `try_wait` (already used the same way in `wait_for_marker` above) tells the two
    // cases apart without adding a real wait of its own; a `try_wait` error is treated the same
    // as "still running" so every existing caller (whose child is always alive here) keeps
    // sending `q` exactly as before.
    if !matches!(child.try_wait(), Ok(Some(_))) {
        writer.write_all(b"q").expect("write quit");
    }
    drop(writer);
    let _ = child.wait();

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if *reader_done.lock().unwrap() {
            break;
        }
        assert!(Instant::now() <= deadline);
        std::thread::sleep(POLL_INTERVAL);
    }
    reader_handle.join().expect("reader thread panicked");

    Arc::try_unwrap(output).unwrap().into_inner().unwrap()
}

/// Feeds `raw` PTY bytes through a fresh `vt100::Parser` and renders the
/// final alternate-screen contents as SVG.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn svg_snapshot(raw: &[u8], rows: u16, cols: u16) -> String {
    let mut last_alternate_end = 0usize;
    let mut pos = 0usize;
    let mut parser = vt100::Parser::new(rows, cols, 0);
    while pos < raw.len() {
        let end = (pos + 64).min(raw.len());
        parser.process(&raw[pos..end]);
        if parser.screen().alternate_screen() {
            last_alternate_end = end;
        }
        pos = end;
    }

    let mut snap_parser = vt100::Parser::new(rows, cols, 0);
    snap_parser.process(&raw[..last_alternate_end]);
    render_svg(snap_parser.screen(), rows, cols)
}

#[cfg(not(target_arch = "wasm32"))]
const CHAR_PX: f64 = 8.4;
#[cfg(not(target_arch = "wasm32"))]
const LINE_PX: f64 = 18.0;
#[cfg(not(target_arch = "wasm32"))]
const PAD: f64 = 8.0;

#[cfg(not(target_arch = "wasm32"))]
fn css_color(color: vt100::Color, default: &str) -> String {
    match color {
        vt100::Color::Default => default.to_owned(),
        vt100::Color::Idx(idx) => ansi_palette(idx).to_owned(),
        vt100::Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
    }
}

#[cfg(not(target_arch = "wasm32"))]
const fn ansi_palette(idx: u8) -> &'static str {
    match idx {
        0 => "#000000",
        1 => "#800000",
        2 => "#008000",
        3 => "#808000",
        4 => "#000080",
        5 => "#800080",
        6 => "#008080",
        7 => "#c0c0c0",
        8 => "#808080",
        9 => "#ff5555",
        10 => "#55ff55",
        11 => "#ffff55",
        12 => "#5555ff",
        13 => "#ff55ff",
        14 => "#55ffff",
        15 => "#ffffff",
        _ => "#888888",
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(not(target_arch = "wasm32"))]
struct Run {
    col: u16,
    text: String,
    fg: String,
    bg: String,
    bold: bool,
    italic: bool,
}

#[cfg(not(target_arch = "wasm32"))]
fn collect_runs(screen: &vt100::Screen, row: u16, cols: u16) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();

    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            continue;
        };
        let fg = css_color(cell.fgcolor(), "#cccccc");
        let bg = css_color(cell.bgcolor(), "none");
        let bold = cell.bold();
        let italic = cell.italic();
        let contents = cell.contents();
        let glyph = if contents.is_empty() {
            " ".to_owned()
        } else {
            contents.to_owned()
        };

        let extends_current = runs.last().is_some_and(|prev: &Run| {
            prev.fg == fg && prev.bg == bg && prev.bold == bold && prev.italic == italic
        });

        if extends_current {
            if let Some(last) = runs.last_mut() {
                last.text.push_str(&glyph);
            }
        } else {
            runs.push(Run {
                col,
                text: glyph,
                fg,
                bg,
                bold,
                italic,
            });
        }
    }

    runs
}

#[cfg(not(target_arch = "wasm32"))]
fn tspan_attrs(run: &Run) -> String {
    let x = f64::from(run.col).mul_add(CHAR_PX, PAD);
    let mut attrs = format!("fill=\"{}\" x=\"{x:.1}\"", run.fg);
    if run.bold {
        attrs.push_str(" font-weight=\"bold\"");
    }
    if run.italic {
        attrs.push_str(" font-style=\"italic\"");
    }
    if run.bg != "none" {
        write!(attrs, " style=\"background:{}\"", run.bg).expect("String write");
    }
    attrs
}

#[cfg(not(target_arch = "wasm32"))]
fn render_svg(screen: &vt100::Screen, rows: u16, cols: u16) -> String {
    let w = PAD.mul_add(2.0, f64::from(cols) * CHAR_PX);
    let h = PAD.mul_add(2.0, f64::from(rows) * LINE_PX);

    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\">\n\
         <rect width=\"100%\" height=\"100%\" fill=\"#111111\"/>\n"
    );

    for row in 0..rows {
        let y = (f64::from(row) + 1.0).mul_add(LINE_PX, PAD);
        // xml:space="preserve": without it, SVG collapses runs of
        // whitespace, so a run of leading spaces used to indent text to its
        // real column (see `collect_runs`/`tspan_attrs`, which position runs
        // by literal spaces rather than per-character x offsets) silently
        // collapses to nothing and the text renders left-aligned instead of
        // at its actual terminal column.
        writeln!(
            out,
            "<text font-family=\"monospace\" font-size=\"14\" y=\"{y:.1}\" xml:space=\"preserve\">"
        )
        .expect("String write");
        for run in collect_runs(screen, row, cols) {
            writeln!(
                out,
                "  <tspan {}>{}</tspan>",
                tspan_attrs(&run),
                xml_escape(&run.text)
            )
            .expect("String write");
        }
        out.push_str("</text>\n");
    }

    out.push_str("</svg>\n");
    out
}

/// Writes `bytes` to `tests/snapshots/<file_name>`, creating the directory
/// if needed. Used for the PNG/SVG artifacts: reviewed visually (`git diff`
/// on a binary/SVG file, or opening the PNG), not text-diffed by insta.
///
/// Only for output that some `insta` assertion in the same test already pinned byte-for-byte.
/// This overwrites a *tracked* file on every run, so writing anything clock- or
/// machine-dependent here leaves the working copy permanently dirty and puts noise in every
/// unrelated commit made after a test run. Use [`write_scratch_file`] for that instead.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_snapshot_file(file_name: &str, bytes: &[u8]) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    std::fs::create_dir_all(&dir).expect("create snapshots dir");
    std::fs::write(dir.join(file_name), bytes).expect("write snapshot file");
}

/// Writes `bytes` to cargo's per-test scratch directory and returns the path, for a visual-review
/// artifact that can't be pinned byte-for-byte.
///
/// [`CARGO_TARGET_TMPDIR`] lives under `target/`, so unlike [`write_snapshot_file`] this leaves
/// no tracked file to go stale or dirty. Print the returned path from the test so a reviewer can
/// still open the artifact after a run.
///
/// [`CARGO_TARGET_TMPDIR`]: https://doc.rust-lang.org/cargo/reference/environment-variables.html
#[cfg(not(target_arch = "wasm32"))]
pub fn write_scratch_file(file_name: &str, bytes: &[u8]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("snapshots");
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    let path = dir.join(file_name);
    std::fs::write(&path, bytes).expect("write scratch file");
    path
}
