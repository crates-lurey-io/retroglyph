# Unicode Text Handling for Terminal Grid Rendering

Reference document for implementing correct Unicode text handling in a Rust terminal/grid rendering
library. Covers the full stack from codepoints to display widths to emoji sequences.

## Table of Contents

1. [Grapheme Clusters vs Codepoints vs Chars](#1-grapheme-clusters-vs-codepoints-vs-chars)
2. [UAX #11 East Asian Width](#2-uax-11-east-asian-width)
3. [Extended Grapheme Clusters (EGC)](#3-extended-grapheme-clusters-egc)
4. [Combining Characters in a Cell Grid](#4-combining-characters-in-a-cell-grid)
5. [Emoji Rendering](#5-emoji-rendering)
6. [Bidirectional Text (BiDi)](#6-bidirectional-text-bidi)
7. [Wide Character Handling in a Cell Grid](#7-wide-character-handling-in-a-cell-grid)
8. [Normalization (NFC/NFD)](#8-normalization-nfcnfd)
9. [The unicode-width Disagreement Problem](#9-the-unicode-width-disagreement-problem)
10. [Concrete Rust Implementation](#10-concrete-rust-implementation)

---

## 1. Grapheme Clusters vs Codepoints vs Chars

Three levels of text granularity matter for terminal rendering, and conflating them is a source of
bugs:

### Bytes

UTF-8 encoding uses 1-4 bytes per codepoint. Rust `&str` is always valid UTF-8. Never index by byte
offset without checking you're on a char boundary.

### Codepoints (Rust `char`)

A `char` is a single Unicode scalar value (U+0000 to U+10FFFF, excluding surrogates). Rust's `char`
is 4 bytes wide. Many user-perceived characters are multiple codepoints:

- `é` can be U+0065 (e) + U+0301 (combining acute accent) = 2 codepoints
- `🇺🇸` = U+1F1FA + U+1F1F8 = 2 codepoints (regional indicator symbols)
- `👨‍👩‍👧‍👦` = 7 codepoints (4 emoji + 3 ZWJ)

### Grapheme clusters (user-perceived characters)

A grapheme cluster is what a human considers "one character." This is what occupies one cell
position (or two, for wide characters) in a terminal grid. Defined by UAX #29.

### The unit of storage in a terminal cell must be the grapheme cluster, not the codepoint

### The `unicode-segmentation` crate

The [unicode-segmentation](https://docs.rs/unicode-segmentation) crate (v1.13.x, based on UAX #29)
provides grapheme cluster segmentation:

```rust
use unicode_segmentation::UnicodeSegmentation;

let s = "a̐éö̲\r\n";
let graphemes: Vec<&str> = s.graphemes(true).collect();
// ["a̐", "é", "ö̲", "\r\n"]
// true = extended grapheme clusters (always use true)

// Cursor-based API for streaming / partial input:
use unicode_segmentation::GraphemeCursor;

let s = "hello";
let mut cursor = GraphemeCursor::new(0, s.len(), true);
let next_boundary = cursor.next_boundary(s, 0);
// Returns Ok(Some(1)) -- next grapheme boundary at byte offset 1
```

Key points:

- Always pass `true` to get _extended_ grapheme clusters (EGC). Legacy grapheme clusters are a

  historical artifact.

- `GraphemeCursor` is useful when processing input byte-by-byte from a PTY, since you can feed

  partial chunks.

- There is no upper bound on grapheme cluster size in bytes. A single EGC can be arbitrarily long

  (e.g., many combining marks stacked). Set a reasonable cap for adversarial input (notcurses caps
  at ~64 codepoints per EGC).

---

## 2. UAX #11 East Asian Width

[UAX #11](https://www.unicode.org/reports/tr11/) defines the `East_Asian_Width` property, which
classifies every codepoint into one of six categories:

| Category  | Abbr | Width  | Examples                     |
| --------- | ---- | ------ | ---------------------------- |
| Fullwidth | F    | 2      | Ｈｅｌｌｏ (fullwidth ASCII) |
| Wide      | W    | 2      | 漢字, most emoji             |
| Halfwidth | H    | 1      | ｶﾀｶﾅ (halfwidth katakana)    |
| Narrow    | Na   | 1      | ASCII, Latin                 |
| Ambiguous | A    | 1 or 2 | Greek letters, some symbols  |
| Neutral   | N    | 1      | Most other scripts           |

Ambiguous characters are the hard case. In an East Asian context (CJK locale), they render as 2
cells wide. In a Western context, they render as 1 cell. The `unicode-width` crate provides both
`width()` (treats Ambiguous as narrow) and `width_cjk()` (treats Ambiguous as wide) behind the
`"cjk"` feature flag.

### Important caveat from UAX #11 itself

> The East_Asian_Width property is not intended for use by modern terminal emulators without
> appropriate tailoring on a case-by-case basis.

This means UAX #11 is a starting point, not a complete solution. Terminal emulators must layer
additional rules on top (see [section 9](#9-the-unicode-width-disagreement-problem)).

### The `unicode-width` crate

The [unicode-width](https://docs.rs/unicode-width) crate (v0.2.x) implements display width
calculation. As of version 0.2, it handles _string-level_ width (not just per-character), which is
critical for emoji sequences:

```rust
use unicode_width::UnicodeWidthStr;
use unicode_width::UnicodeWidthChar;

// Character-level width:
assert_eq!('A'.width(), Some(1));    // Narrow
assert_eq!('漢'.width(), Some(2));   // Wide
assert_eq!('\u{0301}'.width(), Some(0)); // Combining mark

// String-level width (handles sequences):
assert_eq!("hello".width(), 5);
assert_eq!("漢字".width(), 4);

// CJK context (Ambiguous = wide):
#[cfg(feature = "cjk")]
assert_eq!("α".width_cjk(), 2);  // Greek alpha is Ambiguous
```

The `unicode-width` v0.2 rules, in order of precedence:

1. **String-level sequences** (width differs from sum of char widths):
   - `"\r\n"` has width 1
   - Well-formed emoji ZWJ sequences have width 2
   - Emoji modifier sequences (skin tone) have width 2
   - Emoji presentation sequences (base + VS16) have width 2
   - Text presentation sequences (base + VS15) have width 1 (when base has `Emoji_Presentation`)
   - Script-specific ligatures (Arabic Lam-Alef = width 1, etc.)

1. **Character-level widths** (when no sequence rule applies):
   - `Default_Ignorable_Code_Point` = width 0
   - `Grapheme_Extend` = width 0
   - `East_Asian_Width` of `Fullwidth` or `Wide` = width 2
   - Everything else = width 1

---

## 3. Extended Grapheme Clusters (EGC)

An Extended Grapheme Cluster (EGC) is the Unicode-standard definition of a user-perceived character.
It is defined by UAX #29 grapheme cluster boundary rules and is the correct unit for terminal cell
storage.

### What makes up an EGC

An EGC can consist of:

- A single codepoint: `A` (U+0041)
- A base + combining marks: `é` = U+0065 + U+0301
- A Hangul syllable: composed from L, V, T jamo
- Regional indicator pairs: `🇺🇸` = U+1F1FA + U+1F1F8
- Emoji ZWJ sequences: `👨‍👩‍👧‍👦` = U+1F468 U+200D U+1F469 U+200D U+1F467 U+200D U+1F466
- Emoji modifier sequences: `👋🏽` = U+1F44B + U+1F3FD
- Any base + arbitrary combining marks

### How notcurses stores EGCs

Notcurses (the C TUI library by Nick Black) uses an `nccell` structure where each cell stores an
EGC. The key insight from notcurses:

- Each cell in the grid stores one EGC (an arbitrary-length UTF-8 string).
- For common single-codepoint characters, the EGC can be inlined directly into the cell struct

  (small-string optimization). Notcurses inlines up to 4 bytes (one UTF-8 codepoint) directly in the
  cell's `gcluster` field (a `uint32_t`).

- Longer EGCs (multi-codepoint grapheme clusters) are stored in a separate string pool, and the cell

  holds an index/offset into that pool. The high bit of `gcluster` distinguishes inline vs. pool
  storage.

- Wide characters (width 2) occupy their primary cell plus a "continuation" marker in the next cell.

  The continuation cell has no EGC of its own.

- During rendering, notcurses determines the EGC, foreground color, background color, and style for

  each physical terminal cell by descending through the plane stack.

### How xterm.js stores cell content

xterm.js packs cell content into a single 32-bit integer (`content` field in `CellData`):

```text
Bits 1-21:   Codepoint (UTF-32, max 0x10FFFF)
Bit 22:      IS_COMBINED flag
Bits 23-24:  wcwidth value (0, 1, or 2)
```rust

- **Single codepoint**: stored directly in bits 1-21. Width in bits 23-24.
- **Combined content** (multi-codepoint EGC): the `IS_COMBINED` flag is set, and the full string is

  stored in a separate `combinedData` string field. The codepoint bits are cleared. Width is still
  packed in bits 23-24.

- This is a similar small-string-optimization pattern to notcurses: the common case (single BMP or

  supplementary codepoint) is stored inline without allocation, while the rare case (combining
  marks, ZWJ sequences) spills to a heap string.

### How Alacritty stores cell content

Alacritty's `Cell` struct (from `alacritty_terminal/src/term/cell.rs`):

```rust
pub struct Cell {
    pub c: char,           // Primary character (single codepoint)
    pub fg: Color,
    pub bg: Color,
    pub flags: Flags,      // Includes WIDE_CHAR, WIDE_CHAR_SPACER, etc.
    pub extra: Option<Arc<CellExtra>>,  // Rare data, allocated on demand
}

pub struct CellExtra {
    zerowidth: Vec<char>,  // Combining / zero-width characters
    underline_color: Option<Color>,
    hyperlink: Option<Hyperlink>,
}
```

Key design choices:

- The primary character is a single `char`. This handles the common case (one codepoint).
- Combining/zero-width characters are stored in `CellExtra.zerowidth`, which is only allocated when

  needed (wrapped in `Arc` for cheap cloning).

- Wide characters use flag bits: `WIDE_CHAR` on the primary cell, `WIDE_CHAR_SPACER` on the

  continuation cell. There is also `LEADING_WIDE_CHAR_SPACER` for when a wide char would land at the
  last column (the spacer goes in the last column, the char wraps to the next line).

- Cell size is kept to 24 bytes on 64-bit (verified by a test).

---

## 4. Combining Characters in a Cell Grid

Combining characters (General_Category `Mn`, `Mc`, `Me`) have zero display width and visually attach
to the preceding base character. In a terminal cell grid:

### The problem

When the terminal receives a combining character, it must attach it to the _previous_ cell's content
rather than placing it in a new cell. This means:

1. Receiving codepoint U+0301 (combining acute accent) after cell at column 5 containing `e`
2. The cell at column 5 now contains the EGC `é` (U+0065 + U+0301)
3. The cursor does _not_ advance

### Implementation strategy

```rust
fn handle_codepoint(&mut self, cp: char) {
    let width = unicode_width::UnicodeWidthChar::width(cp);

    match width {
        Some(0) => {
            // Zero-width: append to the previous cell's content
            if let Some(prev_cell) = self.get_previous_cell_mut() {
                prev_cell.push_combining(cp);
            }
            // Don't advance cursor
        }
        Some(w) => {
            // Normal or wide character
            let cell = self.cell_at_cursor_mut();
            cell.set_char(cp);
            cell.set_width(w as u8);
            if w == 2 {
                // Mark next cell as wide-char continuation
                let next = self.cell_at_mut(self.cursor_col + 1);
                next.set_wide_continuation(true);
            }
            self.cursor_col += w;
        }
        None => {
            // Control character (width = None), handle separately
        }
    }
}
```

### Edge cases

- **Combining marks at the start of a line**: No base character to attach to. Typical behavior:

  treat the combining mark as its own cell (display as dotted circle + combining mark), or ignore
  it, or store it as a standalone EGC.

- **Multiple combining marks**: Stack them all onto the same base cell. `e` + U+0301 + U+0327 = `ḗ`

  (e with acute and cedilla). All part of one EGC.

- **Combining marks after wide characters**: Attach to the wide character's primary cell (the left

  cell), not the continuation cell.

- **Maximum stacking depth**: Set a limit. An adversarial input can send thousands of combining

  marks on one base. Notcurses limits to ~64 codepoints per EGC. Beyond the limit, drop or ignore
  additional combiners.

---

## 5. Emoji Rendering

Emoji is the single hardest width problem in terminal rendering. There are multiple sequence types,
and terminals widely disagree on handling.

### Emoji presentation vs. text presentation

Many characters have two forms:

- **Text presentation** (default for some): monochrome, 1 cell wide. E.g., `☺` (U+263A)
- **Emoji presentation** (default for some): colorful, 2 cells wide. E.g., `😀` (U+1F600)

The `Emoji_Presentation` property determines the default. Characters _without_ `Emoji_Presentation`
default to text (1 cell); characters _with_ it default to emoji (2 cells).

### Variation Selectors

- **VS15** (U+FE0E): Force text presentation (1 cell)
- **VS16** (U+FE0F): Force emoji presentation (2 cells)

Example: `☺` (U+263A) is text presentation by default (1 cell). `☺️` (U+263A + U+FE0F) forces emoji
presentation (2 cells).

**Terminal support is poor.** As of 2026, only 3 of 12 tested terminals correctly handle VS16 width
changes (iTerm2, Ghostty, and vterm.js). Kitty, WezTerm, Alacritty, VS Code, and xterm.js all get
this wrong. ([terminfo.dev](https://terminfo.dev/text/variation-selector-16-emoji))

### ZWJ sequences

Zero Width Joiner (U+200D) connects multiple emoji into a single glyph:

```text
👨‍👩‍👧‍👦 = 👨 + ZWJ + 👩 + ZWJ + 👧 + ZWJ + 👦
U+1F468 U+200D U+1F469 U+200D U+1F467 U+200D U+1F466
```text

The entire sequence is one EGC and should occupy 2 cells. But the _character-level_ sum would be 8 +
0 + 0 + 0 = 8 (four emoji at width 2 each, three ZWJs at width 0). This is why `unicode-width` v0.2
operates on strings, not just characters: it recognizes ZWJ sequences and returns width 2 for the
whole thing.

### Emoji modifier sequences (skin tones)

A base emoji + Fitzpatrick skin tone modifier = one EGC, width 2:

```text
👋🏽 = 👋 (U+1F44B) + 🏽 (U+1F3FD)
```text

### Keycap sequences

Digit/symbol + VS16 + U+20E3 (combining enclosing keycap):

```text
1️⃣ = 1 + U+FE0F + U+20E3
```yaml

Width: 2 cells.

### Flag sequences

Two Regional Indicator symbols form a flag:

```text
🇺🇸 = 🇺 (U+1F1FA) + 🇸 (U+1F1F8)
```yaml

Width: 2 cells. But individual Regional Indicators should have width 1 (they are
`Grapheme_Extend=Prepend` in new versions).

### Implementation for emoji width

```rust
use unicode_width::UnicodeWidthStr;
use unicode_segmentation::UnicodeSegmentation;

/// Get the display width of a grapheme cluster.
fn grapheme_width(grapheme: &str) -> usize {
    // unicode-width v0.2+ handles emoji sequences at the string level
    UnicodeWidthStr::width(grapheme)
}

/// Process a string into (grapheme, width) pairs for cell placement.
fn layout_string(s: &str) -> Vec<(&str, usize)> {
    s.graphemes(true)
        .map(|g| (g, grapheme_width(g)))
        .collect()
}
```

### The fundamental problem

Even with perfect library support, the _terminal emulator displaying your output_ may disagree on
the width. If you write a 2-wide emoji and the terminal renders it as 1-wide, everything after it
will be misaligned. There is no complete solution; you must match the target terminal's behavior or
accept some misalignment. See [section 9](#9-the-unicode-width-disagreement-problem).

---

## 6. Bidirectional Text (BiDi)

### UAX #9 and the BiDi Algorithm

[UAX #9](https://www.unicode.org/reports/tr9/) defines the Unicode Bidirectional Algorithm (UBA). It
determines the display order of text containing both left-to-right (LTR) and right-to-left (RTL)
scripts (Arabic, Hebrew, etc.).

Key concepts:

- **Paragraph direction**: Overall direction (LTR or RTL) of a paragraph.
- **Embedding levels**: Each character gets a level (even = LTR, odd = RTL).
- **Runs**: Contiguous sequences of characters at the same level.
- The algorithm reorders runs for display but does not change the logical order in memory.

### Why terminals are special

Terminals have fundamental BiDi challenges that graphical applications don't face:

1. **Split responsibility**: The terminal emulator sees only the displayed portion; the application

   running inside knows the full text. Neither alone has enough information for correct BiDi.

1. **No paragraph awareness**: Terminal output is a byte stream. The terminal doesn't know where

   paragraphs begin/end, or whether adjacent lines belong to the same logical paragraph.

1. **Cropping**: Applications must crop text to fit the terminal width. The BiDi algorithm must run

   on the _full_ paragraph before cropping, but only the application knows the full paragraph.
   Running BiDi on already-cropped text produces incorrect results.

1. **Cell-grid model**: Terminal cells are addressed by (column, row). BiDi reordering changes which

   column a character appears in, but cursor positioning and cell addressing remain LTR.

### The terminal-wg BiDi recommendation

The [freedesktop.org terminal BiDi spec](https://terminal-wg.pages.freedesktop.org/bidi/) proposes
two modes:

**Implicit mode** (for simple utilities like `cat`, `echo`):

- The terminal emulator performs BiDi reordering on each paragraph.
- Text arrives in logical order; the terminal reorders for display.
- Base paragraph direction can be LTR, RTL, or auto-detected.
- BiDi control characters are discarded (level 1).
- This should be the default mode.

**Explicit mode** (for full-screen apps like vim, tmux):

- The application handles all BiDi layout itself.
- Text arrives in visual order; the terminal does no reordering.
- Explicit LTR = current behavior of most terminals (no BiDi at all).
- Explicit RTL = cells laid out right-to-left.

### Practical recommendation for a grid library

For a grid/buffer library (as opposed to a full terminal emulator):

1. **Store text in logical order** in the cell grid. Each cell holds its EGC.
2. **BiDi is a display-layer concern.** The grid stores logical content; a rendering pass applies

   the BiDi algorithm to produce visual order.

3. **Don't implement BiDi initially.** Most terminal applications today operate in explicit LTR
   mode. BiDi support is a later enhancement.

4. If you do implement it, use an existing UBA implementation. In Rust,
   [unicode-bidi](https://docs.rs/unicode-bidi) implements UAX #9.

5. notcurses explicitly does not handle BiDi itself: "notcurses does not currently handle
   right-to-left text in any special way, but terminals often apply their own heuristics."

---

## 7. Wide Character Handling in a Cell Grid

CJK characters (Han ideographs, katakana, hangul syllables, etc.) occupy 2 terminal cells. This is
the most fundamental layout challenge after basic character placement.

### The two-cell model

A wide character occupies two adjacent cells in the same row:

```yaml
Column:  0   1   2   3   4
Content: [H] [ ] [e] [l] [l]
         ^^^ ^^^
         漢 (continuation)
```c

- Column 0: Contains the character `漢`, marked as `WIDE_CHAR`, width=2
- Column 1: Empty spacer cell, marked as `WIDE_CHAR_SPACER` (or continuation)
- The spacer cell must not contain independent content

### Edge cases to handle

**1. Wide char at last column:** If cursor is at the last column and a wide character arrives, it
won't fit. Two options:

- **Wrap**: Place a spacer at the last column (Alacritty calls this `LEADING_WIDE_CHAR_SPACER`),

  then place the wide char at columns 0-1 of the next row.

- **Truncate**: Ignore the wide character or replace with a space.

Most terminals choose wrap. Alacritty uses `LEADING_WIDE_CHAR_SPACER` to mark the dangling
last-column cell.

**2. Overwriting half of a wide character:** If a narrow character is written at the position of a
wide char's spacer (column 1 in the example), the entire wide character must be destroyed:

```text
// Before: columns 0-1 contain 漢
// Write 'x' at column 1:
// Column 0 must be cleared (it's now an orphaned half)
// Column 1 now contains 'x'
```c

Similarly, writing _anything_ at column 0 (the primary cell) destroys the spacer at column 1.

Notcurses documentation states: "It is not possible to print a narrow glyph over half of a wide
glyph without obliterating the other half."

**3. Wide char overwriting two narrow chars:** A wide char at column 3 will overwrite whatever was
at columns 3 and 4.

**4. Wide char overwriting part of another wide char:** If columns 2-3 contain a wide char and a new
wide char is written at column 3, it obliterates the old wide char at 2-3 AND occupies 3-4. Columns
2 must be cleared. Notcurses: "it is thus possible for a single wide character to obliterate four
columns' worth of glyphs."

### Implementation

```rust
#[derive(Clone, Debug)]
pub struct Cell {
    /// The grapheme cluster content, empty for spacer cells
    content: CompactString,
    /// Display width: 0 for combining, 1 for narrow, 2 for wide
    width: u8,
    /// Cell flags
    flags: CellFlags,
    // ... style, colors, etc.
}

bitflags::bitflags! {
    pub struct CellFlags: u8 {
        /// This cell is the start of a wide character
        const WIDE_CHAR          = 0b0000_0001;
        /// This cell is the continuation (spacer) of a wide character
        const WIDE_CHAR_SPACER   = 0b0000_0010;
        /// Spacer placed at last column when wide char wraps
        const LEADING_SPACER     = 0b0000_0100;
    }
}

impl Grid {
    fn write_char_at(&mut self, col: usize, row: usize, grapheme: &str, width: usize) {
        // 1. If writing over a wide char spacer, clear the primary cell
        if self.cell(col, row).flags.contains(CellFlags::WIDE_CHAR_SPACER) {
            let primary = col - 1;
            self.cell_mut(primary, row).clear();
        }

        // 2. If writing over a wide char primary, clear the spacer
        if self.cell(col, row).flags.contains(CellFlags::WIDE_CHAR) {
            self.cell_mut(col + 1, row).clear();
        }

        // 3. If this is a wide char, also clear whatever is at col+1
        if width == 2 {
            let next = col + 1;
            if next >= self.cols {
                // Wide char at last column: wrap or truncate
                self.cell_mut(col, row).set_leading_spacer();
                // Write at start of next row instead...
                return;
            }
            // If col+1 is a wide char primary, clear its spacer too
            if self.cell(next, row).flags.contains(CellFlags::WIDE_CHAR) {
                self.cell_mut(next + 1, row).clear();
            }
            self.cell_mut(next, row).set_spacer();
        }

        // 4. Place the character
        let cell = self.cell_mut(col, row);
        cell.content = grapheme.into();
        cell.width = width as u8;
        cell.flags = if width == 2 {
            CellFlags::WIDE_CHAR
        } else {
            CellFlags::empty()
        };
    }
}
```

---

## 8. Normalization (NFC/NFD)

Unicode allows the same visual character to be represented in multiple ways:

- **NFD** (Canonical Decomposition): `é` = U+0065 + U+0301 (e + combining acute)
- **NFC** (Canonical Composition): `é` = U+00E9 (single precomposed codepoint)
- **NFKD/NFKC**: Compatibility decomposition/composition (also maps, e.g., `ﬁ` to `fi`)

### When normalization matters for terminals

**Input from the PTY:** Programs may emit text in any normalization form. macOS HFS+ historically
used NFD for filenames, so `ls` output on macOS may contain NFD-encoded filenames. Most other
systems use NFC or no specific form.

**Width calculation:** `unicode-width` v0.2 explicitly guarantees: "Canonically equivalent strings
are assigned the same width (CJK and non-CJK)." So `é` in NFC (1 codepoint, width 1) and `é` in NFD
(2 codepoints, width 1+0=1) produce the same string width. However, UAX #11 notes that the
`East_Asian_Width` property does NOT preserve canonical equivalence for individual codepoints.

**Grapheme segmentation:** `unicode-segmentation` correctly handles both NFC and NFD input. An `e` +
combining accent in NFD is one grapheme cluster, same as the precomposed `é` in NFC.

**Cell storage:** You have two choices:

1. **Normalize to NFC on input**: Fewer codepoints per cell, more compact. This is what most

   terminals do implicitly.

1. **Store as-is**: Preserve the original encoding. This means NFD sequences take more storage per

   cell (multiple chars instead of one) but avoids lossy transformation.

### Recommendation

Normalize to NFC on input. It reduces storage, simplifies comparisons, and matches the behavior of
most terminals. Use the `unicode-normalization` crate:

```rust
use unicode_normalization::UnicodeNormalization;

let nfd = "e\u{0301}";  // NFD: e + combining acute
let nfc: String = nfd.nfc().collect();
assert_eq!(nfc, "\u{00E9}");  // NFC: é (single codepoint)

// Quick check (avoids full normalization if already NFC):
use unicode_normalization::is_nfc_quick;
use unicode_normalization::IsNormalized;

match is_nfc_quick(input.chars()) {
    IsNormalized::Yes => { /* already NFC, use as-is */ }
    _ => { /* normalize */ }
}
```

Avoid NFKC/NFKD in a terminal. Compatibility decomposition changes semantics (e.g., `ﬁ` to `fi`, `①`
to `1`), which is wrong for a terminal that should faithfully render whatever the application sends.

---

## 9. The unicode-width Disagreement Problem

The single biggest practical problem in terminal width calculation: **different terminals compute
different widths for the same character or sequence.**

### Sources of disagreement

1. **Unicode version mismatch**: The terminal uses one Unicode version's width tables, your library

   uses another. New emoji added in Unicode 15 might be width 2 in your library but width 1 (or
   unknown) in an older terminal.

1. **Ambiguous width**: East Asian Width "Ambiguous" characters are 1 cell in Western locales, 2

   cells in CJK locales. Your library has to guess which context the terminal is using. There is no
   reliable way to query this.

1. **Emoji presentation**: VS16 (U+FE0F) should promote certain characters from width 1 to width 2.

   As of 2026, only 25% of tested terminals handle this correctly. Most TUI libraries (including
   Rich/Python) also get this wrong.

1. **ZWJ sequences**: Some terminals render `👨‍👩‍👧‍👦` as width 2 (correct), others as width 8 (sum of

   components), others fall back to individual emoji. The library returns 2, but the terminal may
   display it differently based on font support.

1. **New emoji**: Every Unicode version adds new emoji. A terminal using Unicode 13 tables won't

   know that a Unicode 15 emoji should be width 2; it'll likely default to width 1.

1. **wcwidth() vs. unicode-width**: The C `wcwidth()` function (from libc) operates on single

   codepoints and has no concept of sequences. It often uses outdated tables. `unicode-width` v0.2
   is string-aware and current, but it may disagree with the terminal (which might use its own
   `wcwidth()` or custom tables).

### The `?2027` escape sequence

Some terminals support DECSACE mode `?2027` for Unicode grapheme cluster width handling. WezTerm
reports mode `?2027` as permanently set. This is an attempt to signal that the terminal handles
VS16/emoji sequences properly, but adoption is incomplete.

### Practical mitigation strategies

1. **Match your target**: If you control the terminal (e.g., building an integrated terminal), use

   the same width tables everywhere.

1. **Probe the terminal**: At startup, print a known character sequence and query the cursor

   position to determine the terminal's actual width calculation. This is fragile but sometimes
   necessary.

1. **Offer width configuration**: Let users configure "ambiguous width = 1 or 2" and "emoji width

   strategy."

1. **Use unicode-width for best-effort**: It tracks modern Unicode and handles emoji sequences. It

   will match most modern terminals (Ghostty, iTerm2) for common cases.

1. **Accept imperfection**: For adversarial inputs or obscure sequences, alignment will break. This

   is a known, unsolved problem across the terminal ecosystem.

1. **Version awareness**: Log which Unicode version your width tables use. If users report

   misalignment, this helps diagnose version mismatches.

---

## 10. Concrete Rust Implementation

### Cargo dependencies

```toml
[dependencies]
unicode-segmentation = "1.13"
unicode-width = "0.2"
unicode-normalization = "0.1"
# Optional, for BiDi support

# unicode-bidi = "0.3"

```

### Core cell type

```rust
use std::sync::Arc;

/// Compact string for grapheme cluster storage.
/// Single codepoints (the common case) can be stored inline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphemeContent {
    /// Empty cell (space)
    Empty,
    /// Single char (common case, no allocation)
    Single(char),
    /// Multi-codepoint EGC (combining marks, ZWJ sequences, etc.)
    Multi(Arc<str>),
}

impl GraphemeContent {
    pub fn as_str(&self) -> &str {
        match self {
            GraphemeContent::Empty => " ",
            GraphemeContent::Single(c) => {
                // This is a simplification; real impl would use a buffer
                // or encode into a stack-allocated array
                unimplemented!("need a char-to-str helper")
            }
            GraphemeContent::Multi(s) => s,
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, GraphemeContent::Empty)
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct CellFlags: u8 {
        const WIDE           = 0b0000_0001;
        const WIDE_SPACER    = 0b0000_0010;
        const LEADING_SPACER = 0b0000_0100;
        const WRAP           = 0b0000_1000;
    }
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub content: GraphemeContent,
    pub width: u8,
    pub flags: CellFlags,
    pub fg: Color,
    pub bg: Color,
    // Rare data (combining chars beyond the primary EGC, hyperlinks):
    pub extra: Option<Arc<CellExtra>>,
}

#[derive(Clone, Debug, Default)]
pub struct CellExtra {
    pub hyperlink: Option<String>,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            content: GraphemeContent::Empty,
            width: 1,
            flags: CellFlags::empty(),
            fg: Color::Default,
            bg: Color::Default,
            extra: None,
        }
    }
}
```

### Processing input into cells

```rust
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use unicode_normalization::UnicodeNormalization;

/// Process a string into grapheme clusters with their display widths.
pub fn segment_and_measure(input: &str) -> Vec<(String, usize)> {
    // Normalize to NFC first
    let normalized: String = input.nfc().collect();

    normalized
        .graphemes(true)
        .map(|g| {
            let width = UnicodeWidthStr::width(g);
            (g.to_string(), width)
        })
        .collect()
}

/// Write a grapheme cluster to the grid at the given position.
pub fn write_grapheme(
    grid: &mut Grid,
    row: usize,
    col: usize,
    grapheme: &str,
    width: usize,
) -> usize {
    let cols = grid.columns();

    // Handle wide character at last column
    if width == 2 && col + 1 >= cols {
        // Place leading spacer, return to indicate wrap needed
        grid.cell_mut(row, col).flags |= CellFlags::LEADING_SPACER;
        return 0; // signal: caller should wrap to next line
    }

    // Clean up any wide character we're overwriting
    cleanup_overwrite(grid, row, col, width);

    // Set cell content
    let cell = grid.cell_mut(row, col);
    cell.content = if grapheme.chars().count() == 1 {
        GraphemeContent::Single(grapheme.chars().next().unwrap())
    } else {
        GraphemeContent::Multi(Arc::from(grapheme))
    };
    cell.width = width as u8;
    cell.flags = if width == 2 {
        CellFlags::WIDE
    } else {
        CellFlags::empty()
    };

    // Place spacer for wide characters
    if width == 2 {
        let spacer = grid.cell_mut(row, col + 1);
        spacer.content = GraphemeContent::Empty;
        spacer.width = 0;
        spacer.flags = CellFlags::WIDE_SPACER;
    }

    width
}

/// Clean up cells that would be partially overwritten.
fn cleanup_overwrite(grid: &mut Grid, row: usize, col: usize, width: usize) {
    for c in col..col + width {
        let flags = grid.cell(row, c).flags;

        // Overwriting a wide char's spacer: clear the primary cell
        if flags.contains(CellFlags::WIDE_SPACER) && c > 0 {
            grid.cell_mut(row, c - 1).clear();
        }

        // Overwriting a wide char's primary: clear the spacer
        if flags.contains(CellFlags::WIDE) && c + 1 < grid.columns() {
            grid.cell_mut(row, c + 1).clear();
        }
    }
}
```

### Handling combining characters from a PTY stream

```rust
use unicode_width::UnicodeWidthChar;

/// Process a single codepoint from PTY input.
/// Returns the number of columns the cursor should advance.
pub fn process_codepoint(
    grid: &mut Grid,
    cursor_row: usize,
    cursor_col: usize,
    ch: char,
) -> usize {
    match UnicodeWidthChar::width(ch) {
        None => {
            // Control character (e.g., \n, \r, \t, \x1b)
            // Handle via control code processing, not cell placement
            0
        }
        Some(0) => {
            // Zero-width / combining character
            // Attach to the previous cell
            if cursor_col > 0 {
                let prev_col = cursor_col - 1;
                let prev_flags = grid.cell(cursor_row, prev_col).flags;

                // If prev is a wide char spacer, attach to the primary cell
                let target_col = if prev_flags.contains(CellFlags::WIDE_SPACER) {
                    prev_col.saturating_sub(1)
                } else {
                    prev_col
                };

                let cell = grid.cell_mut(cursor_row, target_col);
                cell.append_combining(ch);
            }
            // Cursor does not advance
            0
        }
        Some(w) => {
            // Regular character with width 1 or 2
            // Build a single-char grapheme and write it
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            write_grapheme(grid, cursor_row, cursor_col, s, w)
        }
    }
}
```

### String width measurement utility

```rust
use unicode_width::UnicodeWidthStr;

/// Measure the display width of a string, accounting for all
/// Unicode sequences (emoji, ZWJ, VS16, etc.)
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Truncate a string to fit within `max_width` display columns.
/// Returns the truncated string and its actual display width.
pub fn truncate_to_width(s: &str, max_width: usize) -> (&str, usize) {
    let mut current_width = 0;
    let mut last_valid_end = 0;

    for grapheme in s.graphemes(true) {
        let gw = UnicodeWidthStr::width(grapheme);
        if current_width + gw > max_width {
            break;
        }
        current_width += gw;
        last_valid_end += grapheme.len();
    }

    (&s[..last_valid_end], current_width)
}
```

---

## Summary of Crate Responsibilities

| Crate                   | Purpose                     | Version | UAX             |
| ----------------------- | --------------------------- | ------- | --------------- |
| `unicode-segmentation`  | Grapheme cluster boundaries | 1.13.x  | UAX #29         |
| `unicode-width`         | Display width calculation   | 0.2.x   | UAX #11 + emoji |
| `unicode-normalization` | NFC/NFD conversion          | 0.1.x   | UAX #15         |
| `unicode-bidi`          | Bidirectional algorithm     | 0.3.x   | UAX #9          |

## Key Design Decisions for a Grid Library

1. **Store EGCs, not codepoints.** Each cell holds one grapheme cluster.
2. **Use small-string optimization.** Single codepoints (the 99% case) should be stored inline

   without heap allocation.

3. **Wide chars use primary + spacer cells.** Mark both with flags.3. **Handle the overwrite invariant.** Writing to any cell that is part of a wide character must   clear the other half.
4. **Normalize to NFC on input.** Reduces storage and simplifies comparison.2. **Use string-level width from unicode-width v0.2.** Character-level width is insufficient for
   emoji sequences.

5. **Cap EGC length.** Defend against adversarial combining mark stacking.2. **Defer BiDi.** Store logical order; implement BiDi reordering as a display-layer concern.
6. **Accept width disagreement.** No solution exists for perfect width agreement across all   terminals. Match the most common behavior and offer configuration.## Sources

- [unicode-segmentation crate](https://docs.rs/unicode-segmentation) - UAX #29 implementation
- [unicode-width crate](https://docs.rs/unicode-width) - UAX #11 implementation with emoji sequence

  support

- [unicode-normalization crate](https://docs.rs/unicode-normalization) - UAX #15 implementation
- [UAX #11: East Asian Width](https://www.unicode.org/reports/tr11/) - Unicode Standard Annex
- [UAX #29: Unicode Text Segmentation](https://www.unicode.org/reports/tr29/) - Grapheme cluster

  boundaries

- [terminfo.dev](https://terminfo.dev/text/variation-selector-16-emoji) - Terminal emoji support

  testing

- [Notcurses wiki](https://nick-black.com/dankwiki/index.php/Notcurses) - EGC handling and wide char

  semantics

- [Alacritty cell.rs](https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/src/term/cell.rs) -

  Cell structure with wide char flags

- [xterm.js CellData.ts](https://github.com/xtermjs/xterm.js/blob/master/src/common/buffer/CellData.ts) -

  Packed cell content representation

- [BiDi in Terminal Emulators](https://terminal-wg.pages.freedesktop.org/bidi/) - freedesktop.org

  BiDi spec draft

- [Rich issue #3897](https://github.com/Textualize/rich/issues/3897) - VS16 width bug

  (representative of ecosystem-wide problem)

- [WezTerm issue #6912](https://github.com/wezterm/wezterm/issues/6912) - VS16 width handling
- [wcwidth PR #97](https://github.com/jquast/wcwidth/pull/97) - Adding VS16 support to Python

  wcwidth
