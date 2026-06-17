# Reference: xterm.js

## Summary

xterm.js is a TypeScript terminal emulator library for the browser, and the most widely deployed web
terminal in existence. It powers VS Code's integrated terminal, Hyper, Tabby, and hundreds of cloud
IDEs and web tools. Its rendering pipeline (DOM, Canvas 2D, and WebGL backends), glyph texture
atlas, layered canvas architecture, and addon system are all relevant reference points for building
a modern terminal component.

## Overview

- **Language:** TypeScript (core library has zero runtime dependencies)
- **License:** MIT
- **Repository:** <https://github.com/xtermjs/xterm.js> (~21k stars)
- **Package:** `@xterm/xterm` on npm; also ships `@xterm/headless` for Node.js (parser without
  rendering)
- **Current version:** 5.5.0 stable, 6.x in beta
- **Origin:** Started as a JavaScript port of the X11 xterm; evolved into its own project

## Notable Users

| Project                          | Category                |
| -------------------------------- | ----------------------- |
| VS Code (+ Cursor, Codium forks) | Desktop/web IDE         |
| Hyper                            | Electron terminal app   |
| Tabby                            | Electron terminal app   |
| Theia / OpenSumi                 | Cloud IDE frameworks    |
| Eclipse Che / Codenvy            | Cloud workspaces        |
| JupyterLab                       | Computational notebooks |
| Replit                           | Browser IDE             |
| Azure Cloud Shell                | Cloud shell             |
| Portainer                        | Docker management       |
| Proxmox VE                       | Virtualization platform |
| ttyd / GoTTY                     | Terminal-over-web tools |
| CoderPad / CodeInterview.io      | Interview platforms     |
| Wave Terminal                    | AI-native terminal      |
| Coder                            | Self-hosted remote dev  |

The GitHub dependency graph shows thousands of downstream repos.

## Rendering Architecture

xterm.js has evolved through three rendering backends, each still available:

### 1. DOM Renderer (default fallback)

- Renders each cell as `<span>` elements with CSS styling.
- Painfully slow for large outputs due to sheer number of `span x CSS rule` computations.
- Advantage: native text selection, accessibility, browser find (Ctrl+F) just work.
- A faster DOM renderer was explored (issue #4604) using a minimal glyph-to-font approach, reducing
  red-flagged frames.

### 2. Canvas 2D Renderer (`@xterm/addon-canvas`, formerly built-in)

- Uses multiple layered `<canvas>` elements for separation of concerns (text, cursor, selection,
  links are separate layers).
- **Texture atlas / glyph cache:** ASCII characters with their foreground + 8 color + 8 bright/bold
  variants are pre-rendered into an `ImageBitmap`. This atlas is used to blit glyphs via
  `drawImage()` instead of `fillText()` per character.
- Render layers diff against previous state and only redraw changed cells.
- Was built-in to core; extracted to an addon in v5 to reduce default bundle size.
- Kept primarily for fallback on environments where WebGL doesn't work (older Safari, some Linux
  VMs, iOS).

### 3. WebGL Renderer (`@xterm/addon-webgl`, recommended)

- Builds a `Float32Array` with all draw data, uploads it to the GPU in one shot.
- Uses vertex + fragment shaders to render from the texture atlas.
- Massively faster than Canvas 2D because it eliminates per-cell `drawImage()` calls.
- Multiple texture atlas pages supported (starting at 512x512, scaling up to 2048x2048, up to 8-16
  pages depending on GPU).
