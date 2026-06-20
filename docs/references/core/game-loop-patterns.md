# Game Loop Patterns for a Rust Terminal/Grid Rendering Library

## Summary

There are six major game loop patterns relevant to a terminal grid library: fixed-timestep
accumulator loops, turn-based blocking reads, library-owned callback loops, user-owned poll/render
loops, platform event loop integration (winit), and async event-driven loops. The best architecture
for a library supporting both roguelikes and action games is a **user-owned loop with
library-provided poll + render primitives** (ratatui's approach), optionally bundling convenience
wrappers for common patterns (fixed-timestep, turn-based blocking). This gives maximum flexibility
without forcing a single loop model on users.

---

## 1. Fixed Timestep ("Fix Your Timestep")

The canonical reference is Glenn Fiedler's "Fix Your Timestep" article. The pattern decouples
simulation from rendering using an accumulator.

### Problem

Variable delta time makes physics non-deterministic and unstable. Frame-rate-dependent gameplay
feels different on different machines.

### Solution

The renderer **produces**time; the simulation**consumes** it in fixed `dt` chunks. Leftover time
carries over via an accumulator. Interpolation between the previous and current state smooths
rendering.

### Key concepts

- **Accumulator**: stores unprocessed real time between frames
- **Fixed dt**: simulation always steps by the same amount (e.g., 1/60s or 1/120s)
- **Spiral of death**: when simulation can't keep up, accumulator grows unboundedly. Mitigate by

  clamping `frame_time` to a maximum (e.g., 0.25s).

- **Interpolation alpha**: `alpha = accumulator / dt`, used to lerp between `previous_state` and

  `current_state` for rendering

### Rust implementation

```rust
use std::time::{Duration, Instant};

const DT: Duration = Duration::from_micros(16_667); // ~60 Hz simulation
const MAX_FRAME_TIME: Duration = Duration::from_millis(250);

struct GameState {
    // position, velocity, etc.
    x: f64,
    vel: f64,
}

impl GameState {
    fn integrate(&mut self, dt_secs: f64) {
        self.x += self.vel * dt_secs;
    }

    fn interpolate(prev: &GameState, curr: &GameState, alpha: f64) -> GameState {
        GameState {
            x: prev.x * (1.0 - alpha) + curr.x * alpha,
            vel: curr.vel,
        }
    }
}

fn fixed_timestep_loop() {
    let mut current_state = GameState { x: 0.0, vel: 100.0 };
    let mut previous_state = current_state.clone();
    let mut accumulator = Duration::ZERO;
    let mut current_time = Instant::now();

    loop {
        let new_time = Instant::now();
        let mut frame_time = new_time - current_time;
        if frame_time > MAX_FRAME_TIME {
            frame_time = MAX_FRAME_TIME; // prevent spiral of death
        }
        current_time = new_time;
        accumulator += frame_time;

        // process_input();

        while accumulator >= DT {
            previous_state = current_state.clone();
            current_state.integrate(DT.as_secs_f64());
            accumulator -= DT;
        }

        let alpha = accumulator.as_secs_f64() / DT.as_secs_f64();
        let render_state = GameState::interpolate(&previous_state, &current_state, alpha);
        // render(&render_state);
    }
}
```

### When to use

Real-time games with physics, animation, or smooth movement. Not needed for pure turn-based games
where the world only advances on player input.

[Source: Gaffer on Games](https://gafferongames.com/post/fix_your_timestep/)
[Source: Game Programming Patterns](https://gameprogrammingpatterns.com/game-loop.html)

---

## 2. Turn-Based Polling Model (BearLibTerminal)

BearLibTerminal's API is designed for traditional roguelikes. The game owns the loop and blocks on
input.

### API design

```c
// Core input functions:
int terminal_read();      // blocks until input available, returns event
int terminal_has_input();  // non-blocking check: is there an event?
int terminal_peek();      // non-blocking: returns next event without consuming it
int terminal_state(int);  // query current state (key pressed, mouse position, etc.)

// Core output functions:
void terminal_clear();
void terminal_put(int x, int y, int code);
void terminal_print(int x, int y, const char* s);
void terminal_refresh();  // double-buffered: commits scene to screen
```

### Typical game loop

```rust
// Idiomatic Rust equivalent of BearLibTerminal pattern
fn turn_based_loop(term: &mut Terminal) {
    loop {
        // Render current state
        term.clear();
        draw_map(term);
        draw_entities(term);
        draw_ui(term);
        term.refresh(); // present double-buffered frame

        // Block until player acts
        let event = term.read(); // blocks here

        // Process the turn
        match event {
            Event::Key(KeyCode::Escape) => break,
            Event::Key(key) => {
                let action = map_key_to_action(key);
                if let Some(action) = action {
                    player_turn(action);
                    ai_turn();
                    update_world();
                }
            }
            Event::Close => break,
            _ => {} // resize, mouse, etc.
        }
    }
}
```

### Key properties

- **Blocking read**: `terminal_read()` suspends the thread until input arrives. Zero CPU when idle.
- **Non-blocking check**: `has_input()` + `peek()` allow hybrid patterns (e.g., animations between

  turns).

- **Double buffering**: all output goes to a back buffer; `refresh()` swaps it to screen.
- **State queries**: `terminal_state(TK_SHIFT)` checks modifier keys after reading an event.

### Hybrid: animations in a turn-based game

```rust
fn turn_based_with_animations(term: &mut Terminal) {
    loop {
        // Animate even while waiting for input
        if term.has_input() {
            let event = term.read();
            handle_turn(event);
        } else {
            // No input yet: advance animations, re-render
            update_animations();
        }

        term.clear();
        draw_world(term);
        term.refresh();

        if !term.has_input() {
            // Sleep briefly to cap animation FPS
            std::thread::sleep(Duration::from_millis(16));
        }
    }
}
```

[Source: BearLibTerminal Reference](http://foo.wyrd.name/en:bearlibterminal:reference)

---

## 3. Library-Owned Loop (bracket-lib's GameState::tick)

bracket-lib (formerly RLTK) takes ownership of the main loop. The user implements a `GameState`
trait with a single `tick()` callback.

### API

```rust
// User implements this trait:
pub trait GameState {
    fn tick(&mut self, ctx: &mut BTerm);
}

// Library owns the loop:
fn main() -> BError {
    let context = BTermBuilder::simple80x50()
        .with_title("My Game")
        .build()?;

    let gs = MyGameState::new();
    main_loop(context, gs) // never returns until exit
}
```

### Inside tick()

```rust
struct MyGameState {
    map: Vec<Tile>,
    player: Position,
    mode: GameMode,
}

impl GameState for MyGameState {
    fn tick(&mut self, ctx: &mut BTerm) {
        // Check input (non-blocking, returns Option<VirtualKeyCode>)
        if let Some(key) = ctx.key {
            match key {
                VirtualKeyCode::Escape => ctx.quit(),
                VirtualKeyCode::Left => self.player.x -= 1,
                VirtualKeyCode::Right => self.player.x += 1,
                // ...
                _ => {}
            }
        }

        // Update game logic
        self.update_ai();
        self.update_fov();

        // Render
        ctx.cls();
        self.draw_map(ctx);
        ctx.print(self.player.x, self.player.y, "@");
    }
}
```

### Characteristics

- **Library owns the loop**: `main_loop()` never returns. All game logic lives in `tick()`.
- **Tick called every frame**: not every turn. For turn-based games, the user must implement their

  own state machine inside `tick()` to distinguish "waiting for input" from "processing turn."

- **Cross-platform**: bracket-lib's `main_loop` wraps winit or wasm event loops internally.
- **Simple but inflexible**: works well for single-window games. Harder to integrate with external

  systems, custom threading, or non-game UI.

### Trade-offs

| Pro                            | Con                                           |
| ------------------------------ | --------------------------------------------- |
| Minimal boilerplate            | No control over frame timing                  |
| Cross-platform (native + wasm) | Turn-based games need internal state machines |
| Handles window management      | Can't integrate external event sources easily |
| Good for tutorials/prototyping | Hard to compose with other libraries          |

[Source: bracket-lib examples](https://github.com/amethyst/bracket-lib)

---

## 4. User-Owned Loop (ratatui's Approach)

ratatui provides rendering primitives and leaves the event loop entirely to the user. Event handling
comes from crossterm (or termion/termwiz).

### Core pattern

```rust
use std::time::Duration;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{Terminal, backend::CrosstermBackend};

fn user_owned_loop() -> Result<()> {
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut app = App::new();

    while app.running {
        // Render
        terminal.draw(|frame| app.render(frame))?;

        // Poll for events with timeout
        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key.code);
                }
            }
        }

        // Update (animations, timers, etc.)
        app.update();
    }

    Ok(())
}
```

### Poll timeout controls the loop mode

```rust
// Turn-based: long timeout, nearly blocking
event::poll(Duration::from_millis(1000))?;

// Real-time: short timeout, continuous updates
event::poll(Duration::from_millis(16))?;

// Non-blocking: immediate return
event::poll(Duration::ZERO)?;
```

### TEA (The Elm Architecture) variant

ratatui documents a Model/Update/View architecture:

```rust
// Message enum captures all possible actions
enum Message {
    Increment,
    Decrement,
    Quit,
}

// Main loop
fn main() -> Result<()> {
    let mut terminal = init_terminal()?;
    let mut model = Model::default();

    while model.running {
        terminal.draw(|f| view(&model, f))?;

        if let Some(msg) = handle_event(&model)? {
            let mut current_msg = Some(msg);
            while let Some(m) = current_msg {
                current_msg = update(&mut model, m);
            }
        }
    }

    Ok(())
}

fn handle_event(model: &Model) -> Result<Option<Message>> {
    if event::poll(Duration::from_millis(250))? {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                return Ok(match key.code {
                    KeyCode::Char('j') => Some(Message::Increment),
                    KeyCode::Char('k') => Some(Message::Decrement),
                    KeyCode::Char('q') => Some(Message::Quit),
                    _ => None,
                });
            }
        }
    }
    Ok(None)
}
```

### Why this is the best library pattern

| Pro                                             | Con                                      |
| ----------------------------------------------- | ---------------------------------------- |
| User controls timing completely                 | More boilerplate than callback model     |
| Works for any game type (turn-based, real-time) | User must handle frame timing themselves |
| Easy to integrate with other systems            | No built-in cross-platform loop          |
| Composable with async, threads, etc.            | User needs to understand event polling   |
| Library stays simple and focused                |                                          |

[Source: ratatui docs](https://ratatui.rs/concepts/event-handling/)
[Source: ratatui TEA pattern](https://ratatui.rs/concepts/application-patterns/the-elm-architecture/)

---

## 5. Integration with winit's Event Loop

winit (the standard Rust windowing library) owns the event loop and requires you to implement
`ApplicationHandler`. This is relevant for a terminal library that renders to a GPU-backed window
rather than a real terminal.

### ApplicationHandler trait

```rust
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

struct App {
    window: Option<Window>,
    game: GameState,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Create window and GPU resources here
        if self.window.is_none() {
            self.window = Some(
                event_loop.create_window(Window::default_attributes()).unwrap()
            );
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::KeyboardInput { event, .. } => {
                self.game.handle_input(event);
            }

            WindowEvent::RedrawRequested => {
                self.game.update();
                self.game.render();

                // For continuous rendering (real-time games):
                self.window.as_ref().unwrap().request_redraw();
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Called after all events processed, before blocking.
        // Good place to request_redraw() for real-time games.
        // For turn-based: skip this, only redraw on input.
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App { window: None, game: GameState::new() };

    // Choose one:
    event_loop.set_control_flow(ControlFlow::Poll); // real-time: loop continuously
    // event_loop.set_control_flow(ControlFlow::Wait); // turn-based: sleep until event

    event_loop.run_app(&mut app).unwrap();
}
```

### ControlFlow modes

| Mode                              | Behavior                                             | Use case                                                |
| --------------------------------- | ---------------------------------------------------- | ------------------------------------------------------- |
| `ControlFlow::Poll`               | Returns immediately from waiting, spins continuously | Real-time games, animations                             |
| `ControlFlow::Wait`               | Blocks until OS delivers an event                    | Turn-based games, text editors, low power               |
| `ControlFlow::WaitUntil(instant)` | Blocks until event OR deadline                       | Turn-based with periodic animation (e.g., cursor blink) |

### Key events in sequence

1. `new_events(StartCause)` - batch of events starting
2. `window_event(...)` - individual window events (input, resize, redraw)
3. `about_to_wait()` - all events processed, about to block/poll again

### Rendering strategy for games

```rust
// Real-time: render in RedrawRequested, request_redraw() in about_to_wait()
fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
    self.window.as_ref().unwrap().request_redraw();
}

// Turn-based: render in RedrawRequested, request_redraw() only on state change
fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
    match event {
        WindowEvent::KeyboardInput { event, .. } => {
            if self.game.handle_input(event) {
                // State changed, need redraw
                self.window.as_ref().unwrap().request_redraw();
            }
        }
        WindowEvent::RedrawRequested => {
            self.game.render();
        }
        _ => {}
    }
}
```

### pump_events (user-owned loop escape hatch)

winit provides `pump_app_events()` on some platforms (not web, not iOS) to let you own the loop:

```rust
use winit::platform::pump_events::EventLoopExtPumpEvents;

fn user_owned_winit_loop() {
    let mut event_loop = EventLoop::new().unwrap();
    let mut app = App::default();

    loop {
        // Process pending events without blocking
        event_loop.pump_app_events(None, &mut app);

        // Your own timing, updates, etc.
        app.game.update();

        if app.should_quit {
            break;
        }
    }
}
```

This is discouraged by winit for portability reasons but can be useful for integrating with existing
game loops.

[Source: winit docs](https://docs.rs/winit/latest/winit/)
[Source: winit ApplicationHandler](https://docs.rs/winit/latest/winit/application/trait.ApplicationHandler.html)

---

## 6. Async Game Loops (tokio Integration)

### crossterm EventStream

crossterm provides `EventStream` (behind the `event-stream` feature) which implements
`futures::Stream`, compatible with tokio/async-std:

```rust
use crossterm::event::EventStream;
use futures::StreamExt;
use tokio::time::{self, Duration};

async fn async_game_loop(terminal: &mut Terminal<impl Backend>) -> Result<()> {
    let mut events = EventStream::new();
    let mut tick_interval = time::interval(Duration::from_millis(16));
    let mut app = App::new();

    loop {
        tokio::select! {
            // Fixed-rate game tick
            _ = tick_interval.tick() => {
                app.update();
                terminal.draw(|f| app.render(f))?;
            }

            // Input events
            event = events.next() => {
                match event {
                    Some(Ok(Event::Key(key))) => {
                        if key.kind == KeyEventKind::Press {
                            app.handle_key(key.code);
                        }
                    }
                    Some(Err(_)) | None => break,
                    _ => {}
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut terminal = init_terminal()?;
    async_game_loop(&mut terminal).await?;
    restore_terminal()?;
    Ok(())
}
```

### Multiple async event sources

The real power of async loops is composing multiple event sources:

```rust
async fn multiplayer_game_loop(
    terminal: &mut Terminal<impl Backend>,
    network: &mut NetworkStream,
) -> Result<()> {
    let mut events = EventStream::new();
    let mut tick = time::interval(Duration::from_millis(50));
    let mut app = App::new();

    loop {
        tokio::select! {
            _ = tick.tick() => {
                app.update();
                terminal.draw(|f| app.render(f))?;
            }
            event = events.next() => {
                if let Some(Ok(event)) = event {
                    app.handle_input(event);
                }
            }
            msg = network.next() => {
                if let Some(Ok(msg)) = msg {
                    app.handle_network(msg);
                }
            }
        }

        if app.should_quit { break; }
    }

    Ok(())
}
```

### Trade-offs (2)

| Pro                                           | Con                                                      |
| --------------------------------------------- | -------------------------------------------------------- |
| Natural composition of multiple event sources | Adds tokio dependency                                    |
| No manual polling/threading                   | Async complexity (lifetimes, pinning)                    |
| `select!` replaces complex poll logic         | Single-threaded by default (use `spawn` for parallelism) |
| Works well for networked games                | Overhead for simple single-player games                  |

[Source: crossterm EventStream](https://docs.rs/crossterm/latest/crossterm/event/struct.EventStream.html)

---

## 7. Supporting Both Real-Time and Turn-Based from One Library

### The core insight

Turn-based and real-time games differ only in **when updates happen**and**when rendering
happens**, not in _what_ the library provides. A well-designed library should provide primitives,
not prescribe a loop.

### Architecture: building blocks approach

```rust
/// The library provides these primitives:
pub struct Terminal { /* ... */ }

impl Terminal {
    // Input
    pub fn poll_event(&mut self, timeout: Duration) -> Option<Event>;
    pub fn read_event(&mut self) -> Event; // blocking

    // Output (double-buffered)
    pub fn clear(&mut self);
    pub fn put(&mut self, x: i32, y: i32, glyph: Glyph);
    pub fn print(&mut self, x: i32, y: i32, text: &str);
    pub fn present(&mut self); // swap buffers

    // Timing
    pub fn fps(&self) -> f64;
    pub fn delta_time(&self) -> Duration;
}
```

### The user picks their loop pattern

```rust
// Turn-based roguelike
fn roguelike_loop(term: &mut Terminal) {
    loop {
        render_world(term);
        term.present();

        let event = term.read_event(); // blocks
        if let Some(action) = event_to_action(event) {
            execute_turn(action);
        }
    }
}

// Real-time action game
fn action_game_loop(term: &mut Terminal) {
    let mut last = Instant::now();
    let dt = Duration::from_micros(16_667);
    let mut acc = Duration::ZERO;

    loop {
        let now = Instant::now();
        acc += now - last;
        last = now;

        while let Some(event) = term.poll_event(Duration::ZERO) {
            handle_input(event);
        }

        while acc >= dt {
            update_physics(dt);
            acc -= dt;
        }

        render_world(term);
        term.present();
    }
}

// Hybrid: turn-based with idle animations
fn hybrid_loop(term: &mut Terminal) {
    loop {
        if let Some(event) = term.poll_event(Duration::from_millis(50)) {
            if let Some(action) = event_to_action(event) {
                execute_turn(action);
            }
        }

        update_animations();
        render_world(term);
        term.present();
    }
}
```

### Optional convenience wrappers

The library can provide opinionated wrappers on top of the primitives:

```rust
/// Convenience: run a simple game loop (library-owned).
/// Good for prototyping. Not required.
pub fn run<S: GameState>(term: Terminal, state: S) { /* ... */ }

pub trait GameState {
    fn tick(&mut self, ctx: &mut TickContext);
}

pub struct TickContext<'a> {
    pub term: &'a mut Terminal,
    pub dt: Duration,
    pub key: Option<KeyCode>,
    pub mouse: Option<MouseEvent>,
}
```

### Design principles

1. **Primitives first**: `poll_event`, `read_event`, `put`, `present`. These are the foundation.
2. **Blocking and non-blocking input**: both `read_event()` (blocking) and `poll_event(timeout)`

   (non-blocking with configurable timeout).

3. **No forced frame timing**: the library renders when `present()` is called, not on its own
   schedule.

4. **Optional convenience layer**: a `run()` function or `GameState` trait for users who want a
   simple loop.

5. **Async-compatible**: provide an `event_stream()` method returning a `Stream<Item = Event>` for
   async users.

---

## 8. Frame Timing and VSync

### Terminal backends

For terminal-based rendering (crossterm/termion), there's no GPU vsync. Frame timing is controlled
by the application:

```rust
// Simple frame limiter
let target_fps = 60;
let frame_duration = Duration::from_secs(1) / target_fps;

loop {
    let frame_start = Instant::now();

    process_input();
    update();
    render();

    let elapsed = frame_start.elapsed();
    if elapsed < frame_duration {
        std::thread::sleep(frame_duration - elapsed);
    }
}
```

### spin_sleep for precise timing

`std::thread::sleep` has poor granularity on many OSes (Windows: ~15ms, Linux: ~1ms). The
`spin_sleep` crate provides sub-millisecond precision:

```rust
use spin_sleep::SpinSleeper;

let sleeper = SpinSleeper::default();
let frame_duration = Duration::from_micros(16_667); // 60 FPS

loop {
    let frame_start = Instant::now();

    process_input();
    update();
    render();

    let elapsed = frame_start.elapsed();
    if elapsed < frame_duration {
        sleeper.sleep(frame_duration - elapsed);
    }
}
```

### GPU-backed window (winit + wgpu)

For a GPU-backed terminal emulator window, vsync is handled by the swap chain:

```rust
// wgpu surface configuration
let config = wgpu::SurfaceConfiguration {
    present_mode: wgpu::PresentMode::Fifo, // vsync ON (default, most compatible)
    // present_mode: wgpu::PresentMode::Mailbox, // vsync with lowest latency
    // present_mode: wgpu::PresentMode::Immediate, // no vsync
    // ...
};
```

| Present Mode | Behavior                                  | Use case                   |
| ------------ | ----------------------------------------- | -------------------------- |
| `Fifo`       | Wait for vsync, guaranteed no tearing     | Default, power-efficient   |
| `Mailbox`    | Submit newest frame at vsync, discard old | Low-latency gaming         |
| `Immediate`  | Present immediately, may tear             | Benchmarking, uncapped FPS |

### Frame timing for turn-based games

Turn-based games should **not** spin at 60 FPS when idle. Use event-driven rendering:

```rust
// Only re-render when something changes
fn efficient_turn_based(term: &mut Terminal) {
    let mut dirty = true;

    loop {
        if dirty {
            render_world(term);
            term.present();
            dirty = false;
        }

        // Block until input (zero CPU while idle)
        let event = term.read_event();
        if handle_event(event) {
            dirty = true; // state changed, need re-render
        }
    }
}
```

---

## 9. Concrete Rust Code: Complete Example for Each Pattern

### Pattern A: Fixed Timestep (real-time action game)

```rust
use std::time::{Duration, Instant};

const TICK_RATE: Duration = Duration::from_micros(16_667); // 60 Hz
const MAX_FRAME: Duration = Duration::from_millis(250);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut term = Terminal::new()?;
    let mut game = GameWorld::new();
    let mut prev_state = game.snapshot();
    let mut accumulator = Duration::ZERO;
    let mut last_time = Instant::now();

    loop {
        // Timing
        let now = Instant::now();
        let mut frame_time = now - last_time;
        if frame_time > MAX_FRAME { frame_time = MAX_FRAME; }
        last_time = now;
        accumulator += frame_time;

        // Input (non-blocking drain)
        while let Some(ev) = term.poll_event(Duration::ZERO) {
            match ev {
                Event::Key(KeyCode::Escape) => return Ok(()),
                ev => game.handle_input(ev),
            }
        }

        // Fixed-step simulation
        while accumulator >= TICK_RATE {
            prev_state = game.snapshot();
            game.update(TICK_RATE.as_secs_f64());
            accumulator -= TICK_RATE;
        }

        // Interpolated render
        let alpha = accumulator.as_secs_f64() / TICK_RATE.as_secs_f64();
        let render_state = GameWorld::lerp(&prev_state, &game.snapshot(), alpha);

        term.clear();
        render_state.draw(&mut term);
        term.present();
    }
}
```

### Pattern B: Turn-based blocking (classic roguelike)

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut term = Terminal::new()?;
    let mut game = Dungeon::new();

    loop {
        // Render
        term.clear();
        game.draw(&mut term);
        term.present();

        // Block for input
        let event = term.read_event();

        // Process
        match event {
            Event::Key(KeyCode::Escape) | Event::Close => break,
            Event::Key(key) => {
                if let Some(action) = game.key_to_action(key) {
                    game.player_act(action);
                    game.enemies_act();
                    game.resolve_effects();
                }
            }
            _ => {}
        }
    }

    Ok(())
}
```

### Pattern C: Library-owned callback (bracket-lib style)

```rust
fn main() -> BError {
    let context = BTermBuilder::simple80x50()
        .with_title("Roguelike")
        .build()?;
    main_loop(context, MyGame::new())
}

struct MyGame { /* state */ }

impl GameState for MyGame {
    fn tick(&mut self, ctx: &mut BTerm) {
        // Called every frame by the library
        match self.mode {
            Mode::WaitingForInput => {
                if let Some(key) = ctx.key {
                    self.process_turn(key);
                    self.mode = Mode::Processing;
                }
            }
            Mode::Processing => {
                self.run_ai();
                self.mode = Mode::WaitingForInput;
            }
        }

        ctx.cls();
        self.draw(ctx);
    }
}
```

### Pattern D: User-owned with poll/render (ratatui style)

```rust
fn main() -> color_eyre::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::default();

    while app.running {
        terminal.draw(|f| app.view(f))?;

        if crossterm::event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = crossterm::event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key.code);
                }
            }
        }

        app.tick();
    }

    ratatui::restore();
    Ok(())
}
```

### Pattern E: winit ApplicationHandler

```rust
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

struct App {
    window: Option<Window>,
    game: Game,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.window = Some(
            event_loop.create_window(Window::default_attributes()).unwrap()
        );
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                self.game.render();
                // Continuous: always request next frame
                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.game.handle_input(event);
            }
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App { window: None, game: Game::new() };
    event_loop.run_app(&mut app).unwrap();
}
```

### Pattern F: Async with tokio

```rust
use crossterm::event::{EventStream, Event, KeyCode, KeyEventKind};
use futures::StreamExt;
use tokio::time;

#[tokio::main]
async fn main() -> Result<()> {
    let mut terminal = init_terminal()?;
    let mut app = App::new();
    let mut events = EventStream::new();
    let mut tick = time::interval(Duration::from_millis(50));

    loop {
        tokio::select! {
            _ = tick.tick() => {
                app.update();
                terminal.draw(|f| app.render(f))?;
            }
            event = events.next() => {
                match event {
                    Some(Ok(Event::Key(k))) if k.kind == KeyEventKind::Press => {
                        if k.code == KeyCode::Char('q') { break; }
                        app.handle_key(k.code);
                    }
                    Some(Err(_)) | None => break,
                    _ => {}
                }
            }
        }
    }

    restore_terminal()?;
    Ok(())
}
```

---

## 10. Recommendation for a Terminal Grid Library

### Primary recommendation: User-owned loop with library primitives

Follow ratatui's approach. The library provides:

1. **`Terminal`** struct with rendering methods (`clear`, `put`, `print`, `present`)
2. **`poll_event(timeout: Duration) -> Option<Event>`** for non-blocking input
3. **`read_event() -> Event`** for blocking input (turn-based convenience)
4. **Double-buffered rendering** (user calls `present()` to swap)

The user writes their own loop. This supports every pattern from pure turn-based to fixed-timestep
real-time.

### Optional additions (in order of priority)

1. **Convenience `run()` function** with a `GameState` trait for quick prototyping (bracket-lib

   pattern, but optional)

1. **`EventStream`** returning `impl Stream<Item = Event>` for async/tokio users
1. **`ControlFlow` hint** on the terminal to signal intent (wait-for-input vs. continuous), which
   the backend can use to optimize (e.g., `ControlFlow::Wait` for GPU-backed window)

### Why not library-owned?

- Forces a state-machine style on turn-based games (unnatural)
- Can't compose with external event sources (network, file watchers)
- Can't integrate with user's preferred async runtime
- Makes testing harder (can't call `tick()` directly without a full `BTerm` context)

### Why not turn-based-only blocking?

- Can't do animations, particle effects, or smooth scrolling
- Can't support real-time action games
- Locks out hybrid designs (e.g., animated combat in a turn-based game)

### Concrete API sketch

```rust
pub struct Terminal { /* ... */ }

impl Terminal {
    pub fn new(options: TerminalOptions) -> Result<Self>;

    // Input
    pub fn poll_event(&mut self, timeout: Duration) -> Option<Event>;
    pub fn read_event(&mut self) -> Event;
    pub fn has_event(&self) -> bool;

    // Output
    pub fn clear(&mut self);
    pub fn put(&mut self, pos: impl Into<Point>, glyph: Glyph);
    pub fn print(&mut self, pos: impl Into<Point>, text: &str);
    pub fn present(&mut self);

    // Info
    pub fn size(&self) -> Size;
    pub fn elapsed(&self) -> Duration; // since last present()
}

// Async support (behind feature flag)
#[cfg(feature = "async")]
impl Terminal {
    pub fn event_stream(&self) -> impl Stream<Item = Result<Event>>;
}

// Convenience runner (behind feature flag)
#[cfg(feature = "runner")]
pub fn run<S: GameState>(options: TerminalOptions, state: S) -> Result<()>;

#[cfg(feature = "runner")]
pub trait GameState {
    fn tick(&mut self, ctx: &mut TickContext);
}
```

This design lets a roguelike developer write `loop { render; read_event; process_turn }` with zero
ceremony, while an action game developer writes a fixed-timestep loop with
`poll_event(Duration::ZERO)`, and a networked game uses `tokio::select!` with the event stream. One
library, every pattern.

---

## Sources

- **Kept**:

  [Gaffer on Games: Fix Your Timestep](https://gafferongames.com/post/fix_your_timestep/) - the
  canonical reference for fixed-timestep game loops with accumulator and interpolation

- **Kept**:

  [Game Programming Patterns: Game Loop](https://gameprogrammingpatterns.com/game-loop.html) -
  comprehensive taxonomy of loop patterns with trade-off analysis

- **Kept**: [winit docs.rs](https://docs.rs/winit/latest/winit/) - primary docs for

  ApplicationHandler, ControlFlow, and event loop design

- **Kept**:

  [winit ApplicationHandler](https://docs.rs/winit/latest/winit/application/trait.ApplicationHandler.html) -
  trait API details, lifecycle events

- **Kept**: [ratatui event handling](https://ratatui.rs/concepts/event-handling/) - user-owned loop

  patterns

- **Kept**:

  [ratatui TEA pattern](https://ratatui.rs/concepts/application-patterns/the-elm-architecture/) -
  full Model/Update/View example with code

- **Kept**: [BearLibTerminal reference](http://foo.wyrd.name/en:bearlibterminal:reference) -

  complete API reference for the blocking turn-based model

- **Kept**: [bracket-lib hello_minimal.rs](https://github.com/amethyst/bracket-lib) - canonical

  example of library-owned GameState::tick pattern

- **Kept**: [crossterm event module](https://docs.rs/crossterm/latest/crossterm/event/index.html) -

  poll/read API for synchronous input, EventStream for async

- **Dropped**: Various GitHub page chrome/navigation from bracket-lib repo pages (extracted raw file

  instead)

## Gaps

- **bracket-lib internals**: Could not fetch `main_loop` source code to see exactly how it wraps

  winit internally. The public API and examples were sufficient.

- **spin_sleep benchmarks**: No benchmark data fetched for precise sleep accuracy across OSes. The

  crate is well-known in the Rust gamedev community.

- **wgpu present mode latency numbers**: No concrete latency measurements for Fifo vs Mailbox vs

  Immediate. Would need profiling on target hardware.

- **Bevy/macroquad loop patterns**: Did not cover these larger engines. They use library-owned loops

  similar to bracket-lib but with ECS scheduling. Not directly relevant to a terminal grid library.
