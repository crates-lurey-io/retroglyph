//! E2E tests for the `rg` library.

use retroglyph::Terminal;
use retroglyph::backend::Headless;
use retroglyph::color::Color;
use retroglyph::style::Style;

#[test]
fn test_e2e_movement() {
    let backend = Headless::new(10, 10);
    let mut term = Terminal::new(backend);

    // Initial draw
    term.put(5, 5, '@');
    term.present();

    assert_eq!(term.backend().grid().get(5, 5).glyph(), '@');

    // Move player
    term.clear();
    term.put(6, 5, '@');
    term.present();

    assert_eq!(term.backend().grid().get(5, 5).glyph(), ' ');
    assert_eq!(term.backend().grid().get(6, 5).glyph(), '@');
}

#[test]
fn test_e2e_style() {
    let backend = Headless::new(10, 10);
    let mut term = Terminal::new(backend);

    let red_style = Style::new().fg(Color::RED);
    term.put_styled(1, 1, 'A', red_style);
    term.present();

    assert_eq!(term.backend().grid().get(1, 1).style(), red_style);
}

#[test]
fn test_e2e_headless_demo_scenario() {
    use retroglyph::color::AnsiColor;
    use retroglyph::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    let backend = Headless::new(40, 15);
    let mut term = Terminal::new(backend);

    // Initial Scene Draw Helper
    let draw_scene = |term: &mut Terminal<Headless>, player_x: u16, player_y: u16| {
        term.clear();

        // Draw room
        term.fg(Color::Ansi(AnsiColor::White));
        for x in 0..40 {
            term.put(x, 0, '─');
            term.put(x, 14, '─');
        }
        for y in 0..15 {
            term.put(0, y, '│');
            term.put(39, y, '│');
        }
        term.put(0, 0, '┌');
        term.put(39, 0, '┐');
        term.put(0, 14, '└');
        term.put(39, 14, '┘');

        // Draw player
        term.fg(Color::Ansi(AnsiColor::Green));
        term.put(player_x, player_y, '@');

        // Draw enemies
        term.fg(Color::Ansi(AnsiColor::Red));
        term.put(10, 8, 'g');
        term.put(20, 10, 'D');
        term.reset_style();

        // Draw status
        term.print(1, 1, "HP: 100  Level: 1");
    };

    // Render Initial Frame
    draw_scene(&mut term, 5, 5);
    term.present();

    // 1. Snapshot assertion for Frame 1
    let expected_frame_1 = "\
┌──────────────────────────────────────┐
│HP:·100··Level:·1·····················│
│······································│
│······································│
│······································│
│····@·································│
│······································│
│······································│
│·········g····························│
│······································│
│···················D··················│
│······································│
│······································│
│······································│
└──────────────────────────────────────┘
";
    assert_eq!(term.backend().grid().to_string(), expected_frame_1);

    // 2. Inject movement input and consume it
    term.backend_mut().push_event(Event::Key(KeyEvent {
        code: KeyCode::Right,
        modifiers: KeyModifiers::NONE,
    }));

    let event = term.read();
    let mut player_x = 5;
    let player_y = 5;
    if let Event::Key(key_event) = event {
        if key_event.code == KeyCode::Right {
            player_x += 1;
        }
    }

    // Render Second Frame
    draw_scene(&mut term, player_x, player_y);
    term.present();

    // 3. Snapshot assertion for Frame 2
    let expected_frame_2 = "\
┌──────────────────────────────────────┐
│HP:·100··Level:·1·····················│
│······································│
│······································│
│······································│
│·····@································│
│······································│
│······································│
│·········g····························│
│······································│
│···················D··················│
│······································│
│······································│
│······································│
└──────────────────────────────────────┘
";
    assert_eq!(term.backend().grid().to_string(), expected_frame_2);
}
