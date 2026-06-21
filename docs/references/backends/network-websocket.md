# Research: Network/WebSocket Streaming Backend for Rust Terminal Grid Rendering

## Summary

A network backend streams a server-side terminal grid to a web browser over WebSocket. The proven architecture (used by ttyd, GoTTY, and MUD clients) is: server renders to an in-memory cell buffer, computes diffs against the previous frame, serializes diffs as binary messages, and sends them over a WebSocket. The client reconstructs the grid and renders via Canvas/WebGL or a DOM grid. For a Rust library that already has a `Buffer`/`Cell` abstraction (like ratatui's), this maps naturally: the diff is computed server-side, serialized with a compact binary protocol, optionally compressed with zstd, and the client applies patches to a mirrored buffer.

## Findings

### 1. Architecture: Server-Side Rendering with Diff Streaming

The core architecture is a thin-server/thin-client model:

```
Server                          Network              Client (Browser)
+-----------------------+       WebSocket        +---------------------+
| Game loop             |                        | Mirror buffer       |
| -> render to Buffer   |  --- cell diffs --->   | -> apply diffs      |
| -> diff(prev, curr)   |                        | -> render to canvas |
| -> serialize + send   |  <-- input events ---  | -> capture keys     |
+-----------------------+                        +---------------------+
```

The server maintains two buffers: `current` (what was just rendered) and `previous` (what the client already has). After each frame, `diff(previous, current)` produces a list of `(position, cell)` changes. Only those changes are sent. The `previous` buffer is then swapped to `current`.

ratatui's `Buffer::diff()` method already produces exactly this: a sequence of `(u16, u16, &Cell)` tuples representing changed positions. This is the ideal integration point. [ratatui Buffer docs](https://docs.rs/ratatui/latest/ratatui/buffer/struct.Buffer.html)

**Key design decisions:**
- Frame rate: server-driven tick (e.g., 30fps for games, or event-driven for interactive UIs)
- Diff granularity: per-cell (simple, effective) vs. per-region (more complex, marginal gains for text UIs)
- Serialization: binary (compact) vs. JSON (debuggable) vs. ANSI escape sequences (compatible with xterm.js but wasteful)

### 2. Protocol Design Options

Three viable approaches, from simplest to most complex:

**Option A: Structured Cell Diffs (Recommended)**

Send changed cells as a binary or JSON message. Each cell diff includes position, character, foreground color, background color, and style modifiers.

```
Binary layout per cell (compact):
  [x: u16] [y: u16] [char: 1-4 bytes UTF-8] [fg: u8 or u32] [bg: u8 or u32] [modifiers: u16]
```

Advantages: compact, the client reconstructs exactly the server's view, supports arbitrary rendering (Canvas, WebGL, DOM). The client does not need a terminal emulator.

Estimated size: for 256-color mode, ~10-12 bytes per changed cell. An 80x24 terminal with 200 changed cells per frame = ~2.4 KB/frame uncompressed. With zstd, typically 40-60% compression on repetitive cell data.

**Option B: ANSI Escape Sequences**

Serialize the diff as ANSI/VT escape sequences (cursor moves, color changes, character writes). The client feeds these into xterm.js, which handles all terminal emulation.

This is what ttyd and GoTTY do. ttyd's protocol is simple: the server prepends a single command byte to each WebSocket message. `OUTPUT` (0x30 '0') carries raw PTY output (ANSI sequences), `INPUT` (0x30 '0') carries user keystrokes going the other direction. [ttyd protocol.c](https://github.com/tsl0922/ttyd/blob/main/src/protocol.c)

Advantages: trivial client (just pipe bytes into xterm.js), battle-tested. Disadvantages: relies on xterm.js for rendering (no custom Canvas rendering), ANSI sequences are verbose compared to structured diffs, cursor positioning adds overhead.

**Option C: Compressed Full Framebuffer**

Send the entire buffer every frame, compressed. Simple but wasteful for text UIs where <10% of cells change per frame. Only viable if the buffer is small (e.g., 80x24 = 1920 cells, ~20 KB uncompressed, ~2-5 KB compressed with zstd).

Could work as a fallback for reconnection (send a full snapshot) while using diffs for steady-state.

**Recommended hybrid:** Use Option A (cell diffs) for normal frames and Option C (compressed full framebuffer) for reconnection snapshots. Reserve Option B only if xterm.js compatibility is a hard requirement.

**Message framing (inspired by ttyd):**

```
enum MessageType : u8 {
    // Server -> Client
    FullSnapshot = 0x01,    // Full buffer state (on connect/reconnect)
    CellDiff     = 0x02,    // Incremental cell changes
    Resize       = 0x03,    // Terminal dimensions changed
    CursorPos    = 0x04,    // Cursor position update
    Title        = 0x05,    // Window title change
    Bell         = 0x06,    // Audible bell

    // Client -> Server
    KeyInput     = 0x10,    // Keyboard input
    MouseInput   = 0x11,    // Mouse event
    ResizeReq    = 0x12,    // Client reports viewport size
    Ping         = 0x13,    // Keepalive / latency measurement
}
```

### 3. WebSocket Libraries in Rust

**tokio-tungstenite** (recommended for standalone servers)
- Mature, widely used, async WebSocket built on tungstenite.
- Implements `Stream` + `Sink` traits for ergonomic async usage.
- Supports TLS via `native-tls` or `rustls` feature flags.
- Nagle's algorithm can be disabled (`set_nodelay(true)`) for lower latency.
- Performance is "decent" but not the fastest; recent versions (>0.26.2) have closed the gap with fastwebsockets.
- [Crate](https://crates.io/crates/tokio-tungstenite) | [Docs](https://docs.rs/tokio-tungstenite)

**axum WebSocket** (recommended for web applications)
- Built into the `axum` web framework (feature flag `ws`).
- Uses `WebSocketUpgrade` extractor for clean integration with axum routing and state.
- Supports `split()` for concurrent read/write via `futures_util::StreamExt`.
- Handles HTTP upgrade negotiation automatically.
- Best choice if the server also serves static files (the HTML/JS client) and has REST endpoints.
- [Docs](https://docs.rs/axum/latest/axum/extract/ws/index.html)

**fastwebsockets** (for maximum performance)
- From the Deno project. Minimal, focused on speed.
- Zero-copy frame parsing, no intermediate allocation for most frames.
- Passes Autobahn test suite, fuzzed with libfuzzer.
- Does not support `permessage-deflate` yet.
- Uses hyper for HTTP upgrade (`upgrade` feature).
- Best if WebSocket throughput is the bottleneck.
- [Docs](https://docs.rs/fastwebsockets)

**warp** (legacy option)
- warp has WebSocket support but the framework has stalled in maintenance.
- Prefer axum for new projects.

**Recommendation:** Use axum's WebSocket support. It gives you a complete web server (serve the client HTML/JS, REST API for metadata, WebSocket for streaming) in one crate. axum uses tokio-tungstenite under the hood.

### 4. Client-Side Rendering

Three approaches for the browser client:

**Canvas 2D (recommended for custom rendering)**
- Render each cell as a rectangle + text glyph on an HTML5 Canvas.
- Full control over fonts, colors, effects (glowing text, animations).
- Maintain a JS-side cell buffer; on receiving diffs, update the buffer and redraw only changed cells.
- Performance: Canvas 2D text rendering handles thousands of glyphs per frame easily. A 200x60 grid at 30fps is trivial.
- Font measurement: use `ctx.measureText()` at init to compute cell dimensions for a monospace font.

**WebGL (for advanced effects)**
- Render cells as textured quads using a font atlas (pre-rendered glyph texture).
- Each cell is 4 vertices with UV coordinates into the atlas, plus color uniforms.
- Can handle very large grids (200x100+) at 60fps with minimal CPU.
- More complex setup (shader programs, atlas generation), but enables effects like CRT filters, scanlines, bloom.
- Good reference: alacritty's renderer uses a similar glyph atlas approach (though native OpenGL, not WebGL).

**DOM Grid (simplest, worst performance)**
- Create a `<span>` or `<div>` per cell, update `textContent` and `style` on diff.
- Performance degrades above ~80x24 with frequent updates.
- Only viable for very simple, low-update-rate UIs.

**xterm.js (if using ANSI protocol)**
- If using Option B (ANSI escape sequences), xterm.js handles all rendering.
- Supports Canvas and WebGL renderers out of the box.
- Has a sophisticated flow control mechanism: `write()` is non-blocking, buffers up to 50MB, processes data in chunks within a 16ms frame budget.
- Supports watermark-based flow control with `pause()`/`resume()` callbacks for back-pressure propagation over WebSockets.
- [xterm.js docs](https://xtermjs.org/docs/) | [Flow control guide](https://xtermjs.org/docs/guides/flowcontrol/)

**Recommendation:** Canvas 2D for a custom terminal/game grid. xterm.js if you want traditional terminal compatibility with minimal client code.

### 5. Latency Considerations and Buffering Strategies

**Sources of latency in the pipeline:**
1. Game tick interval (e.g., 33ms at 30fps)
2. Diff computation (~0.1ms for an 80x24 buffer)
3. Serialization + compression (~0.1-0.5ms with zstd level 1)
4. WebSocket send + network RTT (variable, 1-100ms+ depending on geography)
5. Client-side deserialization + rendering (~1-5ms)

**Strategies:**

- **Disable Nagle's algorithm** (`TCP_NODELAY`): critical for interactive applications. Without it, small WebSocket frames may be delayed up to 200ms waiting for a full TCP segment. tokio-tungstenite exposes `disable_nagle` in `connect_async_with_config`.

- **Coalesce diffs within a tick**: don't send a WebSocket message for every cell change. Batch all changes from one render pass into a single message. This is natural if the game loop is: `update() -> render_to_buffer() -> compute_diff() -> send_one_message()`.

- **Frame rate adaptation**: if the client can't keep up (acknowledged via ACKs or measured via RTT), reduce the server-side frame rate or skip frames. Send the latest full diff, not queued partial diffs.

- **Flow control**: implement watermark-based back-pressure as described in xterm.js's flow control guide. The client sends ACK messages after processing N bytes. The server pauses the game's render-to-network pipeline when too many unacknowledged bytes are in flight. This prevents buffer bloat in the WebSocket layer.

- **Compression tradeoff**: zstd at level 1 adds ~0.1ms latency but can reduce message size by 40-60%. At level 3+ the latency grows. For a game at 30fps, level 1 is the right choice. For event-driven UIs, higher levels are fine since messages are infrequent.

- **Binary WebSocket messages**: use `Message::Binary` rather than `Message::Text`. Avoids UTF-8 validation overhead on both sides.

### 6. Prior Art

**ttyd** (C, libwebsockets + xterm.js)
- The most mature terminal-over-WebSocket tool. Simple protocol: single command byte prefix per message.
- Message types: `OUTPUT` (server->client terminal data), `INPUT` (client->server keystrokes), `RESIZE_TERMINAL` (client reports size), `SET_WINDOW_TITLE`, `SET_PREFERENCES`, `PAUSE`, `RESUME`.
- Spawns a PTY per connection, streams raw PTY output (ANSI sequences) to xterm.js on the client.
- Supports readonly mode, authentication, TLS, max clients, ping interval.
- No diff computation; sends raw PTY output bytes.
- [GitHub](https://github.com/tsl0922/ttyd)

**GoTTY** (Go, gorilla/websocket + xterm.js/hterm)
- Same architecture as ttyd: WebSocket relay between PTY and browser.
- Supports reconnection (`--reconnect` flag with configurable interval).
- Multi-client sharing via tmux: `gotty tmux new -A -s gotty top`.
- Read-only by default; `-w` flag enables client input.
- [GitHub](https://github.com/yudai/gotty)

**tmux Control Mode**
- tmux's `-C` flag outputs structured text (not ANSI) describing terminal changes. A client can parse these to reconstruct the display.
- Relevant concept: decoupling "what changed" from "how to render it."

**MUD Protocols (GMCP, MSDP)**
- GMCP (Generic MUD Communication Protocol) sends structured data (JSON) alongside the text stream via telnet subnegotiation (telnet option code 201).
- MSDP (MUD Server Data Protocol) defines typeless variables, arrays, and tables for out-of-band data.
- MSDP over GMCP allows using JSON format for MSDP-structured data.
- Relevant patterns: out-of-band data channels, client capability negotiation, event-based reporting (subscribe to variable changes).
- [GMCP Spec](https://tintin.mudhalla.net/protocols/gmcp/)

**SSH Terminal Forwarding**
- SSH forwards raw PTY I/O (stdin/stdout/stderr) plus out-of-band messages for window size changes (RFC 4254 window-change).
- Uses flow control via TCP and SSH's own windowing.
- Not directly applicable but the "channel" abstraction (data + control messages on the same connection) is a useful pattern.

**Browsh** (Go)
- Headless browser rendered to a terminal. Not directly relevant but demonstrates browser-to-terminal cell mapping.

### 7. Input Forwarding

Client keyboard and mouse events must be forwarded to the server.

**Keyboard:**
- Capture `keydown`/`keyup` events in JavaScript. Send the key code and modifier state (shift, ctrl, alt, meta).
- For terminal compatibility, convert to the byte sequence a terminal would send (e.g., Ctrl+C = 0x03, arrow keys = ESC sequences). This is complex; xterm.js handles it if you use Option B.
- For a game, send structured key events: `{ key: "ArrowUp", modifiers: ["shift"] }`. Simpler and more flexible.

**Mouse:**
- Capture `mousedown`, `mouseup`, `mousemove`, `wheel` events.
- Convert pixel coordinates to cell coordinates: `cell_x = floor(pixel_x / cell_width)`, `cell_y = floor(pixel_y / cell_height)`.
- Send: `{ type: "mouse_down", x: cell_x, y: cell_y, button: 0 }`.

**Message format:**
```
KeyInput:   [0x10] [key_code: u32] [modifiers: u8]
MouseInput: [0x11] [event_type: u8] [x: u16] [y: u16] [button: u8]
```

**Latency:** Input events are tiny (< 20 bytes) and infrequent relative to display data. They should be sent immediately (no batching).

### 8. Reconnection and State Synchronization

**On initial connect:**
1. Server sends `FullSnapshot`: the complete current buffer state (all cells, dimensions, cursor position).
2. Server begins streaming `CellDiff` messages from subsequent frames.

**On disconnect + reconnect:**
1. Client opens a new WebSocket.
2. Client sends a reconnect message (optionally with a session ID or sequence number).
3. Server sends a new `FullSnapshot`, then resumes diffs.

**Sequence numbers for robustness:**
- Each `CellDiff` message carries a monotonically increasing sequence number.
- The client tracks the last applied sequence number.
- On reconnect, the client sends its last sequence number. If the server still has buffered diffs since that sequence, it can send just those diffs instead of a full snapshot. Otherwise, full snapshot.

**Session management:**
- Assign a UUID per session on first connect.
- Store session state server-side (game state, buffer, sequence counter).
- On reconnect with a valid session ID, resume the game rather than restarting.
- Expire sessions after a timeout (e.g., 5 minutes without any connection).

### 9. Compression

**zstd (recommended)**
- Very fast compression/decompression. Level 1 adds negligible latency (~0.1ms for small payloads).
- Rust crate: `zstd` (wraps the C library). Provides `encode_all` / `decode_all` for simple use, or streaming `Encoder`/`Decoder` for larger data.
- Dictionary mode: pre-train a dictionary on typical cell diff payloads. Can improve compression ratio by 20-40% for small messages (< 1 KB) where zstd normally struggles.
- [Crate](https://crates.io/crates/zstd)

**Brotli**
- Better compression ratio than zstd at comparable speeds for text-heavy data.
- Rust crate: `brotli`.
- Higher decompression overhead than zstd in the browser (unless using `DecompressionStream` API, which supports brotli natively in modern browsers).

**WebSocket permessage-deflate**
- Built into the WebSocket protocol (RFC 7692). Compresses each message with zlib/deflate.
- Pros: transparent, no custom code needed. Both tungstenite and browsers support it.
- Cons: per-message overhead, no dictionary sharing across messages, and fastwebsockets does not support it yet.
- Not recommended for low-latency use due to per-frame overhead and inability to tune compression level per message.

**Delta compression**
- Since diffs are already a form of delta, further delta compression (e.g., sending "same as last diff but with these cells changed") has diminishing returns.
- For spectator mode, a single diff stream can be shared across all watchers (multicast pattern).

**Practical recommendation:** Apply zstd level 1 compression to each `CellDiff` binary message before sending. On the client, decompress with the JavaScript `zstd-codec` library or the `DecompressionStream` API. For messages under ~100 bytes (e.g., 1-2 cell changes), skip compression (the overhead exceeds the savings).

### 10. Multiplayer and Spectator Mode

**Single-player remote rendering (baseline):**
- One game instance, one WebSocket connection.
- Server owns all state. Client is a dumb display + input forwarder.

**Spectator mode:**
- Multiple WebSocket clients subscribe to the same game instance.
- Server computes diffs once per frame, then broadcasts the same serialized message to all connected spectators.
- Use `tokio::sync::broadcast` channel: one producer (game loop), N consumers (WebSocket tasks).
- New spectators receive a `FullSnapshot` on connect, then join the broadcast stream.
- Spectators are read-only (no input forwarding).

**Multiplayer (shared world):**
- Multiple players send input events; all receive the same world state diffs.
- Server must handle input ordering and conflict resolution (game-logic concern, not networking).
- Each player may have a different viewport (if the world is larger than one screen). In this case, each player gets their own diff stream based on their viewport position. This breaks the "broadcast one diff" optimization; each player's diff must be computed independently.

**Scaling considerations:**
- For <100 spectators, a single server broadcasting diffs is fine. A 2.4 KB diff at 30fps = ~72 KB/s per client = 7.2 MB/s for 100 clients.
- For larger audiences, consider a fan-out tier (WebSocket relay servers) or switch to WebRTC data channels for peer-assisted distribution.
- Rate limiting: cap the number of input messages per second per client to prevent abuse.

**Chat / out-of-band data:**
- GMCP's pattern is useful here: send structured JSON messages on the same WebSocket for chat, player stats, inventory, etc.
- Use the message type byte to distinguish game data from metadata channels.

## Sources

- Kept: [ttyd](https://github.com/tsl0922/ttyd) - Primary reference for terminal-over-WebSocket architecture and protocol design. Examined protocol.c source directly.
- Kept: [GoTTY](https://github.com/yudai/gotty) - Architecture documentation, reconnection support, multi-client sharing via tmux.
- Kept: [tokio-tungstenite](https://docs.rs/tokio-tungstenite) - Async WebSocket docs, performance notes, Nagle's algorithm control.
- Kept: [axum WebSocket](https://docs.rs/axum/latest/axum/extract/ws/) - WebSocket extractor API, concurrent read/write pattern.
- Kept: [fastwebsockets](https://docs.rs/fastwebsockets) - High-performance alternative, zero-copy frame parsing.
- Kept: [ratatui Buffer](https://docs.rs/ratatui/latest/ratatui/buffer/struct.Buffer.html) - Buffer diff mechanism, Cell structure, serialization support.
- Kept: [xterm.js flow control](https://xtermjs.org/docs/guides/flowcontrol/) - Watermark-based back-pressure, WebSocket flow control patterns.
- Kept: [GMCP Protocol](https://tintin.mudhalla.net/protocols/gmcp/) - MUD out-of-band data protocol, JSON over telnet, event-based reporting.
- Dropped: alacritty grid source - Alacritty's grid module was fetched but too large/unfocused; its renderer concepts are noted in the WebGL section.
- Dropped: mudbytes.net forum - HTTP 500, unavailable.
- Dropped: Wikipedia MUD protocol - 404, does not exist.

## Gaps

1. **Benchmarks for zstd on cell diff payloads**: No direct benchmarks found for compressing terminal cell data specifically. Would need to profile with real game output to determine optimal compression level and minimum message size threshold for compression.

2. **WebGL font atlas in browser**: Detailed implementation references for WebGL-based monospace grid rendering in JavaScript were not found in this search pass. The concept is well-understood (used by alacritty, wezterm natively), but a JavaScript/WebGL tutorial or library specifically for this would help implementation.

3. **Browser zstd decompression performance**: The `zstd-codec` npm package wraps the C library via WASM. Decompression latency in the browser for small messages (~1-5 KB) was not benchmarked. The `DecompressionStream` API (native browser) supports deflate and gzip but not zstd as of 2025 in all browsers.

4. **WebTransport as alternative to WebSocket**: WebTransport (HTTP/3-based) offers lower latency (no head-of-line blocking) and unreliable datagrams. Could be a better transport for real-time game streaming but has narrower browser support. Worth investigating as a future upgrade path.

5. **Security**: Authentication, rate limiting, and origin validation patterns for game WebSocket servers were not deeply researched. ttyd's approach (basic auth, origin checking) is a starting point.