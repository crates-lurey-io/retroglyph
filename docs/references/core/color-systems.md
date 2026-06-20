# Color Systems Reference for Terminal/Grid Rendering

## Table of Contents

1. [sRGB vs Linear Color Space](#1-srgb-vs-linear-color-space)
2. [Alpha Blending in Linear vs sRGB Space](#2-alpha-blending-in-linear-vs-srgb-space)
3. [Terminal Color Palettes](#3-terminal-color-palettes)
4. [Named Color Databases](#4-named-color-databases)
5. [RGB to ANSI 256 Conversion](#5-rgb-to-ansi-256-conversion)
6. [WCAG Contrast Ratio Calculation](#6-wcag-contrast-ratio-calculation)
7. [Color Type Design in Rust](#7-color-type-design-in-rust)
8. [HSL/HSV Conversion](#8-hslhsv-conversion)
9. [Palette Generation and Color Harmony](#9-palette-generation-and-color-harmony)
10. [Relevant Rust Crates](#10-relevant-rust-crates)

---

## 1. sRGB vs Linear Color Space

### What is sRGB?

sRGB (standard Red Green Blue) is a non-linear color space designed in 1996 by HP and Microsoft. It
encodes brightness values using a transfer function (gamma curve) that allocates more precision to
dark values, matching human perception. When you see `rgb(128, 128, 128)` in CSS or a pixel value of
128 in a PNG, that's an sRGB value.

sRGB is the default color space for the web, image files (PNG, JPEG), and terminal emulators. An
sRGB value of 128 is **not** 50% of the physical light output; it's roughly 21.4% of linear
brightness.

### What is Linear RGB?

Linear RGB maps values proportionally to physical light intensity. A value of 0.5 means exactly half
the photons of 1.0. Mathematical operations (addition, multiplication, interpolation) produce
physically correct results only in linear space.

### The Transfer Functions

The sRGB specification defines a piecewise function, not a simple power curve:

```rust
/// Convert a single sRGB channel (0.0..1.0) to linear
fn srgb_to_linear(s: f64) -> f64 {
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// Convert a single linear channel (0.0..1.0) to sRGB
fn linear_to_srgb(l: f64) -> f64 {
    if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}
```

The threshold `0.04045` (and its corresponding linear threshold `0.0031308`) defines where the curve
switches from a linear segment (for near-black values) to the power curve. The WCAG spec
historically used `0.03928` instead, but `0.04045` is the IEC standard value. For 8-bit color values
the difference is negligible.

A common approximation uses a simple gamma of 2.2:

```rust
fn srgb_to_linear_approx(s: f64) -> f64 { s.powf(2.2) }
fn linear_to_srgb_approx(l: f64) -> f64 { l.powf(1.0 / 2.2) }
```

This approximation is within ~0.4% of the true sRGB curve and can be acceptable for terminal
rendering where precision demands are lower than in professional color management.

### When to Convert

For a terminal rendering library, the sRGB/linear distinction matters in two operations:

- **Color blending/interpolation**: Blending two sRGB values directly produces visually incorrect

  results. The midpoint of sRGB(0, 0, 0) and sRGB(255, 255, 255) is sRGB(128, 128, 128), but that's
  perceptually too dark. The correct midpoint is sRGB(188, 188, 188).

- **Luminance calculations**: WCAG contrast ratios require converting to linear RGB first.

For a cell-based terminal renderer that primarily sets foreground/background colors without
sub-pixel blending, you can often stay entirely in sRGB. The conversion becomes important if you
implement color interpolation, gradients, or alpha compositing.

### Precision Warning

Converting 8-bit sRGB to 8-bit linear RGB loses significant precision in dark values. The value 13
in sRGB maps to linear ~1, and values 1-12 all map to linear 0. If you need to work in linear space,
use at least `f32` or `u16` for intermediate values.

### Sources

- [sRGB Wikipedia / IEC 61966-2-1](https://en.wikipedia.org/wiki/SRGB)
- [Red Blob Games: sRGB WebGL](https://www.redblobgames.com/x/2445-srgb-webgl/)
- [palette crate: Linear and Non-linear RGB](https://docs.rs/palette/latest/palette/rgb/index.html)

---

## 2. Alpha Blending in Linear vs sRGB Space

### The Problem

Most software performs alpha blending directly on sRGB values. This is wrong, but widespread. The
standard "over" compositing formula is:

```text
result = src_color * src_alpha + dst_color * (1 - src_alpha)
```text

When `src_color` and `dst_color` are sRGB-encoded, the linear interpolation happens in non-linear
space, producing results that don't match physical light mixing.

### Visual Example

A checkerboard alternating between sRGB values 64 and 192 appears to average to sRGB ~146, not 128.
This is because the physical light from 192 contributes disproportionately more than 64. The correct
average requires converting to linear, averaging, then converting back:

```rust
fn blend_correct(a_srgb: u8, b_srgb: u8) -> u8 {
    let a_lin = srgb_to_linear(a_srgb as f64 / 255.0);
    let b_lin = srgb_to_linear(b_srgb as f64 / 255.0);
    let result_lin = (a_lin + b_lin) / 2.0;
    (linear_to_srgb(result_lin) * 255.0).round() as u8
}
// blend_correct(64, 192) => ~146, not 128
```

### Correct Alpha Blending

```rust
/// Alpha-blend src over dst, both in sRGB space.
/// Alpha is always linear (0.0 = transparent, 1.0 = opaque).
fn alpha_blend(
    src: [u8; 3], src_alpha: f64,
    dst: [u8; 3],
) -> [u8; 3] {
    let mut result = [0u8; 3];
    for i in 0..3 {
        let s = srgb_to_linear(src[i] as f64 / 255.0);
        let d = srgb_to_linear(dst[i] as f64 / 255.0);
        let blended = s * src_alpha + d * (1.0 - src_alpha);
        result[i] = (linear_to_srgb(blended) * 255.0).round() as u8;
    }
    result
}
```

Alpha values themselves are always linear (50% alpha means 50% coverage). Only the color channels
need the sRGB conversion.

### Pre-multiplied Alpha

In pre-multiplied alpha, color channels are stored as `color * alpha`. This makes compositing
simpler and avoids artifacts at transparent edges:

```rust
// Pre-multiplied alpha compositing (in linear space):
// result = src + dst * (1 - src_alpha)
```

Pre-multiplied alpha is standard in GPU rendering but rarely used in terminal contexts.

### Practical Decision for Terminal Libraries

Terminal emulators themselves perform no alpha blending; they receive final RGB values via escape
sequences. If your library supports layered cells or translucent overlays, you must composite them
before sending to the terminal. Whether to blend in linear or sRGB space depends on your quality
requirements. Blending in sRGB is simpler and still common in terminal applications. For a roguelike
or TUI, the visual difference is minor. For a pixel-art renderer or color-critical tool, linear
blending is worth the extra computation.

### Sources (2)

- [Fractolog: Color Space Correctness in Alpha Blending](https://www.fractolog.com/2024/07/color-space-correctness-in-alpha-blending/)
- [The Hacks of Life: sRGB, Pre-Multiplied Alpha, and Compression](http://hacksoflife.blogspot.com/2022/06/srgb-pre-multiplied-alpha-and.html)
- [RenderWonk: Adventures with Gamma-Correct Rendering](https://renderwonk.com/blog/index.php/archive/adventures-with-gamma-correct-rendering/)

---

## 3. Terminal Color Palettes

Three generations of terminal color coexist. Every modern terminal emulator supports all three.

### ANSI 16 Colors (SGR codes 30-37, 40-47, 90-97, 100-107)

The original VT100-era model. 8 base colors + 8 bright variants, accessed through SGR (Select
Graphic Rendition) codes:

| Index | Name                | FG Code | BG Code | Typical sRGB Value |
| ----- | ------------------- | ------- | ------- | ------------------ |
| 0     | Black               | 30      | 40      | #000000            |
| 1     | Red                 | 31      | 41      | #AA0000            |
| 2     | Green               | 32      | 42      | #00AA00            |
| 3     | Yellow              | 33      | 43      | #AA5500            |
| 4     | Blue                | 34      | 44      | #0000AA            |
| 5     | Magenta             | 35      | 45      | #AA00AA            |
| 6     | Cyan                | 36      | 46      | #00AAAA            |
| 7     | White               | 37      | 47      | #AAAAAA            |
| 8     | Bright Black (Gray) | 90      | 100     | #555555            |
| 9     | Bright Red          | 91      | 101     | #FF5555            |
| 10    | Bright Green        | 92      | 102     | #55FF55            |
| 11    | Bright Yellow       | 93      | 103     | #FFFF55            |
| 12    | Bright Blue         | 94      | 104     | #5555FF            |
| 13    | Bright Magenta      | 95      | 105     | #FF55FF            |
| 14    | Bright Cyan         | 96      | 106     | #55FFFF            |
| 15    | Bright White        | 97      | 107     | #FFFFFF            |

The actual RGB values are **user-configurable** in every modern terminal. The values above are xterm
defaults. Many terminals ship with different schemes (e.g., Solarized, Dracula, Gruvbox remap these
indices).

Escape sequence format:

```text
ESC[31m        # Set foreground to Red (index 1)
ESC[42m        # Set background to Green (index 2)
ESC[1;34m      # Bold + Blue (historically bold = bright)
ESC[0m         # Reset all attributes
```text

### 256-Color Palette (SGR 38;5;n / 48;5;n)

The xterm 256-color extension, structured as three regions:

| Range   | Description             | Count |
| ------- | ----------------------- | ----- |
| 0-15    | Standard ANSI 16 colors | 16    |
| 16-231  | 6x6x6 RGB color cube    | 216   |
| 232-255 | Grayscale ramp          | 24    |

**Color cube formula** (indices 16-231):

```rust
/// Convert RGB (each 0-5) to 256-color index
fn rgb_to_cube_index(r: u8, g: u8, b: u8) -> u8 {
    16 + 36 * r + 6 * g + b
}

/// Convert 256-color cube index to approximate sRGB
fn cube_index_to_rgb(index: u8) -> (u8, u8, u8) {
    let index = index - 16;
    let r = index / 36;
    let g = (index % 36) / 6;
    let b = index % 6;
    // The 6 levels map to these sRGB values:
    let to_srgb = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
    (to_srgb(r), to_srgb(g), to_srgb(b))
}
// Levels: 0 -> 0, 1 -> 95, 2 -> 135, 3 -> 175, 4 -> 215, 5 -> 255
```

**Grayscale ramp** (indices 232-255):

```rust
/// 24 shades of gray, from dark (232) to light (255)
fn gray_index_to_rgb(index: u8) -> u8 {
    8 + 10 * (index - 232)
    // 232 -> 8, 233 -> 18, ..., 255 -> 238
}
```

Note the grayscale ramp does not include pure black (0) or pure white (255); those are at indices
0/16 and 15/231.

Escape sequence format:

```text
ESC[38;5;196m   # Foreground: index 196 (bright red from cube)
ESC[48;5;232m   # Background: index 232 (near-black gray)
```text

### 24-bit Truecolor (SGR 38;2;r;g;b / 48;2;r;g;b)

Direct RGB specification, 16.7 million colors. Supported by most modern terminals (iTerm2,
Alacritty, Kitty, WezTerm, Windows Terminal, GNOME Terminal, Ghostty).

Escape sequence format:

```text
ESC[38;2;255;128;0m    # Foreground: orange
ESC[48;2;0;0;64m       # Background: dark navy
```text

There's also a colon-separated variant (`38:2::r:g:b:m`) from the ITU T.416 standard, but semicolons
are more widely supported.

**Detection**: Check `$COLORTERM` for `truecolor` or `24bit`. The `TERM` variable is unreliable for
this purpose.

```rust
fn supports_truecolor() -> bool {
    std::env::var("COLORTERM")
        .map(|v| v == "truecolor" || v == "24bit")
        .unwrap_or(false)
}
```

### Sources (3)

- [Terminal Color Fundamentals | Terminfo.dev](https://terminfo.dev/fundamentals/color-fundamentals)
- [24-bit truecolor | Terminfo.dev](https://terminfo.dev/extensions/24-bit-truecolor)
- [XTerm Control Sequences](https://invisible-island.net/xterm/ctlseqs/ctlseqs.pdf)

---

## 4. Named Color Databases

### X11 Colors (rgb.txt)

The X11 color name database originated in 1985 with the X Window System. It lives in
`/usr/share/X11/rgb.txt` (or is compiled into the X server). It defines ~750 entries (including
numbered variants like `"Gray0"` through `"Gray100"` and `"Blue1"` through `"Blue4"`).

Key characteristics:

- Names are case-insensitive and space-insensitive (`"DarkGreen"` = `"dark green"`)
- Many colors have numbered brightness variants (1=100%, 2=93.2%, 3=80.4%, 4=54.8%)
- About 140 unique base color names

### CSS/W3C Named Colors

CSS adopted the X11 list with modifications. The current CSS Color Level 4 spec defines 148 named
colors (147 + `rebeccapurple`). Notable clashes with X11:

| Name   | X11 Value     | CSS/W3C Value | Note                              |
| ------ | ------------- | ------------- | --------------------------------- |
| Gray   | #BEBEBE (75%) | #808080 (50%) | Major difference                  |
| Green  | #00FF00       | #008000       | X11 green = CSS lime              |
| Maroon | #B03060       | #800000       | Different hue entirely            |
| Purple | #A020F0       | #800080       | X11 is violet, CSS is deep purple |

The 16 original HTML colors (black, silver, gray, white, maroon, red, purple, fuchsia, green, lime,
olive, yellow, navy, blue, teal, aqua) date back to HTML 3.2 / VGA.

### BearLibTerminal Named Colors

BearLibTerminal's `color_from_name()` uses a different scheme, specified as `"[brightness] hue"`:

**Base hues**: grey/gray, red, flame, orange, amber, yellow, lime, chartreuse, green, sea,
turquoise, cyan, sky, azure, blue, han, violet, purple, fuchsia, magenta, pink, crimson, transparent

**Brightness modifiers**: lightest, lighter, light, dark, darker, darkest

**Format variants**:

- By name: `"red"`, `"light green"`, `"darker cyan"`
- Hex: `"#RRGGBB"` or `"#AARRGGBB"`
- Decimal: `"R,G,B"` or `"A,R,G,B"`
- Custom palette: add names via `terminal_set("palette.octarine = #50FF25")`

This is useful as a design pattern: rather than trying to support hundreds of X11 names, a small set
of hue names with brightness modifiers covers most use cases and is easier to implement.

### Recommended Approach for a Library

A practical named color implementation for a terminal library:

```rust
/// Named color support with multiple resolution strategies
fn color_from_name(name: &str) -> Option<(u8, u8, u8)> {
    // 1. Check hex format: #RGB, #RRGGBB, #AARRGGBB
    if name.starts_with('#') {
        return parse_hex(name);
    }

    // 2. Check CSS/W3C named colors (148 entries)
    if let Some(rgb) = CSS_COLORS.get(name.to_lowercase().as_str()) {
        return Some(*rgb);
    }

    // 3. Optionally: parse "light red", "dark blue" modifiers
    if let Some(rgb) = parse_modified_color(name) {
        return Some(rgb);
    }

    None
}
```

### Sources (4)

- [X11 color names - Wikipedia](https://en.wikipedia.org/wiki/X11_color_names)
- [CSS `<named-color>` - MDN](https://developer.mozilla.org/en-US/docs/Web/CSS/named-color)
- [BearLibTerminal Reference: color_from_name](http://foo.wyrd.name/en:bearlibterminal:reference#color_from_name)

---

## 5. RGB to ANSI 256 Conversion

When a terminal only supports 256 colors, truecolor RGB values must be mapped to the nearest palette
entry. There are several approaches with different accuracy/performance tradeoffs.

### Simple Euclidean Distance (in sRGB space)

The most common approach: find the palette entry with minimum Euclidean distance in RGB space.

```rust
/// The 6 levels used in the xterm 256-color cube
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Convert RGB to nearest xterm 256-color index
fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    // Check if it's close to a grayscale value
    if r == g && g == b {
        if r < 8 { return 16; }       // near-black -> black in cube
        if r > 248 { return 231; }     // near-white -> white in cube
        return 232 + ((r as f64 - 8.0) / 10.0).round() as u8;
    }

    // Find nearest point in the 6x6x6 cube
    let ri = nearest_cube_index(r);
    let gi = nearest_cube_index(g);
    let bi = nearest_cube_index(b);

    // Also check the nearest grayscale
    let gray_avg = (r as u16 + g as u16 + b as u16) / 3;
    let gray_idx = if gray_avg < 8 {
        16
    } else if gray_avg > 248 {
        231
    } else {
        232 + ((gray_avg as f64 - 8.0) / 10.0).round() as u8
    };
    let gray_val = if gray_idx >= 232 {
        8 + 10 * (gray_idx - 232) as u16
    } else if gray_idx == 16 { 0 } else { 255 };

    let cube_r = CUBE_LEVELS[ri as usize];
    let cube_g = CUBE_LEVELS[gi as usize];
    let cube_b = CUBE_LEVELS[bi as usize];

    let cube_dist = color_dist(r, g, b, cube_r, cube_g, cube_b);
    let gray_dist = color_dist(r, g, b, gray_val as u8, gray_val as u8, gray_val as u8);

    if gray_dist < cube_dist {
        gray_idx
    } else {
        16 + 36 * ri + 6 * gi + bi
    }
}

fn nearest_cube_index(v: u8) -> u8 {
    match v {
        0..=47 => 0,
        48..=114 => 1,
        115..=154 => 2,
        155..=194 => 3,
        195..=234 => 4,
        235..=255 => 5,
        _ => unreachable!(),
    }
}

fn color_dist(r1: u8, g1: u8, b1: u8, r2: u8, g2: u8, b2: u8) -> i32 {
    let dr = r1 as i32 - r2 as i32;
    let dg = g1 as i32 - g2 as i32;
    let db = b1 as i32 - b2 as i32;
    dr * dr + dg * dg + db * db
}
```

### Perceptually-Weighted Distance

Human eyes are most sensitive to green, then red, then blue. Weighted Euclidean distance improves
results:

```rust
fn weighted_color_dist(r1: u8, g1: u8, b1: u8, r2: u8, g2: u8, b2: u8) -> i32 {
    let dr = r1 as i32 - r2 as i32;
    let dg = g1 as i32 - g2 as i32;
    let db = b1 as i32 - b2 as i32;
    // Common weights: R=2 or 3, G=4, B=1
    2 * dr * dr + 4 * dg * dg + db * db
}
```

### CIEDE2000 (Most Accurate)

The `rgbto256` tool by taylordotfish converts via the CIEDE2000 color difference formula in CIELAB
space. This is the most perceptually accurate but significantly more expensive. For a terminal
library, this is overkill unless color fidelity is critical.

### The ansi_colours Approach

The `ansi_colours` crate (by mina86) balances accuracy and performance. It avoids brute-force
iteration over all 256 entries by:

1. Computing the nearest cube entry analytically (not by iterating all 216 cube colors)
2. Computing the nearest grayscale entry analytically
3. Comparing the two candidates using a perceptually-weighted metric

This is the approach used by the `rgb2ansi256` crate (a pure-Rust port of `ansi_colours`'s C
implementation), which also supports `const fn` for compile-time conversion.

### Reverse Conversion (256 to RGB)

```rust
fn ansi256_to_rgb(index: u8) -> (u8, u8, u8) {
    match index {
        // ANSI 16: use a standard table
        0..=15 => ANSI_16_TABLE[index as usize],
        // Color cube
        16..=231 => {
            let i = index - 16;
            let r = i / 36;
            let g = (i % 36) / 6;
            let b = i % 6;
            let f = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            (f(r), f(g), f(b))
        }
        // Grayscale ramp
        232..=255 => {
            let v = 8 + 10 * (index - 232);
            (v, v, v)
        }
    }
}
```

### Sources (5)

- [ansi_colours crate](https://docs.rs/ansi_colours/latest/ansi_colours/)
- [rgb2ansi256 crate](https://github.com/rhysd/rgb2ansi256)
- [rgbto256 (CIEDE2000 approach)](https://github.com/taylordotfish/rgbto256)

---

## 6. WCAG Contrast Ratio Calculation

The Web Content Accessibility Guidelines define contrast ratio in terms of relative luminance, which
requires converting sRGB values to linear light first.

### Step 1: Relative Luminance

```rust
/// Calculate relative luminance per WCAG 2.x.
/// Input: sRGB values 0-255.
/// Output: luminance 0.0 (black) to 1.0 (white).
fn relative_luminance(r: u8, g: u8, b: u8) -> f64 {
    let r_lin = srgb_channel_to_linear(r as f64 / 255.0);
    let g_lin = srgb_channel_to_linear(g as f64 / 255.0);
    let b_lin = srgb_channel_to_linear(b as f64 / 255.0);

    0.2126 * r_lin + 0.7152 * g_lin + 0.0722 * b_lin
}

fn srgb_channel_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
```

The coefficients `0.2126, 0.7152, 0.0722` are the ITU-R BT.709 luminance weights (the same standard
sRGB is based on). They reflect that green contributes most to perceived brightness.

### Step 2: Contrast Ratio

```rust
/// Calculate WCAG contrast ratio between two colors.
/// Returns a value from 1.0 (identical) to 21.0 (black on white).
fn contrast_ratio(
    r1: u8, g1: u8, b1: u8,
    r2: u8, g2: u8, b2: u8,
) -> f64 {
    let l1 = relative_luminance(r1, g1, b1);
    let l2 = relative_luminance(r2, g2, b2);

    let lighter = l1.max(l2);
    let darker = l1.min(l2);

    (lighter + 0.05) / (darker + 0.05)
}
```

The `0.05` offset accounts for ambient light contribution (from IEC-4WD).

### WCAG Thresholds

| Level | Text Size                          | Minimum Ratio |
| ----- | ---------------------------------- | ------------- |
| AA    | Normal text (< 18pt, < 14pt bold)  | 4.5:1         |
| AA    | Large text (>= 18pt, >= 14pt bold) | 3:1           |
| AAA   | Normal text                        | 7:1           |
| AAA   | Large text                         | 4.5:1         |

### Practical Use: Auto-Selecting Foreground Color

```rust
/// Pick black or white text for best readability on the given background.
fn best_foreground(bg_r: u8, bg_g: u8, bg_b: u8) -> (u8, u8, u8) {
    let lum = relative_luminance(bg_r, bg_g, bg_b);
    // White text on dark backgrounds, black text on light backgrounds.
    // The threshold ~0.179 is where contrast ratios with black and white are equal.
    if lum > 0.179 {
        (0, 0, 0) // black
    } else {
        (255, 255, 255) // white
    }
}
```

### Sources (6)

- [WCAG 2.x: Relative Luminance](https://www.w3.org/WAI/GL/wiki/Relative_luminance)
- [WCAG Technique G17: 7:1 Contrast Ratio](https://www.w3.org/WAI/WCAG22/Techniques/general/G17)
- [Understanding SC 1.4.6: Contrast (Enhanced)](https://w3c.github.io/wcag/understanding/contrast-enhanced)

---

## 7. Color Type Design in Rust

### Ratatui's Color Enum

Ratatui defines a `Color` enum that covers all three terminal color generations:

```rust
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Color {
    #[default]
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    White,
    Rgb(u8, u8, u8),
    Indexed(u8),
}
```

Key design decisions:

- Named variants for the 16 ANSI colors provide ergonomic usage
- `Rgb(u8, u8, u8)` for truecolor
- `Indexed(u8)` for the 256-color palette
- `Reset` restores the terminal's default color
- `Default` is `Reset`, not black; the library doesn't assume what the terminal's default is
- `Copy` + `Clone`; colors are small values always passed by copy
- No alpha channel; terminals don't support transparency at the cell level

### Crossterm's Color Enum

Crossterm uses a different naming convention (matching the terminal's perspective where base colors
are "dark" and bright variants are the default):

```rust
pub enum Color {
    Reset,
    Black,
    DarkGrey,
    Red,        // This is the "bright" red
    DarkRed,    // This is ANSI red
    Green,
    DarkGreen,
    Yellow,
    DarkYellow,
    Blue,
    DarkBlue,
    Magenta,
    DarkMagenta,
    Cyan,
    DarkCyan,
    White,
    Grey,
    Rgb { r: u8, g: u8, b: u8 },
    AnsiValue(u8),
}
```

The naming inversion between ratatui and crossterm (ratatui's `Red` = crossterm's `DarkRed`) is a
common source of confusion. Ratatui provides `From`/`Into` conversion impls.

### BearLibTerminal's Color Representation

BearLibTerminal uses a raw 32-bit packed format: `color_t` is `u32` in BGRA/0xAARRGGBB layout. It
provides alpha because BearLibTerminal renders via OpenGL, not terminal escape sequences. Colors are
constructed via:

- `color_from_argb(a, r, g, b)` for components
- `color_from_name("red")` for named colors

### Suggested Design for a Grid Library

```rust
/// A color that can represent any terminal color mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    /// Terminal's default foreground/background
    Default,
    /// One of the 16 ANSI colors (indices 0-15)
    Ansi(AnsiColor),
    /// 256-color palette index
    Indexed(u8),
    /// 24-bit truecolor
    Rgb { r: u8, g: u8, b: u8 },
}

/// The 16 standard ANSI colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AnsiColor {
    Black = 0,
    Red = 1,
    Green = 2,
    Yellow = 3,
    Blue = 4,
    Magenta = 5,
    Cyan = 6,
    White = 7,
    BrightBlack = 8,
    BrightRed = 9,
    BrightGreen = 10,
    BrightYellow = 11,
    BrightBlue = 12,
    BrightMagenta = 13,
    BrightCyan = 14,
    BrightWhite = 15,
}

/// RGBA color for compositing before terminal output.
/// Used internally when layers need alpha blending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color::Rgb { r, g, b }
    }

    /// Convert any color to an approximate RGB value.
    /// ANSI colors use a standard table; Indexed uses xterm values.
    pub fn to_rgb(&self) -> Option<(u8, u8, u8)> {
        match self {
            Color::Default => None, // Unknown without terminal context
            Color::Ansi(c) => Some(ANSI_16_RGB[*c as usize]),
            Color::Indexed(i) => Some(ansi256_to_rgb(*i)),
            Color::Rgb { r, g, b } => Some((*r, *g, *b)),
        }
    }

    /// Downgrade this color to a 256-color index.
    pub fn to_indexed(&self) -> Option<u8> {
        match self {
            Color::Default => None,
            Color::Ansi(c) => Some(*c as u8),
            Color::Indexed(i) => Some(*i),
            Color::Rgb { r, g, b } => Some(rgb_to_ansi256(*r, *g, *b)),
        }
    }
}
```

Design principles:

- Separate `AnsiColor` enum with `repr(u8)` for zero-cost conversion to index
- `Default` rather than `Reset` (describes what it is, not what escape to emit)
- `to_rgb()` returns `Option` because `Default` has no fixed RGB value
- `to_indexed()` for automatic fallback on 256-color terminals
- `Rgba` as a separate type for internal compositing, not in the public `Color` enum

### Sources (7)

- [ratatui Color enum source](https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/style/color.rs)
- [crossterm Color enum](https://docs.rs/crossterm/latest/crossterm/style/enum.Color.html)
- [BearLibTerminal reference](http://foo.wyrd.name/en:bearlibterminal:reference#color)

---

## 8. HSL/HSV Conversion

HSL (Hue, Saturation, Lightness) and HSV (Hue, Saturation, Value) are cylindrical representations of
RGB useful for color manipulation: adjusting brightness, generating harmonies, desaturation.

### RGB to HSL

```rust
/// Convert RGB (0-255 each) to HSL.
/// Returns (h: 0.0-360.0, s: 0.0-1.0, l: 0.0-1.0)
fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let r = r as f64 / 255.0;
    let g = g as f64 / 255.0;
    let b = b as f64 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;

    if (max - min).abs() < f64::EPSILON {
        return (0.0, 0.0, l); // achromatic
    }

    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };

    let h = if (max - r).abs() < f64::EPSILON {
        let mut h = (g - b) / d;
        if g < b { h += 6.0; }
        h
    } else if (max - g).abs() < f64::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };

    (h * 60.0, s, l)
}
```

### HSL to RGB

```rust
/// Convert HSL to RGB.
/// Input: h: 0.0-360.0, s: 0.0-1.0, l: 0.0-1.0
/// Output: (r, g, b) each 0-255
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    if s.abs() < f64::EPSILON {
        let v = (l * 255.0).round() as u8;
        return (v, v, v); // achromatic
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let h_norm = h / 360.0;

    let to_rgb = |t: f64| -> u8 {
        let t = ((t % 1.0) + 1.0) % 1.0; // normalize to 0..1
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 1.0 / 2.0 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v * 255.0).round() as u8
    };

    (
        to_rgb(h_norm + 1.0 / 3.0),
        to_rgb(h_norm),
        to_rgb(h_norm - 1.0 / 3.0),
    )
}
```

### RGB to HSV

```rust
/// Convert RGB to HSV.
/// Returns (h: 0.0-360.0, s: 0.0-1.0, v: 0.0-1.0)
fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let r = r as f64 / 255.0;
    let g = g as f64 / 255.0;
    let b = b as f64 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;

    let v = max;
    let s = if max.abs() < f64::EPSILON { 0.0 } else { d / max };

    if d.abs() < f64::EPSILON {
        return (0.0, s, v);
    }

    let h = if (max - r).abs() < f64::EPSILON {
        let mut h = (g - b) / d;
        if g < b { h += 6.0; }
        h
    } else if (max - g).abs() < f64::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };

    (h * 60.0, s, v)
}
```

### HSL vs HSV: When to Use Which

- **HSL** is better for picking lighter/darker variants of a color. `L=0` is always black, `L=1` is

  always white, `L=0.5` is the "pure" color. The `lighten` and `darken` operations in CSS work in
  HSL.

- **HSV** is better for color pickers and understanding "how much color vs. how much white." `V=0`

  is always black, `S=0` is always white/gray.

- For **color harmony** operations (complementary, analogous), both work equally well since they

  share the same hue angle.

- The `palette` crate uses `Hsl` and `Hsv` types in its own color space system. `colorsys` also

  provides `Hsl`.

### Color Manipulation Examples

```rust
/// Lighten a color by a percentage (0.0-1.0)
fn lighten(r: u8, g: u8, b: u8, amount: f64) -> (u8, u8, u8) {
    let (h, s, l) = rgb_to_hsl(r, g, b);
    let l = (l + amount).min(1.0);
    hsl_to_rgb(h, s, l)
}

/// Darken a color by a percentage
fn darken(r: u8, g: u8, b: u8, amount: f64) -> (u8, u8, u8) {
    let (h, s, l) = rgb_to_hsl(r, g, b);
    let l = (l - amount).max(0.0);
    hsl_to_rgb(h, s, l)
}

/// Desaturate (toward gray)
fn desaturate(r: u8, g: u8, b: u8, amount: f64) -> (u8, u8, u8) {
    let (h, s, l) = rgb_to_hsl(r, g, b);
    let s = (s - amount).max(0.0);
    hsl_to_rgb(h, s, l)
}

/// Invert a color
fn invert(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    (255 - r, 255 - g, 255 - b)
}
```

### Sources (8)

- [HSL and HSV - Wikipedia](https://en.wikipedia.org/wiki/HSL_and_HSV)
- [Color conversion algorithms (JavaScript reference)](https://gist.github.com/mjijackson/5311256)

---

## 9. Palette Generation and Color Harmony

Color harmony rules generate aesthetically pleasing color combinations by rotating around the
HSL/HSV hue wheel.

### Harmony Types

All formulas below operate on hue (0-360 degrees) and preserve the original saturation and
lightness:

```rust
/// Generate harmony colors from a base color
fn complementary(h: f64) -> Vec<f64> {
    vec![h, (h + 180.0) % 360.0]
}

fn analogous(h: f64) -> Vec<f64> {
    vec![
        (h + 330.0) % 360.0, // -30
        h,
        (h + 30.0) % 360.0,  // +30
    ]
}

fn triadic(h: f64) -> Vec<f64> {
    vec![h, (h + 120.0) % 360.0, (h + 240.0) % 360.0]
}

fn split_complementary(h: f64) -> Vec<f64> {
    vec![
        h,
        (h + 150.0) % 360.0,
        (h + 210.0) % 360.0,
    ]
}

fn tetradic(h: f64) -> Vec<f64> {
    vec![
        h,
        (h + 90.0) % 360.0,
        (h + 180.0) % 360.0,
        (h + 270.0) % 360.0,
    ]
}
```

### Generating Shade Ramps

For terminal UIs, generating a shade ramp from a single base color is useful for borders,
highlights, and backgrounds:

```rust
/// Generate N shades from dark to light for a given hue/saturation
fn shade_ramp(h: f64, s: f64, n: usize) -> Vec<(u8, u8, u8)> {
    (0..n)
        .map(|i| {
            let l = (i as f64 + 0.5) / n as f64; // avoid pure black/white
            hsl_to_rgb(h, s, l)
        })
        .collect()
}

/// Generate a monochromatic palette: base + lighter + darker variants
fn monochromatic(r: u8, g: u8, b: u8, steps: usize) -> Vec<(u8, u8, u8)> {
    let (h, s, _) = rgb_to_hsl(r, g, b);
    shade_ramp(h, s, steps)
}
```

### Perceptually Uniform Gradients

For more visually even gradients, interpolate in CIELAB or OKLCH space rather than HSL. The
`palette` crate supports this via its `Lab` and `Oklch` types. HSL gradients can produce unexpected
bright spots (e.g., the cyan/yellow artifacts when interpolating between red and green).

### Practical: Terminal Theme Generation

```rust
/// Generate a basic terminal theme from a single accent color
fn generate_theme(accent_r: u8, accent_g: u8, accent_b: u8) -> Theme {
    let (h, s, _) = rgb_to_hsl(accent_r, accent_g, accent_b);

    Theme {
        background: hsl_to_rgb(h, s * 0.1, 0.08),      // very dark, low-sat
        surface: hsl_to_rgb(h, s * 0.1, 0.12),          // slightly lighter
        foreground: hsl_to_rgb(h, s * 0.05, 0.85),      // light, nearly white
        accent: (accent_r, accent_g, accent_b),
        accent_dim: hsl_to_rgb(h, s * 0.7, 0.3),
        complement: {
            let ch = (h + 180.0) % 360.0;
            hsl_to_rgb(ch, s, 0.5)
        },
        error: hsl_to_rgb(0.0, 0.8, 0.5),               // red
        warning: hsl_to_rgb(45.0, 0.9, 0.5),             // amber
        success: hsl_to_rgb(120.0, 0.6, 0.4),            // green
    }
}
```

---

## 10. Relevant Rust Crates

### palette (comprehensive, type-safe)

The `palette` crate is the most complete color library in the Rust ecosystem. It encodes color
spaces as type parameters, making it impossible to accidentally blend sRGB values in linear space.

```rust
use palette::{Srgb, LinSrgb, IntoColor, Hsl};

// Parse and convert
let srgb = Srgb::new(0.5f32, 0.3, 0.8);
let linear: LinSrgb = srgb.into_linear();

// Blend in linear space (correct)
let a: LinSrgb = Srgb::new(1.0f32, 0.0, 0.0).into_linear();
let b: LinSrgb = Srgb::new(0.0f32, 0.0, 1.0).into_linear();
let mixed = LinSrgb::new(
    (a.red + b.red) / 2.0,
    (a.green + b.green) / 2.0,
    (a.blue + b.blue) / 2.0,
);
let result: Srgb = mixed.into_encoding();

// Convert to HSL
let hsl: Hsl = srgb.into_color();
```

Key types: `Srgb`, `Srgba`, `LinSrgb`, `LinSrgba`, `Hsl`, `Hsv`, `Lab`, `Oklch`, `Lch`

Pros: Correct by construction, extensive color space support, no_std compatible. Cons: Complex type
system, steep learning curve, heavy dependency for simple use cases.

### ansi_colours (256-color conversion)

Focused on one thing: converting between truecolor and the xterm 256-color palette.

```rust
use ansi_colours::{ansi256_from_rgb, rgb_from_ansi256};

let idx = ansi256_from_rgb((100u8, 200, 150));
let (r, g, b) = rgb_from_ansi256(idx);
```

Pros: Fast, accurate, interop with `rgb`, `ansi_term`, `termcolor` crates. Cons: Only does 256-color
conversion; no other color operations.

### rgb2ansi256 (compile-time 256-color conversion)

A pure-Rust port of ansi_colours with `const fn` support:

```rust
use rgb2ansi256::rgb_to_ansi256;

// Compute at compile time
const SPRING_GREEN: u8 = rgb_to_ansi256(0, 255, 175);
```

Pros: Zero dependencies, const fn, no unsafe code. Cons: LGPL-3.0 license (inherited from
ansi_colours C library).

### colorsys (HSL/RGB/CMYK conversion)

General-purpose color conversion with manipulation methods:

```rust
use colorsys::{Rgb, Hsl, ColorTransform, ColorAlpha};

let rgb = Rgb::from((245.0, 152.0, 53.0));
let hsl: Hsl = (&rgb).into();

let mut rgb2 = rgb.clone();
rgb2.lighten(20.0);           // Lighten by 20%
rgb2.saturate(colorsys::SaturationInSpace::Hsl(10.0));

let hex = rgb.to_hex_string(); // "#f59835"
let css = rgb.to_css_string(); // "rgb(245,152,53)"
```

Pros: Simple API, `no_std` support, CSS string parsing, includes Ansi256 type. Cons: Uses `f64`
throughout (heavier than needed for terminal colors).

### Summary Table

| Crate          | Purpose                   | Size  | no_std | Key Feature                 |
| -------------- | ------------------------- | ----- | ------ | --------------------------- |
| `palette`      | Full color science        | Large | Yes    | Type-safe color spaces      |
| `ansi_colours` | RGB <-> 256               | Tiny  | Yes    | Fast, accurate matching     |
| `rgb2ansi256`  | RGB -> 256                | Tiny  | Yes    | `const fn` support          |
| `colorsys`     | Conversion + manipulation | Small | Yes    | HSL lighten/darken/saturate |

### Recommendation

For a terminal/grid rendering library that wants minimal dependencies:

1. **Implement core color math inline** (sRGB<->linear, HSL conversion, contrast ratio). The

   formulas are short and stable.

1. **Use `ansi_colours` or `rgb2ansi256`** for 256-color fallback. The algorithm is tricky to get

   right and these crates are tiny.

1. **Make `palette` optional** behind a feature flag for users who want perceptually-uniform

   interpolation, CIELAB, OKLCH, or other advanced features.

```toml
[dependencies]
rgb2ansi256 = "0.1"

[dependencies.palette]
version = "0.7"
optional = true
default-features = false
features = ["std"]

[features]
default = []
palette = ["dep:palette"]
```

---

## Appendix: Quick Reference

### sRGB to Linear (single channel)

```text
if srgb <= 0.04045: linear = srgb / 12.92
else:               linear = ((srgb + 0.055) / 1.055) ^ 2.4
```text

### Linear to sRGB (single channel)

```text
if linear <= 0.0031308: srgb = linear * 12.92
else:                    srgb = 1.055 * linear ^ (1/2.4) - 0.055
```text

### Relative Luminance

```text
L = 0.2126 * R_linear + 0.7152 * G_linear + 0.0722 * B_linear
```text

### Contrast Ratio

```text
CR = (L_lighter + 0.05) / (L_darker + 0.05)
```text

### 256-Color Cube Levels

```yaml
Index:  0    1    2    3    4    5
Value:  0   95  135  175  215  255
```text

### 256-Color Grayscale Ramp

```text
Index 232-255 -> value = 8 + 10 * (index - 232)
Range: 8, 18, 28, ..., 238
```text

### Terminal Escape Sequences

```text
ANSI 16 FG:     ESC[30m .. ESC[37m, ESC[90m .. ESC[97m
ANSI 16 BG:     ESC[40m .. ESC[47m, ESC[100m .. ESC[107m
256-color FG:   ESC[38;5;{n}m
256-color BG:   ESC[48;5;{n}m
Truecolor FG:   ESC[38;2;{r};{g};{b}m
Truecolor BG:   ESC[48;2;{r};{g};{b}m
Reset:          ESC[0m
```rust

---

## Sources (9)

### Kept

- [Red Blob Games: sRGB WebGL](https://www.redblobgames.com/x/2445-srgb-webgl/) - Clear explanation

  of sRGB/linear with precision analysis

- [Fractolog: Color Space Correctness in Alpha Blending](https://www.fractolog.com/2024/07/color-space-correctness-in-alpha-blending/) -

  Empirical testing of alpha blending in different color spaces

- [palette crate docs](https://docs.rs/palette/latest/palette/) - Authoritative Rust color library

  documentation

- [palette::rgb module](https://docs.rs/palette/latest/palette/rgb/index.html) - Explains linear vs

  non-linear RGB from Rust perspective

- [Terminfo.dev: Color Fundamentals](https://terminfo.dev/fundamentals/color-fundamentals) - Modern

  reference for terminal color systems

- [Terminfo.dev: 24-bit Truecolor](https://terminfo.dev/extensions/24-bit-truecolor) - Truecolor

  escape sequence spec

- [W3C: Relative Luminance](https://www.w3.org/WAI/GL/wiki/Relative_luminance) - Official WCAG

  luminance formula

- [W3C: Technique G17](https://www.w3.org/WAI/WCAG22/Techniques/general/G17) - Contrast ratio

  calculation

- [ratatui Color source](https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/style/color.rs) -

  Real-world Rust terminal color enum design

- [crossterm Color docs](https://docs.rs/crossterm/latest/crossterm/style/enum.Color.html) - Another

  Rust terminal color enum

- [ansi_colours crate](https://docs.rs/ansi_colours/latest/ansi_colours/) - RGB to 256-color

  conversion

- [rgb2ansi256](https://github.com/rhysd/rgb2ansi256) - const fn Rust port of ansi_colours
- [colorsys crate](https://docs.rs/colorsys/latest/colorsys/) - HSL/RGB conversion and manipulation
- [X11 color names - Wikipedia](https://en.wikipedia.org/wiki/X11_color_names) - Definitive X11 vs

  W3C color comparison

- [BearLibTerminal Reference](http://foo.wyrd.name/en:bearlibterminal:reference#color_from_name) -

  Named color API design reference

- [XTerm Control Sequences](https://invisible-island.net/xterm/ctlseqs/ctlseqs.pdf) - Canonical

  xterm escape sequence spec

### Dropped

- [RenderWonk: Adventures with Gamma-Correct Rendering](https://renderwonk.com/blog/index.php/archive/adventures-with-gamma-correct-rendering/) -

  GPU-focused, not terminal-relevant

- [Real-Time Rendering blog: PNG sRGB](https://www.realtimerendering.com/blog/png-srgb-cutoutdecal-aa-problematic/) -

  About GPU AA, not applicable

- [rgbto256 by taylordotfish](https://github.com/taylordotfish/rgbto256) - CIEDE2000 approach,

  overkill for terminal use

- Various color converter web tools - No unique information beyond formulas already documented
- [tutorialpedia: ANSI Color Escape Sequences](https://www.tutorialpedia.org/blog/list-of-ansi-color-escape-sequences/) -

  SEO content, terminfo.dev is better
