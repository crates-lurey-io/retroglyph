# ADR 006: Extended Grapheme Clusters & Text Layout

**Status:**Draft**Date:**2026-06-18**Parent:** [ADR 001: Architecture](001-architecture.md)

## Context

Currently, the `Grid` and `Terminal` methods (`put`, `print_styled`) operate strictly on a single
`char` per cell. While perfectly fast and simple for ASCII roguelikes, this fundamentally breaks
modern Unicode support (combining marks, emojis) which require multiple `char`s per visual cell.

We need to support Extended Grapheme Clusters (EGCs) and robust text layout, but we **refuse** to
sacrifice the blazing-fast ergonomics of writing a simple `char` to a grid.

## Decisions & Rust API Guidelines

1. **The Alacritty Fast-Path:** We will update `Cell` to store a primary `char`, plus an optional

   heap allocation (`Option<Arc<String>>`) for rare complex graphemes. This avoids `enum`
   allocations for 99% of terminal output.

1. **Encapsulation (C-STRUCT-PRIVATE):** `Cell` fields will be strictly private. We solve the

   verbosity problem not by exposing fields, but by providing a clean constructor:
   `Cell::new(ch, style)` automatically handles the complex `flags` and `extra` internals.

1. **String Processing:** We will introduce `unicode-segmentation` for string printing and layout,

   iterating over `.graphemes(true)`.

1. **String Measurement:** We will use `UnicodeWidthStr::width` to calculate the display width of

   grapheme strings, correctly identifying 2-width CJK characters and emojis.

1. **Bitflags (C-BITFLAG):** We will use the `bitflags` crate for `CellFlags` instead of enums.
1. **Custom Types (C-CUSTOM-TYPE):** Bounded layout methods will take a `Rect` struct rather than
   loose `x, y, w, h` arguments to convey meaning and prevent parameter swapping.

1. **Common Traits (C-COMMON-TRAITS):** All public types will eagerly implement `Debug`, `Clone`,

   `PartialEq`, `Eq`, `Hash`, and `Default` where possible.

1. **Getter Naming (C-GETTER):** Property accessors will omit the `get_` prefix.

---

## Detailed Implementation Milestones

### M1: EGC Foundation & Cell Upgrade

**Goal:** Upgrade the fundamental grid unit to support grapheme clusters via the fast-path model,
strictly enforcing invariants.

### 1. Add Dependencies (`Cargo.toml`)

```toml
unicode-segmentation = "1.13.0"
bitflags = "2.4.2"
```

**2. Update `Cell` Representation (`src/cell.rs`)** Replace the current `Cell` implementation,
strictly adhering to `C-STRUCT-PRIVATE`:

```rust
use std::sync::Arc;
use crate::style::Style;

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
    pub struct CellFlags: u8 {
        /// This cell is the start of a wide (2-cell) character
        const WIDE_CHAR = 0b0000_0001;
        /// This cell is the invisible spacer right of a wide character
        const WIDE_CHAR_SPACER = 0b0000_0010;
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Cell {
    glyph: char,
    style: Style,
    flags: CellFlags,
    /// Allocated ONLY when the grapheme cluster consists of >1 char
    extra: Option<Arc<String>>,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            glyph: ' ',
            style: Style::default(),
            flags: CellFlags::empty(),
            extra: None,
        }
    }
}

impl Cell {
    /// Ergonomic public API. Hides the complexity of EGCs and Flags.
    pub fn new(glyph: char, style: Style) -> Self {
        Self {
            glyph,
            style,
            flags: CellFlags::empty(),
            extra: None,
        }
    }

    // C-GETTER accessors
    pub fn glyph(&self) -> char { self.glyph }
    pub fn style(&self) -> Style { self.style }
    pub fn flags(&self) -> CellFlags { self.flags }
    pub fn extra(&self) -> Option<&Arc<String>> { self.extra.as_ref() }

    // Crate-internal setters for the Grid to enforce wide-character invariants
    pub(crate) fn set_glyph(&mut self, glyph: char) { self.glyph = glyph; }
    pub(crate) fn set_style(&mut self, style: Style) { self.style = style; }
    pub(crate) fn set_flags(&mut self, flags: CellFlags) { self.flags = flags; }
    pub(crate) fn set_extra(&mut self, extra: Option<Arc<String>>) { self.extra = extra; }
}
```

**3. Upgrade `Terminal` Printing (`src/terminal.rs`)** Provide both a fast-path `put_char` and a
string-aware `put_grapheme`:

```rust
pub fn put_char(&mut self, col: u16, row: u16, ch: char, style: Style) {
    use unicode_width::UnicodeWidthChar;
    let width = ch.width().unwrap_or(0);

    // Fast path clearing of wide char invariants
    if self.grid.cell(col, row).flags().contains(CellFlags::WIDE_CHAR_SPACER) && col > 0 {
        let prev = self.grid.cell_mut(col - 1, row);
        prev.set_glyph(' ');
        prev.set_flags(prev.flags() - CellFlags::WIDE_CHAR);
    }
    // ... apply same clearing logic for (col+1) and (col+2) ...

    let cell = self.grid.cell_mut(col, row);
    cell.set_glyph(ch);
    cell.set_style(style);
    cell.set_extra(None);

    if width == 2 && col + 1 < self.grid.width() {
        cell.set_flags(cell.flags() | CellFlags::WIDE_CHAR);
        let spacer = self.grid.cell_mut(col + 1, row);
        spacer.set_glyph(' ');
        spacer.set_style(style);
        spacer.set_extra(None);
        spacer.set_flags(spacer.flags() | CellFlags::WIDE_CHAR_SPACER);
    } else {
        cell.set_flags(cell.flags() - CellFlags::WIDE_CHAR);
    }
}

pub fn put_grapheme(&mut self, col: u16, row: u16, grapheme: &str, style: Style) {
    use unicode_width::UnicodeWidthStr;
    let width = grapheme.width();

    let mut chars = grapheme.chars();
    let first_char = chars.next().unwrap_or(' ');

    // Call the fast path to handle all the grid clearing and wide char invariants
    self.put_char(col, row, first_char, style);

    // If the grapheme has combining marks or ZWJs, allocate them to `extra`
    if chars.next().is_some() {
        self.grid.cell_mut(col, row).set_extra(Some(Arc::new(grapheme.to_string())));
    }
}
```

