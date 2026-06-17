# FTXUI

- **Language**: C++ (C++17 minimum, C++20 module support)
- **Repository**: <https://github.com/ArthurSonzogni/FTXUI>
- **License**: MIT
- **Stars**: ~10.3k
- **Latest version**: v7.0.0
- **Author**: Arthur Sonzogni

## Summary

FTXUI is a zero-dependency, cross-platform C++ library for building terminal user interfaces using a
functional, declarative API inspired by React. It provides a three-layer architecture (screen, DOM,
component), built-in layout engine with flexbox-like semantics, and compiles to WebAssembly for
browser-hosted demos. Its API uses modern C++ idioms (lambdas, shared_ptr composition, pipe operator
chaining) to let developers describe UI declaratively rather than imperatively.

## Architecture

FTXUI is split into three modules:

1. **ftxui::screen** -- Low-level grid of character cells. Handles colors, pixels, and terminal
   output. You can `Print()` a screen directly.
2. **ftxui::dom** -- Hierarchical `Element` tree for layout and composition. Elements are arranged
   with `hbox`, `vbox`, `gridbox`, and `flexbox`. Decorators like `border`, `bold`, `color`, `flex`
   are applied via the pipe operator (`|`).
3. **ftxui::component** -- Interactive widgets that respond to events (keyboard, mouse, resize). A
   `Component` is a `shared_ptr<ComponentBase>` that implements `Render()` (returning an `Element`),
   `OnEvent()`, and `Add()` for parent/child relationships.

The separation is clean: `Element` is a single-frame rendering primitive, `Component` produces
dynamic multi-frame UI with state and event handling.

### Rendering approach

FTXUI reprints the entire frame on every render cycle. There is no differential/dirty-region update.
The author has stated this is intentional: sending a single chunk for the whole frame can be cheaper
than sending diffs at various screen locations, and it avoids garbage artifacts during terminal
resize. Multiple events are batched before rendering (the event queue is drained before a frame is
drawn), so there is no per-event re-render lag.

### Component model

Components form a tree. The tree structure determines keyboard navigation (focus traversal). Key
built-in components:

- **Input widgets**: `Input`, `Menu`, `Toggle`, `Checkbox`, `Radiobox`, `Dropdown`, `Slider`,
  `Button`
- **Layout containers**: `Container::Horizontal`, `Container::Vertical`, `Container::Tab`,
  `Container::Stacked`
- **Structural**: `ResizableSplit` (mouse-draggable panes), `Window` (draggable/resizable),
  `Collapsible`, `Modal`
- **Decorators**: `Renderer` (override rendering), `CatchEvent` (intercept events), `Maybe`
  (conditional visibility), `Hoverable`

The decorator pattern is central. Components compose via the pipe operator:

```cpp
component = component
  | Renderer([](Element e) { return e | border; })
  | CatchEvent(handler)
  | Maybe(&show_flag);
```

### Event handling

Events propagate top-down through the component tree. `CatchEvent` intercepts events before they
reach children. `OnEvent()` returns `bool` to indicate consumption. Thread-safe external event
injection is supported via `App::PostEvent(Event::Custom)` or `App::RequestAnimationFrame()`.

### Canvas drawing

FTXUI includes a `Canvas` element for freeform drawing using braille, block, or simple characters.
This enables graphs, animations, and game-like visuals within the terminal.

## What it does well

