# Research: Text Layout and Inline Formatting for Terminal Grid Rendering

## Summary

Terminal/roguelike libraries have converged on similar patterns for inline-formatted text:
BearLibTerminal uses bracket tags (`[color=red]`), libtcod embeds control codes in the byte stream
(special char values 1-5 for preset colors, plus raw RGB embedding), and rot.js uses printf-style
`%c{color}` specifiers. The ratatui model is different: no inline parsing at all, instead composing
styled text from typed `Span`/`Line`/`Text` structs. For a new Rust library, the best approach is a
two-layer design: a tag parser that converts markup strings into a `Vec<StyledSpan>` (similar to
ratatui's model), plus a layout engine that handles word wrapping, alignment, and measurement over
those spans within bounded rectangles.

## Findings

### 1. BearLibTerminal's Inline Tag System

BearLibTerminal's `print` function processes inline tags within the string being printed. Tags are
enclosed in square brackets and modify rendering state for subsequent characters. All tag effects
are **local to a single `print` call**and automatically reset when the call returns.**Supported
tags:**

- `[color=red]` / `[bkcolor=gray]` — set foreground/background color. Color names are parsed by

  `color_from_name()` which supports named colors (red, lime, azure, etc.), brightness modifiers
  (light, dark, lighter, darker, lightest, darkest), hex (`#RRGGBB`, `#AARRGGBB`), comma-separated
  decimal (`R,G,B` or `A,R,G,B`), and custom palette entries.

- `[U+E001]` or `[0xE001]` — insert an arbitrary Unicode code point by hex value.
- `[+]` — composition marker. `a[+]^` composites two glyphs in the same cell, like `â`. Internally

  uses the composition mode (tile stacking).

- `[offset=4,8]` — pixel-level offset for subsequent characters, as if placed via `put_ext`. Useful

  for sub-cell positioning.

- `[/color]`, `[/bkcolor]`, `[/offset]` — reset tags to pre-call defaults. These are simple

  set/reset pairs, not a stack, so nesting does not truly nest.

- `[[` and `]]` — escape sequences to print literal bracket characters.

**`print_ext`** adds bounded rectangle printing with alignment:

```c
terminal_print_ext(x, y, width, height, align, string);
// align: TK_ALIGN_LEFT | TK_ALIGN_RIGHT | TK_ALIGN_CENTER
//        TK_ALIGN_TOP  | TK_ALIGN_BOTTOM | TK_ALIGN_MIDDLE
```

**`measure` / `measure_ext`** compute the dimensions a string would occupy when printed, including
tag processing and word wrapping within a bounding box. The `measure` function parses and strips
tags just like `print` does, but produces dimensions instead of drawing.

[Source: BearLibTerminal Reference](http://foo.wyrd.name/en:bearlibterminal:reference)

### 2. libtcod's Color Control Codes

libtcod takes a fundamentally different approach: instead of text-parseable tags, it embeds
**special byte values** directly in the string. This works because the control characters occupy
code points below the printable range.

### Control code constants

- `TCOD_COLCTRL_1` through `TCOD_COLCTRL_5` — preset color slots. Each slot stores both a foreground

  and background color, pre-registered via `TCOD_console_set_color_control(slot, fore, back)`.

- `TCOD_COLCTRL_FORE_RGB` — followed by 3 bytes (R, G, B) inline. Sets foreground directly.
- `TCOD_COLCTRL_BACK_RGB` — followed by 3 bytes (R, G, B) inline. Sets background directly.
- `TCOD_COLCTRL_STOP` — resets foreground and background to the colors active before the print call.

**String length calculation** (`TCOD_console_stringLength`) skips control characters and their data
bytes, counting only printable characters. The `TCOD_console_forward` function advances a string
pointer by N _visible_ characters, also skipping control sequences.

**Text alignment** is handled via `TCOD_alignment_t` parameter: `TCOD_LEFT`, `TCOD_RIGHT`,
`TCOD_CENTER`. The print functions compute the starting x position based on alignment and the
visible string length.

**Word wrapping** uses a greedy algorithm. When a line exceeds `maxx`, it scans backward from the
overflow point looking for whitespace. If found, it breaks there; otherwise it force-breaks at the
boundary. The algorithm processes `\n`-delimited sub-messages within the overall string.

**Measurement** is built into the print system: `TCOD_console_get_height_rect` calls the same
`print_internal` function with `count_only=true`, which runs the full layout algorithm (including
word wrapping) but skips the actual `put_char` calls.

The newer UTF-8 parser (`fp_next`, `fp_peek`, `next_split_`) uses `utf8proc` for proper Unicode
handling, character width detection (including CJK double-width awareness), and line-break category
detection (using Unicode categories like `ZL`, `ZP`, `CC` for separators and control characters).

[Source: libtcod/console_printing.c](https://github.com/libtcod/libtcod/blob/main/src/libtcod/console_printing.c)

### 3. rot.js Inline Color Formatting

rot.js uses printf-inspired format specifiers embedded in text strings:

- `%c{red}` — set foreground color to "red"
- `%b{blue}` — set background color to "blue"
- `%c{}` / `%b{}` — reset to default foreground/background (empty value = null = default)

**Tokenization** is a two-pass process defined in `text.ts`:

1. **First pass** — regex-based split using `/%([bc]){([^}]*)}/g`. Produces an interleaved stream of

   `TYPE_TEXT`, `TYPE_FG`, and `TYPE_BG` tokens.

1. **Second pass** (`breakLines`) — processes the text tokens for word wrapping within `maxWidth`.

   Inserts `TYPE_NEWLINE` tokens at appropriate break points.

**Word wrapping logic** in `breakLines`:

- Removes leading spaces at the start of each line.
- Handles explicit `\n` characters by splitting tokens.
- When a line would exceed `maxWidth`, searches for a space within the current token to break at. If

  none found in the current token, looks backward to the most recent token containing a space. As a
  last resort, force-breaks mid-word.

- Trailing spaces before newlines are stripped.

**Measurement** (`Text.measure`) processes the token stream and counts `TYPE_TEXT` character lengths
per line, tracking maximum width and total height.

**Rendering** (`Display.drawText`) iterates the token stream, calling
`this.draw(cx, cy, char, fg, bg)` for each character, advancing the cursor. Color tokens update the
current `fg`/`bg` state variables. Newline tokens reset `cx` and increment `cy`.

```typescript
const RE_COLORS = /%([bc]){([^}]*)}/g;
const TYPE_TEXT = 0;
const TYPE_NEWLINE = 1;
const TYPE_FG = 2;
const TYPE_BG = 3;
```

[Source: rot.js/src/text.ts](https://github.com/ondras/rot.js/blob/master/src/text.ts)

### 4. ratatui's Typed Text Model

ratatui avoids inline markup entirely in favor of a typed hierarchy:

- **`Span`** — the smallest unit. A contiguous string where all characters share one `Style`.

  Fields: `content: Cow<'a, str>`, `style: Style`.

- **`Line`** — a single line composed of `Vec<Span>`. Has its own `style` (applied before span

  styles), and an optional `alignment: Option<Alignment>`.

- **`Text`**— multiple lines: `Vec<Line>`. Also has a `style` and `alignment`.**Key design
  properties:**

- `Span::width()` returns the Unicode display width (via `unicode-width` crate).
- `Line::width()` sums the widths of all contained spans.
- `Span::styled_graphemes(base_style)` yields `StyledGrapheme` items for rendering, patching the

  base style with the span's style.

- Styles compose via `patch_style`: the span's style overlays the line's style, which overlays the

  text's style. Missing fields inherit from the parent.

- The `Paragraph` widget handles word wrapping and alignment for `Text`, with `Wrap { trim: bool }`

  config.

- `Line` supports `left_aligned()`, `centered()`, `right_aligned()` convenience methods.

This model is entirely programmatic; styled text is constructed in code:

```rust
let line = Line::from(vec![
    Span::styled("Hello", Style::new().blue()),
    Span::raw(" world!"),
]);
```

The tradeoff: no parsing complexity or escape handling, but verbose for inline formatting in
data-driven text (dialog, descriptions).

[Source: ratatui docs](https://docs.rs/ratatui/latest/ratatui/text/index.html)

### 5. Word Wrapping Algorithms

Two primary algorithms are used:

**Greedy (minimum-lines) algorithm:** The standard approach used by most terminals, word processors,
and all the roguelike libraries above. Places as many words on the current line as possible, then
wraps to the next line. Pseudocode:

````text
SpaceLeft := LineWidth
for each Word in Text
    if (Width(Word) + SpaceWidth) > SpaceLeft
        insert line break before Word
        SpaceLeft := LineWidth - Width(Word)
    else
        SpaceLeft := SpaceLeft - (Width(Word) + SpaceWidth)
```javascript

- Advantages: O(n) time, simple, predictable, operates in a single pass.
- Disadvantage: can produce lines of wildly varying lengths (one line near-full, the next with a

  single long word).

**Knuth-Plass (optimal) algorithm:** Used by TeX. Minimizes the sum of squared space at line ends
across the entire paragraph. Uses dynamic programming to evaluate all possible break points
simultaneously.

- Advantages: more aesthetically pleasing, even line lengths.
- Disadvantage: O(n^2) worst case (O(n) typical), needs the full paragraph text upfront, more

  complex to implement.

- For monospace grids, the visual improvement over greedy is marginal because there is no inter-word

  space stretching. **Greedy is the right default for terminal grids.**

### For monospace grids specifically

- Each character is exactly 1 cell wide (except CJK double-width characters, which are 2).
- Word width = character count (simple `len()` or `unicode_width`).
- No kerning, no variable glyph widths, no fractional spacing.
- Break on whitespace, optionally on hyphens.
- Force-break words longer than the available width (all libraries above do this).

[Source: Wikipedia - Line wrap and word wrap](https://en.wikipedia.org/wiki/Line_wrap_and_word_wrap)

### 6. Text Alignment Within Bounded Rectangles

Alignment operates on two axes: horizontal and vertical.

**Horizontal alignment** (per-line):

- **Left**: `start_x = rect.x` (default)
- **Right**: `start_x = rect.x + rect.width - line_width`
- **Center**: `start_x = rect.x + (rect.width - line_width) / 2`

All three roguelike libraries handle this identically. libtcod and BearLibTerminal support it as a
parameter to the print function. The `x` parameter meaning changes based on alignment:

- Left: `x` is the left edge
- Right: `x` is the right edge
- Center: `x` is the center point

**Vertical alignment** (across all lines in a block):

- **Top**: `start_y = rect.y` (default)
- **Middle**: `start_y = rect.y + (rect.height - total_lines) / 2`
- **Bottom**: `start_y = rect.y + rect.height - total_lines`

BearLibTerminal supports both axes via combinable flags (`TK_ALIGN_CENTER | TK_ALIGN_MIDDLE`).
libtcod supports only horizontal alignment. rot.js does not support alignment at all (always
top-left).

### Implementation approach

1. First pass: perform word wrapping to determine all line breaks and line widths.
2. Compute `total_height` (number of lines).
3. Apply vertical alignment to find `start_y`.
4. For each line, apply horizontal alignment to find `start_x`.
5. Render characters starting from `(start_x, start_y)`.

This two-pass approach (wrap then align) is necessary because vertical alignment needs the total
height, which is only known after wrapping completes. BearLibTerminal's `measure_ext` + `print_ext`
API reflects this: measure first, then print with alignment.

### 7. Text Measurement

All three libraries provide measurement APIs that compute the bounding box of text without rendering
it:

- **BearLibTerminal**: `terminal_measure(s)` returns `{width, height}` for unwrapped text.

  `terminal_measure_ext(w, h, s)` returns dimensions with word wrapping within the given bbox.

- **libtcod**: `TCOD_console_get_height_rect(con, x, y, w, h, fmt, ...)` returns the number of

  lines. It calls the same `print_internal` with `count_only=true`.

- **rot.js**: `Text.measure(str, maxWidth)` tokenizes the string, processes word wrapping, then sums

  character widths per line to find `{width, height}`.

**Design pattern**: measurement should share the same code path as rendering, with a flag to skip
actual cell writes. This guarantees measurement and rendering agree on layout. libtcod's
`count_only` parameter is the canonical example.

For a Rust implementation:

```rust
struct TextMetrics {
    width: u16,   // maximum line width in cells
    height: u16,  // total lines
    lines: Vec<LineMetrics>,  // per-line widths for alignment
}

struct LineMetrics {
    width: u16,
    span_count: usize,
}
````

### 8. Rich Text Parsing and Tag Tokenization

The tag parsing problem is straightforward for bracket-style tags. Key considerations:

### Escaping

- BearLibTerminal: `[[` produces a literal `[`, `]]` produces `]`.
- rot.js: no escaping mechanism (relies on `%` being rare in game text).
- libtcod: no escaping needed (control codes are non-printable).

**Parser structure** (for BearLibTerminal-style tags):

Tokens fall into categories:

1. **Text** — plain characters to render.
2. **SetColor(color)** — change foreground color.
3. **SetBgColor(color)** — change background color.
4. **ResetColor** — restore default foreground.
5. **ResetBgColor** — restore default background.
6. **Codepoint(u32)** — insert a Unicode code point.
7. **Compose** — begin composition (next glyph overlays previous cell).
8. **SetOffset(x, y)** — set pixel offset.
9. **ResetOffset**— clear pixel offset.**State machine approach:**

````yaml
NORMAL: read char
  '[' -> check next:
    '[' -> emit Text('[')        // escaped bracket
    else -> READING_TAG
  other -> accumulate into current Text token

READING_TAG: read chars until ']'
  parse tag name and value
  emit appropriate command token
  -> NORMAL
```text

### Tag value parsing

- `color=red` -> SetColor, parse "red" via color name resolution
- `bkcolor=#FF0000` -> SetBgColor, parse hex
- `U+E001` or `0xE001` -> Codepoint, parse hex
- `offset=4,8` -> SetOffset, parse comma-separated ints
- `/color` -> ResetColor
- `/bkcolor` -> ResetBgColor
- `/offset` -> ResetOffset
- `+` -> Compose

### 9. Concrete Rust Design for Tag Parser and Layout Engine

#### Tag Parser

```rust
/// A token produced by parsing a tagged string.
#[derive(Debug, Clone, PartialEq)]
pub enum Token<'a> {
    /// Plain text to render.
    Text(&'a str),
    /// Set foreground color.
    Color(Color),
    /// Set background color.
    BgColor(Color),
    /// Reset foreground color to default.
    ResetColor,
    /// Reset background color to default.
    ResetBgColor,
    /// Insert a specific Unicode code point (e.g., from [U+E001]).
    Codepoint(char),
    /// Compose next glyph onto the previous cell ([+]).
    Compose,
    /// Set sub-cell pixel offset ([offset=x,y]).
    Offset(i16, i16),
    /// Reset pixel offset.
    ResetOffset,
}

/// Parse a tagged string into tokens.
///
/// Tags use square bracket syntax: [color=red], [/color], [U+E001], [+], etc.
/// Literal brackets are escaped by doubling: [[ -> [, ]] -> ].
pub fn tokenize(input: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();
    let mut text_start: Option<usize> = None;

    while let Some(&(i, ch)) = chars.peek() {
        match ch {
            '[' => {
                // Flush pending text
                if let Some(start) = text_start.take() {
                    tokens.push(Token::Text(&input[start..i]));
                }
                chars.next();
                // Check for escaped bracket
                if chars.peek().map(|&(_, c)| c) == Some('[') {
                    tokens.push(Token::Text("["));
                    chars.next();
                    continue;
                }
                // Read until closing ']'
                let tag_start = chars.peek().map(|&(i, _)| i).unwrap_or(input.len());
                let mut tag_end = tag_start;
                while let Some(&(j, c)) = chars.peek() {
                    if c == ']' {
                        tag_end = j;
                        chars.next();
                        break;
                    }
                    tag_end = j + c.len_utf8();
                    chars.next();
                }
                let tag_content = &input[tag_start..tag_end];
                if let Some(token) = parse_tag(tag_content) {
                    tokens.push(token);
                }
            }
            ']' => {
                // Flush pending text
                if let Some(start) = text_start.take() {
                    tokens.push(Token::Text(&input[start..i]));
                }
                chars.next();
                // Check for escaped bracket
                if chars.peek().map(|&(_, c)| c) == Some(']') {
                    tokens.push(Token::Text("]"));
                    chars.next();
                } else {
                    // Stray ']', treat as text
                    tokens.push(Token::Text("]"));
                }
            }
            _ => {
                if text_start.is_none() {
                    text_start = Some(i);
                }
                chars.next();
            }
        }
    }

    // Flush remaining text
    if let Some(start) = text_start {
        tokens.push(Token::Text(&input[start..]));
    }

    tokens
}

fn parse_tag<'a>(content: &'a str) -> Option<Token<'a>> {
    let content = content.trim();

    // Reset tags
    if content.eq_ignore_ascii_case("/color") {
        return Some(Token::ResetColor);
    }
    if content.eq_ignore_ascii_case("/bkcolor") {
        return Some(Token::ResetBgColor);
    }
    if content.eq_ignore_ascii_case("/offset") {
        return Some(Token::ResetOffset);
    }

    // Compose
    if content == "+" {
        return Some(Token::Compose);
    }

    // Unicode code point: U+XXXX or 0xXXXX
    if let Some(hex) = content.strip_prefix("U+").or_else(|| content.strip_prefix("0x")) {
        if let Ok(cp) = u32::from_str_radix(hex, 16) {
            if let Some(ch) = char::from_u32(cp) {
                return Some(Token::Codepoint(ch));
            }
        }
        return None;
    }

    // Key=value tags
    if let Some((key, value)) = content.split_once('=') {
        let key = key.trim();
        let value = value.trim();
        match key.to_ascii_lowercase().as_str() {
            "color" => {
                return Some(Token::Color(parse_color(value)?));
            }
            "bkcolor" => {
                return Some(Token::BgColor(parse_color(value)?));
            }
            "offset" => {
                let (x, y) = value.split_once(',')?;
                let x: i16 = x.trim().parse().ok()?;
                let y: i16 = y.trim().parse().ok()?;
                return Some(Token::Offset(x, y));
            }
            _ => return None,
        }
    }

    None
}
````

#### Styled Span (intermediate representation)

```rust
/// A span of text with uniform style, produced after tag parsing.
/// This is the "resolved" form ready for layout.
#[derive(Debug, Clone)]
pub struct StyledSpan {
    pub text: String,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub offset: Option<(i16, i16)>,
    pub compose: bool,
}

/// Convert a token stream into styled spans by tracking style state.
pub fn resolve_spans(tokens: &[Token<'_>], default_fg: Color, default_bg: Color) -> Vec<StyledSpan> {
    let mut spans = Vec::new();
    let mut current_text = String::new();
    let mut fg = default_fg;
    let mut bg = default_bg;
    let mut offset: Option<(i16, i16)> = None;
    let mut compose = false;

    let flush = |spans: &mut Vec<StyledSpan>,
                 text: &mut String,
                 fg: Color,
                 bg: Color,
                 offset: Option<(i16, i16)>| {
        if !text.is_empty() {
            spans.push(StyledSpan {
                text: std::mem::take(text),
                fg: Some(fg),
                bg: Some(bg),
                offset,
                compose: false,
            });
        }
    };

    for token in tokens {
        match token {
            Token::Text(t) => current_text.push_str(t),
            Token::Codepoint(ch) => current_text.push(*ch),
            Token::Color(c) => {
                flush(&mut spans, &mut current_text, fg, bg, offset);
                fg = *c;
            }
            Token::BgColor(c) => {
                flush(&mut spans, &mut current_text, fg, bg, offset);
                bg = *c;
            }
            Token::ResetColor => {
                flush(&mut spans, &mut current_text, fg, bg, offset);
                fg = default_fg;
            }
            Token::ResetBgColor => {
                flush(&mut spans, &mut current_text, fg, bg, offset);
                bg = default_bg;
            }
            Token::Offset(x, y) => {
                flush(&mut spans, &mut current_text, fg, bg, offset);
                offset = Some((*x, *y));
            }
            Token::ResetOffset => {
                flush(&mut spans, &mut current_text, fg, bg, offset);
                offset = None;
            }
            Token::Compose => {
                flush(&mut spans, &mut current_text, fg, bg, offset);
                compose = true;
                // The compose flag applies to the next character only.
                // Handle at render time by not advancing the cursor.
            }
        }
    }
    flush(&mut spans, &mut current_text, fg, bg, offset);
    spans
}
```

#### Layout Engine

```rust
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAlign { Left, Center, Right }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VAlign { Top, Middle, Bottom }

/// A positioned glyph ready for rendering into the grid.
#[derive(Debug, Clone)]
pub struct PlacedGlyph {
    pub x: u16,
    pub y: u16,
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub offset: Option<(i16, i16)>,
    pub compose: bool, // if true, don't clear cell first
}

/// Measurement result.
#[derive(Debug, Clone)]
pub struct TextMetrics {
    pub width: u16,
    pub height: u16,
}

/// Measure text within a bounding width.
pub fn measure(spans: &[StyledSpan], max_width: u16) -> TextMetrics {
    let lines = wrap_lines(spans, max_width);
    let width = lines.iter().map(|l| l.width).max().unwrap_or(0);
    let height = lines.len() as u16;
    TextMetrics { width, height }
}

struct WrappedLine {
    /// Glyphs on this line with relative x positions (0-based).
    glyphs: Vec<(char, usize)>, // (char, span_index)
    width: u16,
}

/// Perform greedy word wrapping over styled spans.
/// Returns lines of characters with their span indices.
fn wrap_lines(spans: &[StyledSpan], max_width: u16) -> Vec<WrappedLine> {
    let mut lines: Vec<WrappedLine> = vec![WrappedLine {
        glyphs: Vec::new(),
        width: 0,
    }];
    let mut col: u16 = 0;
    let max_w = max_width as usize;

    for (span_idx, span) in spans.iter().enumerate() {
        // Split span text into words (preserving whitespace info)
        let mut word_start = 0;
        let chars: Vec<char> = span.text.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let ch = chars[i];

            if ch == '\n' {
                // Hard line break
                lines.push(WrappedLine { glyphs: Vec::new(), width: 0 });
                col = 0;
                i += 1;
                continue;
            }

            let ch_width = ch.width().unwrap_or(0) as u16;

            if col + ch_width > max_width && col > 0 {
                // Need to wrap. Try to break at last space on current line.
                let current_line = lines.last_mut().unwrap();
                if let Some(break_pos) = find_last_space(&current_line.glyphs) {
                    // Move characters after the break to a new line
                    let remainder: Vec<_> = current_line.glyphs.drain(break_pos + 1..).collect();
                    // Trim the trailing space
                    if let Some(last) = current_line.glyphs.last() {
                        if last.0 == ' ' {
                            current_line.glyphs.pop();
                        }
                    }
                    current_line.width = current_line.glyphs.iter()
                        .map(|(c, _)| c.width().unwrap_or(0) as u16)
                        .sum();

                    let new_width: u16 = remainder.iter()
                        .map(|(c, _)| c.width().unwrap_or(0) as u16)
                        .sum();
                    lines.push(WrappedLine { glyphs: remainder, width: new_width });
                    col = new_width;
                } else {
                    // No space found, force break
                    lines.push(WrappedLine { glyphs: Vec::new(), width: 0 });
                    col = 0;
                }
            }

            lines.last_mut().unwrap().glyphs.push((ch, span_idx));
            col += ch_width;
            lines.last_mut().unwrap().width = col;
            i += 1;
        }
    }

    lines
}

fn find_last_space(glyphs: &[(char, usize)]) -> Option<usize> {
    glyphs.iter().rposition(|(ch, _)| *ch == ' ')
}

/// Lay out text within a bounded rectangle with alignment.
pub fn layout(
    spans: &[StyledSpan],
    rect_x: u16,
    rect_y: u16,
    rect_w: u16,
    rect_h: u16,
    h_align: HAlign,
    v_align: VAlign,
) -> Vec<PlacedGlyph> {
    let lines = wrap_lines(spans, rect_w);
    let total_lines = lines.len().min(rect_h as usize);

    // Vertical alignment offset
    let y_offset = match v_align {
        VAlign::Top => 0,
        VAlign::Middle => (rect_h as usize).saturating_sub(total_lines) / 2,
        VAlign::Bottom => (rect_h as usize).saturating_sub(total_lines),
    };

    let mut placed = Vec::new();

    for (line_idx, line) in lines.iter().take(total_lines).enumerate() {
        // Horizontal alignment offset
        let x_offset = match h_align {
            HAlign::Left => 0u16,
            HAlign::Center => rect_w.saturating_sub(line.width) / 2,
            HAlign::Right => rect_w.saturating_sub(line.width),
        };

        let mut cx = 0u16;
        for &(ch, span_idx) in &line.glyphs {
            let span = &spans[span_idx];
            placed.push(PlacedGlyph {
                x: rect_x + x_offset + cx,
                y: rect_y + y_offset as u16 + line_idx as u16,
                ch,
                fg: span.fg.unwrap_or_default(),
                bg: span.bg.unwrap_or_default(),
                offset: span.offset,
                compose: span.compose,
            });
            cx += ch.width().unwrap_or(0) as u16;
        }
    }

    placed
}
```

### 10. Design Recommendations

1. **Two-layer architecture.** Separate the tag parser from the layout engine. The parser converts

   `&str` with tags into `Vec<StyledSpan>`. The layout engine takes `&[StyledSpan]` (or a
   ratatui-like `Line`/`Span` tree) and produces positioned glyphs. This allows:

   - Programmatic construction without parsing (like ratatui).
   - Data-driven markup for dialog/descriptions (like BearLibTerminal).
   - Caching parsed spans across frames.

1. **Use BearLibTerminal-style bracket tags**, not libtcod's byte-embedding. Bracket tags are

   human-readable, debuggable, and work naturally with Rust's `&str`. libtcod's approach was born
   from C-era constraints. rot.js's `%c{}` syntax is fine but less extensible.

1. **Greedy word wrapping is sufficient.** Knuth-Plass is overkill for monospace grids where

   character widths are uniform. The visual difference is negligible and the complexity cost is not
   justified.

1. **Measure and render should share code.** Use a single layout pass that produces

   `Vec<PlacedGlyph>`, then measure reads dimensions from that result and render writes glyphs to
   the grid. Alternatively, use a `count_only` flag like libtcod, but the separate-output approach
   is more Rust-idiomatic and avoids mutable state.

1. **Support both horizontal and vertical alignment.** BearLibTerminal's combinable alignment flags

   are the best API here. Use bitflags or two separate enums (HAlign + VAlign).

1. **Tag effects should be call-scoped.** Like BearLibTerminal, all color/offset changes within a

   `print` call should automatically reset when the call returns. No leaked state.

1. **Support `unicode-width`** for CJK double-width characters from the start. All modern roguelike

   libraries have had to retrofit this.

1. **Keep tag syntax minimal.** Start with `[color=X]`, `[bkcolor=X]`, `[/color]`, `[/bkcolor]`,

   `[U+XXXX]`. Add `[+]` and `[offset=x,y]` only if composition and sub-cell positioning are needed.
   Don't overdesign the tag set.

1. **Provide a `measure` function** that returns `TextMetrics { width, height }` for pre-layout

   sizing. This is critical for UI layout (sizing panels to fit content, centering dialogs, etc.).

1. **Consider a builder API** alongside tag parsing for programmatic use:

   ```rust
   Text::new()
       .fg(Color::RED).write("Warning: ")
       .reset().write("this is important")
       .build()
   ```

## Sources

- **Kept**: [BearLibTerminal Reference](http://foo.wyrd.name/en:bearlibterminal:reference) — primary

  documentation for print/measure/tags, comprehensive coverage of the tag system and alignment API

- **Kept**:

  [libtcod console_printing.c](https://github.com/libtcod/libtcod/blob/main/src/libtcod/console_printing.c)
  — full source code showing color control implementation, word wrapping, alignment, and measurement
  via count_only

- **Kept**: [rot.js text.ts](https://github.com/ondras/rot.js/blob/master/src/text.ts) — complete

  tokenizer and word-wrapping implementation in ~160 lines

- **Kept**: [rot.js display.ts](https://github.com/ondras/rot.js/blob/master/src/display/display.ts)

  — drawText rendering loop showing token interpretation

- **Kept**: [ratatui text module docs](https://docs.rs/ratatui/latest/ratatui/text/index.html) —

  Span/Line/Text hierarchy documentation

- **Kept**: [ratatui Span docs](https://docs.rs/ratatui/latest/ratatui/text/struct.Span.html) —

  detailed API including width(), styled_graphemes(), style patching

- **Kept**: [ratatui Line docs](https://docs.rs/ratatui/latest/ratatui/text/struct.Line.html) —

  alignment support, width calculation, span composition

- **Kept**:

  [Wikipedia: Line wrap and word wrap](https://en.wikipedia.org/wiki/Line_wrap_and_word_wrap) —
  greedy and Knuth-Plass algorithm descriptions, pseudocode

- **Dropped**: libtcod readthedocs main page — just an index with no content about color controls
- **Dropped**: libtcod 1.6.4 docs index — table of contents only, actual content in

  JavaScript-rendered frames that couldn't be fetched

## Gaps

- **BearLibTerminal `[font=X]` tag**: the reference docs do not document a `font` tag in the `print`

  function. Font switching may be handled at a different level (via `terminal_set`), or this tag may
  be undocumented. The source code (C++) could confirm, but was not accessible via the available
  URLs.

- **Knuth-Plass implementation details for monospace**: while the greedy algorithm is recommended,

  if Knuth-Plass were desired, no Rust crate was found that specifically targets monospace grids.
  The `textwrap` crate implements Knuth-Plass for proportional text.

- **libtcod newer C++ printing API**: the newer `tcod::print` C++ API may use a different approach

  to color formatting (possibly ANSI-like or tag-based). The C source analyzed here covers the
  legacy but still-active C API.

- **ratatui Paragraph wrapping implementation**: the actual word-wrapping code in ratatui's

  `Paragraph` widget was not examined. It would be worth reviewing for Rust-specific patterns.
