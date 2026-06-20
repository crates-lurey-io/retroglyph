# BearLibTerminal Complete API Reference

Sources: [Official Reference](http://foo.wyrd.name/en:bearlibterminal:reference),
[Configuration Reference](http://foo.wyrd.name/en:bearlibterminal:reference:configuration),
[Input Reference](http://foo.wyrd.name/en:bearlibterminal:reference:input),
[Design Overview](http://foo.wyrd.name/en:bearlibterminal:design),
[C Header](https://github.com/tommyettinger/BearLibTerminal/blob/master/Terminal/Include/C/BearLibTerminal.h)

---

## 1. Architecture Overview

BearLibTerminal provides a pseudoterminal window with a grid of character cells. It is NOT a
stream-based console; it is a scene-based rendering system.

**Core model:**- A**scene**is composed of multiple**layers** (0-255), each a full grid of cells.

- Each **cell**can hold a stack of**tiles** (when composition is on).
- Each tile in a stack has its own foreground color and pixel offset.
- Only layer 0 has a background color per cell. Layers 1+ are transparent.
- The scene is double-buffered: `put`/`print` modify the back buffer; `refresh` commits it to

  screen.

- Drawing order is fixed: left-to-right, top-to-bottom, layer 0 first, then layer 1, etc.
- All tile codes are Unicode code points in the Basic Multilingual Plane (~65k slots).
- Colors are 32-bit BGRA in 0xAARRGGBB format.
- Rendering uses OpenGL.

**Types:**

```c
typedef uint32_t color_t;  // 0xAARRGGBB

typedef struct dimensions_t_ {
    int width;
    int height;
} dimensions_t;
```

---

## 2. Initialization and Configuration

### terminal_open

```c
int terminal_open();
```

Initializes BearLibTerminal. Configures the window with defaults:

- 80x25 cells
- Fixedsys Excelsior font
- White text on black background

The window is NOT shown until the first call to `terminal_refresh()`.

**Returns:** boolean (non-zero = success, 0 = failure). If initialization fails, details are in the
log file (default: `bearlibterminal.log`).

Until `terminal_open()` succeeds, all other library functions are no-ops that return error codes.

### terminal_close

```c
void terminal_close();
```

Closes the window and deinitializes the library. Symmetric to `terminal_open()`.

### terminal_set

```c
int terminal_set(const char* s);
```

The primary configuration function. Handles library options, font/tileset management, and
configuration file access.

### Returns:**boolean. On failure, no changes are applied (transaction semantics).**Variants

- `terminal_setf(const char* s, ...)` -- printf-style formatting
- `terminal_wset(const wchar_t* s)` -- wide string
- `terminal_wsetf(const wchar_t* s, ...)` -- wide printf-style

See [Section 3: Configuration String Language](#3-configuration-string-language) for full details.

### terminal_get

```c
const char* terminal_get(const char* key, const char* default_);
```

Retrieves the value of a library option or configuration file property. Returns `default_` if not
found.

Memory of the returned string is managed by the library and remains valid until the next
`terminal_get` call with the same key. Always returns a valid C string (empty string if no option
and no default).

**Variants:** `terminal_wget(const wchar_t* key, const wchar_t* default_)`

C++ template version can parse into arbitrary types:

```cpp
template<typename T, typename C>
T terminal_get(const C* key, const T& default_ = T());
// Uses std::basic_istringstream to parse the result string
```

---

## 3. Configuration String Language

### Format

Semicolon-separated key-value parameters:

```text
window.title='foo'; window.size=80x25;
```text

Related parameters can be grouped:

```yaml
window: title='foo', size=80x25;
```yaml

Rules:

- Extra spaces, commas, and semicolons are ignored.
- Newlines are currently ignored (even in quoted values), but this may change.
- Quoting (single or double) is required only if value contains delimiters or to preserve

  whitespace.

- Escape: Pascal-style double-quote (`'I''m feeling lucky!'`).
- No backslash escaping.

### Library Options

#### terminal group

| Option              | Default | Description                                                                                |
| ------------------- | ------- | ------------------------------------------------------------------------------------------ |
| `terminal.encoding` | `utf8`  | Encoding/codepage for unibyte strings. Can be set to ANSI codepages (e.g. `Windows-1251`). |

#### window group

| Option              | Default           | Description                                                     |
| ------------------- | ----------------- | --------------------------------------------------------------- |
| `window.size`       | `80x25`           | Window size in cells.                                           |
| `window.cellsize`   | `auto`            | Cell size in pixels, or `auto` to derive from font.             |
| `window.title`      | `BearLibTerminal` | Window title.                                                   |
| `window.icon`       | (none)            | Path to `.ico` file for the window icon.                        |
| `window.resizeable` | `false`           | Allow user to resize the window. Generates `TK_RESIZED` events. |
| `window.fullscreen` | `false`           | Enable fullscreen mode.                                         |

#### input group

| Option                    | Default      | Description                                                                                                  |
| ------------------------- | ------------ | ------------------------------------------------------------------------------------------------------------ |
| `input.filter`            | `keyboard`   | List of events the application wants. Others are silently consumed. See [Input Filtering](#input-filtering). |
| `input.precise-mouse`     | `false`      | When `false`, `TK_MOUSE_MOVE` only fires on cell changes. When `true`, any pixel movement fires it.          |
| `input.mouse-cursor`      | `true`       | Show/hide the mouse cursor.                                                                                  |
| `input.cursor-symbol`     | `0x5F` (`_`) | Character used as cursor in `terminal_read_str`.                                                             |
| `input.cursor-blink-rate` | `500`        | Cursor blink rate in milliseconds for `terminal_read_str`.                                                   |
| `input.alt-functions`     | `true`       | Intercept Alt-key combos (Alt+Enter for fullscreen, Alt+Plus/Minus/Zero for zoom).                           |

#### output group

| Option                  | Default | Description                                                                   |
| ----------------------- | ------- | ----------------------------------------------------------------------------- |
| `output.postformatting` | `true`  | Enable processing of `[color=...]`, `[U+...]`, etc. tags in `terminal_print`. |
| `output.vsync`          | `true`  | Enable/disable OpenGL vertical sync.                                          |
| `output.tab-width`      | `4`     | Tab stop width in cells for `terminal_print`.                                 |

#### log group

| Option      | Default               | Description                                                               |
| ----------- | --------------------- | ------------------------------------------------------------------------- |
| `log.file`  | `bearlibterminal.log` | Log file path.                                                            |
| `log.level` | `error`               | Log level: `none`, `fatal`, `error`, `warning`, `info`, `debug`, `trace`. |
| `log.mode`  | `truncate`            | `truncate` = restart log each run. `append` = continue writing.           |

#### palette group

Custom named colors can be added:

```text
palette.octarine = #50FF25;
palette.lush = dark 80,255,37;
```text

These become usable in `color_from_name()` and `[color=octarine]` print tags.

### Font and Tileset Management

Format:

```text
[name ](font|offset): resource, param=value, ...;
```text

#### Name (optional)

An alternative font face name (e.g. `italic`, `runic`). Named fonts are accessed via `[font=name]`
tags in `terminal_print`:

```c
terminal_set("italic font: italic.ttf, size=12");
terminal_print(2, 1, "Its eyes are [font=italic]glowing[/font].");
```

The active font can also be selected programmatically:

```c
void terminal_font(const char* name);   // terminal_font8 internally
void terminal_wfont(const wchar_t* name);
```

#### Offset (code point)

A number specifying the starting Unicode code point:

- Single tile: `0x5E: circumflex.png;`
- Tileset: `0xE000: tileset.png, size=16x16;`

Tiles within a tileset are placed consecutively from the offset (row by row, left to right). Font
characters and custom tiles share the same Unicode code space. Recommended: use the Private Use Area
(U+E000-U+EFFF) for custom tilesets to avoid conflicts with text characters.

The special name `font` is equivalent to offset `0` (the default/main font).

#### Resource

Can be:

- **File path:** `UbuntuMono-R.ttf`, `tileset.png`
- **Memory buffer:** `address:size` format via `terminal_setf("font: %p:%d, ...", buffer, size)`
- **Raw BGRA pixels:** requires `raw-size` parameter:

  `terminal_setf("0xE000: %p:%d, raw-size=%dx%d", pixels, W*H*4, W, H)`

#### Bitmap Tileset Parameters

| Parameter       | Default       | Description                                                                                                                                        |
| --------------- | ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `size`          | (whole image) | Size of a single tile in pixels (e.g. `8x16`, `16x16`). If omitted, entire image = one sprite.                                                     |
| `resize`        | (none)        | Target size to resize tiles to.                                                                                                                    |
| `resize-filter` | `bilinear`    | `nearest`, `bilinear`, or `bicubic`.                                                                                                               |
| `resize-mode`   | `stretch`     | `stretch`, `fit`, or `crop`.                                                                                                                       |
| `raw-size`      | (none)        | Dimensions of a raw pixel array (required when loading from memory as BGRA).                                                                       |
| `codepage`      | `ascii`       | Codepage for mapping tile indices to Unicode slots. Built-in: `ascii`, `437`, `866`, `1250`, `1251`. Can also be a file path to a custom codepage. |
| `align`         | `center`      | Tile alignment within cell: `center`, `top-left`, `bottom-left`, `top-right`, `bottom-right`.                                                      |
| `spacing`       | `1x1`         | Alignment area in cells (e.g. `2x1` for a tile spanning 2 columns).                                                                                |
| `transparent`   | `auto`        | Background color for image formats lacking alpha.                                                                                                  |

#### TrueType Font Parameters

| Parameter            | Default    | Description                                                                                                                                                       |
| -------------------- | ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `size`               | (required) | Either a single number (average lowercase height in px, e.g. `size=12`) or explicit dimensions (e.g. `size=8x16`, library picks the largest font size that fits). |
| `size-reference`     | `@`        | Character used for size probing.                                                                                                                                  |
| `mode`               | `normal`   | Rasterization: `monochrome`, `normal`, or `lcd`. Note: `lcd` forces an opaque black background.                                                                   |
| `codepage`           | (none)     | **Reverse** codepage: maps Unicode to relative indices, used to load a select subset of glyphs to consecutive slots.                                              |
| `align`              | `center`   | Same as bitmap.                                                                                                                                                   |
| `spacing`            | `1x1`      | Same as bitmap.                                                                                                                                                   |
| `use-box-drawing`    | `false`    | Use font's Box Drawing chars instead of auto-generating them.                                                                                                     |
| `use-block-elements` | `false`    | Use font's Block Element chars instead of auto-generating them.                                                                                                   |
| `hinting`            | `normal`   | `normal` (font's native hinter), `autohint` (FreeType autohinter), or `none`.                                                                                     |

#### Removing a Tileset

```yaml
0xE000: none;
```text

All relevant options for a tileset must be specified in a single `terminal_set` call; they are
replaced wholesale.

### Configuration File

BearLibTerminal searches for an INI file at startup. It checks:

1. A file named after the application executable
2. The first available `.ini` file in CWD, then the executable directory

The `[BearLibTerminal]` section provides initial configuration. Logging options are applied first,
then other groups are applied individually.

INI format rules:

- Comments: `#` or `;`. Lines starting with whitespace are also treated as comments.
- Section/property names are case-insensitive.
- Duplicate (reopened) sections are allowed.
- No backslash escaping (`\n` is two literal characters).
- Whitespace around names/values is trimmed.
- A property line may contain grouped properties (same format as `terminal_set` strings).

Modifying the config file from code:

```c
terminal_set("ini.game.tile-size = 16");  // sets [Game] tile-size=16
// Empty value removes the property
```

---

## 4. Output Functions

### terminal_clear

```c
void terminal_clear();
```

Clears the entire scene (ALL layers). Sets every cell's background to the current background color.

### terminal_clear_area

```c
void terminal_clear_area(int x, int y, int w, int h);
```

Clears a rectangular area on the CURRENT layer only. On layer 0, also sets background color of
affected cells to the current background color.

### terminal_crop

```c
void terminal_crop(int x, int y, int w, int h);
```

Sets a crop area for the current layer. Dimensions are in cells. Anything outside the crop rectangle
on this layer is not rendered. Disabled by setting width or height to 0, or by calling
`terminal_clear()` (which resets all layers' crops).

### terminal_refresh

```c
void terminal_refresh();
```

Commits the scene (back buffer) to the screen (front buffer). This is when rendering actually
happens.

The first call after `terminal_open()` also makes the window visible. Between `terminal_open()` and
the first `terminal_refresh()`, the window is hidden.

If the OS requests a repaint (e.g., window was obscured), BearLibTerminal redraws the last committed
front buffer.

### terminal_put

```c
void terminal_put(int x, int y, int code);
```

Places a tile associated with the Unicode code point `code` into cell (x, y) on the current layer.

- If `code` is not associated with any tile, a "not-a-character" placeholder (thin rectangle) is

  shown.

- The code must be a Unicode code point. Even bitmap fonts have their tiles mapped to proper Unicode

  points internally.

- Respects the current foreground color, layer, and composition mode.

### terminal_put_ext

```c
void terminal_put_ext(int x, int y, int dx, int dy, int code, color_t* corners);
```

Extended version of `terminal_put` with pixel offset and per-corner coloring.

- `dx`, `dy`: Pixel offset relative to the tile's normal position in the cell. Each tile in a

  composition stack has independent offsets. Offset does NOT change draw order; use layers for
  proper Z-ordering.

- `corners`: Array of 4 `color_t` values for the tile corners: top-left, bottom-left, bottom-right,

  top-right (counter-clockwise from top-left). Enables smooth color gradients across tiles. Pass
  `NULL` to use the current foreground color uniformly.

C++ convenience overload:

```cpp
void terminal_put_ext(int x, int y, int dx, int dy, int code); // corners = NULL
```

### terminal_print

```c
dimensions_t terminal_print(int x, int y, const char* s);
dimensions_t terminal_print_ext(int x, int y, int w, int h, int align, const char* s);
```

Prints a string starting at (x, y). Each character is placed as if by
`terminal_put`/`terminal_put_ext`, respecting current color, layer, and composition settings.

**`terminal_print_ext`** adds auto-wrapping and alignment within a bounding box (w, h):

| Constant           | Value | Description              |
| ------------------ | ----- | ------------------------ |
| `TK_ALIGN_DEFAULT` | `0`   | Top-left (default)       |
| `TK_ALIGN_LEFT`    | `1`   | Left-align horizontally  |
| `TK_ALIGN_RIGHT`   | `2`   | Right-align horizontally |
| `TK_ALIGN_CENTER`  | `3`   | Center horizontally      |
| `TK_ALIGN_TOP`     | `4`   | Top-align vertically     |
| `TK_ALIGN_BOTTOM`  | `8`   | Bottom-align vertically  |
| `TK_ALIGN_MIDDLE`  | `12`  | Center vertically        |

Combine with `|`: e.g. `TK_ALIGN_CENTER | TK_ALIGN_MIDDLE` for full centering.

### Returns:**`dimensions_t` with the width and height (in cells) of the printed area.**Variants

- `terminal_printf(int x, int y, const char* s, ...)` -- printf formatting
- `terminal_wprint(int x, int y, const wchar_t* s)` -- wide string
- `terminal_wprintf(int x, int y, const wchar_t* s, ...)` -- wide printf
- `terminal_printf_ext(...)`, `terminal_wprint_ext(...)`, `terminal_wprintf_ext(...)` -- extended

  variants

#### Print Tags (Inline Formatting)

Tags are processed when `output.postformatting` is `true` (default). All tag effects are local to a
single `terminal_print` call.

| Tag                      | Description                                                                                                        |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `[color=red]`            | Set foreground color. Parsed by `color_from_name`.                                                                 |
| `[/color]`               | Reset foreground to what it was before the print call.                                                             |
| `[bkcolor=gray]`         | Set background color.                                                                                              |
| `[/bkcolor]`             | Reset background.                                                                                                  |
| `[font=italic]`          | Switch to a named font.                                                                                            |
| `[/font]`                | Reset to default font.                                                                                             |
| `[U+E001]` or `[0xE001]` | Insert an arbitrary character by code point.                                                                       |
| `[+]`                    | Compose next character with the previous one (stacking). E.g. `a[+]^` produces something like `a` with `^` on top. |
| `[offset=4,8]`           | Add a pixel offset to subsequent characters (as if by `put_ext`).                                                  |
| `[/offset]`              | Reset pixel offset.                                                                                                |
| `[[`                     | Literal `[` bracket.                                                                                               |
| `]]`                     | Literal `]` bracket.                                                                                               |

Tags are simple set/reset pairs, not truly nestable. No need to manually close every tag; all
effects end when the `print` call returns.

Example:

```c
terminal_printf("[offset=%d,%d]g[+][color=red]^[/color] (red-hooded goblin)", dx, dy);
```

### terminal_measure

```c
dimensions_t terminal_measure(const char* s);
dimensions_t terminal_measure_ext(int w, int h, const char* s);
```

Returns the size (in cells) a string WOULD occupy if printed. `terminal_measure_ext` measures with
auto-wrapping within a bounding box. No alignment parameter since alignment doesn't affect size.

**Variants:** `terminal_measuref`, `terminal_wmeasure`, `terminal_wmeasuref`,
`terminal_measuref_ext`, `terminal_wmeasure_ext`, `terminal_wmeasuref_ext`.

---

## 5. Rendering State

### terminal_color

```c
void terminal_color(color_t color);
```

Sets the current foreground color for all subsequent output operations. `color_t` is 32-bit
0xAARRGGBB.

Current value readable via `terminal_state(TK_COLOR)`.

C++ overloads accept string color names:

```cpp
void terminal_color(const char* name);
void terminal_color(const wchar_t* name);
```

### terminal_bkcolor

```c
void terminal_bkcolor(color_t color);
```

Sets the current background color. Only affects layer 0. Same overloads as `terminal_color`.

Current value readable via `terminal_state(TK_BKCOLOR)`.

### terminal_composition

```c
void terminal_composition(int mode);
```

Controls tile composition behavior:

- `TK_OFF` (0, default): `put`/`print` **replaces** the cell's contents.
- `TK_ON` (1): `put`/`print` **adds** to the cell's tile stack.

No enforced limit on tiles per cell. Each stacked tile retains its own foreground color and pixel
offset.

Current value readable via `terminal_state(TK_COMPOSITION)`.

### terminal_layer

```c
void terminal_layer(int index);
```

Selects the active layer (0-255). Layer 0 is the default, lowest layer.

- Only layer 0 has per-cell background colors.
- `terminal_clear_area` affects only the current layer.
- `terminal_clear` wipes ALL layers.
- Layers provide strict Z-ordering for oversized tiles and logical scene separation (e.g., animated

  dungeon on layer 0, static UI on layer 1).

Current value readable via `terminal_state(TK_LAYER)`.

### terminal_font

```c
void terminal_font(const char* name);
void terminal_wfont(const wchar_t* name);
```

Selects the current font by name. Named fonts are defined via `terminal_set`:

```c
terminal_set("italic font: italic.ttf, size=12");
terminal_font("italic");
// subsequent put/print use the italic font
terminal_font(""); // reset to default
```

---

## 6. Readback Functions

### terminal_pick

```c
int terminal_pick(int x, int y, int index);
```

Returns the character code of a tile in the specified cell on the current layer.

- `index`: 0-based index into the cell's tile stack.
- Returns 0 if no tile at that index (use this to enumerate: increment index until 0).
- Performs reverse codepage translation per `terminal.encoding`.

C++ default: `terminal_pick(int x, int y)` with `index = 0`.

### terminal_pick_color

```c
color_t terminal_pick_color(int x, int y, int index);
```

Returns the foreground color of the tile at `index` in the specified cell.

C++ default: `terminal_pick_color(int x, int y)` with `index = 0`.

### terminal_pick_bkcolor

```c
color_t terminal_pick_bkcolor(int x, int y);
```

Returns the background color of the specified cell (layer 0 only; no index parameter).

---

## 7. Input System

### terminal_read

```c
int terminal_read();
```

Returns the next event from the input queue. **Blocks** if the queue is empty. Check
`terminal_has_input()` first to avoid blocking.

The returned value is a `TK_*` constant. For key releases, the code is OR'd with `TK_KEY_RELEASED`
(0x100).

### terminal_peek

```c
int terminal_peek();
```

Like `terminal_read()` but does NOT remove the event from the queue. **Non-blocking**: returns 0
(`TK_INPUT_NONE`) if the queue is empty.

### terminal_has_input

```c
int terminal_has_input();
```

Returns boolean: non-zero if the next `terminal_read()` will return immediately without blocking.

### terminal_state

```c
int terminal_state(int slot);
```

Returns the current value of a state slot. These represent library state consistent with the input
queue position (see [Input Queue Consistency](#input-queue-consistency)).

Some states are NOT queued and are always real-time: `TK_WIDTH`, `TK_HEIGHT`, `TK_CELL_WIDTH`,
`TK_CELL_HEIGHT`, `TK_COLOR`, `TK_BKCOLOR`, `TK_LAYER`, `TK_COMPOSITION`, `TK_FULLSCREEN`.

### terminal_check

```c
int terminal_check(int slot);
```

Wrapper around `terminal_state()` that returns a boolean (state > 0). Exists because many languages
don't implicitly convert int to bool.

### terminal_read_str

```c
int terminal_read_str(int x, int y, char* buffer, int max);
int terminal_read_wstr(int x, int y, wchar_t* buffer, int max);
```

Simple blocking string input. Displays user input at (x, y) with length limit `max`. Uses the
current layer. Restores the scene before returning.

### Returns

- String length (>= 0) on Enter confirmation.
- `TK_INPUT_CANCELLED` (-1) on Escape or window close.

The buffer can be pre-filled with initial text.

### Input Filtering

The `input.filter` option controls which events `terminal_read` returns. Unfiltered events are
consumed silently.

```c
terminal_set("input.filter = [keyboard, mouse+]");
```

Filter names:

- **Groups:** `keyboard`, `mouse`, `keypad`, `arrows`
- **Single events:** Any `TK_*` name without the prefix, case-insensitive (e.g. `close`, `a`,

  `mouse-move`)

- **Character sets:** e.g. `wasd`, `0123456789` (must not match a group name)
- **`+` suffix:** enables both press AND release events (e.g. `mouse+`)
- **Default:** `keyboard` only
- **Disable filtering:** `input.filter = none` or empty list

System events (`TK_CLOSE`, `TK_RESIZED`) are always enabled regardless of filter.

### Input Queue Consistency

All input events are queued internally. States returned by `terminal_state()` reflect the state as
of the most recently dequeued event, NOT real-time. This ensures consistency:

```c
int key = terminal_read();
if (key == TK_A && terminal_check(TK_SHIFT)) {
    // Shift+A: this check is reliable because TK_SHIFT state
    // reflects whether Shift was held when A was pressed/dequeued.
}
```

### TK_KEY_RELEASED Flag

Value: `0x100`. OR'd with key codes for release events.

```c
int key = terminal_read();
if (key == TK_A)                        // A pressed
if (key == (TK_B | TK_KEY_RELEASED))    // B released
if ((key & ~TK_KEY_RELEASED) == TK_C)   // C pressed or released
```

Auto-repeat generates multiple press events before a single release. No intermediate releases during
auto-repeat.

### TK_CHAR / TK_WCHAR States

After dequeuing an event, these states provide the character produced:

- `TK_CHAR`: ANSI code translated per `terminal.encoding`.
- `TK_WCHAR`: Unicode code point.

Use for custom text input:

```c
int key = terminal_read();
if (terminal_check(TK_WCHAR)) {
    wchar_t ch = (wchar_t)terminal_state(TK_WCHAR);
    // append ch to input buffer
}
```

### TK_MOUSE_WHEEL State

Returns scroll direction and step count for the most recent `TK_MOUSE_SCROLL` event. Positive = up,
negative = down (e.g., 1, -2).

### TK_MOUSE_CLICKS State

Returns the number of fast consecutive clicks for the most recent mouse button press (1 = single
click, 2 = double click, etc.).

---

## 8. TK\_\* Constants (Complete List)

All values from `BearLibTerminal.h`:

### Keyboard Keys

| Constant                 | Hex              | Description                                 |
| ------------------------ | ---------------- | ------------------------------------------- |
| `TK_A` .. `TK_Z`         | `0x04` .. `0x1D` | Letter keys                                 |
| `TK_1` .. `TK_9`         | `0x1E` .. `0x26` | Number keys 1-9                             |
| `TK_0`                   | `0x27`           | Number key 0                                |
| `TK_RETURN` / `TK_ENTER` | `0x28`           | Enter key (both names alias the same value) |
| `TK_ESCAPE`              | `0x29`           | Escape                                      |
| `TK_BACKSPACE`           | `0x2A`           | Backspace                                   |
| `TK_TAB`                 | `0x2B`           | Tab                                         |
| `TK_SPACE`               | `0x2C`           | Space                                       |
| `TK_MINUS`               | `0x2D`           | `-`                                         |
| `TK_EQUALS`              | `0x2E`           | `=`                                         |
| `TK_LBRACKET`            | `0x2F`           | `[`                                         |
| `TK_RBRACKET`            | `0x30`           | `]`                                         |
| `TK_BACKSLASH`           | `0x31`           | `\`                                         |
| `TK_SEMICOLON`           | `0x33`           | `;`                                         |
| `TK_APOSTROPHE`          | `0x34`           | `'`                                         |
| `TK_GRAVE`               | `0x35`           | `` ` ``                                     |
| `TK_COMMA`               | `0x36`           | `,`                                         |
| `TK_PERIOD`              | `0x37`           | `.`                                         |
| `TK_SLASH`               | `0x38`           | `/`                                         |

### Function / Navigation / Modifier Keys

| Constant            | Hex              | Description      |
| ------------------- | ---------------- | ---------------- |
| `TK_F1` .. `TK_F12` | `0x3A` .. `0x45` | Function keys    |
| `TK_PAUSE`          | `0x48`           | Pause/Break      |
| `TK_INSERT`         | `0x49`           | Insert           |
| `TK_HOME`           | `0x4A`           | Home             |
| `TK_PAGEUP`         | `0x4B`           | Page Up          |
| `TK_DELETE`         | `0x4C`           | Delete           |
| `TK_END`            | `0x4D`           | End              |
| `TK_PAGEDOWN`       | `0x4E`           | Page Down        |
| `TK_RIGHT`          | `0x4F`           | Right arrow      |
| `TK_LEFT`           | `0x50`           | Left arrow       |
| `TK_DOWN`           | `0x51`           | Down arrow       |
| `TK_UP`             | `0x52`           | Up arrow         |
| `TK_SHIFT`          | `0x70`           | Shift modifier   |
| `TK_CONTROL`        | `0x71`           | Control modifier |
| `TK_ALT`            | `0x72`           | Alt modifier     |

### Keypad

| Constant               | Hex              | Description  |
| ---------------------- | ---------------- | ------------ |
| `TK_KP_DIVIDE`         | `0x54`           | Numpad `/`   |
| `TK_KP_MULTIPLY`       | `0x55`           | Numpad `*`   |
| `TK_KP_MINUS`          | `0x56`           | Numpad `-`   |
| `TK_KP_PLUS`           | `0x57`           | Numpad `+`   |
| `TK_KP_ENTER`          | `0x58`           | Numpad Enter |
| `TK_KP_1` .. `TK_KP_9` | `0x59` .. `0x61` | Numpad 1-9   |
| `TK_KP_0`              | `0x62`           | Numpad 0     |
| `TK_KP_PERIOD`         | `0x63`           | Numpad `.`   |

### Mouse Events and States

| Constant           | Hex    | Type        | Description                                                       |
| ------------------ | ------ | ----------- | ----------------------------------------------------------------- |
| `TK_MOUSE_LEFT`    | `0x80` | Event/State | Left mouse button                                                 |
| `TK_MOUSE_RIGHT`   | `0x81` | Event/State | Right mouse button                                                |
| `TK_MOUSE_MIDDLE`  | `0x82` | Event/State | Middle mouse button                                               |
| `TK_MOUSE_X1`      | `0x83` | Event/State | Extra mouse button 1                                              |
| `TK_MOUSE_X2`      | `0x84` | Event/State | Extra mouse button 2                                              |
| `TK_MOUSE_MOVE`    | `0x85` | Event       | Mouse moved (cell or pixel granularity per `input.precise-mouse`) |
| `TK_MOUSE_SCROLL`  | `0x86` | Event       | Mouse wheel scrolled                                              |
| `TK_MOUSE_X`       | `0x87` | State       | Current mouse X position in cells                                 |
| `TK_MOUSE_Y`       | `0x88` | State       | Current mouse Y position in cells                                 |
| `TK_MOUSE_PIXEL_X` | `0x89` | State       | Current mouse X position in pixels                                |
| `TK_MOUSE_PIXEL_Y` | `0x8A` | State       | Current mouse Y position in pixels                                |
| `TK_MOUSE_WHEEL`   | `0x8B` | State       | Scroll delta from last `TK_MOUSE_SCROLL` event                    |
| `TK_MOUSE_CLICKS`  | `0x8C` | State       | Consecutive click count (1=click, 2=double-click)                 |

### Property / Terminal State Slots

| Constant         | Hex    | Description                                              |
| ---------------- | ------ | -------------------------------------------------------- |
| `TK_WIDTH`       | `0xC0` | Terminal width in cells                                  |
| `TK_HEIGHT`      | `0xC1` | Terminal height in cells                                 |
| `TK_CELL_WIDTH`  | `0xC2` | Cell width in pixels                                     |
| `TK_CELL_HEIGHT` | `0xC3` | Cell height in pixels                                    |
| `TK_COLOR`       | `0xC4` | Current foreground color                                 |
| `TK_BKCOLOR`     | `0xC5` | Current background color                                 |
| `TK_LAYER`       | `0xC6` | Current layer index                                      |
| `TK_COMPOSITION` | `0xC7` | Current composition mode (0=off, 1=on)                   |
| `TK_CHAR`        | `0xC8` | ANSI char code from last event (per `terminal.encoding`) |
| `TK_WCHAR`       | `0xC9` | Unicode code point from last event                       |
| `TK_EVENT`       | `0xCA` | Code of the last dequeued event                          |
| `TK_FULLSCREEN`  | `0xCB` | Fullscreen state                                         |

### System Events

| Constant     | Hex    | Description                                                |
| ------------ | ------ | ---------------------------------------------------------- |
| `TK_CLOSE`   | `0xE0` | Window close requested (close button, Alt+F4)              |
| `TK_RESIZED` | `0xE1` | Window resized by user (requires `window.resizeable=true`) |

### Flags and Special Values

| Constant             | Value   | Description                                                |
| -------------------- | ------- | ---------------------------------------------------------- |
| `TK_KEY_RELEASED`    | `0x100` | OR'd with key code for release events                      |
| `TK_OFF`             | `0`     | Composition off                                            |
| `TK_ON`              | `1`     | Composition on                                             |
| `TK_INPUT_NONE`      | `0`     | No input (returned by `terminal_peek` when queue is empty) |
| `TK_INPUT_CANCELLED` | `-1`    | `terminal_read_str` cancelled (Escape or window close)     |

### Alignment Constants

| Constant           | Value | Description       |
| ------------------ | ----- | ----------------- |
| `TK_ALIGN_DEFAULT` | `0`   | Top-left          |
| `TK_ALIGN_LEFT`    | `1`   | Left              |
| `TK_ALIGN_RIGHT`   | `2`   | Right             |
| `TK_ALIGN_CENTER`  | `3`   | Horizontal center |
| `TK_ALIGN_TOP`     | `4`   | Top               |
| `TK_ALIGN_BOTTOM`  | `8`   | Bottom            |
| `TK_ALIGN_MIDDLE`  | `12`  | Vertical center   |

---

## 9. Tile Composition Model and Layer System

### Layers

The scene consists of up to 256 layers (indices 0-255), each a full grid of cells with the same
dimensions as the terminal window.

- **Layer 0** is the only layer with per-cell background colors.
- Layers are drawn in ascending order (0 first, then 1, 2, ...), providing strict Z-ordering.
- Each layer can be independently cleared (`terminal_clear_area`), cropped (`terminal_crop`), and

  written to.

- `terminal_clear()` wipes ALL layers and resets all crop rectangles.

### Use cases

1. Oversized tiles: A 2x2 tile on layer 0 would be partially overwritten by adjacent cells drawn

   later. Place it on layer 1 to draw on top.

1. Logical separation: Static UI on one layer, animated game world on another. Update each

   independently.

### Tile Composition

When `terminal_composition(TK_ON)` is active, `terminal_put` and `terminal_print` ADD tiles to a
cell's stack instead of replacing the contents.

Each tile in the stack has:

- Its own foreground color (set at the time of placement via `terminal_color`)
- Its own pixel offset (set via `terminal_put_ext` dx/dy or `[offset=...]` print tags)
- Its own corner colors (set via `terminal_put_ext` corners parameter)

There is no enforced limit on tiles per cell.

**Print tag composition:** `a[+]^` places `a` and `^` in the same cell as if composition were on,
producing a combined glyph.

### Tile Alignment

When a tile is larger than one cell, its `align` parameter determines where it's anchored:

- `center` (default): centered in the cell, good for character glyphs with slight size variations.
- `top-left`: anchored to top-left corner, good for terrain/sprite tiles larger than one cell.
- `bottom-left`, `top-right`, `bottom-right`: other anchor points.

The `spacing` parameter (e.g. `2x1`) defines the alignment area in cells. A tile with `spacing=2x2`
is centered/aligned within a 2x2 cell region.

---

## 10. Unicode and Codepage Handling

### String Encoding

The library exports three variants of every string function:

- `terminal_*8` (int8_t / char): UTF-8 or ANSI (per `terminal.encoding`)
- `terminal_*16` (int16_t / wchar_t on Windows): UTF-16
- `terminal_*32` (int32_t / wchar_t on Linux/macOS): UTF-32

The C header detects `wchar_t` size and maps `terminal_wset`, `terminal_wprint`, etc. to the correct
suffix.

### terminal.encoding Option

Controls how unibyte (8-bit) strings are interpreted. Default: `utf8`. Can be set to ANSI codepages
like `Windows-1251` for legacy support.

This affects:

- How strings passed to `terminal_set`, `terminal_print`, etc. are decoded.
- How `terminal_pick` performs reverse translation.
- What `TK_CHAR` state returns (translated per the encoding).

`TK_WCHAR` always returns a Unicode code point regardless of this setting.

### Codepages for Tilesets

Codepages control how tile indices in a bitmap tileset map to Unicode code points.

Built-in codepages: `ascii`, `437`, `866`, `1250`, `1251`.

Custom codepages: provide a text file path. The file contains a comma-separated list of Unicode code
points:

```text
0xF00C, 0xF062, 0xF001, 0xF0E7, 0xF013, 0xF043
```text

For bitmap tilesets, the codepage maps tile index to Unicode (forward mapping: index N goes to code
point C).

For TrueType tilesets, the codepage is inverted: it maps Unicode to relative index (reverse mapping:
load only specific glyphs from the font to consecutive slots).

---

## 11. Utility Functions

### terminal_delay

```c
void terminal_delay(int period);
```

Suspends execution for `period` milliseconds.

### color_from_name

```c
color_t color_from_name(const char* name);
color_t color_from_wname(const wchar_t* name);
```

Parses a color name string and returns the 0xAARRGGBB value.

Name format: `[brightness] hue`

### Brightness modifiers:**`lightest`, `lighter`, `light`, `dark`, `darker`, `darkest`**Hue formats

- Named: `grey`/`gray`, `red`, `flame`, `orange`, `amber`, `yellow`, `lime`, `chartreuse`, `green`,

  `sea`, `turquoise`, `cyan`, `sky`, `azure`, `blue`, `han`, `violet`, `purple`, `fuchsia`,
  `magenta`, `pink`, `crimson`, `transparent`

- Hex: `#RRGGBB` or `#AARRGGBB` (e.g. `#80905025`)
- Decimal: `R,G,B` or `A,R,G,B` (e.g. `128,200,150,75`)
- Plain integer: numeric string (e.g. `16744448`)

Custom colors can be added via `terminal_set("palette.name = value")` and used anywhere colors are
parsed (including `[color=name]` print tags).

### color_from_argb

```c
color_t color_from_argb(uint8_t a, uint8_t r, uint8_t g, uint8_t b);
```

Combines four 8-bit channels into a 32-bit color value:

```c
return ((color_t)a << 24) | (r << 16) | (g << 8) | b;
```

---

## 12. Exported DLL Functions (Language-Agnostic)

The shared library exports these C functions with explicit encoding suffixes. The header's inline
wrappers and C++ overloads are convenience only.

```c
// Core
int terminal_open();
void terminal_close();
void terminal_refresh();
void terminal_clear();
void terminal_clear_area(int x, int y, int w, int h);
void terminal_crop(int x, int y, int w, int h);
void terminal_layer(int index);
void terminal_color(color_t color);
void terminal_bkcolor(color_t color);
void terminal_composition(int mode);

// Configuration (3 encoding variants each)
int terminal_set8(const int8_t* value);
int terminal_set16(const int16_t* value);
int terminal_set32(const int32_t* value);

// Font selection (3 encoding variants each)
void terminal_font8(const int8_t* name);
void terminal_font16(const int16_t* name);
void terminal_font32(const int32_t* name);

// Output
void terminal_put(int x, int y, int code);
void terminal_put_ext(int x, int y, int dx, int dy, int code, color_t* corners);

// Print (3 encoding variants each, extended form only at DLL level)
void terminal_print_ext8(int x, int y, int w, int h, int align, const int8_t* s, int* out_w, int* out_h);
void terminal_print_ext16(int x, int y, int w, int h, int align, const int16_t* s, int* out_w, int* out_h);
void terminal_print_ext32(int x, int y, int w, int h, int align, const int32_t* s, int* out_w, int* out_h);

// Measure (3 encoding variants each)
void terminal_measure_ext8(int w, int h, const int8_t* s, int* out_w, int* out_h);
void terminal_measure_ext16(int w, int h, const int16_t* s, int* out_w, int* out_h);
void terminal_measure_ext32(int w, int h, const int32_t* s, int* out_w, int* out_h);

// Readback
int terminal_pick(int x, int y, int index);
color_t terminal_pick_color(int x, int y, int index);
color_t terminal_pick_bkcolor(int x, int y);

// Input
int terminal_has_input();
int terminal_state(int code);
int terminal_read();
int terminal_peek();

// String input (3 encoding variants each)
int terminal_read_str8(int x, int y, int8_t* buffer, int max);
int terminal_read_str16(int x, int y, int16_t* buffer, int max);
int terminal_read_str32(int x, int y, int32_t* buffer, int max);

// Config query (3 encoding variants each)
const int8_t* terminal_get8(const int8_t* key, const int8_t* default_);
const int16_t* terminal_get16(const int16_t* key, const int16_t* default_);
const int32_t* terminal_get32(const int32_t* key, const int32_t* default_);

// Utility
void terminal_delay(int period);
color_t color_from_name8(const int8_t* name);
color_t color_from_name16(const int16_t* name);
color_t color_from_name32(const int32_t* name);
```

Note: The DLL-level `terminal_print_ext*` and `terminal_measure_ext*` use `int* out_w, int* out_h`
output parameters. The C header's inline wrappers (`terminal_print`, `terminal_measure`, etc.)
return `dimensions_t` structs instead.

Non-extended `terminal_print` is an inline wrapper that calls `terminal_print_ext` with
`w=0, h=0, align=TK_ALIGN_DEFAULT`. Similarly, `terminal_measure` calls `terminal_measure_ext` with
`w=0, h=0`.

---

## 13. Platform Notes

- **Windows:** Link against `BearLibTerminal.lib` (MSVC) or directly against the `.dll` (MinGW).
- **Linux/macOS:** Link against `libBearLibTerminal.so` / `.dylib`.
- **Python:** `pip install bearlibterminal` includes the wrapper and native binary.
- **Lua:** Built-in wrapper. Place the `.so`/`.dll` alongside the script (Linux: drop the `lib`

  prefix).

- **Building:** Requires CMake and GCC 4.6.3+ (Linux) or MinGW with Posix thread model (Windows).
- **License:** MIT (main library), with a few parts under other permissive licenses.
