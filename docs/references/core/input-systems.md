# Research: Unified Input Abstraction for a Rust Terminal/Grid Library

## Summary

A unified input layer must reconcile three fundamentally different event models: winit (rich,
layout-aware keyboard with physical/logical key split and IME), crossterm (simple keycode+modifiers
at cell granularity), and web DOM (key/code string properties with composition events). The design
below defines a common `Event` enum with `KeyEvent`, `MouseEvent`, `GamepadEvent`, and lifecycle
events, plus a `Modifiers` bitflags type and an `InputBackend` trait. Each backend implements a
single conversion: native event to unified event.

## 1. Winit's Keyboard/Mouse Event Model

Winit (v0.30+) provides the richest input model of the three backends.

### Keyboard

````rust
// winit::event::KeyEvent
pub struct KeyEvent {
    pub physical_key: PhysicalKey,   // Layout-independent scancode (KeyCode enum)
    pub logical_key: Key,            // Layout-dependent meaning (Named/Character/Dead/Unidentified)
    pub text: Option<SmolStr>,       // Text produced (respects dead keys, None on release)
    pub location: Location,          // Standard/Left/Right/Numpad
    pub state: ElementState,         // Pressed/Released
    pub repeat: bool,                // OS-generated repeat
    pub text_with_all_modifiers: Option<SmolStr>,
    pub key_without_modifiers: Key,  // Logical key ignoring modifiers
}
```rust

Key design points:

- **PhysicalKey** maps to scancodes (e.g. `KeyCode::KeyW`). Use for WASD-style positional bindings.
- **LogicalKey** (`Key` enum) respects keyboard layout. `Key::Named(NamedKey::Enter)` or

  `Key::Character("a")`. Use for shortcuts like Ctrl+C.

- **Modifiers** arrive separately via `WindowEvent::ModifiersChanged(Modifiers)`. The `Modifiers`

  struct has a `ModifiersState` bitflags (SHIFT, CONTROL, ALT, SUPER) and physical key tracking for
  left/right disambiguation.

- **Location** distinguishes Left/Right modifier keys and Numpad keys.

### Mouse/Pointer

Winit unifies mouse, touch, and pen through pointer events:

- `WindowEvent::PointerButton { device_id, state, position, button: ButtonSource }` where

  `ButtonSource::Mouse(MouseButton)`.

- `WindowEvent::PointerMoved { device_id, position: PhysicalPosition<f64> }` -- pixel coordinates.
- `WindowEvent::MouseWheel { delta: MouseScrollDelta }` with `LineDelta(x,y)` or `PixelDelta(pos)`.
- `WindowEvent::Focused(bool)` for focus/blur.

### IME

```rust
pub enum Ime {
    Enabled,
    Preedit(String, Option<(usize, usize)>),  // preedit text + cursor range
    Commit(String),                             // finalized text
    Disabled,
}
```yaml

Flow: Enabled -> Preedit\* -> Commit -> (back to keys or Disabled).

### Sources

- [winit KeyEvent docs](https://rust-windowing.github.io/winit/winit/event/struct.KeyEvent.html)
- [winit WindowEvent docs](https://docs.rs/winit/latest/winit/event/enum.WindowEvent.html)
- [winit Ime docs](https://docs.rs/winit/latest/winit/event/enum.Ime.html)
- [DeepWiki: winit event handling patterns](https://deepwiki.com/rust-windowing/winit/7.2-event-handling-patterns)

---

## 2. Crossterm's Event Model

Crossterm provides a simpler, terminal-centric model. Events are read from stdin in raw mode.

### Top-level Event

```rust
pub enum Event {
    FocusGained,
    FocusLost,
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),          // Bracketed paste (must be enabled)
    Resize(u16, u16),       // (columns, rows)
}
```text

### KeyEvent

```rust
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub kind: KeyEventKind,      // Press/Release/Repeat
    pub state: KeyEventState,    // Extra state flags
}
```rust

**KeyCode** is a flat enum with no physical/logical distinction:

- Named keys: `Backspace`, `Enter`, `Left`, `Right`, `Up`, `Down`, `Home`, `End`, `PageUp`,

  `PageDown`, `Tab`, `BackTab`, `Delete`, `Insert`, `Esc`

- Character: `Char(char)` -- already reflects layout
- Function: `F(u8)` -- F1-F24
- Lock keys: `CapsLock`, `ScrollLock`, `NumLock`
- Modifiers as keys: `Modifier(ModifierKeyCode)` (with kitty protocol)
- Media keys: `Media(MediaKeyCode)`

**KeyModifiers**is a bitflags struct: `SHIFT`, `CONTROL`, `ALT`, `SUPER`, `HYPER`, `META`, `NONE`.**KeyEventKind**: `Press`, `Release`, `Repeat`. Release/Repeat require the kitty keyboard protocol
(`PushKeyboardEnhancementFlags`).

### MouseEvent

```rust
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub column: u16,         // Cell column (0-indexed)
    pub row: u16,            // Cell row (0-indexed)
    pub modifiers: KeyModifiers,
}
```rust

**MouseEventKind**: `Down(MouseButton)`, `Up(MouseButton)`, `Drag(MouseButton)`, `Moved`,
`ScrollDown`, `ScrollUp`, `ScrollLeft`, `ScrollRight`.

**MouseButton**: `Left`, `Right`, `Middle`.

Coordinates are already in cell units. No pixel-to-cell conversion needed.

### Limitations

- No physical/logical key distinction.
- `BackTab` is a synthetic key representing Shift+Tab, not a separate physical key.
- Mouse buttons on `Up`/`Drag` may not be reported on some terminals (defaults to `Left`).
- Focus events require explicit `EnableFocusChange`.
- No IME support -- terminals handle composition before delivering characters.

### Sources (2)

- [crossterm Event docs](https://docs.rs/crossterm/latest/crossterm/event/enum.Event.html)
- [crossterm KeyEvent docs](https://docs.rs/crossterm/latest/crossterm/event/struct.KeyEvent.html)
- [crossterm KeyCode docs](https://docs.rs/crossterm/latest/crossterm/event/enum.KeyCode.html)
- [crossterm MouseEvent docs](https://docs.rs/crossterm/latest/crossterm/event/struct.MouseEvent.html)

---

## 3. Web DOM Event Model

### KeyboardEvent

DOM keyboard events use string properties rather than enums:

| Property  | Meaning                                   | Example (QWERTY, pressing "A") |
| --------- | ----------------------------------------- | ------------------------------ |
| `key`     | Logical key value, layout-dependent       | `"a"` (or `"A"` with shift)    |
| `code`    | Physical key position, layout-independent | `"KeyA"`                       |
| `keyCode` | **Deprecated** numeric code               | `65`                           |

- `key` is the equivalent of winit's `logical_key` / crossterm's `KeyCode::Char`.
- `code` is the equivalent of winit's `physical_key` / `PhysicalKey::Code(KeyCode)`.
- Modifiers: `altKey`, `ctrlKey`, `metaKey`, `shiftKey` booleans on each event.
- `location`: `DOM_KEY_LOCATION_STANDARD` (0), `LEFT` (1), `RIGHT` (2), `NUMPAD` (3).
- `repeat`: boolean for auto-repeat.
- `isComposing`: boolean, true between `compositionstart` and `compositionend`.

Events: `keydown`, `keyup`. (`keypress` is deprecated.)

### MouseEvent (2)

- `button`: 0=primary, 1=middle, 2=secondary, 3=back, 4=forward.
- `buttons`: bitmask of all pressed buttons.
- `clientX`/`clientY`: viewport pixel coordinates.
- `offsetX`/`offsetY`: relative to target element padding edge.
- `movementX`/`movementY`: delta from last move.
- Modifiers: `altKey`, `ctrlKey`, `metaKey`, `shiftKey`.

Events: `mousedown`, `mouseup`, `mousemove`, `click`, `dblclick`, `wheel`, `contextmenu`.

### WheelEvent

- `deltaX`, `deltaY`, `deltaZ`: scroll amounts.
- `deltaMode`: 0=pixels, 1=lines, 2=pages.

### Composition (IME)

- `compositionstart` -> `compositionupdate`\* -> `compositionend`.
- `CompositionEvent.data` contains the composed string.
- During composition, `KeyboardEvent.isComposing` is true; apps should suppress key handling.

### Focus

- `focus`/`blur` events on the element or window.
- `document.hasFocus()` for polling.

### Sources (3)

- [MDN KeyboardEvent](https://developer.mozilla.org/en-US/docs/Web/API/KeyboardEvent)
- [MDN MouseEvent](https://developer.mozilla.org/en-US/docs/Web/API/MouseEvent)
- [MDN compositionstart](https://developer.mozilla.org/en-US/docs/Web/API/Element/compositionstart_event)

---

## 4. Designing a Common Event Enum

The unified event enum should cover the lowest common denominator plus optional extensions. Backend
converters map native events to this enum. Information that only one backend can provide (like
physical key on crossterm) is represented as `Option`.

```rust
/// Top-level unified event.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Keyboard key press/release/repeat.
    Key(KeyEvent),
    /// Mouse button, movement, or scroll.
    Mouse(MouseEvent),
    /// Gamepad button or axis input.
    Gamepad(GamepadEvent),
    /// IME composition events.
    Ime(ImeEvent),
    /// Window/terminal gained focus.
    FocusGained,
    /// Window/terminal lost focus.
    FocusLost,
    /// Paste from clipboard (bracketed paste in terminals, paste event on web).
    Paste(String),
    /// Surface/terminal resized. Values are in cells (columns, rows).
    Resize { cols: u16, rows: u16 },
    /// Gamepad connected.
    GamepadConnected(GamepadId),
    /// Gamepad disconnected.
    GamepadDisconnected(GamepadId),
}
````

