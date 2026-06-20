# Ratatui

- **Language**: Rust
- **Repository**: <https://github.com/ratatui/ratatui>
- **License**: MIT
- **Current version**: 0.30.1 (June 2026)
- **Stats**: ~20k GitHub stars, 32M+ crates.io downloads, 4,300+ reverse dependencies, 260+

  contributors

- **Forked from**: tui-rs (2023), after original maintainer stepped away

Ratatui is the dominant Rust TUI framework. It is a widget-based, immediate-mode rendering library
with backend abstraction. Not a roguelike library, but its architecture, rendering model, and Rust
API design patterns are relevant reference points.

## Architecture

### Immediate-mode rendering

Ratatui uses immediate-mode rendering, closer to game engine UI (like Dear ImGui) than traditional
retained-mode GUI frameworks. The application redraws the entire UI every frame inside a
`terminal.draw()` closure. There are no persistent widget objects or a retained widget tree. The UI
is a pure function of application state.

```rust
loop {
    terminal.draw(|frame| {
        // Describe entire UI from scratch each frame
        frame.render_widget(some_widget, area);
    })?;
    // Handle events, update state
}
```

### Double-buffered diffing

Despite redrawing everything each frame, Ratatui is efficient. It renders widgets to an in-memory
`Buffer` (a 2D grid of `Cell` values), then diffs the current buffer against the previous frame's
buffer. Only changed cells are sent to the terminal. This is the same approach used by ncurses and
notcurses, but implemented in pure Rust with no C dependencies.

### Backend abstraction

Ratatui does not write ANSI codes directly. The `Terminal` struct is generic over a `Backend` trait
with multiple implementations:

- **ratatui-crossterm**: Cross-platform (Windows/macOS/Linux). Most popular choice.
- **ratatui-termion**: Unix-only, minimalist.
- **ratatui-termwiz**: Advanced terminal features (from wezterm).
- **ratatui-termina**: For typed escape sequence usage.

Application code is identical across backends. The abstraction also enables mock backends for
testing.

### Modular workspace (v0.30.0+)

Since v0.30.0, Ratatui is split into a Cargo workspace:

- **ratatui-core**: Widget traits (`Widget`, `StatefulWidget`), text types (`Span`, `Line`, `Text`),

  `Buffer`, layout, style, symbols. Designed for maximum API stability; widget library authors
  depend on this.

- **ratatui-widgets**: Built-in widgets (Block, Paragraph, List, Table, Chart, etc.).
- **Backend crates**: One per backend.
- **ratatui** (main crate): Re-exports everything for convenience.

This split reduces compile times for downstream widget libraries and enables independent versioning.

### Widget system

Widgets implement the `Widget` trait with a single method:

```rust
fn render(self, area: Rect, buf: &mut Buffer);
```

`StatefulWidget` adds a mutable state parameter for widgets that track scroll position, cursor, etc.

Since v0.26.0, all built-in widgets implement `Widget` for references (`&self`), allowing widgets to
be stored and reused across frames.

Layout uses a constraint-based system: `Constraint::Percentage`, `Constraint::Length`,
`Constraint::Min`, `Constraint::Max`, `Constraint::Ratio`. Constraints divide a `Rect` into
sub-areas.

### No built-in event loop or input handling

Ratatui deliberately does not handle input events. You bring your own event loop and input library
(typically crossterm). This gives full control over async integration (tokio, etc.) but means more
boilerplate.

## Built-in widgets

Block, Paragraph, List, Table, Tabs, Chart (line/scatter/bar), Gauge, Sparkline, BarChart, Calendar,
Canvas (for freeform drawing with shapes, lines, points).

## Notable applications built with Ratatui

