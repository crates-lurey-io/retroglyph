//! 03: Keyboard
//!
//! [`Event::Key`]/[`KeyCode`] decode-and-echo: shows the most recently pressed key
//! (including arrows, modifiers, and function keys) and keeps a scrolling log of the
//! last several keys seen. A drain-events loop is the standard shape for consuming
//! input every tick -- see [`Terminal::drain_events`].
//!
//! ```sh
//! cargo run --example 03_keyboard --features crossterm
//! cargo run --example 03_keyboard --features software
//! cargo run --example 03_keyboard  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Press any key to see it echoed; `q` or `Escape` quits immediately (the quitting key
//! itself is decoded but never drawn, since the frame loop exits before the next
//! present), or close the window.

use retroglyph_core::event::{Event, KeyCode, KeyModifiers};
use retroglyph_core::{Backend, Terminal};
use retroglyph_examples::Example;

/// How many past key events to keep on screen, oldest at the top.
const LOG_LEN: usize = 18;

/// State for the keyboard example: the log of decoded key events seen so far.
#[derive(Default)]
pub struct Keyboard {
    log: Vec<String>,
}

/// Formats a `KeyCode` the way a user would name the key, matching what `03_keyboard`'s
/// snapshot pins: `Char('a')` prints as `a`, everything else uses its variant name.
fn code_name(code: KeyCode) -> String {
    match code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("F{n}"),
        other => format!("{other:?}"),
    }
}

/// Formats held modifiers as a `+`-joined prefix (`"Ctrl+Alt+"`), or an empty string
/// when none are held.
fn modifiers_prefix(modifiers: KeyModifiers) -> String {
    let mut parts = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl");
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt");
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift");
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("{}+", parts.join("+"))
    }
}

impl Keyboard {
    /// Drains pending input: logs every key event, returns `false` if the user asked
    /// to quit (`q`/`Escape`, or the window's close button).
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        let mut quit = false;
        for event in term.drain_events() {
            match event {
                Event::Key(key) => {
                    let entry =
                        format!("{}{}", modifiers_prefix(key.modifiers), code_name(key.code));
                    self.log.push(entry);
                    if self.log.len() > LOG_LEN {
                        self.log.remove(0);
                    }
                    if matches!(key.code, KeyCode::Char('q') | KeyCode::Escape) {
                        quit = true;
                    }
                }
                Event::Close => quit = true,
                _ => {}
            }
        }
        !quit
    }

    /// Draws this frame and presents it.
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        term.print(
            1,
            1,
            "Press any key (arrows, modifiers, F-keys all decode).",
        );
        term.print(1, 2, "q / Escape quits.");
        term.print(1, 4, "Last key:");
        let last = self.log.last().map_or("(none yet)", String::as_str);
        term.print(11, 4, last);

        term.print(1, 6, "Log (oldest first):");
        for (i, entry) in self.log.iter().enumerate() {
            let y = 7 + u16::try_from(i).expect("LOG_LEN fits in u16");
            term.print(1, y, entry);
        }

        term.present().ok();
    }
}

impl Example for Keyboard {
    const NAME: &'static str = "03_keyboard";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(Keyboard);