---

## 5. Keyboard: KeyEvent Design

The key challenge: winit has physical+logical keys, crossterm has only logical-ish KeyCode, and DOM
has key+code strings. The unified model exposes both, with physical key as `Option` since crossterm
can't always provide it.

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct KeyEvent {
    /// What key was pressed, in a layout-dependent way.
    /// Maps from: winit logical_key, crossterm KeyCode, DOM key property.
    pub key: Key,
    /// Physical key position (layout-independent).
    /// Maps from: winit physical_key, DOM code property.
    /// `None` for crossterm (no physical key info).
    pub physical_key: Option<PhysicalKey>,
    /// Text produced by this keypress, if any.
    /// Maps from: winit text, crossterm Char, DOM key (for printable).
    pub text: Option<String>,
    /// Press, Release, or Repeat.
    pub state: KeyState,
    /// Active modifiers at time of event.
    pub modifiers: Modifiers,
    /// Which side of the keyboard (left/right shift, etc).
    /// `None` when the backend can't distinguish.
    pub location: Option<KeyLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyState {
    Pressed,
    Released,
    Repeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyLocation {
    Standard,
    Left,
    Right,
    Numpad,
}
```

### The Key enum (logical)

```rust
/// Logical key identity, layout-dependent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// A printable character.
    Char(char),
    /// A named non-character key.
    Named(NamedKey),
    /// Unidentified key with optional native code.
    Unidentified,
}

/// Named keys shared across all backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedKey {
    // Navigation
    ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
    Home, End, PageUp, PageDown,
    // Editing
    Backspace, Delete, Insert, Enter, Tab, Escape,
    // Function keys
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,
    // Lock keys
    CapsLock, NumLock, ScrollLock,
    // System
    PrintScreen, Pause, Menu,
    // Modifiers (when reported as key events)
    Shift, Control, Alt, Super, Hyper, Meta,
    // Media
    MediaPlay, MediaPause, MediaPlayPause, MediaStop,
    MediaNext, MediaPrevious,
    MediaVolumeUp, MediaVolumeDown, MediaMute,
    // Space
    Space,
}
```

### The PhysicalKey enum

```rust
/// Physical key position, layout-independent. Based on USB HID scancodes.
/// Mirrors the W3C "code" values and winit's KeyCode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhysicalKey {
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ,
    KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT,
    KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    Digit0, Digit1, Digit2, Digit3, Digit4,
    Digit5, Digit6, Digit7, Digit8, Digit9,
    // Row keys
    Backquote, Minus, Equal, BracketLeft, BracketRight,
    Backslash, Semicolon, Quote, Comma, Period, Slash,
    // Whitespace/editing
    Space, Enter, Tab, Backspace, Delete, Insert, Escape,
    // Navigation
    ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
    Home, End, PageUp, PageDown,
    // Function
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,
    // Modifiers
    ShiftLeft, ShiftRight, ControlLeft, ControlRight,
    AltLeft, AltRight, SuperLeft, SuperRight,
    // Numpad
    Numpad0, Numpad1, Numpad2, Numpad3, Numpad4,
    Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
    NumpadAdd, NumpadSubtract, NumpadMultiply, NumpadDivide,
    NumpadDecimal, NumpadEnter,
    // Lock
    CapsLock, NumLock, ScrollLock,
    // System
    PrintScreen, Pause, Menu,
    // Catch-all
    Unknown(u32),
}
```

### Mapping table: Key conversion across backends

| Unified Key             | winit                             | crossterm            | DOM `key`     |
| ----------------------- | --------------------------------- | -------------------- | ------------- |
| `Key::Char('a')`        | `Key::Character("a")`             | `KeyCode::Char('a')` | `"a"`         |
| `Key::Named(Enter)`     | `Key::Named(NamedKey::Enter)`     | `KeyCode::Enter`     | `"Enter"`     |
| `Key::Named(Tab)`       | `Key::Named(NamedKey::Tab)`       | `KeyCode::Tab`       | `"Tab"`       |
| `Key::Named(Escape)`    | `Key::Named(NamedKey::Escape)`    | `KeyCode::Esc`       | `"Escape"`    |
| `Key::Named(ArrowUp)`   | `Key::Named(NamedKey::ArrowUp)`   | `KeyCode::Up`        | `"ArrowUp"`   |
| `Key::Named(Backspace)` | `Key::Named(NamedKey::Backspace)` | `KeyCode::Backspace` | `"Backspace"` |
| `Key::Named(F1)`        | `Key::Named(NamedKey::F1)`        | `KeyCode::F(1)`      | `"F1"`        |

### Mapping table: PhysicalKey conversion

| Unified PhysicalKey      | winit                | DOM `code`    |
| ------------------------ | -------------------- | ------------- |
| `PhysicalKey::KeyA`      | `KeyCode::KeyA`      | `"KeyA"`      |
| `PhysicalKey::Digit1`    | `KeyCode::Digit1`    | `"Digit1"`    |
| `PhysicalKey::ShiftLeft` | `KeyCode::ShiftLeft` | `"ShiftLeft"` |
| `PhysicalKey::Space`     | `KeyCode::Space`     | `"Space"`     |

Crossterm has no physical key equivalent. The converter sets `physical_key: None`.

---

## 6. Modifier Key Normalization

Each backend reports modifiers differently:

| Modifier          | winit `ModifiersState` | crossterm `KeyModifiers` | DOM           |
| ----------------- | ---------------------- | ------------------------ | ------------- |
| Shift             | `SHIFT`                | `SHIFT`                  | `shiftKey`    |
| Control           | `CONTROL`              | `CONTROL`                | `ctrlKey`     |
| Alt / Option      | `ALT`                  | `ALT`                    | `altKey`      |
| Super / Cmd / Win | `SUPER`                | `SUPER`                  | `metaKey`     |
| Hyper             | (not present)          | `HYPER`                  | (not present) |
| Meta              | (not present)          | `META`                   | (not present) |

### Unified Modifiers

```rust
bitflags::bitflags! {
    /// Normalized modifier key state.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Modifiers: u8 {
        const SHIFT   = 0b0000_0001;
        const CONTROL = 0b0000_0010;
        const ALT     = 0b0000_0100;
        const SUPER   = 0b0000_1000;
        const HYPER   = 0b0001_0000;
        const META    = 0b0010_0000;
    }
}

