# Terminal Graphics Protocols for Pixel-Level Rendering

Research for implementing a Sixel/Kitty/iTerm2 graphics backend in a Rust terminal/grid rendering
library, enabling pixel-level graphics within a real terminal emulator.

## Summary

Three major protocols exist for inline terminal graphics: Sixel (1980s DEC standard, widest
support), Kitty Graphics Protocol (modern, feature-rich, growing adoption), and iTerm2 Inline Images
(macOS-focused, simple). A practical implementation should support all three with runtime detection
and automatic fallback. The Rust ecosystem has several crates for sixel encoding (`icy_sixel` for
pure Rust, `sixel-image`/`sixel-tokenizer` from Zellij for manipulation), but Kitty and iTerm2
protocol output is simple enough to implement directly. notcurses provides the best reference
implementation for detection and fallback logic.

---

## 1. Sixel Graphics Protocol

### How it works

Sixel ("six pixels") is a bitmap format from DEC's VT200/VT300 series (1980s). A "sixel" is a column
of 6 vertical pixels encoded as a single printable ASCII character. The protocol embeds graphics
data inline in the terminal output stream via a Device Control String (DCS).

### Escape sequence structure

```text
DCS P1 ; P2 ; P3 ; q <sixel-data> ST
```yaml

Where:

- **DCS** = `ESC P` (7-bit) or `0x90` (8-bit) - introduces the sixel sequence
- **P1** = macro parameter (pixel aspect ratio, typically 0 for default 2:1)
- **P2** = background drawing mode: `0`/`2` = fill background with color, `1` = leave pixels at

  current color

- **P3** = horizontal grid size (ignored by most terminals)
- **q** = indicates this is a sixel command
- **ST** = `ESC \` (string terminator)

### Sixel data encoding

Each sixel character maps to 6 vertical pixels. Characters range from `?` (0x3F, binary 000000) to
`~` (0x7E, binary 111111). The character code minus 0x3F gives the 6-bit pixel pattern, with the
least significant bit at the top.

Example: character `t` (0x74) = binary 110101 = pixels on at positions 0, 2, 4 (top, third, fifth).

### Color registers

Colors are defined inline using the `#` (color introducer) command:

```shell
# Pc ; Pu ; Px ; Py ; Pz

```text

- **Pc** = color register number (0-255)
- **Pu** = coordinate system: `1` = HLS, `2` = RGB
- **Px/Py/Pz** = color values (RGB: 0-100% for each channel; HLS: 0-360 hue, 0-100 lightness, 0-100

  saturation)

To select a previously defined color: `# Pc` (just the number, no coordinates).

### Raster attributes

The `"` command sets image dimensions and aspect ratio:

```text
" Pan ; Pad ; Ph ; Pv
```text

Where Pan:Pad is the pixel aspect ratio, Ph is width in pixels, Pv is height.

### Row control

- `$` (Graphics Carriage Return) - return to left edge of current sixel row
- `-` (Graphics New Line) - advance to next sixel row (6 pixels down)
- `!Pn<char>` (Repeat) - repeat the next character Pn times (run-length compression)

### Encoding example

```text
\x1bPq              # DCS, start sixel
"2;1;100;200         # Raster: aspect 2:1, 100x200 pixels
#0;2;0;0;0           # Color 0: black (RGB 0,0,0)
#1;2;100;100;0       # Color 1: yellow (RGB 100%,100%,0)
#2;2;0;100;0         # Color 2: green (RGB 0,100%,0)
#1~~@@vv@@~~@@~~$    # Draw in yellow, then carriage return
#2??}}GG}}??}}??-    # Draw in green, then new line
#1!14@               # Repeat '@' 14 times in yellow
\x1b\                # ST, end sixel
```text

Key design points:

- Colors are drawn one at a time per row using `$` to overprint
- Run-length encoding (`!`) provides compression
- Up to 256 color registers (many terminals support fewer)
- No alpha channel support in the original spec

