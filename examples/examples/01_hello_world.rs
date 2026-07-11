//! 01: Hello, world!
//!
//! The smallest possible retroglyph example: prints "Hello, world!" centered
//! on a 50x25 grid. The exact same `HelloWorld` implementation runs on every
//! backend this crate is built with -- only the Cargo feature flag changes:
//!
//! ```sh
//! cargo run --example 01_hello_world --features crossterm
//! cargo run --example 01_hello_world --features software
//! cargo run --example 01_hello_world  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Press `q` or `Escape` to quit on the interactive backends, or close the
//! window (the windowed backend reports the close button as an [`Event`],
//! not a forced exit -- it's up to the game to act on it).

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{Backend, Terminal};
use retroglyph_examples::Example;

/// State for the hello-world example (none needed: the text never changes).
///
/// `Default`-derived so `Example::init`'s default implementation applies --
/// no backend-dependent startup state here, so there's nothing to override.
#[derive(Default)]
pub struct HelloWorld;

impl HelloWorld {
    /// Drains pending input, returning `false` if the user asked to quit
    /// (`q`/`Escape`, or the window's close button on windowed backends).
    ///
    /// `&mut self` (unused here) is the shape a real example's event
    /// handler needs -- this one just has nothing to mutate, since
    /// `HelloWorld` has no state.
    #[allow(clippy::needless_pass_by_ref_mut, clippy::unused_self)]
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            match event {
                Event::Key(key) if matches!(key.code, KeyCode::Char('q') | KeyCode::Escape) => {
                    return false;
                }
                Event::Close => return false,
                _ => {}
            }
        }
        true
    }

    /// Draws this frame and presents it. Unconditional -- called every tick
    /// regardless of what `handle_events` saw, since this example's content
    /// never changes.
    ///
    /// `&self` (unused here) is the shape a real example's draw step needs
    /// -- this one just has nothing to read, since `HelloWorld` has no
    /// state.
    #[allow(clippy::unused_self)]
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        term.print(18, 12, "Hello, world!");
        term.present().ok();
    }
}

impl Example for HelloWorld {
    const NAME: &'static str = "01_hello_world";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(HelloWorld);