impl Modifiers {
    /// Returns true if any "command" modifier is set.
    /// On macOS this checks SUPER (Cmd); elsewhere checks CONTROL.
    pub fn command(&self) -> bool {
        if cfg!(target_os = "macos") {
            self.contains(Modifiers::SUPER)
        } else {
            self.contains(Modifiers::CONTROL)
        }
    }
}
```

### Normalization rules

1. **macOS Ctrl+Click**: Some terminals/macOS report Ctrl+LeftClick as right-click. The crossterm

   backend should pass through as-is; the application layer decides.

1. **SUPER vs META**: winit calls it SUPER. DOM calls it Meta. Crossterm has both SUPER and META.

   The unified model maps winit SUPER and DOM `metaKey` to `Modifiers::SUPER`. Crossterm's META maps
   to `Modifiers::META` (rare, mostly for legacy terminals).

1. **Crossterm BackTab**: `KeyCode::BackTab` should be converted to `Key::Named(Tab)` with

   `Modifiers::SHIFT` set.

1. **winit ModifiersChanged**: winit delivers modifiers as a separate event. The backend converter

   should cache the latest `ModifiersState` and attach it to each `KeyEvent` it produces. Do NOT
   emit a separate unified event for modifier changes; they're embedded in key events.

1. **DOM per-event booleans**: DOM events carry `ctrlKey`, `shiftKey`, `altKey`, `metaKey` on every

   event. Convert directly to `Modifiers` bitflags.

---

## 7. IME / Text Input vs Key Events

IME is critical for CJK input, accented characters, and emoji pickers.

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ImeEvent {
    /// IME composition session started.
    Enabled,
    /// Preedit text updated. Cursor position as byte offset range.
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    /// Final text committed. Insert this into the text buffer.
    Commit(String),
    /// IME session ended.
    Disabled,
}
```

### Backend mapping

