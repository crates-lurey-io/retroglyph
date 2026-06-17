# Accessibility Reference for Terminal/Grid Rendering Libraries

## Summary

Building an accessible terminal/grid rendering library requires work across multiple layers: screen
reader integration via platform accessibility APIs (or ARIA for web), WCAG-compliant contrast
enforcement, keyboard-only navigation, color blindness awareness, and OS-level
high-contrast/reduced-motion detection. The Rust ecosystem has practical crates for most of these
concerns, with AccessKit providing cross-platform screen reader integration and the `palette` crate
handling color science. Terminal emulators like xterm.js and Windows Terminal have established
patterns worth studying, while roguelike/BearLibTerminal projects illustrate common accessibility
failures to avoid.

---

## 1. Screen Reader Integration Patterns

### Web Backends (ARIA Roles)

For web-based rendering (e.g., canvas or DOM-based terminal), the W3C ARIA grid pattern defines the
required roles and structure:

- **Container**: `role="grid"` on the outer element, with `aria-labelledby` or `aria-label` for
  identification.
- **Rows**: `role="row"` for each row, nested inside the grid or a `role="rowgroup"`.
- **Cells**: `role="gridcell"` for data cells, `role="columnheader"` / `role="rowheader"` for
  headers.
- **Live regions**: Use `aria-live="assertive"` for streaming output that screen readers should
  announce immediately (xterm.js does exactly this).
- **Position tracking**: `aria-posinset` and `aria-setsize` on list items so the screen reader knows
  position within the scrollback buffer.
- **Selection**: `aria-selected="true"` on selected cells/rows.

xterm.js uses a shadow DOM overlay approach: it maintains a parallel `<div role="list">` container
with `<div role="listitem">` elements mirroring each visible terminal row. This overlay is
positioned over the actual canvas renderer but is only visible to assistive technologies. Key
implementation details from xterm.js `AccessibilityManager.ts`:

```
// Creates accessible row elements mirroring visible terminal content
_rowContainer.setAttribute('role', 'list');
element.setAttribute('role', 'listitem');
element.tabIndex = -1;

// Live region for announcing new output
_liveRegion.setAttribute('aria-live', 'assertive');

// Each row tracks its position in the full buffer
element.setAttribute('aria-posinset', posInSet);
element.setAttribute('aria-setsize', setSize);
```

The `_charsToConsume` queue prevents double-announcing: when a user types a character, it is
announced by the textarea's native accessibility; if the same character arrives as terminal output,
it is suppressed from the live region.

