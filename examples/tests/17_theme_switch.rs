//! Snapshot tests for the `17_theme_switch` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example
//! 17_theme_switch` runs.
//!
//! The headless text snapshots can't see color (see
//! [`Headless::format_view`]'s doc comment), so what they prove is that `t`
//! actually flips [`ThemeSwitch`]'s state -- visible here as the panel title and
//! button label text changing between "Dark"/"Light" -- not that the resulting colors differ.
//! The PNG/SVG snapshots (like every other example's) capture the default startup state, which
//! proves [`Theme::DARK`]'s palette reaches the pixel-level and terminal-I/O render paths; the
//! headless snapshots above are what proves the toggle itself works.
//!
//! [`Headless::format_view`]: retroglyph_core::Headless::format_view
//! [`Theme::DARK`]: retroglyph_widgets::Theme::DARK

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/17_theme_switch.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod theme_switch;

use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use retroglyph_core::{Frame, Headless, Terminal};
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};
use theme_switch::ThemeSwitch;

/// A plain, unmodified key press.
const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// Drives `ThemeSwitch` through `events` (one batch of zero or more events per tick), returning
/// each frame's rendered text.
fn drive(events: &[&[Event]]) -> String {
    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = ThemeSwitch::init(&mut term);

    let mut views = Vec::new();
    for (i, batch) in events.iter().enumerate() {
        for event in *batch {
            term.backend_mut().push_event(event.clone());
        }
        let frame = Frame {
            delta: HEADLESS_FRAME_DELTA,
            frame: i as u64,
        };
        if !state.tick(&mut term, &frame) {
            break;
        }
        views.push(term.backend().format_view());
    }
    views.join("\n--- frame ---\n")
}

#[test]
fn headless_snapshot_default_dark() {
    // No input: the default state (`Theme::DARK`, "Theme: Dark" title, "Switch to Light" button).
    insta::assert_snapshot!(drive(&[&[]]));
}

#[test]
fn headless_snapshot_toggle_and_navigate() {
    // Frame 1: `t` flips to `Theme::LIGHT` -- the title/button text below flips to "Theme:
    // Light"/"Switch to Dark", proving the toggle actually changed state (see the module doc
    // comment for why this text flip, not a color diff, is what a headless snapshot can prove).
    // Frames 2-3 (Right, then Down) exercise the tab-select/list-select input paths the same way
    // -- like the selected tab/item highlight itself, they render as a color-only change this
    // text-only snapshot can't see, so what these two frames actually prove is that neither input
    // path panics or otherwise disturbs the rendered layout, not that the highlight visibly moved.
    insta::assert_snapshot!(drive(&[
        &[key(KeyCode::Char('t'))],
        &[key(KeyCode::Right)],
        &[key(KeyCode::Down)],
    ]));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<ThemeSwitch>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

/// Like [`support::png_snapshot`], but pushes a `t` keypress before the one rendered tick, so the
/// committed pixels are [`Theme::LIGHT`]'s palette rather than [`ThemeSwitch::default`]'s
/// [`Theme::DARK`] -- this is the regression test for the light-theme readability bug fixed
/// alongside it (every `.theme()` mapping used to leave some widgets' text background at
/// [`Style::new()`]'s default, which paints solid black behind the glyph on a real backend
/// instead of blending with the panel's white fill -- invisible on a near-black `Theme::DARK`
/// panel, glaring on `Theme::LIGHT`'s white one). Not built from [`support::png_snapshot`]
/// directly since that helper always runs `E::init` + one plain `tick` with no way to inject
/// input first; duplicating its short body here is cheaper than widening a helper every other
/// example's test also uses just for this one example's needs.
///
/// [`Style::new()`]: retroglyph_core::Style::new
/// [`Theme::DARK`]: retroglyph_widgets::Theme::DARK
/// [`Theme::LIGHT`]: retroglyph_widgets::Theme::LIGHT
#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot_light() {
    use retroglyph_software::SoftwareBackendBuilder;
    use retroglyph_window::Presenter;

    let (cols, rows, scale) = (50u16, 25u16, 2u8);
    let renderer = SoftwareBackendBuilder::new()
        .grid_size(cols, rows)
        .scale(scale)
        .build()
        .expect("software backend init")
        .run_headless()
        .expect("headless renderer init");
    let (cell_w, cell_h) = renderer.cell_size();
    let (width, height) = (u32::from(cols) * cell_w, u32::from(rows) * cell_h);

    let mut term = Terminal::new(renderer);
    let mut state = ThemeSwitch::init(&mut term);
    term.backend_mut().push_event(key(KeyCode::Char('t')));
    let frame = Frame {
        delta: HEADLESS_FRAME_DELTA,
        frame: 0,
    };
    state.tick(&mut term, &frame);

    let mut rgb = Vec::with_capacity(term.backend().pixels().len() * 3);
    for &p in term.backend().pixels() {
        rgb.push(((p >> 16) & 0xff) as u8);
        rgb.push(((p >> 8) & 0xff) as u8);
        rgb.push((p & 0xff) as u8);
    }
    let img: image::RgbImage =
        image::ImageBuffer::from_raw(width, height, rgb).expect("pixel buffer matches dimensions");
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("PNG encode");

    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("17_theme_switch");
    let raw = support::capture_pty(&bin, b"", 25, 50, "toggles theme");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("Alpha"),
        "SVG output missing expected list content"
    );
    support::write_snapshot_file("17_theme_switch.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}

/// Same PTY capture as [`svg_snapshot`], but sends a `t` keypress as input first, so the
/// committed SVG proves `Theme::LIGHT` renders correctly through the real crossterm/ANSI path
/// too, not just the software backend's pixel buffer (see [`png_snapshot_light`]'s doc comment
/// for the bug this guards against).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot_light() {
    let bin = support::build_crossterm_example("17_theme_switch");
    let raw = support::capture_pty(&bin, b"t", 25, 50, "toggles theme");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("Alpha"),
        "SVG output missing expected list content"
    );
    support::write_snapshot_file("17_theme_switch_light.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