| Unified              | winit                     | crossterm | DOM                        |
| -------------------- | ------------------------- | --------- | -------------------------- |
| `ImeEvent::Enabled`  | `Ime::Enabled`            | N/A       | `compositionstart`         |
| `ImeEvent::Preedit`  | `Ime::Preedit(s, cursor)` | N/A       | `compositionupdate` (data) |
| `ImeEvent::Commit`   | `Ime::Commit(s)`          | N/A       | `compositionend` (data)    |
| `ImeEvent::Disabled` | `Ime::Disabled`           | N/A       | (no direct equivalent)     |

### Handling rules

1. **Crossterm**: Terminals handle IME composition themselves. By the time crossterm delivers a

   `KeyEvent`, the composed character is already resolved. Crossterm's `KeyCode::Char(c)` may
   contain multi-byte characters. No `ImeEvent` is emitted from this backend.

1. **Winit**: Forward `WindowEvent::Ime` variants directly.
1. **DOM**: Listen for `compositionstart`, `compositionupdate`, `compositionend`. During composition
   (`isComposing == true`), suppress `keydown`/`keyup` handling to avoid double-processing.

1. **Application rule**: When receiving `ImeEvent::Preedit`, display inline preview. On

   `ImeEvent::Commit`, insert text. Between `Enabled` and `Disabled`, ignore raw `KeyEvent` text.

---

## 8. Mouse Event Design and Coordinate Translation