### 4. Update `Terminal::print_styled`

```rust
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

for span in &line.spans {
    for grapheme in span.content.graphemes(true) {
        let width = grapheme.width();
        if width == 0 { continue; }
        self.put_grapheme(cx, cy, grapheme, span.style);
        cx += width as u16;
    }
}
```

### M2: Layout Primitives

**Goal:** Define the structures for bounding box layout in `src/layout.rs`. All types follow
`C-COMMON-TRAITS` and `C-CUSTOM-TYPE`.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HAlign { #[default] Left, Center, Right }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VAlign { #[default] Top, Middle, Bottom }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlacedGrapheme {
    pub x: u16,
    pub y: u16,
    pub grapheme: String,
    pub style: crate::style::Style,
    pub width: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TextMetrics {
    pub width: u16,
    pub height: u16,
}
```

### M3: Greedy Word Wrapper Engine

**Goal:** Write a function that takes spans, wraps them based on Unicode width, and returns lines.

```rust
struct WrappedLine {
    graphemes: Vec<(String, crate::style::Style, u16)>,
    width: u16,
}

fn wrap_lines(spans: &[crate::text::Span], max_width: u16) -> Vec<WrappedLine> {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let mut lines = vec![WrappedLine { graphemes: Vec::new(), width: 0 }];
    let mut col = 0u16;

    for span in spans {
        for grapheme in span.content.graphemes(true) {
            let width = grapheme.width() as u16;

            if grapheme == "\n" {
                lines.push(WrappedLine { graphemes: Vec::new(), width: 0 });
                col = 0;
                continue;
            }

            if width == 0 { continue; }

            if col + width > max_width && col > 0 {
                let current_line = lines.last_mut().unwrap();
                if let Some(space_idx) = current_line.graphemes.iter().rposition(|(g, _, _)| g == " ") {
                    let remainder: Vec<_> = current_line.graphemes.drain(space_idx + 1..).collect();
                    current_line.graphemes.pop();
                    current_line.width = current_line.graphemes.iter().map(|(_, _, w)| w).sum();

                    let new_width: u16 = remainder.iter().map(|(_, _, w)| w).sum();
                    lines.push(WrappedLine { graphemes: remainder, width: new_width });
                    col = new_width;
                } else {
                    lines.push(WrappedLine { graphemes: Vec::new(), width: 0 });
                    col = 0;
                }
            }

            let current_line = lines.last_mut().unwrap();
            current_line.graphemes.push((grapheme.to_string(), span.style, width));
            col += width;
            current_line.width = col;
        }
    }
    lines
}
```

### M4: Bounded Box Alignment & Measurement

**Goal:** Apply `HAlign` and `VAlign` offsets within a `Rect`.

```rust
pub fn measure(spans: &[crate::text::Span], max_width: u16) -> TextMetrics {
    let lines = wrap_lines(spans, max_width);
    let max_w = lines.iter().map(|l| l.width).max().unwrap_or(0);
    TextMetrics { width: max_w, height: lines.len() as u16 }
}

pub fn layout(
    spans: &[crate::text::Span],
    rect: Rect,
    h_align: HAlign,
    v_align: VAlign
) -> Vec<PlacedGrapheme> {
    let lines = wrap_lines(spans, rect.width);
    let total_lines = std::cmp::min(lines.len(), rect.height as usize) as u16;

    let y_offset = match v_align {
        VAlign::Top => 0,
        VAlign::Middle => rect.height.saturating_sub(total_lines) / 2,
        VAlign::Bottom => rect.height.saturating_sub(total_lines),
    };

    let mut placed = Vec::new();

    for (line_idx, line) in lines.into_iter().take(total_lines as usize).enumerate() {
        let x_offset = match h_align {
            HAlign::Left => 0,
            HAlign::Center => rect.width.saturating_sub(line.width) / 2,
            HAlign::Right => rect.width.saturating_sub(line.width),
        };

        let mut cx = 0;
        for (grapheme, style, width) in line.graphemes {
            placed.push(PlacedGrapheme {
                x: rect.x + x_offset + cx,
                y: rect.y + y_offset + line_idx as u16,
                grapheme,
                style,
                width,
            });
            cx += width;
        }
    }
    placed
}
```

### M5: Terminal Integration

**Goal:** Expose the rendering to the `Terminal` cleanly.

In `src/terminal.rs`:

```rust
pub fn print_box(
    &mut self,
    rect: crate::layout::Rect,
    line: &crate::text::Line,
    h_align: crate::layout::HAlign,
    v_align: crate::layout::VAlign
) {
    let placed = crate::layout::layout(&line.spans, rect, h_align, v_align);
    for p in placed {
        self.put_grapheme(p.x, p.y, &p.grapheme, p.style);
    }
}
```
