//! E2E tests for the `rg` library.

use rg::backend::Headless;
use rg::color::Color;
use rg::style::Style;
use rg::Terminal;

#[test]
fn test_e2e_movement() {
    let backend = Headless::new(10, 10);
    let mut term = Terminal::new(backend);

    // Initial draw
    term.put(5, 5, '@');
    term.present();

    assert_eq!(term.backend().grid().get(5, 5).glyph, '@');

    // Move player
    term.clear();
    term.put(6, 5, '@');
    term.present();

    assert_eq!(term.backend().grid().get(5, 5).glyph, ' ');
    assert_eq!(term.backend().grid().get(6, 5).glyph, '@');
}

#[test]
fn test_e2e_style() {
    let backend = Headless::new(10, 10);
    let mut term = Terminal::new(backend);

    let red_style = Style::new().fg(Color::RED);
    term.put_styled(1, 1, 'A', red_style);
    term.present();

    assert_eq!(term.backend().grid().get(1, 1).style, red_style);
}
