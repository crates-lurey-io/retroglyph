//! 02: Styles & color -- every `Color` variant, via `put_styled`
//!
//! Shows off every [`Color`] variant via [`Style`]. Builds on 01's `App`/`rg_gallery_run!` shape
//! -- the only new concept here is `put_styled` (an explicit [`Style`] per cell) alongside 01's
//! stateful `print`.
//!
//! ```sh
//! cargo run --example 02_styles_and_color                          # Headless (structure only, no color)
//! cargo run --example 02_styles_and_color --features crossterm     # Terminal
//! cargo run --example 02_styles_and_color --features default-font  # Desktop window
//! cargo run --example 02_styles_and_color --features default-font --target wasm32-unknown-unknown  # WASM
//! ```
//!
//! Headless prints plain glyphs with no color at all.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use gem::space::{Hsv, Srgb};
use retroglyph_core::{App, Backend, Color, Flow, Frame, Style, Terminal};
use retroglyph_gallery::{any_key_pressed_or_window_closed, rg_gallery_run};

struct StylesAndColor;

impl<B: Backend> App<B> for StylesAndColor {
    fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
        term.reset_style();
        term.print(0, 0, "02: Styles & Color");

        // `Color::Ansi` -- respects the user's terminal theme, so its real
        // RGB is whatever they've configured. `ansi_approx_rgb` below is
        // only a guess at typical defaults, used to pick legible label text.
        term.print(0, 2, "ANSI (16):");
        for i in 0..16u8 {
            let ansi = retroglyph_core::AnsiColor::try_from(i).expect("0..16 are valid indices");
            let (r, g, b) = ansi_approx_rgb(ansi);
            let style = Style::new().bg(Color::Ansi(ansi)).fg(contrast_fg(r, g, b));
            let label = format!("{i:02}");
            for col in 0..3u16 {
                let ch = label.chars().nth(col as usize).unwrap_or(' ');
                term.put_styled(col + u16::from(i) * 3, 3, ch, style);
            }
        }

        // `Color::Indexed` -- a fixed 256-entry palette, exact regardless of
        // theme. Drawn as colored blocks (`fg`, not `bg`) so no contrast
        // logic is needed -- indices 0-15 mirror the ANSI colors above,
        // 16-231 are a 6x6x6 color cube, 232-255 a grayscale ramp.
        term.print(0, 5, "Indexed (256):");
        for i in 0u16..256 {
            let style = Style::new().fg(Color::Indexed(i as u8));
            term.put_styled(i % 32, 6 + i / 32, '\u{2588}', style);
        }

        // `Color::Rgb` -- exact 24-bit color, any of 16 million values.
        term.print(0, 15, "RGB (truecolor):");
        for x in 0..48u16 {
            let (r, g, b) = hue_to_rgb(f32::from(x) / 48.0);
            let style = Style::new().bg(Color::Rgb { r, g, b });
            term.put_styled(x, 16, ' ', style);
        }

        term.present().expect("present failed");

        if any_key_pressed_or_window_closed(term) {
            Flow::Exit
        } else {
            Flow::Continue
        }
    }
}

/// Approximate default RGB for each `AnsiColor`.
const fn ansi_approx_rgb(color: retroglyph_core::AnsiColor) -> (u8, u8, u8) {
    use retroglyph_core::AnsiColor::{
        Black, Blue, BrightBlack, BrightBlue, BrightCyan, BrightGreen, BrightMagenta, BrightRed,
        BrightWhite, BrightYellow, Cyan, Green, Magenta, Red, White, Yellow,
    };
    match color {
        Black => (0, 0, 0),
        Red => (170, 0, 0),
        Green => (0, 170, 0),
        Yellow => (170, 85, 0),
        Blue => (0, 0, 170),
        Magenta => (170, 0, 170),
        Cyan => (0, 170, 170),
        White => (170, 170, 170),
        BrightBlack => (85, 85, 85),
        BrightRed => (255, 85, 85),
        BrightGreen => (85, 255, 85),
        BrightYellow => (255, 255, 85),
        BrightBlue => (85, 85, 255),
        BrightMagenta => (255, 85, 255),
        BrightCyan => (85, 255, 255),
        BrightWhite => (255, 255, 255),
    }
}

/// Black text on light backgrounds, white text on dark ones.
fn contrast_fg(r: u8, g: u8, b: u8) -> Color {
    let srgb = Srgb::new(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    );
    if srgb.is_dark() {
        Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        }
    } else {
        Color::Rgb { r: 0, g: 0, b: 0 }
    }
}

/// Srgb, for the truecolor gradient.
fn hue_to_rgb(h: f32) -> (u8, u8, u8) {
    let srgb = Srgb::from(Hsv::new(h, 1.0, 1.0)).clamp();
    (
        (srgb.r * 255.0).round() as u8,
        (srgb.g * 255.0).round() as u8,
        (srgb.b * 255.0).round() as u8,
    )
}

rg_gallery_run!(StylesAndColor, "02: Styles & Color", 60, 18);