### Unified MouseEvent

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    /// Position in cell coordinates (column, row), 0-indexed.
    pub cell: CellPosition,
    /// Sub-cell pixel offset within the cell. `None` if unavailable.
    /// Range: (0..cell_width, 0..cell_height).
    pub sub_cell: Option<(f32, f32)>,
    pub modifiers: Modifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellPosition {
    pub col: u16,
    pub row: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MouseEventKind {
    Down(MouseButton),
    Up(MouseButton),
    Drag(MouseButton),
    Moved,
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Other(u8),
}
```

### Pixel-to-cell coordinate translation

This only applies to winit and web backends, since crossterm already delivers cell coordinates.

```rust
/// Grid metrics needed for coordinate translation.
pub struct GridMetrics {
    /// Width of a single cell in pixels.
    pub cell_width: f32,
    /// Height of a single cell in pixels.
    pub cell_height: f32,
    /// Pixel offset of the grid's top-left corner from the window's top-left.
    pub grid_offset_x: f32,
    pub grid_offset_y: f32,
}

impl GridMetrics {
    /// Convert pixel position to cell coordinates with sub-cell offset.
    pub fn pixel_to_cell(&self, px: f64, py: f64) -> (CellPosition, (f32, f32)) {
        let local_x = (px as f32 - self.grid_offset_x).max(0.0);
        let local_y = (py as f32 - self.grid_offset_y).max(0.0);

        let col = (local_x / self.cell_width) as u16;
        let row = (local_y / self.cell_height) as u16;

        let sub_x = local_x % self.cell_width;
        let sub_y = local_y % self.cell_height;

        (CellPosition { col, row }, (sub_x, sub_y))
    }
}
```

### Backend mapping (2)

| Backend   | Native coordinates                | Conversion                                          |
| --------- | --------------------------------- | --------------------------------------------------- |
| crossterm | Cell (column, row) directly       | None needed. `sub_cell = None`                      |
| winit     | `PhysicalPosition<f64>` in pixels | Use `GridMetrics::pixel_to_cell()`                  |
| web DOM   | `offsetX`/`offsetY` in CSS pixels | Use `GridMetrics::pixel_to_cell()` with DPR scaling |

### Scroll normalization

- Winit `LineDelta(x, y)`: map `y > 0` to `ScrollUp`, `y < 0` to `ScrollDown`, `x` to

  `ScrollLeft`/`ScrollRight`.

- Winit `PixelDelta(pos)`: divide by cell height to get line-equivalent scrolls, or emit fractional

  scroll amounts as repeated line scrolls.

- DOM `WheelEvent`: check `deltaMode`. 0=pixels (divide by cell height), 1=lines (use directly),

  2=pages (multiply by visible rows).

- Crossterm: `ScrollUp`, `ScrollDown`, `ScrollLeft`, `ScrollRight` are already discrete line events.

---

## 9. Gamepad Input (gilrs)

The `gilrs` crate provides cross-platform gamepad support (Linux/BSD evdev, Windows WGI/XInput,
macOS, Wasm).

### gilrs event model

```rust
// gilrs::ev::Event
pub struct Event {
    pub id: GamepadId,
    pub event: EventType,
    pub time: SystemTime,
}

pub enum EventType {
    ButtonPressed(Button, Code),
    ButtonRepeated(Button, Code),
    ButtonReleased(Button, Code),
    ButtonChanged(Button, f32, Code),  // analog trigger/button
    AxisChanged(Axis, f32, Code),      // stick axis, -1.0..1.0
    Connected,
    Disconnected,
    Dropped,
    ForceFeedbackEffectCompleted,
}
```

Buttons: `South`/`East`/`North`/`West` (face), triggers, bumpers, thumbsticks, d-pad,
Select/Start/Mode. Axes: `LeftStickX`/`Y`, `RightStickX`/`Y`, `LeftZ`/`RightZ`, `DPadX`/`DPadY`.

### Unified gamepad events

```rust
pub type GamepadId = u32;

#[derive(Debug, Clone, PartialEq)]
pub enum GamepadEvent {
    ButtonPressed {
        id: GamepadId,
        button: GamepadButton,
    },
    ButtonReleased {
        id: GamepadId,
        button: GamepadButton,
    },
    ButtonRepeated {
        id: GamepadId,
        button: GamepadButton,
    },
    /// Analog button value changed. Range: 0.0..=1.0
    ButtonChanged {
        id: GamepadId,
        button: GamepadButton,
        value: f32,
    },
    /// Axis value changed. Range: -1.0..=1.0
    AxisChanged {
        id: GamepadId,
        axis: GamepadAxis,
        value: f32,
    },
}

/// Standard gamepad button layout (matches gilrs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GamepadButton {
    South, East, North, West,       // Face buttons (A/B/X/Y or Cross/Circle/Triangle/Square)
    LeftBumper, RightBumper,         // Shoulder buttons (LB/RB or L1/R1)
    LeftTrigger, RightTrigger,       // Triggers (LT/RT or L2/R2)
    Select, Start, Mode,             // Menu buttons
    LeftStick, RightStick,           // Thumbstick clicks (L3/R3)
    DPadUp, DPadDown, DPadLeft, DPadRight,
    Unknown(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GamepadAxis {
    LeftStickX, LeftStickY,
    RightStickX, RightStickY,
    LeftTrigger, RightTrigger,       // Analog trigger axes
    DPadX, DPadY,
    Unknown(u16),
}
```

### Integration notes

- Gilrs is optional; gate behind a `gamepad` feature flag.
- Gilrs works on wasm via the Gamepad API, so it covers all three target platforms.
- The `Connected`/`Disconnected` events map to top-level

  `Event::GamepadConnected`/`GamepadDisconnected`.

- `Dropped` and `ForceFeedbackEffectCompleted` are internal to gilrs; don't surface in the unified

  API.

- Gilrs requires polling (`gilrs.next_event()`) on the same thread as the event loop.

---

## 10. Focus/Blur Events

```rust
// Already in the Event enum:
// Event::FocusGained
// Event::FocusLost
```

| Backend   | Source                                                                   |
| --------- | ------------------------------------------------------------------------ |
| winit     | `WindowEvent::Focused(true)` / `WindowEvent::Focused(false)`             |
| crossterm | `Event::FocusGained` / `Event::FocusLost` (requires `EnableFocusChange`) |
| web DOM   | `focus` / `blur` events on the canvas/container element                  |

Crossterm requires explicitly enabling focus reporting with `EnableFocusChange` command. The backend
initializer should handle this automatically.

---

## 11. Concrete Trait and Backend Design

### The InputBackend trait

```rust
/// Trait implemented by each platform backend to produce unified events.
pub trait InputBackend {
    /// Poll for the next event. Returns `None` if no event is available.
    /// Implementations should be non-blocking.
    fn poll_event(&mut self) -> Option<Event>;

    /// Block until an event is available, with optional timeout.
    fn wait_event(&mut self, timeout: Option<std::time::Duration>) -> Option<Event>;

    /// Current grid metrics for coordinate translation.
    /// Windowed/web backends must provide this; terminal backends return
    /// a 1:1 identity (cell_width=1, cell_height=1, offset=0).
    fn grid_metrics(&self) -> GridMetrics;
}
```

### Crossterm backend converter (sketch)

```rust
pub struct CrosstermBackend;

impl CrosstermBackend {
    fn convert_key(ct: crossterm::event::KeyEvent) -> Event {
        let state = match ct.kind {
            crossterm::event::KeyEventKind::Press => KeyState::Pressed,
            crossterm::event::KeyEventKind::Release => KeyState::Released,
            crossterm::event::KeyEventKind::Repeat => KeyState::Repeat,
        };

        let modifiers = convert_crossterm_modifiers(ct.modifiers);

        // Handle BackTab -> Tab + Shift
        let (key, text, extra_mods) = match ct.code {
            crossterm::event::KeyCode::Char(c) => (
                Key::Char(c),
                Some(c.to_string()),
                Modifiers::empty(),
            ),
            crossterm::event::KeyCode::Enter => (Key::Named(NamedKey::Enter), None, Modifiers::empty()),
            crossterm::event::KeyCode::Backspace => (Key::Named(NamedKey::Backspace), None, Modifiers::empty()),
            crossterm::event::KeyCode::Tab => (Key::Named(NamedKey::Tab), None, Modifiers::empty()),
            crossterm::event::KeyCode::BackTab => (Key::Named(NamedKey::Tab), None, Modifiers::SHIFT),
            crossterm::event::KeyCode::Esc => (Key::Named(NamedKey::Escape), None, Modifiers::empty()),
            crossterm::event::KeyCode::Up => (Key::Named(NamedKey::ArrowUp), None, Modifiers::empty()),
            crossterm::event::KeyCode::Down => (Key::Named(NamedKey::ArrowDown), None, Modifiers::empty()),
            crossterm::event::KeyCode::Left => (Key::Named(NamedKey::ArrowLeft), None, Modifiers::empty()),
            crossterm::event::KeyCode::Right => (Key::Named(NamedKey::ArrowRight), None, Modifiers::empty()),
            crossterm::event::KeyCode::Home => (Key::Named(NamedKey::Home), None, Modifiers::empty()),
            crossterm::event::KeyCode::End => (Key::Named(NamedKey::End), None, Modifiers::empty()),
            crossterm::event::KeyCode::PageUp => (Key::Named(NamedKey::PageUp), None, Modifiers::empty()),
            crossterm::event::KeyCode::PageDown => (Key::Named(NamedKey::PageDown), None, Modifiers::empty()),
            crossterm::event::KeyCode::Delete => (Key::Named(NamedKey::Delete), None, Modifiers::empty()),
            crossterm::event::KeyCode::Insert => (Key::Named(NamedKey::Insert), None, Modifiers::empty()),
            crossterm::event::KeyCode::F(n) => {
                let named = match n {
                    1 => NamedKey::F1, 2 => NamedKey::F2, 3 => NamedKey::F3,
                    4 => NamedKey::F4, 5 => NamedKey::F5, 6 => NamedKey::F6,
                    7 => NamedKey::F7, 8 => NamedKey::F8, 9 => NamedKey::F9,
                    10 => NamedKey::F10, 11 => NamedKey::F11, 12 => NamedKey::F12,
                    // F13-F24 etc.
                    _ => return Event::Key(KeyEvent {
                        key: Key::Unidentified, physical_key: None,
                        text: None, state, modifiers, location: None,
                    }),
                };
                (Key::Named(named), None, Modifiers::empty())
            }
            _ => (Key::Unidentified, None, Modifiers::empty()),
        };

        Event::Key(KeyEvent {
            key,
            physical_key: None,  // crossterm doesn't provide physical keys
            text,
            state,
            modifiers: modifiers | extra_mods,
            location: None,      // crossterm doesn't distinguish left/right
        })
    }

    fn convert_mouse(ct: crossterm::event::MouseEvent) -> Event {
        let button = |b: crossterm::event::MouseButton| match b {
            crossterm::event::MouseButton::Left => MouseButton::Left,
            crossterm::event::MouseButton::Right => MouseButton::Right,
            crossterm::event::MouseButton::Middle => MouseButton::Middle,
        };

        let kind = match ct.kind {
            crossterm::event::MouseEventKind::Down(b) => MouseEventKind::Down(button(b)),
            crossterm::event::MouseEventKind::Up(b) => MouseEventKind::Up(button(b)),
            crossterm::event::MouseEventKind::Drag(b) => MouseEventKind::Drag(button(b)),
            crossterm::event::MouseEventKind::Moved => MouseEventKind::Moved,
            crossterm::event::MouseEventKind::ScrollUp => MouseEventKind::ScrollUp,
            crossterm::event::MouseEventKind::ScrollDown => MouseEventKind::ScrollDown,
            crossterm::event::MouseEventKind::ScrollLeft => MouseEventKind::ScrollLeft,
            crossterm::event::MouseEventKind::ScrollRight => MouseEventKind::ScrollRight,
        };

        Event::Mouse(MouseEvent {
            kind,
            cell: CellPosition { col: ct.column, row: ct.row },
            sub_cell: None,  // terminals don't provide sub-cell precision
            modifiers: convert_crossterm_modifiers(ct.modifiers),
        })
    }
}

fn convert_crossterm_modifiers(m: crossterm::event::KeyModifiers) -> Modifiers {
    let mut out = Modifiers::empty();
    if m.contains(crossterm::event::KeyModifiers::SHIFT) { out |= Modifiers::SHIFT; }
    if m.contains(crossterm::event::KeyModifiers::CONTROL) { out |= Modifiers::CONTROL; }
    if m.contains(crossterm::event::KeyModifiers::ALT) { out |= Modifiers::ALT; }
    if m.contains(crossterm::event::KeyModifiers::SUPER) { out |= Modifiers::SUPER; }
    if m.contains(crossterm::event::KeyModifiers::HYPER) { out |= Modifiers::HYPER; }
    if m.contains(crossterm::event::KeyModifiers::META) { out |= Modifiers::META; }
    out
}
```

### Winit backend converter (sketch)

```rust
pub struct WinitBackend {
    /// Cached modifier state from ModifiersChanged events.
    modifiers: Modifiers,
    /// Grid metrics for pixel-to-cell conversion.
    metrics: GridMetrics,
}

impl WinitBackend {
    fn convert_key(&self, wk: winit::event::KeyEvent) -> Event {
        let key = match &wk.logical_key {
            winit::keyboard::Key::Character(s) => {
                // Character keys: take first char
                s.chars().next()
                    .map(Key::Char)
                    .unwrap_or(Key::Unidentified)
            }
            winit::keyboard::Key::Named(n) => {
                Key::Named(convert_winit_named_key(*n))
            }
            winit::keyboard::Key::Dead(_) | winit::keyboard::Key::Unidentified(_) => {
                Key::Unidentified
            }
        };

        let physical_key = match wk.physical_key {
            winit::keyboard::PhysicalKey::Code(code) => {
                Some(convert_winit_keycode(code))
            }
            winit::keyboard::PhysicalKey::Unidentified(_) => None,
        };

        let location = Some(match wk.location {
            winit::keyboard::KeyLocation::Standard => KeyLocation::Standard,
            winit::keyboard::KeyLocation::Left => KeyLocation::Left,
            winit::keyboard::KeyLocation::Right => KeyLocation::Right,
            winit::keyboard::KeyLocation::Numpad => KeyLocation::Numpad,
        });

        let state = match wk.state {
            winit::event::ElementState::Pressed if wk.repeat => KeyState::Repeat,
            winit::event::ElementState::Pressed => KeyState::Pressed,
            winit::event::ElementState::Released => KeyState::Released,
        };

        Event::Key(KeyEvent {
            key,
            physical_key,
            text: wk.text.map(|s| s.to_string()),
            state,
            modifiers: self.modifiers,
            location,
        })
    }

    fn convert_ime(&self, ime: winit::event::Ime) -> Event {
        Event::Ime(match ime {
            winit::event::Ime::Enabled => ImeEvent::Enabled,
            winit::event::Ime::Preedit(text, cursor) => ImeEvent::Preedit { text, cursor },
            winit::event::Ime::Commit(text) => ImeEvent::Commit(text),
            winit::event::Ime::Disabled => ImeEvent::Disabled,
        })
    }

    fn convert_pointer_button(
        &self,
        position: winit::dpi::PhysicalPosition<f64>,
        button: winit::event::ButtonSource,
        state: winit::event::ElementState,
    ) -> Option<Event> {
        let mouse_btn = match button {
            winit::event::ButtonSource::Mouse(b) => match b {
                winit::event::MouseButton::Left => MouseButton::Left,
                winit::event::MouseButton::Right => MouseButton::Right,
                winit::event::MouseButton::Middle => MouseButton::Middle,
                winit::event::MouseButton::Back => MouseButton::Back,
                winit::event::MouseButton::Forward => MouseButton::Forward,
                winit::event::MouseButton::Other(n) => MouseButton::Other(n as u8),
            },
            _ => return None, // touch/pen handled separately if needed
        };

        let (cell, sub) = self.metrics.pixel_to_cell(position.x, position.y);

        let kind = match state {
            winit::event::ElementState::Pressed => MouseEventKind::Down(mouse_btn),
            winit::event::ElementState::Released => MouseEventKind::Up(mouse_btn),
        };

        Some(Event::Mouse(MouseEvent {
            kind,
            cell,
            sub_cell: Some(sub),
            modifiers: self.modifiers,
        }))
    }
}
```

### Web backend converter (sketch, wasm_bindgen)

```rust
pub struct WebBackend {
    metrics: GridMetrics,
    dpr: f64, // window.devicePixelRatio
}

impl WebBackend {
    fn convert_keyboard(&self, ev: web_sys::KeyboardEvent, pressed: bool) -> Option<Event> {
        // Suppress during IME composition
        if ev.is_composing() {
            return None;
        }

        let key_str = ev.key();
        let key = if key_str.len() == 1 {
            Key::Char(key_str.chars().next().unwrap())
        } else {
            match key_str.as_str() {
                "Enter" => Key::Named(NamedKey::Enter),
                "Backspace" => Key::Named(NamedKey::Backspace),
                "Tab" => Key::Named(NamedKey::Tab),
                "Escape" => Key::Named(NamedKey::Escape),
                "ArrowUp" => Key::Named(NamedKey::ArrowUp),
                "ArrowDown" => Key::Named(NamedKey::ArrowDown),
                "ArrowLeft" => Key::Named(NamedKey::ArrowLeft),
                "ArrowRight" => Key::Named(NamedKey::ArrowRight),
                "Home" => Key::Named(NamedKey::Home),
                "End" => Key::Named(NamedKey::End),
                "PageUp" => Key::Named(NamedKey::PageUp),
                "PageDown" => Key::Named(NamedKey::PageDown),
                "Delete" => Key::Named(NamedKey::Delete),
                "Insert" => Key::Named(NamedKey::Insert),
                s if s.starts_with('F') => {
                    // F1-F24
                    if let Ok(n) = s[1..].parse::<u8>() {
                        // map F1..F24 to named key
                        convert_f_key(n)
                    } else {
                        Key::Unidentified
                    }
                }
                _ => Key::Unidentified,
            }
        };

        let code_str = ev.code();
        let physical_key = convert_dom_code(&code_str);

        let location = match ev.location() {
            0 => Some(KeyLocation::Standard),
            1 => Some(KeyLocation::Left),
            2 => Some(KeyLocation::Right),
            3 => Some(KeyLocation::Numpad),
            _ => None,
        };

        let mut modifiers = Modifiers::empty();
        if ev.shift_key() { modifiers |= Modifiers::SHIFT; }
        if ev.ctrl_key() { modifiers |= Modifiers::CONTROL; }
        if ev.alt_key() { modifiers |= Modifiers::ALT; }
        if ev.meta_key() { modifiers |= Modifiers::SUPER; }

        let state = if pressed {
            if ev.repeat() { KeyState::Repeat } else { KeyState::Pressed }
        } else {
            KeyState::Released
        };

        let text = if pressed && key_str.len() == 1 {
            Some(key_str.clone())
        } else {
            None
        };

        Some(Event::Key(KeyEvent {
            key,
            physical_key,
            text,
            state,
            modifiers,
            location,
        }))
    }

    fn convert_mouse_down(&self, ev: web_sys::MouseEvent) -> Event {
        let btn = match ev.button() {
            0 => MouseButton::Left,
            1 => MouseButton::Middle,
            2 => MouseButton::Right,
            3 => MouseButton::Back,
            4 => MouseButton::Forward,
            n => MouseButton::Other(n as u8),
        };

        let px = ev.offset_x() as f64 * self.dpr;
        let py = ev.offset_y() as f64 * self.dpr;
        let (cell, sub) = self.metrics.pixel_to_cell(px, py);

        let mut modifiers = Modifiers::empty();
        if ev.shift_key() { modifiers |= Modifiers::SHIFT; }
        if ev.ctrl_key() { modifiers |= Modifiers::CONTROL; }
        if ev.alt_key() { modifiers |= Modifiers::ALT; }
        if ev.meta_key() { modifiers |= Modifiers::SUPER; }

        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(btn),
            cell,
            sub_cell: Some(sub),
            modifiers,
        })
    }

    fn convert_composition(&self, ev: web_sys::CompositionEvent, event_type: &str) -> Option<Event> {
        match event_type {
            "compositionstart" => Some(Event::Ime(ImeEvent::Enabled)),
            "compositionupdate" => Some(Event::Ime(ImeEvent::Preedit {
                text: ev.data().unwrap_or_default(),
                cursor: None, // DOM doesn't provide cursor position
            })),
            "compositionend" => Some(Event::Ime(ImeEvent::Commit(
                ev.data().unwrap_or_default(),
            ))),
            _ => None,
        }
    }
}
```

---

## 12. Complete Type Summary

All types an implementor needs, collected in one place:

```rust
// ---- Core Event ----
pub enum Event { Key(KeyEvent), Mouse(MouseEvent), Gamepad(GamepadEvent),
    Ime(ImeEvent), FocusGained, FocusLost, Paste(String),
    Resize { cols: u16, rows: u16 },
    GamepadConnected(GamepadId), GamepadDisconnected(GamepadId) }