[Source: VT330/VT340 Programmer Reference Manual, Chapter 14](https://vt100.net/docs/vt3xx-gp/chapter14.html)

---

## 2. Kitty Graphics Protocol

### Overview

Designed by Kovid Goyal for the Kitty terminal emulator, this is the most feature-complete terminal
graphics protocol. It uses APC (Application Programming Command) escape sequences and supports
modern features like alpha blending, z-ordering, animations, and Unicode placeholders.

### Escape sequence structure (2)

```text
<ESC>_G<control-data>;<payload><ESC>\
```text

- Control data: comma-separated `key=value` pairs
- Payload: base64-encoded binary data

### Image data formats

| `f` value | Format                               |
| --------- | ------------------------------------ |
| `24`      | 24-bit RGB (3 bytes/pixel)           |
| `32`      | 32-bit RGBA (4 bytes/pixel, default) |
| `100`     | PNG data                             |

Image dimensions are specified with `s` (width) and `v` (height) for raw formats. PNG dimensions are
read from the data itself.

### Transmission methods

| `t` value | Method                         | Use case                      |
| --------- | ------------------------------ | ----------------------------- |
| `d`       | Direct (inline in escape code) | Remote/SSH, default           |
| `f`       | File path                      | Local, avoids base64 overhead |
| `t`       | Temporary file (auto-deleted)  | Local, one-shot               |
| `s`       | Shared memory (POSIX shm)      | Local, highest performance    |

### Chunked transfer (remote/SSH)

Large images are base64-encoded and split into chunks of at most 4096 bytes:

```text
<ESC>_Gs=100,v=30,m=1;<chunk 1><ESC>\
<ESC>_Gm=1;<chunk 2><ESC>\
<ESC>_Gm=0;<final chunk><ESC>\
```text

Only the first chunk carries the full metadata. `m=1` means more data follows; `m=0` marks the final
chunk.

### Actions (`a` key)

| Value | Action                                     |
| ----- | ------------------------------------------ |
| `t`   | Transmit data (default)                    |
| `T`   | Transmit and display                       |
| `q`   | Query support                              |
| `p`   | Put (display previously transmitted image) |
| `d`   | Delete image(s)                            |
| `f`   | Transmit animation frame                   |
| `a`   | Control animation                          |
| `c`   | Compose animation frames                   |

### Placement and display

Images are placed at the current cursor position. Key display controls:

- `x,y,w,h` - source rectangle (which part of image to show)
- `c,r` - display size in columns/rows (terminal scales the image)
- `X,Y` - pixel offset within the first cell
- `z` - z-index (negative = behind text, enabling text-over-image)
- `i` - image ID (for reuse/management)
- `p` - placement ID (multiple placements of same image)
- `C=1` - don't move cursor after placement

### Compression

ZLIB deflate compression with `o=z`, applied before base64 encoding.

### Unicode placeholders

A special Unicode character `U+10EEEE` serves as a placeholder. This enables pixel graphics in
applications that know nothing about the protocol (vim, tmux, etc.) by encoding image ID in the
foreground color and row/column in diacritics:

```shell
# Create virtual placement

<ESC>_Ga=p,U=1,i=<id>,c=<cols>,r=<rows><ESC>\

# Display via Unicode placeholder

printf "\e[38;5;42m\U10EEEE\U0305\U0305\U10EEEE\U0305\U030D\e[39m\n"
```text

### Animation support

- Frame data transmitted with `a=f`
- Frames can be full images or deltas (partial rectangles)
- Terminal-driven animation with configurable gaps between frames (`z` key = milliseconds)
- Animation control: `s=1` stop, `s=2` loading mode, `s=3` run
- Frame composition: overlay rectangles from one frame onto another

### Querying support

Send a query action followed by a primary device attributes request:

```text
<ESC>_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA<ESC>\<ESC>[c
```text

If the terminal responds to the graphics query, it supports the protocol. If only the DA response
comes back, it doesn't.

### Terminals supporting Kitty graphics protocol

Ghostty, Konsole, WezTerm, iTerm2, Warp, wayst, st (with patch), xterm.js, and of course Kitty
itself.

[Source: Kitty Graphics Protocol Specification](https://sw.kovidgoyal.net/kitty/graphics-protocol/)

---

## 3. iTerm2 Inline Images Protocol

### Escape sequence structure (3)

```text
ESC ] 1337 ; File = [args] : <base64-data> BEL
```rust

Or with ST terminator: `ESC ] 1337 ; File = [args] : <base64-data> ESC \`

Arguments are semicolon-separated `key=value` pairs:

| Key                   | Description                              |
| --------------------- | ---------------------------------------- |
| `name`                | Base64-encoded filename                  |
| `size`                | File size in bytes (for progress)        |
| `width`               | Display width (N, Npx, N%, or "auto")    |
| `height`              | Display height (N, Npx, N%, or "auto")   |
| `preserveAspectRatio` | 0 = stretch, 1 = preserve (default)      |
| `inline`              | 0 = download to file, 1 = display inline |

### Width/height units

- `N` - N character cells
- `Npx` - N pixels
- `N%` - N percent of session width/height
- `auto` - use image's inherent size

### Multipart transfer (iTerm2 3.5+)

For tmux integration and large files:

```text
ESC ] 1337 ; MultipartFile = [args] BEL
ESC ] 1337 ; FilePart = <base64 chunk> BEL    # repeat
ESC ] 1337 ; FileEnd BEL
```text

Chunk size limit: 1,048,576 bytes (newer tmux), 256 bytes (older tmux).

### Key differences from Kitty

- Simpler protocol, fewer features
- Sends entire image files (PNG, GIF, etc.) rather than raw pixels; the terminal handles decoding
- No z-ordering, no alpha blending with text
- No animation control (but supports animated GIFs natively)
- No image IDs/reuse mechanism
- Retina display support (since iTerm2 3.2.0)

[Source: iTerm2 Inline Images Documentation](https://iterm2.com/documentation-images.html)

---

## 4. Terminal Support Matrix

### Sixel support

### Supported

- xterm (default since patch 359)
- foot, mlterm, WezTerm, mintty
- Konsole (since 22.04), iTerm2 (since 3.3.0)
- Contour, DomTerm, Bobcat, MacTerm
- VS Code terminal (via xterm-addon-image, since 1.80)
- tmux (with `--enable-sixel`)
- Zellij (since 0.31.0)
- Xfce Terminal, Yakuake

### Unsupported

- Kitty (deliberately; uses its own protocol)
- Alacritty (open issue #910)
- GNOME Terminal / VTE-based terminals (blocked on VTE #253)
- Windows Terminal (open issue #448)
- macOS Terminal.app

[Source: arewesixelyet.com](https://arewesixelyet.com/)

### Kitty graphics protocol support

**Supported:** Kitty, Ghostty, WezTerm, Konsole, iTerm2, Warp, wayst, st (patched), xterm.js

### iTerm2 protocol support

**Supported:** iTerm2, WezTerm (partial), mintty

### Multi-protocol terminals

| Terminal       | Sixel | Kitty | iTerm2  |
| -------------- | ----- | ----- | ------- |
| WezTerm        | Yes   | Yes   | Partial |
| iTerm2         | Yes   | Yes   | Yes     |
| Konsole        | Yes   | Yes   | No      |
| Kitty          | No    | Yes   | No      |
| Ghostty        | No    | Yes   | No      |
| foot           | Yes   | No    | No      |
| xterm          | Yes   | No    | No      |
| Alacritty      | No    | No    | No      |
| GNOME Terminal | No    | No    | No      |

---

## 5. notcurses: Detection and Fallback (NCBLIT_PIXEL)

### Detection mechanism

notcurses probes terminal capabilities at initialization and exposes the result via
`notcurses_check_pixel_support()`, which returns an `ncpixelimpl_e` enum:

```c
typedef enum {
  NCPIXEL_NONE = 0,
  NCPIXEL_SIXEL,
  NCPIXEL_LINUXFB,         // Linux framebuffer (direct pixel access)
  NCPIXEL_ITERM2,
  NCPIXEL_KITTY_STATIC,    // Kitty < 0.20.0 (no C=1, full redraw required)
  NCPIXEL_KITTY_ANIMATED,  // Kitty 0.20.0-0.21.x (C=1 but no self-ref composition)
  NCPIXEL_KITTY_SELFREF,   // Kitty >= 0.22.0 (a=c for self-referential composition)
} ncpixelimpl_e;
```

### Detection strategy

1. Check `TERM`/`TERM_PROGRAM` environment variables for known terminals
2. Send Kitty graphics query (`a=q`) followed by DA1 (`ESC[c`)
3. Check for Sixel support via DA1 response (attribute 4 in response)
4. Fall back to checking TIOCGWINSZ for pixel dimensions (needed for any bitmap protocol)

### Blitter hierarchy with fallback

notcurses uses `ncblitter_e` to select rendering strategy:

```text
NCBLIT_PIXEL   → Sixel/Kitty/iTerm2 bitmaps (best quality)
NCBLIT_BRAILLE → 4x2 pixels per cell via Braille characters
NCBLIT_3x2     → Unicode sextants (3x2 per cell)
NCBLIT_2x2     → Unicode quadrants (2x2 per cell)
NCBLIT_2x1     → Half blocks ▀▄ (2x1 per cell, default)
NCBLIT_1x1     → Space + background color (1x1, ASCII-safe)
```text

When `NCBLIT_DEFAULT` is requested, notcurses auto-selects the best available blitter (but never
auto-selects `NCBLIT_PIXEL` - that must be explicit). If `NCBLIT_PIXEL` is requested but not
available, it degrades to the next best unless `NCVISUAL_OPTION_NODEGRADE` is set.

### Cell integration (sprixels)

notcurses tracks bitmap images as "sprixels" (sprite + pixel). Each sprixel:

- Occupies a rectangular region of cells on an ncplane
- Has a z-index for layering with text
- Cells underneath the sprixel are marked as occupied
- When text needs to overwrite a sprixel cell, notcurses "damages" that cell (making it transparent

  in the bitmap or rewriting it)

For Kitty's `NCPIXEL_KITTY_SELFREF` mode, only transparent cells need rewriting (using
self-referential composition `a=c`). For older Kitty or Sixel, the entire image may need to be
re-emitted.

### Pixel geometry query

```c
void ncplane_pixel_geom(struct ncplane* n,
    unsigned* pxy, unsigned* pxx,        // display region in pixels
    unsigned* celldimy, unsigned* celldimx, // cell size in pixels
    unsigned* maxbmapy, unsigned* maxbmapx); // max bitmap size
```

[Source: notcurses USAGE.md](https://github.com/dankamongmen/notcurses/blob/master/USAGE.md)

---

## 6. Rust Crates for Sixel Encoding

### icy_sixel (pure Rust, recommended)

- **Repo:** [github.com/mkrueger/icy_sixel](https://github.com/mkrueger/icy_sixel)
- **Crate:** `icy_sixel`
- **License:** MIT / Apache 2.0
- 100% pure Rust, no C dependencies
- High-quality color quantization (Wu's algorithm + Floyd-Steinberg dithering)
- SIMD-accelerated decoder
- Full encode/decode support
- CLI tool included (`icy_sixel-cli`)
- Active maintenance

```rust
use icy_sixel::{sixel_encode, EncodeOptions};
let rgba = vec![255, 0, 0, 255]; // Red pixel
let sixel = sixel_encode(&rgba, 1, 1, &EncodeOptions::default())?;
print!("{}", sixel);
```

### sixel-rs (libsixel wrapper)

- **Repo:** [github.com/orhun/sixel-rs](https://github.com/orhun/sixel-rs) (active fork)
- **Crate:** `sixel-rs` (56k downloads)
- Safe Rust wrapper around libsixel (C library)
- Requires libsixel system dependency
- Good quality output (leverages battle-tested C code)
- Less suitable for pure Rust builds or cross-compilation

### sixel-image + sixel-tokenizer (Zellij ecosystem)

- **Repo:** [github.com/zellij-org/sixel-image](https://github.com/zellij-org/sixel-image)
- **Crate:** `sixel-image` (236k downloads), `sixel-tokenizer`
- Purpose: parsing and manipulating existing sixel data, not encoding from pixels
- Used by Zellij terminal multiplexer for sixel passthrough
- Supports streaming parse ("on the wire") and batch parse
- Serialize/deserialize sixel data, crop, resize sixel images

### Recommendation for a rendering library

Use `icy_sixel` for encoding RGBA pixel buffers to sixel output. It's pure Rust, actively
maintained, and handles quantization/dithering well. For Kitty and iTerm2 protocols, implement the
escape sequences directly (they're simple base64 wrappers around PNG or raw RGBA data; no complex
encoding like sixel).

---

## 7. Integration with Cell-Based Rendering

### The fundamental challenge

Terminal graphics operate in two coordinate systems simultaneously:

1. **Cell grid** - character positions (columns x rows)
2. **Pixel grid** - individual pixels within cells

A cell typically spans multiple pixels (e.g., 8x16 pixels for a common font). Graphics must align to
cell boundaries to avoid visual artifacts.

### Cell size discovery

```rust
// POSIX: TIOCGWINSZ ioctl
use libc::{ioctl, winsize, TIOCGWINSZ};
let mut ws: winsize = unsafe { std::mem::zeroed() };
unsafe { ioctl(fd, TIOCGWINSZ, &mut ws) };
let cell_width = ws.ws_xpixel / ws.ws_col;
let cell_height = ws.ws_ypixel / ws.ws_row;

// Or via escape sequence:
// Send: ESC[16t
// Receive: ESC[6;<height>;<width>t (pixel size of a single cell)
```

### Mixing text and pixel regions

Three strategies:

### Strategy 1: Dedicated pixel planes (notcurses approach)

Reserve specific rectangular regions of the cell grid for pixel content. Text rendering skips these
cells. The pixel image is emitted as a single escape sequence occupying N columns x M rows. This is
what notcurses "sprixels" do.

Pros: clean separation, no flicker. Cons: pixel regions must be cell-aligned.

### Strategy 2: Z-ordered overlays (Kitty-specific)

Kitty's z-index support allows placing images behind text (`z` < 0). Text renders normally on top
with transparency. This enables pixel backgrounds with text overlays.

```text
<ESC>_Ga=T,f=100,z=-1;<base64 PNG><ESC>\  # Image behind text
```yaml

Pros: text and graphics truly overlap. Cons: Kitty-only, not supported by Sixel or iTerm2.

### Strategy 3: Unicode placeholder composition (Kitty)

Use the `U+10EEEE` placeholder character to mark cells where images should appear. The terminal
composites the image data with the placeholder positions. Works through tmux, vim, and other
intermediaries.

### Damage tracking for sprixels

When text overwrites cells occupied by a bitmap:

- **Kitty (selfref):** Compose a transparent rectangle over the affected cells using `a=c`
- **Kitty (static/animated):** Re-transmit the entire image with the damaged cells made transparent
- **Sixel:** Re-emit the entire sixel sequence (sixel has no partial update mechanism)
- **iTerm2:** Re-emit the entire image

This means the rendering engine must track which cells are "sprixel-occupied" and handle damage
propagation.

---

## 8. Performance Considerations

### Encoding overhead

| Protocol         | Encoding cost                                                                         | Bandwidth                                 |
| ---------------- | ------------------------------------------------------------------------------------- | ----------------------------------------- |
| Sixel            | **High** - color quantization (256 colors max), dithering, per-color-per-row encoding | Moderate (RLE compression helps)          |
| Kitty (PNG)      | Moderate - PNG compression + base64                                                   | Good (PNG is compact)                     |
| Kitty (raw)      | Low - just base64                                                                     | High (uncompressed RGBA + base64 = 1.33x) |
| Kitty (raw+zlib) | Moderate - zlib + base64                                                              | Good                                      |
| Kitty (file/shm) | **Minimal** - no encoding                                                             | Best (zero-copy for local)                |
| iTerm2           | Depends on image format                                                               | Good (sends compressed file)              |

### Bandwidth analysis (1920x1080 image)

- Raw RGBA: 1920 _1080_ 4 = ~8.3 MB
- Base64 of raw: ~11.1 MB
- Base64 of zlib-compressed: ~2-4 MB (varies with content)
- Base64 of PNG: ~0.5-3 MB (varies with content)
- Sixel output: ~1-5 MB (depends on color complexity, RLE effectiveness)
- Kitty shared memory: 0 bytes over the wire (just the path)

### Key performance strategies

1. **Use shared memory / temp files when local.** Kitty's `t=s` (shared memory) and `t=t` (temp

   file) avoid all serialization overhead. The terminal reads pixels directly. This is the single
   biggest optimization for local rendering.

1. **Send PNG format when possible.** For Kitty, `f=100` sends PNG data, which is far more compact

   than raw pixels for most images. The terminal decompresses on its end.

1. **Use zlib compression for raw data.** Kitty's `o=z` compresses raw RGBA before base64, reducing

   bandwidth significantly.

1. **Minimize re-transmission.** Kitty's image ID system allows placing the same image multiple

   times without re-sending data. For animations, send only changed rectangles.

1. **Sixel-specific optimizations:**
   - Reduce color count (fewer colors = fewer passes per row)
   - Maximize RLE runs (sort colors to produce longer runs)
   - Consider image downscaling before encoding
   - Pre-quantize to the target palette

1. **Chunked transmission.** Both Kitty (4096-byte chunks) and iTerm2 (MultipartFile) support

   chunked transfer, preventing the terminal from blocking on large images.

### Frame rate considerations

For animated content at 30+ FPS:

- Sixel is generally too slow (full re-encode per frame)
- Kitty with self-referential composition can update sub-regions efficiently
- Kitty shared memory + placement updates can achieve real-time video (mpv uses this)
- iTerm2 supports animated GIFs natively but not programmatic animation

---

## 9. Runtime Terminal Detection

### Detection algorithm

```rust
fn detect_graphics_protocol() -> GraphicsProtocol {
    // 1. Check environment variables for known terminals
    let term = std::env::var("TERM").unwrap_or_default();
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();

    // Fast path for known terminals
    match term_program.as_str() {
        "iTerm.app" => return GraphicsProtocol::ITerm2,
        // But still prefer Kitty protocol if available
        _ => {}
    }

    // 2. Query Kitty graphics protocol support
    //    Send: ESC_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA ESC\  ESC[c
    //    If graphics response comes back: Kitty supported
    //    If only DA1 response: Kitty not supported
    if query_kitty_support() {
        return GraphicsProtocol::Kitty;
    }

    // 3. Check DA1 response for Sixel support (attribute "4")
    //    Send: ESC[c
    //    Response: ESC[?62;4;... c  (4 = sixel)
    if da1_has_sixel() {
        return GraphicsProtocol::Sixel;
    }

    // 4. Check for iTerm2 via TERM_PROGRAM or custom escape
    if term_program == "iTerm.app" {
        return GraphicsProtocol::ITerm2;
    }

    // 5. Fallback to character-based rendering
    GraphicsProtocol::None
}
```

### Detecting pixel dimensions

```rust
fn get_cell_pixel_size() -> Option<(u16, u16)> {
    // Method 1: TIOCGWINSZ ioctl
    // Returns ws_xpixel and ws_ypixel (some terminals return 0)

    // Method 2: CSI 16t escape
    // Send: ESC[16t
    // Response: ESC[6;<cell_height>;<cell_width>t

    // Method 3: CSI 14t escape
    // Send: ESC[14t
    // Response: ESC[4;<window_height>;<window_width>t
    // Then divide by rows/cols from TIOCGWINSZ
}
```

### Checking for tmux/screen

If running inside tmux or screen, graphics protocol support depends on both the multiplexer and the
outer terminal. tmux 3.4+ supports sixel passthrough with `--enable-sixel`. Kitty's Unicode
placeholder method works through tmux. Direct Kitty protocol does not work through tmux without
passthrough support.

### XTVERSION for terminal identification

```yaml
Send: ESC[>0q
Response: DCS >|<terminal name and version> ST
```rust

Supported by Kitty, foot, WezTerm, and others. Gives precise terminal identity.

---

## 10. Trade-offs vs. Windowed GPU Rendering

| Aspect             | Terminal Graphics                                                       | Windowed GPU (wgpu/OpenGL)                         |
| ------------------ | ----------------------------------------------------------------------- | -------------------------------------------------- |
| **Resolution**     | Limited by cell grid alignment; max bitmap sizes vary by terminal       | Arbitrary resolution, sub-pixel rendering          |
| **Color depth**    | Sixel: 256 colors. Kitty/iTerm2: full 32-bit RGBA                       | Full 32-bit, HDR possible                          |
| **Refresh rate**   | Sixel: ~1-5 FPS for full screen. Kitty: up to 30+ FPS with optimization | 60+ FPS trivially                                  |
| **Compositing**    | Kitty z-index only; no shader effects                                   | Arbitrary shaders, blending modes                  |
| **Text rendering** | Terminal handles text natively (best quality)                           | Must implement or use a text rendering library     |
| **Input handling** | Terminal handles input, mouse, resize                                   | Must implement from scratch (winit, etc.)          |
| **Deployment**     | Works over SSH, in tmux, on headless servers                            | Requires display server (X11/Wayland/macOS)        |
| **Dependencies**   | Zero (just stdout)                                                      | GPU drivers, windowing library, shader compilation |
| **Portability**    | Any terminal with protocol support                                      | Any OS with GPU support                            |
| **Startup time**   | Instant (already in terminal)                                           | Window creation + GPU init overhead                |
| **UI integration** | Lives within terminal workflow                                          | Separate window, context switch                    |
| **Scrollback**     | Images scroll with terminal history                                     | No scrollback (or must implement)                  |
| **Accessibility**  | Terminal screen readers may handle text cells                           | Must implement accessibility layer                 |

### When to use terminal graphics

- TUI applications that need occasional image display (file managers, image viewers)
- Developer tools that want charts/graphs inline with text output
- Applications that must work over SSH
- Programs that want to stay within the terminal workflow

### When to use GPU rendering

- High frame rate requirements (games, real-time visualization)
- Complex compositing or shader effects
- Applications that need precise pixel-level control
- When terminal compatibility isn't a concern

### Hybrid approach

The most practical strategy for a grid rendering library: offer both backends. Use terminal graphics
as the default (zero dependencies, works everywhere), with an optional GPU backend for applications
that need higher performance. The grid/cell abstraction stays the same; only the output backend
changes.

---

## Sources

### Kept

- **VT330/VT340 Programmer Reference Manual, Chapter 14** (vt100.net/docs/vt3xx-gp/chapter14.html) -

  Original DEC sixel specification; authoritative for the wire format

- **Kitty Graphics Protocol** (sw.kovidgoyal.net/kitty/graphics-protocol/) - Official specification;

  comprehensive coverage of all features including animation

- **iTerm2 Images Documentation** (iterm2.com/documentation-images.html) - Official iTerm2 inline

  images spec

- **Are We Sixel Yet?** (arewesixelyet.com) - Comprehensive sixel terminal support matrix with links

  to patches/issues

- **notcurses USAGE.md** (github.com/dankamongmen/notcurses) - Reference implementation for

  detection, fallback, and sprixel management

- **icy_sixel** (github.com/mkrueger/icy_sixel) - Pure Rust sixel encoder/decoder with SIMD

  acceleration

- **sixel-image** (github.com/zellij-org/sixel-image) - Zellij's sixel manipulation library with

  streaming parser

- **sixel-rs** (github.com/orhun/sixel-rs) - Rust wrapper for libsixel

### Dropped

- Generic Wikipedia articles on sixel - superseded by the VT300 spec
- Blog posts about "displaying images in terminal" - redundant with protocol specs
- notcurses pixel-graphics wiki page (404) - content appears to be in USAGE.md now

---

## Gaps

1. **Ghostty graphics protocol details.** Ghostty supports Kitty protocol but documentation on its

   specific implementation limits (max image size, number of stored images) was not found in this
   research. Worth checking Ghostty's docs directly.

1. **Alacritty sixel/graphics status.** The PR (#4763) was noted but its current merge status is

   unclear. Alacritty may have gained support since this research.

1. **Benchmark data for sixel encoding.** No concrete benchmarks comparing icy_sixel vs. libsixel

   encoding speed were found. The SIMD acceleration claim from icy_sixel is for the decoder, not
   encoder.

1. **tmux passthrough for Kitty protocol.** tmux 3.4+ may support Kitty protocol passthrough via

   `allow-passthrough`, but the exact behavior and limitations need verification.

1. **Windows Terminal graphics support.** Windows Terminal has an open issue (#448) for sixel but

   the timeline and any Kitty protocol plans are unclear.

1. **Concrete bandwidth measurements.** The bandwidth estimates in section 8 are theoretical

   calculations, not measured. Real-world performance depends on terminal rendering speed, not just
   wire bandwidth.