- Shared texture atlas cache code between canvas and WebGL renderers (PR #4170).
- Glyph rendering: rasterizes glyphs on a hidden canvas, packs them into atlas pages, uploads as GPU
  textures. New glyphs are rendered on demand and cached.

### Key rendering design decisions

- **Cell grid:** The terminal is a fixed-size grid of cells. Each cell has a character, foreground,
  background, and decoration attributes.
- **Dirty tracking:** Render layers track what they've drawn and diff against new state, skipping
  unchanged cells.
- **Layered canvases:** Text, cursor, selection, and decorations each get their own canvas,
  composited by the browser. This avoids full redraws when only the cursor blinks.
- **Single-thread limitation:** Parsing and rendering share the main JS thread, which creates
  contention during heavy output. No web worker offloading for parsing yet.

## Input Handling

- Keyboard events captured via `keyDown` handler, composition handlers (for IME), and `beforeInput`
  with cancel support.
- `InputHandler` class processes all VT sequences from the parser, implementing the xterm control
  sequence spec.
- `onData` event emits processed input as string data to be sent to the PTY.
- `onKey` event provides key + DOM event for custom handling.
- Mouse events supported: click, drag, scroll, full mouse tracking modes (SGR, X10, etc.).
- A `sendKey` API was explored (PR #3578) to allow programmatic key injection.
- Mobile/touch support is limited (issue #5377): relies on browser mouse event translation, no
  native touch gestures, basic virtual keyboard integration.

## Addon System

The addon API is minimal:

```typescript
interface ITerminalAddon extends IDisposable {
  activate(terminal: Terminal): void;
}

// Usage:
terminal.loadAddon(new WebLinksAddon());
```

An addon receives the `Terminal` instance on activation and extends it using the public API. Addons
are `IDisposable`, cleaned up when the terminal is disposed.

### Official addons

| Addon                            | Purpose                                  |
| -------------------------------- | ---------------------------------------- |
| `@xterm/addon-webgl`             | GPU-accelerated WebGL2 renderer          |
| `@xterm/addon-canvas`            | Canvas 2D renderer (fallback)            |
| `@xterm/addon-fit`               | Auto-fit terminal to container element   |
| `@xterm/addon-web-links`         | Clickable URL detection                  |
| `@xterm/addon-search`            | Buffer search                            |
| `@xterm/addon-serialize`         | Serialize buffer to VT sequences or HTML |
| `@xterm/addon-image`             | Inline image support                     |
| `@xterm/addon-ligatures`         | Font ligature rendering                  |
| `@xterm/addon-clipboard`         | Clipboard access                         |
| `@xterm/addon-unicode11`         | Unicode 11 character widths              |
| `@xterm/addon-unicode-graphemes` | Grapheme clustering (experimental)       |
| `@xterm/addon-web-fonts`         | Web font integration                     |
| `@xterm/addon-attach`            | WebSocket PTY attachment                 |
| `@xterm/addon-progress`          | OSC 9;4 progress API                     |

Design note: addons that aren't used by VS Code historically get less maintenance attention. Shell
integration, for example, is implemented in VS Code's own code (via OSC 633) rather than as an
xterm.js addon, because the maintainers consider it out of scope for the library itself.

## Strengths

1. **Battle-tested at scale.** Used by VS Code (hundreds of millions of installs), making it the
   most deployed web terminal. Bugs get found and fixed fast.

2. **GPU-accelerated rendering.** The WebGL renderer with texture atlas glyph caching delivers
   performance that approaches native terminals. The texture atlas system (pre-render glyphs, pack
   into GPU textures, draw via shaders) is a well-proven pattern.

3. **Layered canvas architecture.** Separating text, cursor, selection, and decorations into
   independent canvas layers allows targeted redraws and clean code separation.

4. **Zero dependencies.** The core library is self-contained; all optional features are addons.

5. **Mature VT100/xterm compatibility.** Works with bash, vim, tmux, curses-based apps, mouse
   events. Covers the core terminal use cases thoroughly.

6. **Headless mode.** `@xterm/headless` provides the parser and buffer without any DOM, useful for
   server-side terminal state tracking and reconnection via the serialize addon.

7. **Rich Unicode and IME support.** CJK characters, emoji, input method editors all handled.

8. **Well-typed API.** Full TypeScript declarations, experimental APIs clearly marked, semver
   discipline.

## Weaknesses and Limitations

1. **Feature coverage gaps.** Scores 66% (154/233) on the terminfo.dev feature matrix, ranking #11
   of 12 tested terminals. Missing 64 features including: no Kitty graphics protocol, no iTerm2
   inline images (addressed partially by the image addon), gaps in modern TUI sequences.

2. **Single-threaded parsing + rendering.** Parsing and rendering share the main thread, causing
   contention during heavy output. At very wide terminals (2500+ cols), performance degrades
   noticeably. JS has a ~2-4x processing penalty vs compiled languages.

3. **Canvas rendering loses DOM benefits.** Custom text selection, no browser Ctrl+F, no native
   accessibility without explicit screen reader mode. The newer competitor `wterm` (Zig/WASM,
   DOM-based) highlights this tradeoff.

4. **No native mobile/touch support.** Touch interactions are limited; no native gestures, difficult
   text selection on touch devices, basic virtual keyboard integration.

5. **Shell integration is out of scope.** The library deliberately excludes shell integration
   (command detection, cwd tracking). VS Code implements this in its own layer via OSC 633, but this
   logic is complex and not reusable by other xterm.js consumers.

6. **Input latency on busy pages.** Backend latency must stay below 8ms to avoid perceivable
   artifacts. On busy event loops, overall latency hits +32ms due to animation frame batching.

7. **Addon ecosystem is VS Code-centric.** Addons not used by VS Code tend to get less attention.
   Third-party addon development requires navigating non-obvious import paths between in-repo and
   external addon packages.

## Relevance for Building a Terminal Component

Even though xterm.js targets VT100 emulation rather than game terminals, several architectural
patterns transfer directly:

- **Texture atlas glyph caching:** Pre-render character glyphs into atlas textures, blit from the
  atlas rather than rasterizing text per frame. This is the single biggest performance win.
- **Layered rendering:** Separate concerns (background, text, cursor, selection, overlays) into
  independent layers with targeted invalidation.
- **Dirty cell tracking:** Diff the cell grid against previous state; only redraw what changed.
- **Addon/plugin pattern:** Keep the core minimal; let features like search, links, and image
  support live in optional addons with a simple `activate(terminal)` / `dispose()` lifecycle.
- **Headless mode:** Separating the buffer/parser from the renderer enables server-side state
  tracking, testing without a DOM, and serialization for reconnection.
- **Fallback rendering chain:** WebGL > Canvas > DOM, with automatic fallback based on browser
  capabilities.

## Sources

- [GitHub Repository](https://github.com/xtermjs/xterm.js) - README, addon list, real-world users
- [xtermjs.org](http://xtermjs.org/) - Official site
- [terminfo.dev/terminals/xterm-js](https://terminfo.dev/terminals/xterm-js) - Feature matrix
  scoring (66%, 154/233)
- [Canvas Renderer PR #938](https://github.com/xtermjs/xterm.js/pull/938) - Layered canvas
  architecture, texture atlas design
- [WebGL Renderer PR #1790](https://github.com/xtermjs/xterm.js/pull/1790) - GPU rendering via
  shaders + Float32Array
- [Multiple Texture Atlas Pages PR #4244](https://github.com/xtermjs/xterm.js/pull/4244) -
  Multi-page atlas system
- [Addon API PR #2065](https://github.com/xtermjs/xterm.js/pull/2065) - Simple `ITerminalAddon`
  interface
- [Wide Terminal Perf Issue #4175](https://github.com/xtermjs/xterm.js/issues/4175) - Single-thread
  bottleneck analysis
- [Touch Support Issue #5377](https://github.com/xtermjs/xterm.js/issues/5377) - Mobile limitations
- [Shell Integration Issue #5807](https://github.com/xtermjs/xterm.js/issues/5807) - Scope boundary
  discussion
- [libghostty Exploration Issue #5686](https://github.com/xtermjs/xterm.js/issues/5686) - WASM VT
  parser discussion
- [wterm Comparison](https://www.stork.ai/blog/vercels-new-tool-ends-terminal-hell) - DOM vs canvas
  tradeoffs