[Source: xterm.js AccessibilityManager.ts](https://github.com/xtermjs/xterm.js/blob/master/src/browser/AccessibilityManager.ts)
[Source: W3C ARIA Grid Pattern](https://www.w3.org/WAI/ARIA/apg/patterns/grid/)

### Native Backends (OS Accessibility APIs)

For native rendering, each platform has its own accessibility API:

| Platform  | API                       | AccessKit Adapter            |
| --------- | ------------------------- | ---------------------------- |
| Windows   | UI Automation (UIA)       | `accesskit_windows`          |
| macOS     | NSAccessibility (AppKit)  | `accesskit_macos`            |
| Linux/BSD | AT-SPI via D-Bus          | `accesskit_unix` (uses zbus) |
| iOS       | UIAccessibility           | `accesskit_ios`              |
| Android   | Android Accessibility API | `accesskit_android`          |

**AccessKit** is the recommended Rust crate for cross-platform screen reader integration. It
provides a data schema (tree of nodes with roles, names, and properties) inspired by Chromium's
accessibility architecture. The toolkit pushes an initial accessibility tree, then sends incremental
`TreeUpdate`s. The platform adapter translates this into native API calls.

Key AccessKit concepts for a grid library:

- Each cell becomes a `Node` with a `Role` (e.g., `Role::Cell`, `Role::GridCell`).
- Nodes have `NodeId` values that must be stable across updates.
- The tree structure mirrors the visual hierarchy: grid -> rows -> cells.
- Actions like `Action::Focus`, `Action::ScrollIntoView` are handled via the `ActionHandler` trait.
- The `ActivationHandler` / `DeactivationHandler` traits manage lazy initialization, so
  accessibility overhead is zero when no screen reader is active.

```rust
// Pseudocode for AccessKit integration
let mut tree = Tree::new(root_id);
let mut root = Node::new(Role::Grid);
root.set_name("Game Map");

for row_idx in 0..rows {
    let mut row_node = Node::new(Role::Row);
    for col_idx in 0..cols {
        let mut cell = Node::new(Role::Cell);
        cell.set_name(&describe_cell(row_idx, col_idx));
        row_node.push_child(cell_id);
    }
    root.push_child(row_id);
}
```

[Source: AccessKit GitHub](https://github.com/AccessKit/accesskit)
[Source: AccessKit docs.rs](https://docs.rs/accesskit/latest/accesskit/)

---

## 2. WCAG Contrast Requirements

### The Standard

WCAG 2.1 defines two levels of contrast compliance:

| Level            | Ratio | Applies to                           |
| ---------------- | ----- | ------------------------------------ |
| AA (minimum)     | 4.5:1 | Normal text (< 18pt or < 14pt bold)  |
| AA (large text)  | 3:1   | Large text (>= 18pt or >= 14pt bold) |
| AAA (enhanced)   | 7:1   | Normal text                          |
| AAA (large text) | 4.5:1 | Large text                           |

### Contrast Ratio Algorithm

The WCAG contrast ratio formula:

```
contrast_ratio = (L1 + 0.05) / (L2 + 0.05)
```

Where `L1` is the relative luminance of the lighter color and `L2` is the darker. Relative luminance
is computed from linear RGB:

```
L = 0.2126 * R_lin + 0.7152 * G_lin + 0.0722 * B_lin
```

To convert sRGB (0-255) to linear:

```
// For each channel C in {R, G, B}:
c_srgb = C / 255.0
if c_srgb <= 0.04045 {
    c_lin = c_srgb / 12.92
} else {
    c_lin = ((c_srgb + 0.055) / 1.055).powf(2.4)
}
```

### Programmatic Enforcement in Rust

The `palette` crate handles the color science correctly with type-safe color space conversions:

```rust
use palette::{Srgb, LinSrgb};

fn relative_luminance(color: Srgb<f32>) -> f32 {
    let lin: LinSrgb<f32> = color.into_linear();
    0.2126 * lin.red + 0.7152 * lin.green + 0.0722 * lin.blue
}

fn contrast_ratio(fg: Srgb<f32>, bg: Srgb<f32>) -> f32 {
    let l1 = relative_luminance(fg);
    let l2 = relative_luminance(bg);
    let (lighter, darker) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05) / (darker + 0.05)
}

fn meets_aa(fg: Srgb<f32>, bg: Srgb<f32>) -> bool {
    contrast_ratio(fg, bg) >= 4.5
}

fn meets_aaa(fg: Srgb<f32>, bg: Srgb<f32>) -> bool {
    contrast_ratio(fg, bg) >= 7.0
}
```

The `palette` crate also has a (deprecated) `RelativeContrast` trait and `contrast_ratio` function.
For new code, compute luminance via `LinSrgb` conversion as shown above. The crate's type system
prevents the common mistake of computing luminance on gamma-encoded values (which produces wrong
results).

**Implementation strategy**: Validate all theme colors at load time. If a foreground/background pair
fails the minimum contrast check, either:

1. Warn the user and suggest alternatives.
2. Auto-adjust: lighten or darken the foreground until the ratio threshold is met.
3. Provide a `ContrastEnforcer` that wraps color resolution and guarantees minimum ratios.

[Source: W3C WCAG 2.1 Contrast Minimum](https://www.w3.org/TR/WCAG21/#contrast-minimum)
[Source: palette crate](https://docs.rs/palette/latest/palette/)

---

## 3. Terminal Emulator Accessibility Approaches

### xterm.js Screen Reader Mode

xterm.js (used by VS Code's terminal, many web IDEs) maintains a parallel accessibility DOM
alongside the canvas renderer:

1. **Row mirroring**: A `<div role="list">` contains one `<div role="listitem">` per visible row.
   Each row's `textContent` is updated on every render to match the terminal buffer content.

2. **Live region**: A `<div aria-live="assertive">` announces new output. Characters are debounced
   and batched. A cap of 20 lines prevents overwhelming the screen reader with large output dumps
   (announces "Too much output to announce" instead).

3. **Scroll boundary focus**: When a screen reader user focuses the top or bottom boundary row,
   xterm.js scrolls the terminal buffer and shifts focus to maintain navigation flow. This simulates
   virtual scrolling through the full scrollback buffer.

4. **Selection bridging**: The `_handleSelectionChange` method translates browser selection events
   on the accessibility DOM back to terminal buffer coordinates, enabling native text selection to
   work with screen readers.

5. **Input deduplication**: Typed characters are tracked in `_charsToConsume[]`. When a character
   appears as terminal output matching what was just typed, it is not re-announced (since the
   textarea already announced it).

[Source: xterm.js AccessibilityManager.ts](https://github.com/xtermjs/xterm.js/blob/master/src/browser/AccessibilityManager.ts)

### Windows Terminal / Narrator

Windows Terminal uses the Windows UI Automation (UIA) API for Narrator and other screen readers:

- **UIA providers**: The terminal buffer is exposed as a UIA text pattern, allowing Narrator to read
  lines, words, and characters.
- **Cursor tracking**: The UIA implementation tracks the cursor position and announces the current
  line.
- **High contrast**: Windows Terminal respects the system high-contrast theme via the Windows
  contrast theme infrastructure (see section 5).
- **Font scaling**: Ctrl+scroll zooms text. The terminal respects per-profile `fontSize` settings
  and system DPI scaling.

Windows Terminal does not have a separate "screen reader mode" toggle; accessibility is always
active when a screen reader is detected.

[Source: Microsoft Windows Terminal docs](https://learn.microsoft.com/en-us/windows/terminal/)

### Implications for a Library

A terminal/grid library should:

- Maintain a parallel text representation of the grid (even if rendering to a canvas/GPU).
- Expose this text representation through the platform's accessibility API.
- Announce dynamic changes via live regions (web) or UIA text-changed events (Windows) or AT-SPI
  events (Linux).
- Track and announce cursor/focus position changes.
- Support text selection that maps back to grid coordinates.

---

## 4. Keyboard-Only Navigation Patterns for Grids

The W3C ARIA Authoring Practices Guide defines precise keyboard interaction patterns for grids.
These should be followed for interoperability with screen readers and for keyboard-only users.

### Data Grid Navigation

| Key          | Action                                                                |
| ------------ | --------------------------------------------------------------------- |
| Arrow keys   | Move focus one cell in the arrow direction. Do not wrap.              |
| Page Down/Up | Move focus down/up by a page of rows (author-defined count).          |
| Home         | Move to first cell in current row.                                    |
| End          | Move to last cell in current row.                                     |
| Ctrl+Home    | Move to first cell in first row.                                      |
| Ctrl+End     | Move to last cell in last row.                                        |
| Tab          | Move focus out of the grid to the next focusable element on the page. |

### Layout Grid Navigation (more flexible)

Layout grids may optionally wrap focus:

- Right Arrow at row end can wrap to first cell of next row.
- Down Arrow at column bottom can wrap to top of next column.

### Selection

| Key         | Action                               |
| ----------- | ------------------------------------ |
| Ctrl+Space  | Select column.                       |
| Shift+Space | Select row.                          |
| Ctrl+A      | Select all cells.                    |
| Shift+Arrow | Extend selection in arrow direction. |

### Cell Editing / Nested Interaction

When a cell contains editable content or sub-widgets:

- **Enter** or **F2**: Enters cell editing mode (disables grid navigation within the cell).
- **Escape**: Exits cell editing mode and restores grid navigation.
- While in editing mode, arrow keys operate within the cell's content rather than moving between
  cells.

### Implementation Pattern

```rust
enum GridFocusMode {
    Navigation,  // Arrow keys move between cells
    CellEdit,    // Arrow keys operate within the focused cell
}

struct GridNavigation {
    focus_row: usize,
    focus_col: usize,
    mode: GridFocusMode,
    // Only one cell is in the tab order at a time (roving tabindex pattern)
}

impl GridNavigation {
    fn handle_key(&mut self, key: Key, grid: &Grid) {
        match self.mode {
            GridFocusMode::Navigation => match key {
                Key::Right => self.focus_col = (self.focus_col + 1).min(grid.cols - 1),
                Key::Left => self.focus_col = self.focus_col.saturating_sub(1),
                Key::Down => self.focus_row = (self.focus_row + 1).min(grid.rows - 1),
                Key::Up => self.focus_row = self.focus_row.saturating_sub(1),
                Key::Home => self.focus_col = 0,
                Key::End => self.focus_col = grid.cols - 1,
                Key::Enter | Key::F2 => self.mode = GridFocusMode::CellEdit,
                _ => {}
            },
            GridFocusMode::CellEdit => match key {
                Key::Escape => self.mode = GridFocusMode::Navigation,
                // Forward other keys to the cell's widget
                _ => {}
            },
        }
    }
}
```

**Roving tabindex**: Only one element inside the grid participates in the page tab order. When the
user tabs into the grid, focus goes to the last-focused cell. All other cells have `tabindex="-1"`.
This keeps the page's tab sequence short.

[Source: W3C ARIA Grid Pattern](https://www.w3.org/WAI/ARIA/apg/patterns/grid/)

---

## 5. High-Contrast and Reduced-Motion Modes

### Detecting High Contrast

**Windows**: Use the `SystemParametersInfo` API with `SPI_GETHIGHCONTRAST` or listen for
`WM_THEMECHANGED`. In modern Windows apps, `ThemeSettings.HighContrast` provides this. Windows
defines four built-in contrast themes (Aquatic, Desert, Dusk, Night Sky) with user-customizable
color sets. The system exposes named colors (SystemColorWindowColor, SystemColorWindowTextColor,
etc.) that apps should use instead of hard-coded values.

**macOS**: Check `NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast`. Register for
`NSWorkspace.accessibilityDisplayOptionsDidChangeNotification`.

**Linux**: On GTK-based desktops, check the `gtk-theme-name` setting for high-contrast themes. On
GNOME, the `org.gnome.desktop.a11y.interface high-contrast` setting. There is no universal standard
across all Linux desktops.

**Web**: Use `@media (prefers-contrast: more)` or `@media (forced-colors: active)`. In JavaScript:
`window.matchMedia('(prefers-contrast: more)')`.

### Detecting Reduced Motion

**macOS**: `NSWorkspace.shared.accessibilityDisplayShouldReduceMotion`.

**Windows**: `SystemParametersInfo` with `SPI_GETCLIENTAREAANIMATION`, or
`UISettings.AnimationsEnabled`.

**Web**: `@media (prefers-reduced-motion: reduce)`.

**Linux**: `org.gnome.desktop.interface enable-animations` on GNOME.

### Implementation Strategy

```rust
/// System accessibility preferences detected at startup
/// and monitored for runtime changes.
pub struct AccessibilityPreferences {
    pub high_contrast: bool,
    pub reduce_motion: bool,
    pub increase_contrast: bool, // macOS "increase contrast" (borders, etc.)
    pub contrast_theme: Option<ContrastTheme>, // Windows named theme
}

/// Color resolution should consult these preferences:
pub fn resolve_color(
    semantic: SemanticColor,
    prefs: &AccessibilityPreferences,
    theme: &Theme,
) -> Srgb<u8> {
    if prefs.high_contrast {
        theme.high_contrast_palette.get(semantic)
    } else {
        theme.standard_palette.get(semantic)
    }
}
```

When high contrast is active:

- Use the OS-provided system colors rather than theme colors.
- Ensure all borders and separators are visible (don't rely on subtle color differences).
- Remove or simplify background images/gradients.
- Increase border widths.

When reduced motion is active:

- Skip scroll animations, cell transition effects, cursor blinking.
- Make state changes instantaneous rather than animated.

[Source: Microsoft Contrast Themes docs](https://learn.microsoft.com/en-us/windows/apps/design/accessibility/high-contrast-themes)

---

## 6. Color Blindness Considerations

### Types and Prevalence

| Type                       | Affected Cone  | Prevalence (males) | Confusion Colors                    |
| -------------------------- | -------------- | ------------------ | ----------------------------------- |
| Deuteranopia (green-blind) | M-cone (green) | ~6%                | Red/green, brown/green, blue/purple |
| Protanopia (red-blind)     | L-cone (red)   | ~2%                | Red/green, red appears dark         |
| Tritanopia (blue-blind)    | S-cone (blue)  | ~0.01%             | Blue/yellow, blue/green             |
| Achromatopsia (total)      | All cones      | ~0.003%            | All colors                          |

~8% of males and ~0.5% of females have some form of color vision deficiency. Deuteranopia +
protanopia (collectively "red-green color blindness") affect ~8% of males.

### Design Principles

1. **Never rely on color alone** to convey information. Always combine color with at least one other
   visual channel:
   - Shape/icon differences (circle vs. triangle vs. square)
   - Text labels
   - Patterns/textures (hatching, dots, stripes)
   - Position or size
   - Unicode symbols (checkmark, X, warning triangle)

2. **Safe color palette strategy**: Use colors that remain distinguishable under all three
   deficiency types. The key is varying **luminance** and **blue-yellow** axis rather than only
   **red-green**.

3. **Recommended palette foundations** (colorblind-safe):
   - **Okabe-Ito palette**: Specifically designed for color vision deficiency.
     - Orange (#E69F00), Sky Blue (#56B4E9), Bluish Green (#009E73), Yellow (#F0E442), Blue
       (#0072B2), Vermillion (#D55E00), Reddish Purple (#CC79A7), Black (#000000)
   - **IBM Design palette** (colorblind-safe subset)
   - **ColorBrewer** palettes (many are CVD-safe, marked in the tool)

4. **Avoid these combinations**:
   - Red vs. green (most common failure)
   - Red vs. brown
   - Blue vs. purple (for deuteranopia)
   - Green vs. yellow (for protanopia)
   - Blue vs. green (for tritanopia)

### Programmatic Simulation

To test color schemes programmatically, simulate CVD by transforming colors through the
Brettel/Vienot/Machado models. The `palette` crate does not include CVD simulation directly, but you
can implement the standard matrices:

```rust
/// Simulate deuteranopia using the Brettel method.
/// Apply a 3x3 matrix transform in linear RGB space.
fn simulate_deuteranopia(color: LinSrgb<f32>) -> LinSrgb<f32> {
    // Machado 2009 simulation matrix for severity=1.0 (full deuteranopia)
    let r = 0.367322 * color.red + 0.860646 * color.green - 0.227968 * color.blue;
    let g = 0.280085 * color.red + 0.672501 * color.green + 0.047413 * color.blue;
    let b = -0.011820 * color.red + 0.042940 * color.green + 0.968881 * color.blue;
    LinSrgb::new(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
}

/// Check if two colors are distinguishable under all CVD types.
fn is_cvd_safe(a: Srgb<f32>, b: Srgb<f32>, min_delta_e: f32) -> bool {
    let a_lin = a.into_linear();
    let b_lin = b.into_linear();
    for simulate in [simulate_deuteranopia, simulate_protanopia, simulate_tritanopia] {
        let sa = simulate(a_lin);
        let sb = simulate(b_lin);
        // Compute perceptual distance (e.g., delta E in Oklab)
        if delta_e_ok(sa, sb) < min_delta_e {
            return false;
        }
    }
    true
}
```

### For Terminal Color Schemes

When defining the 16 ANSI colors or extended palettes:

- Ensure "red" and "green" ANSI colors have different luminance (not just hue).
- Use brighter red (#FF6B6B) and darker green (#2D8B2D) to maintain luminance contrast.
- Provide an alternate "colorblind" theme that substitutes orange for red and blue for green.
- For status indicators (pass/fail, health bars), always pair color with a symbol: `[✓ Pass]` vs.
  `[✗ Fail]` rather than just green vs. red.

---

## 7. Text Scaling / Large Font Support

### Requirements

- Users may need to increase font size 200-400% beyond defaults.
- WCAG 1.4.4 (Resize Text): Content must remain usable at 200% zoom.
- WCAG 1.4.10 (Reflow): No horizontal scrolling at 400% zoom for content on a 320px-wide viewport.

### Implementation for Grid Libraries

1. **Dynamic grid dimensions**: The grid's row/column count should be recalculated when font size
   changes. A fixed 80x24 grid that clips at large font sizes is a failure.

```rust
pub fn calculate_grid_dimensions(
    viewport_width: u32,
    viewport_height: u32,
    cell_width: u32,
    cell_height: u32,
) -> (u32, u32) {
    let cols = viewport_width / cell_width;
    let rows = viewport_height / cell_height;
    (cols.max(1), rows.max(1))
}
```

2. **Respond to system DPI/scale changes**: On Windows, handle `WM_DPICHANGED`. On macOS, observe
   `NSScreen.backingScaleFactor` changes. xterm.js tracks `dprChange` (device pixel ratio) and
   rescales the accessibility overlay accordingly.

3. **Cell size must scale with font**: If the user's system font size is 2x default, each cell
   should be 2x the base size. Do not use fixed-pixel cell sizes.

4. **Minimum touch target**: For interactive cells, ensure at least 44x44 CSS pixels (per WCAG
   2.5.5) or 24x24 pixels (per WCAG 2.5.8, the AA target).

5. **Scrollable viewport**: When the grid is too large for the viewport at the current font size,
   provide scrolling rather than clipping. Announce viewport changes to screen readers.

6. **CJK and wide characters**: Some characters (CJK ideographs, emoji) occupy two cell widths. The
   library must account for `wcwidth` / Unicode East Asian Width property. xterm.js handles this by
   scaling row widths based on column mappings (`_alignRowWidth`).

---

## 8. How Notcurses Handles High Contrast

Notcurses does not have a dedicated "high contrast mode" in the traditional sense. Instead, it takes
a capability-based approach:

1. **24-bit color with quantization**: Notcurses natively supports 24-bit RGB color and
   automatically quantizes down for terminals with fewer color capabilities. It queries terminal
   capabilities at startup and adapts.

2. **High-contrast text**: The notcurses README explicitly lists "high-contrast text" as a visual
   feature. This refers to its ability to automatically select high-contrast foreground colors when
   rendering text over complex backgrounds (e.g., images). The library analyzes the background and
   picks a foreground that maintains readability.

3. **Theme detection**: Notcurses queries the terminal's background color using `\e]11;?\a` (OSC 11)
   and adjusts rendering accordingly. This helps it determine whether the terminal uses a light or
   dark theme and adapt colors.

4. **No OS high-contrast integration**: Notcurses operates within the terminal abstraction layer. It
   does not directly query Windows high-contrast settings or macOS accessibility preferences, since
   it targets the terminal emulator rather than native windowing. The terminal emulator itself is
   responsible for respecting OS high-contrast settings.

5. **`NCOPTION_NO_ALTERNATE_SCREEN`** and similar options provide control over rendering behavior,
   allowing apps to degrade gracefully.

### Lessons for a Grid Library

- If your library renders to a terminal (not a window), you inherit the terminal's accessibility
  support. You should still:
  - Detect terminal background color and choose appropriate foreground colors.
  - Support theme switching.
  - Avoid color combinations that are illegible on both light and dark backgrounds.
- If your library renders to a window (native/web), you are responsible for querying OS
  accessibility settings directly.

[Source: notcurses GitHub](https://github.com/dankamongmen/notcurses)

---

## 9. Practical Rust Crates for Accessibility

### Core Accessibility

| Crate                                                               | Purpose                                           | Platforms      |
| ------------------------------------------------------------------- | ------------------------------------------------- | -------------- |
| [`accesskit`](https://crates.io/crates/accesskit)                   | Accessibility tree schema and data types          | All            |
| [`accesskit_windows`](https://crates.io/crates/accesskit_windows)   | Windows UI Automation adapter                     | Windows        |
| [`accesskit_macos`](https://crates.io/crates/accesskit_macos)       | NSAccessibility adapter                           | macOS          |
| [`accesskit_unix`](https://crates.io/crates/accesskit_unix)         | AT-SPI (D-Bus) adapter                            | Linux/BSD      |
| [`accesskit_winit`](https://crates.io/crates/accesskit_winit)       | Winit windowing integration                       | Cross-platform |
| [`accesskit_consumer`](https://crates.io/crates/accesskit_consumer) | Tree traversal utilities, embedded assistive tech | All            |

### Text-to-Speech

| Crate                                 | Purpose                               | Platforms                                                                                        |
| ------------------------------------- | ------------------------------------- | ------------------------------------------------------------------------------------------------ |
| [`tts`](https://crates.io/crates/tts) | High-level TTS with multiple backends | Windows (WinRT/SAPI/Tolk), Linux (Speech Dispatcher), macOS (AVFoundation/AppKit), Android, WASM |

The `tts` crate supports screen reader passthrough via the `tolk` feature on Windows, which routes
speech through the active screen reader (NVDA, JAWS) rather than using a separate SAPI voice. This
is critical for screen reader users who have customized their speech settings.

### Color Science

| Crate                                         | Purpose                                                                                                 |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| [`palette`](https://crates.io/crates/palette) | Type-safe color spaces (sRGB, linear RGB, Oklab, Lab, etc.), conversion, contrast calculation, blending |

The `palette` crate is comprehensive for implementing WCAG contrast checks. Its type system enforces
correct linearization before computing luminance. It supports Oklab (perceptually uniform, good for
color difference calculations), CIE Lab (for Delta E), and all standard RGB encodings.

### Terminal / TUI

| Crate                                             | Purpose                                                |
| ------------------------------------------------- | ------------------------------------------------------ |
| [`crossterm`](https://crates.io/crates/crossterm) | Cross-platform terminal I/O (colors, keyboard, cursor) |
| [`ratatui`](https://crates.io/crates/ratatui)     | TUI framework (no built-in accessibility)              |

Note: Neither `crossterm` nor `ratatui` have built-in accessibility support. A grid library using
these would need to bolt on AccessKit or similar for screen reader integration.

### Patterns for Integration

```rust
// Pattern: Trait-based accessibility layer that backends implement

pub trait AccessibleGrid {
    /// Return the text content of a cell for screen readers.
    fn cell_text(&self, row: usize, col: usize) -> String;

    /// Return a human-readable description of a cell.
    fn cell_description(&self, row: usize, col: usize) -> String;

    /// Return the role of a cell (for ARIA/AccessKit).
    fn cell_role(&self, row: usize, col: usize) -> CellRole;

    /// Announce a message to the screen reader (live region / TTS).
    fn announce(&self, message: &str, priority: AnnouncePriority);

    /// Get current focus position.
    fn focused_cell(&self) -> (usize, usize);

    /// Build an AccessKit tree update for the current state.
    fn build_accessibility_tree(&self) -> accesskit::TreeUpdate;
}

pub enum AnnouncePriority {
    Polite,     // aria-live="polite" / low-priority TTS
    Assertive,  // aria-live="assertive" / interrupt current speech
}
```

---

## 10. What BearLibTerminal / Roguelikes Typically Get Wrong

Roguelike games and libraries like BearLibTerminal have historically been among the least accessible
game genres. Common failures:

### 1. Total Reliance on Color for Information

- Health shown as a red-to-green gradient with no numeric readout.
- Enemy types distinguished only by color (red 'D' vs. green 'D').
- Terrain types using only background color with identical foreground characters.
- **Fix**: Always pair color with distinct glyphs, characters, or text labels. A red dragon and
  green dragon should use different characters or have a text description available.

### 2. No Screen Reader Integration Whatsoever

- BearLibTerminal renders to its own OpenGL window with no OS accessibility API integration. It
  provides zero information to screen readers.
- The grid is a visual-only construct, invisible to assistive technology.
- **Fix**: Expose the grid content through AccessKit or similar. Maintain a text representation that
  a screen reader can query.

### 3. Hardcoded, Small Font Size

- BearLibTerminal uses a fixed bitmap font (often 8x8 or similar) with limited scaling options.
- No support for system font size preferences or DPI scaling.
- **Fix**: Support vector fonts. Scale cell size with system DPI. Allow user-configurable font sizes
  up to at least 200% of default.

### 4. No Keyboard Remapping

- Roguelikes often use vi-keys (hjkl) or numpad for movement with no alternative.
- No way to rebind keys for one-handed play or alternative input devices.
- **Fix**: Provide a full key remapping system. Support arrow keys as a default alternative to
  vi-keys. Allow mouse/gamepad input as movement alternatives.

### 5. Information Density Requires Vision

- The entire game state is presented as a grid of characters that must be visually parsed.
- No way to query "what is at position X,Y?" via keyboard.
- No audio cues for spatial relationships.
- **Fix**: Implement a "look" mode where the player can arrow-key through the map and hear/read
  descriptions of each cell. Provide audio cues for important events (combat, items, hazards).

### 6. No Pause or Turn Control

- Even turn-based games may have animations or timed events.
- **Fix**: All animations should be skippable. Ensure the game is fully playable in a step-by-step
  mode.

### 7. Contrast and Color Scheme Rigidity

- Many roguelikes use a single dark-background theme with no alternatives.
- Background/foreground combinations chosen for aesthetics over readability.
- **Fix**: Provide at least dark, light, and high-contrast themes. Validate all color combinations
  against WCAG AA minimums. Offer a colorblind-friendly palette option.

### 8. No Audio Alternative

- Spatial relationships are conveyed only visually.
- **Fix**: Consider sonification (audio representation of grid position), directional audio cues, or
  a text-based "narrator" mode that describes the player's surroundings.

### 9. Unstructured Output

- Game messages scroll past in a log with no way to review them.
- No message categorization or filtering.
- **Fix**: Provide a searchable, scrollable message log. Categorize messages (combat, items,
  environment). Allow screen readers to access the log.

---

## Sources

### Kept

- **xterm.js AccessibilityManager.ts**
  (<https://github.com/xtermjs/xterm.js/blob/master/src/browser/AccessibilityManager.ts>) - Primary
  source for how a production terminal handles screen reader integration in a web context. Shows the
  shadow DOM approach, live regions, scroll boundary handling, and input deduplication.
- **AccessKit GitHub** (<https://github.com/AccessKit/accesskit>) - The only comprehensive
  cross-platform accessibility crate for Rust. Covers the data schema, platform adapters, and
  architecture.
- **W3C ARIA Grid Pattern** (<https://www.w3.org/WAI/ARIA/apg/patterns/grid/>) - Authoritative
  specification for keyboard navigation and ARIA roles in grid widgets. Defines both data grid and
  layout grid patterns.
- **WCAG 2.1 Specification** (<https://www.w3.org/TR/WCAG21/>) - Defines contrast ratio requirements
  and calculations.
- **Microsoft Contrast Themes docs**
  (<https://learn.microsoft.com/en-us/windows/apps/design/accessibility/high-contrast-themes>) -
  Detailed guidance on Windows high-contrast theme integration, system color resources, and best
  practices.
- **palette crate docs** (<https://docs.rs/palette/latest/palette/>) - Type-safe color science library
  with correct sRGB/linear conversions needed for WCAG compliance.
- **tts-rs** (<https://github.com/ndarilek/tts-rs>) - Cross-platform TTS crate for Rust with screen
  reader integration support.
- **notcurses GitHub** (<https://github.com/dankamongmen/notcurses>) - Reference for how a modern TUI
  library handles color capabilities and terminal feature detection.

### Dropped

- WebAIM Contrast Checker (<https://webaim.org/resources/contrastchecker/>) - Only an interactive
  tool, not useful for implementation reference.
- Windows Terminal rendering settings
  (<https://learn.microsoft.com/en-us/windows/terminal/customize-settings/rendering>) - Only covers
  GPU rendering options, not accessibility.
- notcurses visual man page - Covers image/video rendering, not accessibility.

## Gaps

1. **Exact notcurses high-contrast implementation**: The notcurses codebase is large and the
   high-contrast text feature is not well-documented. Would require reading the C source
   (`ncplane_set_fg_*` and related functions) to understand the exact algorithm for auto-selecting
   high-contrast foreground colors.

2. **AT-SPI protocol details for Linux**: The AccessKit Unix adapter handles this, but the specific
   AT-SPI properties needed for grid navigation on Linux (Orca screen reader) are not
   well-documented outside of GNOME accessibility developer guides.

3. **BearLibTerminal specific issues**: BearLibTerminal is largely unmaintained and has no
   accessibility documentation. The failures listed are based on the architecture and common
   roguelike patterns rather than BearLibTerminal-specific analysis.

4. **WCAG 3.0 / APCA**: The upcoming WCAG 3.0 standard proposes APCA (Advanced Perceptual Contrast
   Algorithm) as a replacement for the current luminance-based contrast formula. This may change
   contrast requirements. Currently in draft; not yet adopted.

5. **Color vision deficiency simulation matrices**: The Machado 2009 matrices shown are approximate.
   For production use, consider using a validated CVD simulation library or the full Brettel 1997
   algorithm with proper half-plane projection.

6. **Console/TTY accessibility on Linux without a GUI**: When there is no desktop environment (pure
   TTY), accessibility support is extremely limited. The `speakup` kernel module provides basic
   screen reading for the Linux console, but it is not commonly used and has limited capabilities.