1. **Zero dependencies** -- No ncurses, no terminfo, no external libraries. The entire library is
   self-contained, which simplifies builds and cross-compilation. An amalgamated
   single-header/source option is available from v7.0.0.
   [GitHub README](https://github.com/ArthurSonzogni/FTXUI)

2. **Declarative, composable API** -- The React-inspired functional style with pipe-operator
   chaining produces readable, concise UI code. Decorators compose naturally:
   `text("hello") | bold | border | color(Color::Red)`. This is a stark contrast to imperative TUI
   libraries where you manually position and draw.
   [FTXUI docs](https://arthursonzogni.github.io/FTXUI/)

3. **WebAssembly target** -- FTXUI compiles to WASM and runs against xterm.js in the browser. All
   official examples are playable online. This is unique among C++ TUI libraries and excellent for
   demos and documentation. [HN discussion](https://news.ycombinator.com/item?id=27403877)

4. **Flexbox-like layout** -- `hbox`, `vbox`, `gridbox`, `flexbox` with `flex` decorators handle
   responsive layouts automatically. Elements adapt to terminal dimensions without manual coordinate
   math.

5. **Animation support** -- Built-in `OnAnimation` callbacks and `RequestAnimationFrame()` enable
   smooth animations. The canvas module supports animated drawing demos.
   [GitHub README](https://github.com/ArthurSonzogni/FTXUI)

6. **Modern C++ design** -- Uses lambdas, `shared_ptr`, `std::function`, pipe operators. Feels like
   writing modern C++ rather than fighting a C-era API. Clean source code that is "beautifully laid
   out" per HN commenters. [HN discussion](https://news.ycombinator.com/item?id=27403877)

7. **Cross-platform** -- Linux (primary), macOS, Windows, WebAssembly. UTF-8 and fullwidth character
   support built in. Keyboard and mouse navigation.

8. **Rich widget set out of the box** -- Menus, dropdowns, sliders, tabs, resizable splits,
   draggable windows, modals, collapsibles all included without third-party add-ons.

9. **Excellent packaging** -- Available via CMake FetchContent, vcpkg, Conan, Bazel,
   Debian/Ubuntu/Arch packages, Nix flake, conda-forge, and XMake.

## Where it falls short

1. **Focus management breaks down in complex layouts** -- Nested containers consume navigation
   events greedily. Users report being "locked" inside a child container with no way to tab out to
   siblings. Building custom focus traversal (e.g., tab rings across non-sibling components)
   requires subclassing `ComponentBase` and reimplementing navigation logic from scratch. The author
   acknowledges the problem but no canonical solution exists.
   [Issue #1163](https://github.com/ArthurSonzogni/FTXUI/issues/1163)

2. **Large list rendering performance** -- Rendering 1000+ items in a vertical container takes
   ~700ms. There is no built-in virtualization (render only visible rows). Users must subclass
   `ComponentBase` and `Node` to implement subset rendering with manual height calculation, scroll
   indicators, and render-cycle hacks.
   [Discussion #962](https://github.com/ArthurSonzogni/FTXUI/discussions/962)

3. **Limited documentation for complex use cases** -- Examples are small and self-contained. Users
   consistently report that documentation does not show how to build complex, multi-panel
   applications with custom navigation. "There's no indication to my mind of what the intended
   pattern is for usage to do anything complicated." The intended composition patterns for
   real-world apps are underdocumented.
   [Issue #1163](https://github.com/ArthurSonzogni/FTXUI/issues/1163)

4. **Event propagation model is restrictive** -- `CatchEvent` only intercepts events _before_
   children. There is no built-in mechanism to suppress a child's handling of an event so a parent
   can handle it instead. Users request decorator-level event override capabilities.
   [Issue #1175](https://github.com/ArthurSonzogni/FTXUI/issues/1175)

5. **Internal classes are inaccessible** -- Many internal types live in anonymous namespaces, making
   extension difficult without forking. Users cannot subclass `NodeDecorator` or other internal base
   classes from their own source. The author moved to this approach to avoid breakage on updates,
   but it forces users who need custom behavior to vendor a fork.
   [Issue #1163](https://github.com/ArthurSonzogni/FTXUI/issues/1163)

6. **Full-frame reprint on every render** -- No differential screen updates. For most apps this is
   fine (and avoids flicker/artifact issues), but it can be wasteful over slow connections or on
   resource-constrained devices. [HN discussion](https://news.ycombinator.com/item?id=27403877)

7. **No terminfo/termcap integration** -- FTXUI assumes VT-compatible terminals and does not query
   terminal capabilities. Color detection is limited to checking if "256" or "truecolor" appears in
   the TERM variable. This can cause issues on non-standard terminals.
   [HN discussion](https://news.ycombinator.com/item?id=27403877)

8. **Window component has bugs** -- Draggable windows can disappear or behave incorrectly when
   cluttered or overlapping. [Issue #1024](https://github.com/ArthurSonzogni/FTXUI/issues/1024)

9. **`shared_ptr` overhead** -- Every `Component` is a `shared_ptr<ComponentBase>`. HN commenters
   note this as "Java Disease" (over-reliance on shared ownership). Forgivable in a UI toolkit, but
   adds allocation overhead and makes ownership semantics less explicit.

## Comparison: FTXUI (declarative) vs BearLibTerminal (imperative)

| Aspect                 | FTXUI                                                             | BearLibTerminal                                           |
| ---------------------- | ----------------------------------------------------------------- | --------------------------------------------------------- |
| **Paradigm**           | Declarative/functional, React-inspired                            | Imperative, immediate-mode-like                           |
| **Rendering**          | Build element tree, call `Render()`, library handles layout       | Direct cell-by-cell: `terminal_put(x, y, ch)`             |
| **Layout**             | Automatic: flexbox, hbox/vbox, responsive to terminal size        | Manual: developer calculates all positions                |
| **State management**   | Components hold state, re-render on events                        | Developer manages all state externally                    |
| **Event handling**     | Event tree propagation with `OnEvent()`/`CatchEvent()` decorators | Poll-based: `terminal_read()`, `terminal_has_input()`     |
| **Composability**      | High: pipe operators, decorators, component tree                  | Low: everything is manual function calls                  |
| **Target use case**    | TUI applications (forms, dashboards, tools)                       | Roguelike games, tile-based displays                      |
| **Backend**            | Real terminal (VT codes) or WebAssembly/xterm.js                  | OpenGL window pretending to be a terminal                 |
| **Dependencies**       | Zero                                                              | SDL2/OpenGL (renders its own window)                      |
| **Font/tile support**  | Unicode text only                                                 | Bitmap fonts, tilesets, tile composition                  |
| **Performance model**  | Full frame reprint, batched events                                | Direct GPU-accelerated cell updates                       |
| **API style**          | Modern C++ (lambdas, shared_ptr, templates)                       | C API with wrappers for multiple languages                |
| **Complexity ceiling** | Focus/navigation issues in deeply nested layouts                  | No built-in layout, so complexity is all on the developer |

The fundamental difference: FTXUI manages layout, rendering, and navigation for you (at the cost of
control); BearLibTerminal gives you a grid of cells and gets out of the way (at the cost of doing
everything yourself). FTXUI is better suited for form-heavy TUI applications; BearLibTerminal is
better suited for tile-based games where you need pixel-level control over every cell and custom
rendering with sprites/tilesets.

For a roguelike or tile-based game, FTXUI's automatic layout engine and component tree would fight
against manual tile placement. Its Canvas element can draw freeform, but it is still embedded in the
DOM layout system. BearLibTerminal's direct cell access is a more natural fit for game rendering
where you own the entire grid.

## Projects using FTXUI

Notable projects from the README and ecosystem:

| Project                                                                                         | Description                                         |
| ----------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| [cachyos-cli-installer](https://github.com/cachyos/new-cli-installer)                           | CachyOS Linux installer TUI                         |
| [BestEdrOfTheMarket](https://github.com/Xacone/BestEdrOfTheMarket)                              | EDR testing/simulation tool                         |
| [ostree-tui](https://github.com/AP-Sensing/ostree-tui)                                          | OSTree repository management                        |
| [FTB](https://github.com/Cyxuan0311/FTB)                                                        | Terminal file browser with SSH/MySQL                |
| [Tux-Dock](https://github.com/MARKMENTAL/tuxdock)                                               | Docker TUI frontend                                 |
| [json-tui](https://github.com/ArthurSonzogni/json-tui)                                          | Interactive JSON viewer (by FTXUI author)           |
| [git-tui](https://github.com/ArthurSonzogni/git-tui)                                            | Git TUI (by FTXUI author)                           |
| [inLimbo](https://github.com/nots1dd/inLimbo)                                                   | Terminal music player                               |
| [Captain's log](https://github.com/nikoladucak/caps-log)                                        | Personal logging tool                               |
| [TermBreaker](https://github.com/ArthurSonzogni/termBreaker)                                    | Breakout game (playable in browser via WASM)        |
| [keywords](https://github.com/Oakamoore/keywords)                                               | Word game (playable in browser via WASM)            |
| [sweeper](https://www.thomthom.net/thoughts/2026/01/sweeper-a-hat-tip-to-the-simple-fun-games/) | Minesweeper variant (playable in browser via WASM)  |
| [eCAL monitor](https://github.com/eclipse-ecal/ecal)                                            | Eclipse eCAL middleware monitoring                  |
| [TUIKit](https://github.com/skhelladi/TUIKit)                                                   | Qt-inspired wrapper framework built on top of FTXUI |

The cpp-best-practices Game Jam produced ~11 games built with FTXUI, demonstrating its Canvas
capabilities for game-like rendering.

## Community add-on libraries

- [ftxui-grid-container](https://github.com/mingsheng13/grid-container-ftxui) -- Grid layout
  component
- [ftxui-ip-input](https://github.com/mingsheng13/ip-input-ftxui) -- IP address input widget
- [ftxui-image-view](https://github.com/ljrrjl/ftxui-image-view) -- Image display in terminal
- [ftxui-navigation-tree](https://github.com/Appisolato/navigation-tree-ftxui) -- Tree navigation
  widget
- [MarkdownFTXUI](https://github.com/zvasilev/MarkdownFTXUI) -- Markdown editor/viewer

## Sources

- **Kept**:
  - [GitHub README](https://github.com/ArthurSonzogni/FTXUI) -- Primary source for features,
    architecture, project list
  - [Component module docs](https://arthursonzogni.github.io/FTXUI/module-component.html) --
    Detailed component API reference
  - [HN discussion](https://news.ycombinator.com/item?id=27403877) -- Author comments on design
    decisions, community criticism of terminal assumptions and full-frame reprint
  - [Issue #1163](https://github.com/ArthurSonzogni/FTXUI/issues/1163) -- Focus management
    difficulties, internal class inaccessibility
  - [Discussion #962](https://github.com/ArthurSonzogni/FTXUI/discussions/962) -- Large list
    rendering performance (~700ms for 1000 items)
  - [Issue #1175](https://github.com/ArthurSonzogni/FTXUI/issues/1175) -- Event propagation
    limitations
  - [TUIKit article](https://medium.com/@sofiane.khelladi/why-i-built-tuikit-a-qt-inspired-terminal-ui-framework-for-scientific-applications-7ad1b3a9bafb)
    -- Perspective on FTXUI being "powerful but low-level"
  - [TUI comparison doc](https://github.com/wistrand/melker/blob/main/agent_docs/tui-comparison.md)
    -- Cross-library feature comparison table

- **Dropped**:
  - terminalroot.com tutorial -- Mostly repackaged README content with no original analysis
  - sugggest.com -- SEO aggregator page with inaccurate claims (e.g., "header-only" which it is not)
  - dev.to blog post -- Mentions FTXUI briefly but focuses on the author's own text editor project

## Gaps

- **Benchmark data**: No published benchmarks comparing FTXUI rendering performance to ncurses,
  notcurses, or other TUI libraries. The 700ms figure for 1000-item lists comes from a single user
  report.
- **Windows stability**: The author states Windows support is "experimental" and
  contributor-maintained. No systematic testing data available.
- **Thread safety details**: `PostEvent` is documented as thread-safe, but the broader thread-safety
  model of the component tree is not well documented.
- **Accessibility**: No information found on screen reader compatibility or accessibility features.
