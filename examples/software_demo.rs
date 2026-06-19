//! Minimal software-backend demo using the embedded VGA 8×16 font.
//!
//! Renders a greeting in the centre of the window and exits on Esc or window
//! close.
//!
//! Run with:
//!   `cargo run --example software_demo --features software-default-font`

use rg::backend::software::SoftwareBackendBuilder;
use rg::event::{Event, KeyCode};
use rg::style::Style;
use rg::{Color, Terminal};
use std::time::Duration;

fn main() {
    let backend = SoftwareBackendBuilder::new()
        .title("rg software demo")
        .grid_size(80, 25)
        .scale(2)
        .build()
        .expect("backend init failed (try the `software-default-font` feature)");

    backend
        .run(move |term: &mut Terminal<_>| {
            draw(term);

            // Poll once; `run` loops calling us on every tick.
            // Resize events are auto-applied by the terminal.
            if let Some(event) = term.poll(Duration::from_millis(16)) {
                match event {
                    Event::Key(k) if k.code == KeyCode::Escape => std::process::exit(0),
                    Event::Close => std::process::exit(0),
                    _ => {}
                }
            }
        })
        .expect("event loop failed");
}

fn draw(term: &mut Terminal<impl rg::Backend>) {
    term.clear();
    let size = term.size();
    let msg = "Hello from rg! Press Esc to quit.";

    #[allow(clippy::cast_possible_truncation)]
    let x = (size.width.saturating_sub(msg.len() as u16)) / 2;
    let y = size.height / 2;

    term.bg(Color::BLACK);
    term.fg(Color::BRIGHT_WHITE);
    term.print(x, y, msg);

    let label = "rg";

    #[allow(clippy::cast_possible_truncation)]
    let lx = (size.width.saturating_sub(label.len() as u16)) / 2;
    let ly = y.saturating_sub(2);
    let style = Style::new().fg(Color::BRIGHT_GREEN).bold();
    for (i, ch) in label.chars().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        term.put_styled(lx + i as u16, ly, ch, style);
    }

    term.present();
}
