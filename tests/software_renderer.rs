//! Headless software renderer snapshot tests.
//!
//! Renders scenes via the full `Terminal` → `SoftwareRenderer` pipeline and
//! compares the pixel output against stored PNG baselines.
//!
//! # Updating baselines
//!
//! ```sh
//! RG_SNAPSHOT_UPDATE=overwrite cargo test --features software-default-font \
//!   -p rg --test software_renderer
//! ```

#![cfg(feature = "software-default-font")]

use image::ImageEncoder;
use rg::backend::software::{SoftwareBackendBuilder, SoftwareRenderer};
use rg::color::AnsiColor;
use rg::{Backend, Color, Terminal};

// VGA bitmap font: 8×16 pixels per glyph cell.
const GLYPH_W: u32 = 8;
const GLYPH_H: u32 = 16;

/// Snapshot directory (relative to crate root).
fn snapshot_dir() -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

// ── PNG helper utilities ─────────────────────────────────────────────────────

/// Encode a `u32` pixel buffer (0x00RRGGBB) as PNG bytes.
fn encode_png(pixels: &[u32], width: u32, height: u32) -> Vec<u8> {
    let mut png = Vec::new();
    {
        let encoder = image::codecs::png::PngEncoder::new(&mut png);
        let raw: Vec<u8> = pixels
            .iter()
            .flat_map(|&p| {
                [
                    ((p >> 16) & 0xFF) as u8,
                    ((p >> 8) & 0xFF) as u8,
                    (p & 0xFF) as u8,
                    255,
                ]
            })
            .collect();
        encoder
            .write_image(&raw, width, height, image::ExtendedColorType::Rgba8)
            .expect("encode PNG");
    }
    png
}

/// Convert a `u32` pixel buffer (0x00RRGGBB) to an `RgbaImage`.
fn buffer_to_image(pixels: &[u32], width: u32, height: u32) -> image::RgbaImage {
    image::RgbaImage::from_fn(width, height, |x, y| {
        let idx = (y * width + x) as usize;
        let p = pixels[idx];
        image::Rgba([
            ((p >> 16) & 0xFF) as u8,
            ((p >> 8) & 0xFF) as u8,
            (p & 0xFF) as u8,
            255,
        ])
    })
}

/// Decode PNG bytes into an `RgbaImage`.
fn load_png(data: &[u8]) -> image::RgbaImage {
    image::load_from_memory(data)
        .expect("decode PNG")
        .into_rgba8()
}

// ── Snapshot assertion helper ────────────────────────────────────────────────