// ---- Keyboard ----
pub struct KeyEvent { key: Key, physical_key: Option<PhysicalKey>,
    text: Option<String>, state: KeyState, modifiers: Modifiers,
    location: Option<KeyLocation> }
pub enum Key { Char(char), Named(NamedKey), Unidentified }
pub enum NamedKey { /* ~45 variants: arrows, editing, function, locks, media, modifiers */ }
pub enum PhysicalKey { /* ~100 variants: letters, digits, punctuation, function, modifiers, numpad */ }
pub enum KeyState { Pressed, Released, Repeat }
pub enum KeyLocation { Standard, Left, Right, Numpad }

// ---- Modifiers ----
bitflags! { pub struct Modifiers: u8 { SHIFT, CONTROL, ALT, SUPER, HYPER, META } }

// ---- Mouse ----
pub struct MouseEvent { kind: MouseEventKind, cell: CellPosition,
    sub_cell: Option<(f32, f32)>, modifiers: Modifiers }
pub struct CellPosition { col: u16, row: u16 }
pub enum MouseEventKind { Down(MouseButton), Up(MouseButton), Drag(MouseButton),
    Moved, ScrollUp, ScrollDown, ScrollLeft, ScrollRight }
pub enum MouseButton { Left, Right, Middle, Back, Forward, Other(u8) }