The ecosystem is massive. The [awesome-ratatui](https://github.com/ratatui/awesome-ratatui) list has
hundreds of entries. Selected notable ones:

### Development tools

- **gitui** - Terminal UI for Git (one of the most popular Rust TUI apps)
- **Yazi** - Blazing fast terminal file manager, async I/O
- **lazyjj** - TUI for Jujutsu VCS
- **gitu** - Magit-inspired TUI Git client
- **rainfrog** - Database management TUI for Postgres
- **gobang** - Cross-platform TUI database management
- **ATAC** - Feature-full TUI API client
- **slumber** - Terminal-based HTTP/REST client
- **openapi-tui** - Browse and run OpenAPI-defined APIs
- **serie** - Rich Git commit graph in the terminal
- **scooter** - Interactive find and replace
- **television** - Blazingly fast fuzzy finder
- **joshuto** - Ranger-like file manager

### System/network monitoring

- **bottom** - Cross-platform graphical process/system monitor
- **bandwhich** - Network utilization by process
- **trippy** - Network diagnostic tool
- **bpftop** (Netflix) - Real-time view of running eBPF programs
- **kdash** - Kubernetes dashboard
- **macmon** - Apple Silicon performance monitoring
- **vector** (Datadog) - High-performance observability data pipeline
- **oha** - HTTP traffic monitoring

### Music and media

- **spotify-player** - Full-featured Spotify player
- **spotify-tui**/**spotatui** - Spotify TUI client
- **manga-tui** - Terminal manga reader with image support

### Productivity

- **atuin** - Shell history manager
- **csvlens** - CSV viewer
- **taskwarrior-tui** - Task management
- **mprocs** - Run multiple commands in parallel with separate output
- **linutil** - Linux system administration toolbox

### Games

- **plastic** - NES emulator with Ratatui UI
- **Chess-tui** - Terminal chess
- **astray** - Space strategy game
- **tage** - Turn-based strategy with multiplayer

### Third-party widget ecosystem (40+ crates)

- ratatui-image (sixel/halfblock images)
- tui-textarea / edtui (text editing)
- tui-tree-widget, tui-scrollview, tui-popup
- tachyonfx (shader-like visual effects)
- ratatui-markdown (markdown rendering)
- rat-widget (comprehensive data-input widgets)

### Framework integrations

- **ratzilla** - Ratatui + WebAssembly for browser-based TUI apps
- **bevy_ratatui** - Use Ratatui inside Bevy game engine
- **egui-ratatui** - Ratatui as an egui backend
- **ratatui-wgpu** - GPU rendering backend

### Language bindings

Python, TypeScript, Go, C#, Ruby, Elixir -- all have Ratatui bindings, showing the influence of the
design.

## What it does well

1. **Immediate-mode simplicity**. UI is a function of state. No widget tree synchronization, no

   observer pattern, no callback spaghetti. This is the same mental model as React or SwiftUI
   (describe what it should look like, let the library handle updates) but with full control over
   the render loop.

1. **Efficient rendering via diffing**. Despite redrawing everything each frame, only changed cells

   are sent to the terminal. This makes it practical for high-frequency updates (system monitors,
   real-time dashboards) without flicker.

1. **Backend abstraction**. Swapping terminal backends is a one-line change. Mock backends enable

   headless testing of UI code.

1. **Modular, composable widget system**. The `Widget` trait is minimal (one method). Complex UIs

   are built by composing simple widgets. The constraint-based layout system handles terminal
   resizing gracefully.

1. **Rust-native ergonomics**. Chainable builder APIs for styles, layouts, and widgets. Zero-cost

   abstractions. No unsafe code in the core. Ownership model prevents common TUI bugs (dangling
   widget references, stale state).

1. **Ecosystem size**. 4,300+ reverse dependencies on crates.io. Hundreds of apps. 40+ third-party

   widget crates. Active community (Discord, Matrix, forum, 260+ contributors). This is by far the
   largest Rust TUI ecosystem.

1. **Documentation quality**. Dedicated website (ratatui.rs) with concepts, tutorials, and cookbook.

   Extensive API docs. Templates for getting started. EuroRust 2024 talk.

1. **Modular workspace architecture** (v0.30+). Widget library authors depend only on `ratatui-core`

   for stability. Backends compile independently. Parallel compilation.

## Where it falls short

1. **No built-in state management**. You must manually track all UI state: cursor positions, scroll

   offsets, selection indices, focus order. `StatefulWidget` helps but is minimal compared to
   retained-mode frameworks like Cursive that handle focus, mouse input, and event routing
   automatically.

1. **No built-in event/input handling**. You must integrate crossterm (or similar) yourself and wire

   up the event loop. This is intentional (flexibility) but adds boilerplate, especially for
   beginners. Every Ratatui app needs ~20-50 lines of event loop setup.

1. **No application architecture guidance**. Out of the box, there is no help organizing large

   applications. No built-in component model, routing, or state machine. The community has responded
   with third-party frameworks (tui-realm, rat-salsa, widgetui) but there is no standard approach.

1. **Cross-terminal inconsistency**. Different terminals handle colors, Unicode, and mouse events

   differently. Ratatui cannot fully abstract these differences. Users report color rendering
   glitches and layout issues on specific terminals (especially Windows Terminal, macOS Terminal.app
   vs iTerm2).

1. **Text-only rendering constraint**. Fundamentally limited to terminal cell grids. No true

   graphics (though ratatui-image provides sixel/halfblock workarounds). Limited animation
   capabilities. The Canvas widget provides basic shape drawing but it is still character-cell
   based.

1. **More boilerplate than alternatives**. Compared to Cursive (retained-mode, handles events) or

   iocraft (declarative, React-like), Ratatui requires more code for the same functionality. The
   tradeoff is more control.

1. **Widget trait consumes self**. The original `Widget::render(self, ...)` consumes the widget.

   While v0.26.0 added implementations for `&Widget`, the experimental `WidgetRef` trait remains
   unstable, and the ergonomics of borrowed vs owned widgets can be confusing.

## Relevance as a reference

For a roguelike/TUI game library in Rust, Ratatui's design offers several lessons:

- **Immediate-mode rendering + double-buffered diffing** is the proven approach for terminal UIs. It

  works well for game loops too, where you redraw each frame based on game state.

- **Backend abstraction via traits** is the right pattern. It enables testing, alternative rendering

  targets (web, GPU), and future-proofing.

- **Constraint-based layout** is valuable for panel/HUD arrangement in games, though a roguelike

  also needs direct cell-level access for map rendering.

- **The `Widget` trait pattern** (render to a buffer region) is composable and extensible. A game

  library could use a similar pattern for UI overlays.

- **The modular workspace split** is a good model for separating core types (that downstream

  libraries depend on) from implementations.

- **The ecosystem proves demand**. Hundreds of apps show the market for Rust TUI tooling. The gap is

  that Ratatui is not designed for games; a roguelike-focused library would fill a different niche.

## Sources

- [GitHub: ratatui/ratatui](https://github.com/ratatui/ratatui) - README, ARCHITECTURE.md
- [Ratatui Website - Rendering concepts](https://ratatui.rs/concepts/rendering/)
- [docs.rs/ratatui](https://docs.rs/ratatui/latest/ratatui/) - API documentation
- [awesome-ratatui](https://github.com/ratatui/awesome-ratatui) - Curated app/library list
- [Starlog: Ratatui deep-dive](https://starlog.is/articles/developer-tools/ratatui-ratatui/) -

  Architecture analysis, strengths/weaknesses

- [Cursive vs Ratatui](https://github.com/gyscos/cursive/wiki/Cursive-vs-ratatui) - Comparison from

  Cursive's perspective

- [crates.io/crates/ratatui](https://crates.io/crates/ratatui) - Download statistics