/// Assert that `renderer`'s pixel buffer matches a stored PNG baseline.
///
/// The expected baseline lives at `tests/snapshots/{name}.png`.  On mismatch,
/// `{name}_actual.png` and `{name}_diff.png` are written to the same directory
/// and the test panics with the first pixel difference.
///
/// Set `RG_SNAPSHOT_UPDATE=overwrite` to overwrite the baseline with the
/// current output (use when rendering has intentionally changed).
fn assert_png_snapshot(renderer: &SoftwareRenderer, name: &str) {
    let size = renderer.size();
    let img_w = u32::from(size.width) * GLYPH_W;
    let img_h = u32::from(size.height) * GLYPH_H;
    let pixels = renderer.pixels();

    let snap_dir = snapshot_dir();
    let expected = snap_dir.join(format!("{name}.png"));
    let actual_path = snap_dir.join(format!("{name}_actual.png"));
    let diff_path = snap_dir.join(format!("{name}_diff.png"));

    let actual_png = encode_png(pixels, img_w, img_h);

    // Overwrite baseline when env var is set.
    if std::env::var("RG_SNAPSHOT_UPDATE").as_deref() == Ok("overwrite") {
        std::fs::write(&expected, &actual_png).expect("write baseline PNG");
        let _ = std::fs::remove_file(&actual_path);
        let _ = std::fs::remove_file(&diff_path);
        return;
    }

    let baseline = match std::fs::read(&expected) {
        Ok(d) => d,
        Err(_) => {
            std::fs::write(&actual_path, &actual_png).expect("write actual PNG");
            panic!(
                "no baseline at {};\n  wrote actual to {}\n  set RG_SNAPSHOT_UPDATE=overwrite to accept",
                expected.display(),
                actual_path.display(),
            );
        }
    };

    // Fast-path: byte-identical PNG.
    if actual_png == baseline {
        return;
    }

    // Pixel-level comparison.
    let expected_img = load_png(&baseline);
    let actual_img = buffer_to_image(pixels, img_w, img_h);
    assert_eq!(
        expected_img.dimensions(),
        actual_img.dimensions(),
        "image dimensions differ"
    );

    let (w, h) = expected_img.dimensions();
    let mut diff_img = actual_img.clone();
    let mut first_diff = None;

    for y in 0..h {
        for x in 0..w {
            let exp = expected_img.get_pixel(x, y);
            let act = actual_img.get_pixel(x, y);
            if exp != act {
                if first_diff.is_none() {
                    first_diff = Some((x, y, *exp, *act));
                }
                // Highlight mismatched pixels in bright red.
                diff_img.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
    }

    // Write artifacts.
    actual_img.save(&actual_path).expect("write actual PNG");
    diff_img.save(&diff_path).expect("write diff PNG");

    let (dx, dy, exp, act) =
        first_diff.expect("PNG bytes differ but no pixel diff found (compression?)");
    panic!(
        "pixel mismatch at ({dx},{dy}): expected ({},{},{}) got ({},{},{})\n\
         actual: {}\n\
         diff:   {}\n\
         set RG_SNAPSHOT_UPDATE=overwrite to accept new output",
        exp[0], exp[1], exp[2], act[0], act[1], act[2],
        actual_path.display(),
        diff_path.display(),
    );
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn render_demo_scene() {
    let renderer = SoftwareBackendBuilder::new()
        .grid_size(42, 22)
        .scale(1)
        .build()
        .expect("build software backend")
        .run_headless();

    let mut term = Terminal::new(renderer);

    // Draw room boundary (box-drawing characters).
    term.fg(Color::Ansi(AnsiColor::White));
    for x in 2..41 {
        term.put(x, 2, '─');
        term.put(x, 20, '─');
    }
    for y in 3..20 {
        term.put(2, y, '│');
        term.put(41, y, '│');
    }
    term.put(2, 2, '┌');
    term.put(41, 2, '┐');
    term.put(2, 20, '└');
    term.put(41, 20, '┘');

    // Enemy at room center (Red D).
    term.fg(Color::Ansi(AnsiColor::Red));
    // Center of 2..41, 2..20 = (21, 11).
    term.put(21, 11, 'D');

    // Player (Green @).
    term.fg(Color::Ansi(AnsiColor::Green));
    term.put(5, 5, '@');
    term.reset_style();

    term.present();
    assert_png_snapshot(term.backend(), "render_demo_scene");
}

#[test]
fn sub_cell_offset_does_not_smear() {
    // Verify that the full-frame clear prevents orphaned pixels from
    // sub-cell offset spill across adjacent cells. Goes through the full
    // `Terminal::present()` pipeline so `needs_full_frame()` triggers the
    // all-cells path, which clears the buffer before each render.
    //
    // Frame 1: layer 0 red bg + layer 1 @ at cell (1,0) with dx=+2,
    //           which spills green into cell 2.
    // Frame 2: layer 1 cleared, @ at cell (1,0) with dx=-2,
    //           which spills green into cell 0 instead.
    //
    // The snapshot captures frame 2, which should have clean red in cell 2
    // (no orphaned green pixels from the old spill).
    let mut term = Terminal::new(
        SoftwareBackendBuilder::new()
            .grid_size(3, 1)
            .scale(1)
            .build()
            .expect("build software backend")
            .run_headless(),
    );

    // ── Frame 1: layer 0 bg (red) + layer 1 @ at dx=+2 ──
    term.layer(0);
    term.bg(Color::Rgb {
        r: 128,
        g: 0,
        b: 0,
    });
    for x in 0..3 {
        term.put(x, 0, ' ');
    }

    term.layer(1);
    term.fg(Color::Rgb {
        r: 0,
        g: 255,
        b: 0,
    });
    term.put_offset(1, 0, 2, 0, '@');
    term.present();

    // ── Frame 2: clear layer 1, put @ at dx=-2 ──
    term.layer(1);
    term.clear();
    term.fg(Color::Rgb {
        r: 0,
        g: 255,
        b: 0,
    });
    term.put_offset(1, 0, -2, 0, '@');
    term.present();

    assert_png_snapshot(term.backend(), "sub_cell_offset_does_not_smear");
}