// ---- Gamepad ----
pub type GamepadId = u32;
pub enum GamepadEvent { ButtonPressed{..}, ButtonReleased{..}, ButtonRepeated{..},
    ButtonChanged{..value:f32}, AxisChanged{..value:f32} }
pub enum GamepadButton { South, East, North, West, LeftBumper, RightBumper,
    LeftTrigger, RightTrigger, Select, Start, Mode,
    LeftStick, RightStick, DPadUp, DPadDown, DPadLeft, DPadRight, Unknown(u16) }
pub enum GamepadAxis { LeftStickX, LeftStickY, RightStickX, RightStickY,
    LeftTrigger, RightTrigger, DPadX, DPadY, Unknown(u16) }

// ---- IME ----
pub enum ImeEvent { Enabled, Preedit { text: String, cursor: Option<(usize,usize)> },
    Commit(String), Disabled }

// ---- Coordinate Translation ----
pub struct GridMetrics { cell_width: f32, cell_height: f32,
    grid_offset_x: f32, grid_offset_y: f32 }

// ---- Backend Trait ----
pub trait InputBackend {
    fn poll_event(&mut self) -> Option<Event>;
    fn wait_event(&mut self, timeout: Option<Duration>) -> Option<Event>;
    fn grid_metrics(&self) -> GridMetrics;
}
```

---

## Design Decisions and Trade-offs

1. **Physical key as Option**: Crossterm can't provide physical key info. Rather than inventing a

   mapping from logical keys back to physical (which would be wrong on non-QWERTY layouts), we use
   `Option<PhysicalKey>`. Applications that need physical keys for WASD controls should use winit or
   web backends.

1. **Cell coordinates as primary**: Since this is a grid/terminal library, cell coordinates are the

   natural unit. Pixel-based backends (winit, web) convert to cells using `GridMetrics`. Sub-cell
   precision is available via `sub_cell` for smooth mouse tracking.

1. **No separate ModifiersChanged event**: Winit emits `ModifiersChanged` as a standalone event. The

   unified API folds modifiers into every `KeyEvent` and `MouseEvent` instead. The backend caches
   the latest modifier state.

1. **Gamepad behind feature flag**: Not all applications need gamepad input. The `gilrs` dependency

   is heavy. Gate behind `#[cfg(feature = "gamepad")]`.

1. **IME events separate from key events**: During IME composition, key events should be suppressed.

   Having `ImeEvent` as a separate variant makes it easy for applications to handle text input
   correctly: when you see `ImeEvent::Enabled`, stop processing `KeyEvent.text` until
   `ImeEvent::Disabled`.

1. **Scroll events as discrete variants**: Rather than a continuous `Scroll { dx, dy }`, we use

   `ScrollUp`/`ScrollDown`/`ScrollLeft`/`ScrollRight` to match the terminal model. For pixel-precise
   scrolling (touchpad), the backend should accumulate pixel deltas and emit discrete scroll events
   when they cross a line threshold.

---

## Sources (4)

- Kept:

  [winit KeyEvent docs](https://rust-windowing.github.io/winit/winit/event/struct.KeyEvent.html) -
  primary source for winit keyboard model

- Kept: [winit WindowEvent docs](https://docs.rs/winit/latest/winit/event/enum.WindowEvent.html) -

  full event enum reference

- Kept: [winit Ime docs](https://docs.rs/winit/latest/winit/event/enum.Ime.html) - IME event model

  with examples

- Kept:

  [DeepWiki winit patterns](https://deepwiki.com/rust-windowing/winit/7.2-event-handling-patterns) -
  practical patterns for keyboard/mouse/IME

- Kept: [crossterm Event docs](https://docs.rs/crossterm/latest/crossterm/event/enum.Event.html) -

  crossterm event model

- Kept:

  [crossterm KeyCode docs](https://docs.rs/crossterm/latest/crossterm/event/enum.KeyCode.html) -
  full key code enum

- Kept:

  [crossterm MouseEvent docs](https://docs.rs/crossterm/latest/crossterm/event/struct.MouseEvent.html) -
  mouse event with cell coords

- Kept: [MDN KeyboardEvent](https://developer.mozilla.org/en-US/docs/Web/API/KeyboardEvent) - DOM

  keyboard model, key vs code

- Kept: [MDN MouseEvent](https://developer.mozilla.org/en-US/docs/Web/API/MouseEvent) - DOM mouse

  model with coordinate systems

- Kept:

  [MDN compositionstart](https://developer.mozilla.org/en-US/docs/Web/API/Element/compositionstart_event) -
  DOM IME events

- Kept: [gilrs docs](https://docs.rs/gilrs/latest/gilrs/) - gamepad input crate overview
- Kept: [gilrs EventType docs](https://docs.rs/gilrs/latest/gilrs/ev/enum.EventType.html) - gamepad

  event types

- Kept: [gilrs Button docs](https://docs.rs/gilrs/latest/gilrs/ev/enum.Button.html) - standard

  button layout

- Kept: [gilrs Axis docs](https://docs.rs/gilrs/latest/gilrs/ev/enum.Axis.html) - axis types
- Dropped: winit issue #4233 (raw input on Windows) - platform-specific bug, not relevant to

  abstraction design

- Dropped: winit RawKeyEvent docs - raw device events are lower-level than what the grid library

  needs

## Gaps

1. **Touch input**: Not covered. Winit provides touch/pen via PointerSource, web has TouchEvent.

   Could be added as `Event::Touch(TouchEvent)` with cell coordinates later.

1. **Drag-and-drop**: Winit has DragEntered/DragDropped; not modeled here since it's file-level, not

   cell-level input.

1. **Dead key composition**: Winit's `Key::Dead(Option<char>)` is more nuanced than crossterm or

   DOM. The unified `Key::Unidentified` may lose dead-key identity. Could add `Key::Dead(char)` if
   needed.

1. **Gamepad rumble/force-feedback output**: gilrs supports it, but this is an output concern, not

   input. Separate API.

1. **Key repeat rate**: Varies by OS/terminal. No way to configure from the abstraction layer.
1. **Crossterm kitty protocol detection**: Whether `KeyEventKind::Release`/`Repeat` work depends on
   terminal support. The backend should detect this and potentially only emit `Pressed` events on
   unsupported terminals.
