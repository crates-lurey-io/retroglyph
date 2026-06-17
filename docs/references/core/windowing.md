# Winit Crate Reference

Comprehensive reference for `winit` (latest/v0.30+), the cross-platform window creation and event
loop library for Rust. Covers everything needed to build a terminal/grid rendering application on
top of winit.

**Platforms:** Windows 7+, macOS 10.11+, Linux (X11 + Wayland), Web (WASM), iOS, Android.

---

## 1. Event Loop Architecture

### Core types

- `EventLoop` -- created once via `EventLoop::new().unwrap()`. Owns the platform connection
  (X11/Wayland socket, Win32 message pump, etc.). Not `Send` on most platforms.
- `ActiveEventLoop` -- a reference passed into every callback. Provides `create_window()`,
  `create_custom_cursor()`, `available_monitors()`, `primary_monitor()`, `set_control_flow()`,
  `exit()`, and `display_handle()`.
- `ApplicationHandler` -- trait you implement. The event loop calls methods on your struct.

### ApplicationHandler trait

```rust
pub trait ApplicationHandler<T: 'static = ()> {
    // Required
    fn resumed(&mut self, event_loop: &ActiveEventLoop);
    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent);

    // Provided (optional overrides)
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) { .. }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: T) { .. }
    fn device_event(&mut self, event_loop: &ActiveEventLoop, device_id: DeviceId, event: DeviceEvent) { .. }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) { .. }
    fn suspended(&mut self, event_loop: &ActiveEventLoop) { .. }
    fn exiting(&mut self, event_loop: &ActiveEventLoop) { .. }
    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) { .. }
}
```

Key callback semantics:

- **`resumed()`** -- called when the app can create windows and render surfaces. On desktop this
  fires once after init. On mobile (Android/iOS) it fires each time the app returns to foreground.
  _Always create windows here, not before `run_app()`._
- **`suspended()`** -- render surfaces are invalidated. Drop GPU resources. Only meaningful on
  Android/iOS/Web.
- **`window_event()`** -- all per-window events (input, resize, redraw, close).
- **`device_event()`** -- raw hardware events independent of window focus. Useful for raw mouse
  deltas (e.g., camera control).
- **`about_to_wait()`** -- fires after all pending events are dispatched. Set `ControlFlow` here.
  _Not_ the right place to render; use `RedrawRequested` instead.
- **`new_events(cause)`** -- fires before each batch. `cause` is `StartCause::Init`, `Poll`,
  `WaitCancelled`, or `ResumeTimeReached`.

### Running the loop

```rust
let event_loop = EventLoop::new().unwrap();
event_loop.set_control_flow(ControlFlow::Wait); // or Poll
let mut app = MyApp::default();
event_loop.run_app(&mut app).unwrap();
```

`run_app()` takes ownership of the current thread and never returns (on most platforms). It calls
`exit()` when `ActiveEventLoop::exit()` is invoked.

### ControlFlow

```rust
pub enum ControlFlow {
    Poll,              // spin continuously, ideal for games/animation
    Wait,              // sleep until next event, ideal for reactive UIs
    WaitUntil(Instant) // sleep until a specific time, for timed updates
}
```

Set via `event_loop.set_control_flow()` (before `run_app`) or `active_event_loop.set_control_flow()`
(inside callbacks). Default is `Wait`.

For a terminal emulator: use `Wait` normally, switch to `Poll` during active animations (cursor
blink, scrolling).

### pump_events (external loop integration)

```rust
use winit::platform::pump_events::EventLoopExtPumpEvents;

// Instead of run_app(), pump events in your own loop:
loop {
    let status = event_loop.pump_app_events(Some(Duration::ZERO), &mut app);
    if let PumpStatus::Exit(code) = status {
        std::process::exit(code);
    }
    // do other work here
}
```

**Caveats:**

- Not available on Web or iOS (those platforms own the main thread).
- On Wayland with `ControlFlow::Wait` and timeout `Duration::ZERO`, it can still block if there are
  no events. A known issue; use `ControlFlow::Poll` if you need non-blocking behavior.
- Discouraged by winit authors for most use cases. Prefer `run_app()`.

### EventLoopProxy (cross-thread wakeup)

```rust
let proxy = event_loop.create_proxy();
// From another thread:
proxy.send_event(MyEvent::DataReady).unwrap();
// Received in ApplicationHandler::user_event()
```

`EventLoopProxy` is `Send + Sync`. Use it to wake the event loop from background threads (e.g., PTY
reader thread in a terminal).

---

## 2. Window Creation and WindowAttributes

Windows are created inside `resumed()` via `ActiveEventLoop::create_window()`:

```rust
fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    let attrs = Window::default_attributes()
        .with_title("My Terminal")
        .with_surface_size(LogicalSize::new(800.0, 600.0))
        .with_min_surface_size(LogicalSize::new(200.0, 100.0))
        .with_visible(false)       // show after first render to avoid flash
        .with_transparent(false)
        .with_decorations(true);
    self.window = Some(event_loop.create_window(attrs).unwrap());
}
```

### WindowAttributes fields

| Field                       | Type                      | Default          | Notes                                                                                                  |
| --------------------------- | ------------------------- | ---------------- | ------------------------------------------------------------------------------------------------------ |
| `surface_size`              | `Option<Size>`            | `None`           | Initial client area size                                                                               |
| `min_surface_size`          | `Option<Size>`            | `None`           | Minimum resize constraint                                                                              |
| `max_surface_size`          | `Option<Size>`            | `None`           | Maximum resize constraint                                                                              |
| `surface_resize_increments` | `Option<Size>`            | `None`           | Resize step size. **For terminal emulators**: set to cell size so the window snaps to grid boundaries. |
| `position`                  | `Option<Position>`        | `None`           | Initial position on screen                                                                             |
| `resizable`                 | `bool`                    | `true`           |                                                                                                        |
| `enabled_buttons`           | `WindowButtons`           | `all()`          | Bitmask: minimize, maximize, close                                                                     |
| `title`                     | `String`                  | `"winit window"` | Title bar text                                                                                         |
| `maximized`                 | `bool`                    | `false`          | Start maximized                                                                                        |
| `visible`                   | `bool`                    | `true`           | Set `false` to avoid showing garbage before first render                                               |
| `transparent`               | `bool`                    | `false`          | Window transparency support                                                                            |
| `blur`                      | `bool`                    | `false`          | Background blur effect                                                                                 |
| `decorations`               | `bool`                    | `true`           | Title bar and borders                                                                                  |
| `window_icon`               | `Option<Icon>`            | `None`           | Taskbar/title bar icon                                                                                 |
| `preferred_theme`           | `Option<Theme>`           | `None`           | `Light`, `Dark`, or system default                                                                     |
| `content_protected`         | `bool`                    | `false`          | Prevent screen capture                                                                                 |
| `window_level`              | `WindowLevel`             | `Normal`         | `AlwaysOnBottom`, `Normal`, `AlwaysOnTop`                                                              |
| `active`                    | `bool`                    | `true`           | Request initial keyboard focus                                                                         |
| `cursor`                    | `Cursor`                  | `Default`        | Initial cursor icon                                                                                    |
| `fullscreen`                | `Option<Fullscreen>`      | `None`           | Start in fullscreen mode                                                                               |
| `parent_window`             | `Option<RawWindowHandle>` | `None`           | Child window (unsafe)                                                                                  |

### Key Window methods (post-creation)

```rust
window.id() -> WindowId
window.scale_factor() -> f64
window.request_redraw()               // enqueues RedrawRequested
window.pre_present_notify()           // call before swapchain present (Wayland frame callback)
window.inner_size() -> PhysicalSize<u32>
window.outer_size() -> PhysicalSize<u32>
window.request_inner_size(Size) -> Option<PhysicalSize<u32>>
window.set_min_inner_size(Option<Size>)
window.set_title(&str)
window.set_visible(bool)
window.set_fullscreen(Option<Fullscreen>)
window.set_cursor(Cursor)
window.set_cursor_visible(bool)
window.set_cursor_grab(CursorGrabMode)  // Confined | Locked | None
window.set_cursor_position(Position)
window.set_ime_allowed(bool)
window.set_ime_cursor_area(Position, Size)
window.set_theme(Option<Theme>)
window.theme() -> Option<Theme>
window.set_decorations(bool)
window.focus_window()
window.drag_window()                   // initiate OS window drag
window.drag_resize_window(ResizeDirection)
```

---

## 3. Keyboard Events

### KeyEvent struct

```rust
pub struct KeyEvent {
    pub physical_key: PhysicalKey,   // layout-independent scan position
    pub logical_key: Key,            // layout-dependent meaning
    pub text: Option<SmolStr>,       // text produced by this keypress
    pub location: KeyLocation,       // Left, Right, Numpad, Standard
    pub state: ElementState,         // Pressed or Released
    pub repeat: bool,                // OS key-repeat
}
```

Delivered via `WindowEvent::KeyboardInput { device_id, event: KeyEvent, is_synthetic }`.

`is_synthetic` is `true` for fake press/release events generated when a window gains/loses focus
(X11, Windows only).

### PhysicalKey (scancode equivalent)

```rust
pub enum PhysicalKey {
    Code(KeyCode),                   // e.g., KeyCode::KeyW, KeyCode::ArrowUp
    Unidentified(NativeKeyCode),     // platform-specific fallback
}
```

`KeyCode` maps to the physical position on the keyboard, independent of layout. `KeyCode::KeyW` is
always the key at the W position on QWERTY, even if the user's layout is AZERTY (where that key
produces "Z").

Use `PhysicalKey` for: game-style bindings, WASD movement, position-dependent shortcuts.

Platform-specific scancode conversion (Windows/macOS/X11/Wayland only):

```rust
use winit::platform::scancode::PhysicalKeyExtScancode;
let scancode: Option<u32> = physical_key.to_scancode();
let key = PhysicalKey::from_scancode(42);
```

### LogicalKey (Key enum)

```rust
pub enum Key<Str = SmolStr> {
    Named(NamedKey),           // Enter, Escape, Tab, ArrowUp, F1, etc.
    Character(Str),            // "a", "A", "é", "1", etc.
    Unidentified(NativeKey),   // fallback
    Dead(Option<char>),        // dead key for compose sequences
}
```

`Key` represents what the keypress _means_ in the current layout. Affected by Shift and other
modifiers (except Ctrl).

Matching:

```rust
match event.logical_key.as_ref() {
    Key::Character("c") if modifiers.control_key() => { /* Ctrl+C */ },
    Key::Named(NamedKey::Enter) => { /* Enter */ },
    Key::Named(NamedKey::Escape) => { /* Escape */ },
    _ => {}
}
```

`key.to_text()` converts to text representation: `Key::Named(NamedKey::Enter)` -> `Some("\r")`,
`Key::Named(NamedKey::F1)` -> `None`.

### key_without_modifiers (platform extension)

Available on Windows, macOS, X11, Wayland, Orbital:

```rust
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
// Ignores all modifiers including Shift, Caps Lock, Ctrl
let base_key = event.key_without_modifiers();
// text_with_all_modifiers includes Ctrl effects
let text = event.text_with_all_modifiers();
```

### Text input strategy

For a terminal emulator:

1. Use `event.text` for character input (already accounts for layout + modifiers).
2. Use `event.logical_key` for named keys (Enter, Escape, arrows, function keys).
3. Use `event.physical_key` only for configurable keybindings (let users remap).
4. Check `event.repeat` to distinguish held keys from initial presses.
5. Track `ModifiersChanged` for modifier state; don't rely on per-event modifier bits.

### IME (Input Method Editor)

Enable with `window.set_ime_allowed(true)`. Position the candidate box with
`window.set_ime_cursor_area(position, size)`.

Events arrive as `WindowEvent::Ime(ime)`:

```rust
pub enum Ime {
    Enabled,
    Preedit(String, Option<(usize, usize)>),  // composing text + cursor range
    Commit(String),                             // final text
    Disabled,
}
```

Platform support: macOS, Windows, X11, Wayland. Not supported on iOS/Android/Web/Orbital.

### ModifiersChanged

```rust
WindowEvent::ModifiersChanged(modifiers) => {
    let state: ModifiersState = modifiers.state();
    // state.shift_key(), state.control_key(), state.alt_key(), state.super_key()
}
```

---

## 4. Mouse Events

### Cursor movement

```rust
WindowEvent::CursorMoved { device_id, position } => {
    // position: PhysicalPosition<f64>, relative to top-left of client area
}
WindowEvent::CursorEntered { device_id } => { .. }
WindowEvent::CursorLeft { device_id } => { .. }
```

Position is in physical pixels. Divide by `scale_factor` to get logical coordinates.

### Mouse buttons

```rust
WindowEvent::MouseInput { device_id, state, button } => {
    // state: ElementState::Pressed | Released
    // button: MouseButton::Left, Right, Middle, Back, Forward, Other(u16)
}
```

### Scroll / Mouse wheel

```rust
WindowEvent::MouseWheel { device_id, delta, phase } => {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => {
            // discrete line-based scrolling (standard mouse wheel)
            // positive y = scroll down (content moves up)
        }
        MouseScrollDelta::PixelDelta(pos) => {
            // smooth pixel-based scrolling (touchpad)
            // pos: PhysicalPosition<f64>
        }
    }
    // phase: TouchPhase - Started, Moved, Ended, Cancelled (for touchpad)
}
```

### Trackpad gestures (macOS/iOS)

```rust
WindowEvent::PinchGesture { delta, phase, .. }     // zoom, delta is magnification factor
WindowEvent::RotationGesture { delta, phase, .. }   // rotation in degrees
WindowEvent::DoubleTapGesture { .. }                 // smart zoom (macOS)
WindowEvent::PanGesture { delta, phase, .. }         // iOS only
WindowEvent::TouchpadPressure { pressure, stage, .. } // Force Touch (macOS)
```

### DeviceEvent (raw, unfocused)

```rust
DeviceEvent::MouseMotion { delta: (f64, f64) }  // raw unfiltered motion
DeviceEvent::MouseWheel { delta }
DeviceEvent::Button { button, state }
DeviceEvent::Key(RawKeyEvent)
```

Delivered regardless of window focus. By default, winit ignores these for unfocused windows on
Linux. Change with `active_event_loop.listen_device_events(DeviceEvents::Always)`.

---

## 5. HiDPI / Scaling

### Coordinate systems

| Type                                          | Description                                      |
| --------------------------------------------- | ------------------------------------------------ |
| `PhysicalSize<u32>` / `PhysicalPosition<i32>` | Actual device pixels                             |
| `LogicalSize<f64>` / `LogicalPosition<f64>`   | DPI-independent units                            |
| `Size` / `Position`                           | Enum wrapping either Physical or Logical variant |

Conversion:

```
Physical = Logical * scale_factor
Logical  = Physical / scale_factor
```

All winit output is in physical units. Input methods accept either via the `Size`/`Position` enums.

### Scale factor

```rust
let sf: f64 = window.scale_factor();
// Typical values: 1.0 (96 DPI), 1.25, 1.5, 2.0 (Retina/HiDPI), 3.0 (mobile)
```

### ScaleFactorChanged event

```rust
WindowEvent::ScaleFactorChanged { scale_factor, inner_size_writer } => {
    // scale_factor: the new factor
    // inner_size_writer: InnerSizeWriter to override the new window size
    // By default, the OS suggests a new size; you can override it:
    let _ = inner_size_writer.request_inner_size(
        PhysicalSize::new(new_width, new_height)
    );
}
```

Triggers when:

- Window moves between monitors with different DPI
- User changes system DPI settings
- Display resolution changes

### For a terminal/grid app

1. Store `scale_factor` and update on `ScaleFactorChanged`.
2. Compute cell size in physical pixels: `cell_physical = cell_logical * scale_factor`.
3. Set `surface_resize_increments` to cell size so the window snaps to grid boundaries.
4. On `Resized(physical_size)`, recompute grid dimensions: `cols = width / cell_width`,
   `rows = height / cell_height`.
5. Do NOT cast float positions with `as u32`; use the `.cast()` methods which round properly.

---

## 6. Platform Quirks

### macOS

- **Minimum OS**: macOS 10.11 (same as Rust). Regularly tested on 10.14.
- **Menu bar**: Winit does _not_ create a menu bar or set an `NSApplicationDelegate` by default. If
  you need standard macOS menus (Edit > Copy/Paste, Window > Minimize, etc.), you must create them
  yourself using `objc2-app-kit`. Winit guarantees it won't register its own app delegate, so you
  can set a custom one.
- **Window initialization**: Many operations (creating windows, fetching monitors) require the app
  to be "ready". Always create windows inside `resumed()`, not in `main()` before `run_app()`.
- **Option as Alt**: Via `WindowExtMacOS`, configure whether the Option key produces characters or
  acts as Alt for keybindings. Options: `None`, `OnlyLeft`, `OnlyRight`, `Both`.

  ```rust
  use winit::platform::macos::{WindowExtMacOS, OptionAsAlt};
  window.set_option_as_alt(OptionAsAlt::Both);
  ```

- **Tabbing**: `WindowAttributesExtMacOS::with_tabbing_identifier()` groups windows into tabs.
  `ActiveEventLoopExtMacOS::set_allows_automatic_window_tabbing(bool)` controls system tab behavior.
- **Activation policy**: `EventLoopBuilderExtMacOS::with_activation_policy()` -- `Regular` (dock +
  menu bar), `Accessory` (no dock icon), `Prohibited` (background only).
- **Fullscreen**: `Fullscreen::Exclusive` works but has caveats; the system may not allow switching
  apps without first leaving fullscreen.
- **Force Touch / Pressure**: `TouchpadPressure` events report pressure (0.0-1.0) and click stage.

### Wayland

- **Windows don't appear until you draw**: The Wayland compositor won't display anything until the
  client has attached a buffer. Start with `visible: false` or render immediately.
- **Client-side decorations (CSD)**: Winit provides its own title bar and window borders via the
  `sctk` (smithay-client-toolkit). Controlled by feature flags:
  - `wayland-csd-adwaita` (default) -- GNOME-style decorations
  - `wayland-csd-adwaita-crossfont` -- same, using crossfont for rendering
  - `wayland-csd-adwaita-notitle` -- no title bar text
- **No primary monitor**: `primary_monitor()` always returns `None` on Wayland (protocol
  limitation).
- **No window positioning**: `set_outer_position()` is a no-op (Wayland doesn't let clients choose
  their position).
- **Frame callback**: Call `window.pre_present_notify()` before presenting each frame. This
  integrates with Wayland's frame callback protocol to avoid over-rendering.
- **`pump_events` quirk**: With `ControlFlow::Wait` and `timeout = Duration::ZERO`, it can still
  block if there are no events.
- **dlopen**: By default, Wayland libraries are loaded via `dlopen`. Disable with the
  `wayland-dlopen` feature flag.

### Windows

- **Minimum OS**: Windows 7.
- **DPI awareness**: Winit automatically sets per-monitor DPI awareness. The
  `WindowAttributesExtWindows` trait provides additional control. The window correctly handles
  `WM_DPICHANGED`.
- **Shift + NumLock**: Holding Shift overrides NumLock, causing numpad keys to act as arrows. The OS
  sends fake key events for this that are NOT marked as `is_synthetic`.
- **Backdrop/Mica**: `WindowAttributesExtWindows::with_backdrop_type()` supports `Auto`, `None`,
  `MainWindow`, `TransientWindow`, `Mica`, `Acrylic`, `Tabbed` (Windows 11).
- **Corner rounding**: `WindowAttributesExtWindows::with_corner_preference()` -- `Default`,
  `DoNotRound`, `Round`, `RoundSmall` (Windows 11).
- **Taskbar icon**: `WindowExtWindows::set_taskbar_icon()`.
- **Custom menu**: `WindowAttributesExtWindows::with_menu()` accepts an `HMENU`.
- **AnyThread wrapper**: `WindowBorrowExtWindows::any_thread()` returns an `AnyThread<&Window>` that
  implements `HasWindowHandle` without thread restrictions.
- **Dark title bar**: Set `preferred_theme` to `Theme::Dark` or use `WindowExtWindows::set_theme()`.

### X11

- **Visual selection**: `WindowAttributesExtX11::with_x11_visual(visual_id)` for specific X11
  visuals (needed for transparency in some compositors).
- **Screen selection**: `WindowAttributesExtX11::with_x11_screen(screen_id)` to place windows on
  specific screens.
- **Synthetic focus events**: When a window gains/loses focus, synthetic key press/release events
  are generated for all currently held keys. Marked with `is_synthetic: true`.

---

## 7. Rendering Backend Integration (raw-window-handle)

### Trait-based interop

Winit's `Window` implements `HasWindowHandle` and `HasDisplayHandle` from the `raw-window-handle`
crate (feature-gated):

```rust
// Cargo.toml
winit = { version = "0.30", features = ["rwh_06"] }
```

```rust
use raw_window_handle::{HasWindowHandle, HasDisplayHandle};

let window_handle = window.window_handle().unwrap();  // RawWindowHandle
let display_handle = window.display_handle().unwrap(); // RawDisplayHandle
// Also available on ActiveEventLoop:
let display_handle = event_loop.display_handle().unwrap();
```

### Handle types per platform

| Platform | Window Handle                           | Display Handle                          |
| -------- | --------------------------------------- | --------------------------------------- |
| Windows  | `Win32WindowHandle` (HWND)              | `WindowsDisplayHandle`                  |
| macOS    | `AppKitWindowHandle` (NSView)           | `AppKitDisplayHandle`                   |
| X11      | `XlibWindowHandle` (Window + visual_id) | `XlibDisplayHandle` (Display\*, screen) |
| Wayland  | `WaylandWindowHandle` (wl_surface)      | `WaylandDisplayHandle` (wl_display)     |
| Web      | `WebWindowHandle` (canvas id)           | `WebDisplayHandle`                      |

### Usage with renderers

Pass handles to `wgpu`, `ash` (Vulkan), `glutin` (OpenGL), `softbuffer`, etc.:

```rust
// wgpu example
let surface = instance.create_surface(&window).unwrap();

// softbuffer (CPU rendering, useful for terminal)
let context = softbuffer::Context::new(display_handle).unwrap();
let mut surface = softbuffer::Surface::new(&context, window_handle).unwrap();
```

### Feature flags for raw-window-handle versions

- `rwh_06` -- raw-window-handle 0.6 (current, recommended)
- `rwh_05` -- raw-window-handle 0.5 (legacy)
- `rwh_04` -- raw-window-handle 0.4 (legacy)

---

## 8. Fullscreen Modes

```rust
pub enum Fullscreen {
    Exclusive(VideoModeHandle),          // changes display resolution
    Borderless(Option<MonitorHandle>),   // covers screen without mode change
}
```

### Borderless fullscreen

```rust
// Fullscreen on current monitor
window.set_fullscreen(Some(Fullscreen::Borderless(None)));

// Fullscreen on specific monitor
let monitor = event_loop.primary_monitor().unwrap();
window.set_fullscreen(Some(Fullscreen::Borderless(Some(monitor))));

// Exit fullscreen
window.set_fullscreen(None);
```

Preferred for terminal emulators. No display mode change, fast toggle, other windows remain
accessible.

### Exclusive fullscreen

```rust
let monitor = event_loop.primary_monitor().unwrap();
let mode = monitor.video_modes().next().unwrap(); // pick desired mode
window.set_fullscreen(Some(Fullscreen::Exclusive(mode)));
```

Changes the actual display resolution. On macOS, switching apps may require exiting fullscreen
first. When the window is closed, the video mode is restored automatically.

### VideoModeHandle

Query available modes:

```rust
for monitor in event_loop.available_monitors() {
    for mode in monitor.video_modes() {
        let size: PhysicalSize<u32> = mode.size();
        let bit_depth: u16 = mode.bit_depth();
        let refresh_rate_mhz: u32 = mode.refresh_rate_millihertz();
    }
}
```

### MonitorHandle

```rust
monitor.name() -> Option<String>
monitor.size() -> PhysicalSize<u32>
monitor.position() -> PhysicalPosition<i32>
monitor.scale_factor() -> f64
monitor.refresh_rate_millihertz() -> Option<u32>
monitor.video_modes() -> impl Iterator<Item = VideoModeHandle>
```

---

## 9. Cursor Management

### Setting the cursor icon

```rust
use winit::window::{Cursor, CursorIcon};

window.set_cursor(CursorIcon::Text.into());    // I-beam for text
window.set_cursor(CursorIcon::Default.into());  // standard arrow
window.set_cursor(CursorIcon::Pointer.into());  // hand/link pointer
```

36 standard icons defined (W3C CSS cursor spec): `Default`, `Pointer`, `Text`, `VerticalText`,
`Crosshair`, `Move`, `Grab`, `Grabbing`, `Wait`, `Progress`, `Help`, `NotAllowed`, `NoDrop`, `Copy`,
`Alias`, `Cell`, `ContextMenu`, `ZoomIn`, `ZoomOut`, `ColResize`, `RowResize`, `NResize`, `SResize`,
`EResize`, `WResize`, `NeResize`, `NwResize`, `SeResize`, `SwResize`, `EwResize`, `NsResize`,
`NeswResize`, `NwseResize`, `AllScroll`, `DndAsk`, `AllResize`.

### Custom cursors

```rust
let source = CustomCursorSource::from_rgba(rgba_data, width, height, hotspot_x, hotspot_y)?;
let cursor = event_loop.create_custom_cursor(source);
window.set_cursor(cursor.into());
```

Max size on web: typically 128x128 (browser-dependent).

### Cursor visibility and grab

```rust
window.set_cursor_visible(false);  // hide cursor (e.g., while typing)

// Grab modes:
window.set_cursor_grab(CursorGrabMode::None);     // free movement
window.set_cursor_grab(CursorGrabMode::Confined);  // keep within window bounds
window.set_cursor_grab(CursorGrabMode::Locked);    // lock in place, only deltas reported
```

`Confined` keeps the cursor within the window. `Locked` hides and locks the cursor, only reporting
raw deltas via `DeviceEvent::MouseMotion`. Not all modes are supported on all platforms.

### Cursor positioning

```rust
window.set_cursor_position(LogicalPosition::new(100.0, 100.0))?;
```

---

## 10. Multi-Window Support

### Window identification

Each window gets a unique `WindowId` (a `Copy + Hash + Eq` wrapper around `usize`). All
`WindowEvent`s include the originating `WindowId`.

### Pattern

```rust
struct App {
    windows: HashMap<WindowId, WindowState>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let w = event_loop.create_window(Window::default_attributes()).unwrap();
        self.windows.insert(w.id(), WindowState::new(w));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = self.windows.get_mut(&id) else { return };
        match event {
            WindowEvent::CloseRequested => {
                self.windows.remove(&id);
                if self.windows.is_empty() {
                    event_loop.exit();
                }
            }
            WindowEvent::RedrawRequested => {
                state.render();
            }
            _ => {}
        }
    }
}
```

### Child windows

```rust
use winit::raw_window_handle::HasRawWindowHandle;

let parent_handle = parent_window.raw_window_handle().unwrap();
let attrs = Window::default_attributes()
    .with_title("child")
    .with_surface_size(LogicalSize::new(200.0, 200.0));
// Safety: parent handle must be valid
let attrs = unsafe { attrs.with_parent_window(Some(parent_handle)) };
let child = event_loop.create_window(attrs).unwrap();
```

Child windows are positioned relative to their parent. Supported on Windows and X11; limited support
on other platforms.

### Creating windows at runtime

New windows can be created at any time inside callbacks that receive `&ActiveEventLoop`:

```rust
WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
    let new_window = event_loop.create_window(Window::default_attributes()).unwrap();
    self.windows.insert(new_window.id(), WindowState::new(new_window));
}
```

---

## 11. WASM / Web Target

### Overview

On the web platform, a winit `Window` is backed by an `HTMLCanvasElement`. Compiled to
`wasm32-unknown-unknown` using `web-sys` bindings.

Supported browsers: Chrome, Firefox, Safari 13.1+.

### Canvas management

```rust
use winit::platform::web::{WindowAttributesExtWebSys, WindowExtWebSys};

// Option 1: Let winit create a canvas, then insert into DOM
let attrs = Window::default_attributes();
let window = event_loop.create_window(attrs).unwrap();
let canvas: web_sys::HtmlCanvasElement = window.canvas().unwrap();
document.body().unwrap().append_child(&canvas).unwrap();

// Option 2: Provide your own canvas
let canvas: web_sys::HtmlCanvasElement = /* get from DOM */;
let attrs = Window::default_attributes().with_canvas(Some(canvas));

// Option 3: Auto-append to document body
let attrs = Window::default_attributes().with_append(true);
```

### CSS considerations

Avoid CSS `transform`, `border`, and `padding` on the canvas element. These cause inaccurate results
for:

- Surface/window size calculations
- Pointer coordinate mapping

### Event loop differences

The browser owns the main thread. `run_app()` never returns; it uses `requestAnimationFrame` and
event listeners internally. `pump_events` is not available.

- `ControlFlow::Poll` -- uses `requestAnimationFrame` or `setTimeout(0)` (configurable via
  `PollStrategy`)
- `ControlFlow::Wait` -- only wakes on events
- `ControlFlow::WaitUntil` -- uses `setTimeout` (configurable via `WaitUntilStrategy`)

### Keyboard events on web

`KeyboardEvent.code` maps to `PhysicalKey`, `KeyboardEvent.key` maps to `LogicalKey`. Dead keys may
be reported as the actual key depending on browser/OS.

### Focus and prevent default

```rust
use winit::platform::web::WindowExtWebSys;
// Prevent browser default behavior (e.g., scrolling on wheel, Tab switching focus)
window.set_prevent_default(true);
```

Canvas focus: `with_focusable(true)` (default) sets `tabindex` on the canvas so it can receive
keyboard events.

### DPI / scaling on web

Scale factor comes from `window.devicePixelRatio`. The canvas size is controlled via
`ResizeObserver` using `device-pixel-content-box` where available, with fallback to `contentRect`.

### Fullscreen on web

Uses the Fullscreen API. `Fullscreen::Borderless(None)` requests fullscreen. Monitor permission may
be needed for multi-monitor setups:

```rust
use winit::platform::web::ActiveEventLoopExtWebSys;
let future = event_loop.request_detailed_monitor_permission();
```

### Main thread safety

`MainThreadMarker` ensures certain operations only run on the main thread. The `Dispatcher` handles
cross-thread closure execution.

---

## Quick Reference: Event Flow for a Terminal Emulator

```
EventLoop::new()
    └─> run_app(&mut app)
        ├─> new_events(Init)
        ├─> resumed()
        │   └─> create_window()
        │       └─> set_ime_allowed(true)
        │
        │   ┌─── event loop iteration ───┐
        │   │                              │
        ├─> window_event(KeyboardInput)    │
        │   └─> process key, update grid   │
        │       └─> request_redraw()       │
        │                                  │
        ├─> window_event(Ime::Commit)      │
        │   └─> insert text into grid      │
        │                                  │
        ├─> window_event(CursorMoved)      │
        │   └─> track mouse position       │
        │                                  │
        ├─> window_event(MouseInput)       │
        │   └─> handle selection/click     │
        │                                  │
        ├─> window_event(MouseWheel)       │
        │   └─> scroll terminal buffer     │
        │                                  │
        ├─> window_event(Resized)          │
        │   └─> recompute grid dimensions  │
        │       └─> resize PTY             │
        │                                  │
        ├─> window_event(ScaleFactorChanged)
        │   └─> recompute cell metrics     │
        │       └─> request_redraw()       │
        │                                  │
        ├─> window_event(RedrawRequested)  │
        │   └─> render grid to surface     │
        │       └─> pre_present_notify()   │
        │       └─> present frame          │
        │                                  │
        ├─> window_event(Focused)          │
        │   └─> start/stop cursor blink    │
        │                                  │
        ├─> window_event(CloseRequested)   │
        │   └─> cleanup, event_loop.exit() │
        │                                  │
        ├─> about_to_wait()                │
        │   └─> set_control_flow()         │
        │                                  │
        └───────────────────────────────────┘
```

---

## Sources

- [winit crate docs (latest)](https://docs.rs/winit/latest/winit/)
- [ActiveEventLoop docs](https://docs.rs/winit/latest/winit/event_loop/struct.ActiveEventLoop.html)
- [ApplicationHandler trait](https://docs.rs/winit/latest/winit/application/trait.ApplicationHandler.html)
- [WindowEvent enum](https://docs.rs/winit/latest/winit/event/enum.WindowEvent.html)
- [KeyEvent struct](https://docs.rs/winit/latest/winit/event/struct.KeyEvent.html)
- [Key enum](https://docs.rs/winit/latest/winit/keyboard/enum.Key.html)
- [PhysicalKey enum](https://docs.rs/winit/latest/winit/keyboard/enum.PhysicalKey.html)
- [DPI module](https://docs.rs/winit/latest/winit/dpi/index.html)
- [Fullscreen enum](https://docs.rs/winit/latest/winit/window/enum.Fullscreen.html)
- [CursorIcon enum](https://docs.rs/winit/latest/winit/window/enum.CursorIcon.html)
- [platform::macos](https://docs.rs/winit/latest/winit/platform/macos/index.html)
- [platform::wayland](https://docs.rs/winit/latest/winit/platform/wayland/index.html)
- [platform::windows](https://docs.rs/winit/latest/winit/platform/windows/index.html)
- [platform::web](https://docs.rs/winit/latest/winit/platform/web/index.html)
- [DeepWiki: Window Trait and Lifecycle](https://deepwiki.com/rust-windowing/winit/3.2-window-trait-and-lifecycle)
- [DeepWiki: Web (WASM) Implementation](<https://deepwiki.com/rust-windowing/winit/5.4-web-(webassembly)-implementation>)
- [winit changelog v0.30](https://docs.rs/winit/0.30.0/winit/changelog/v0_30/index.html)
- [winit GitHub: rust-windowing/winit](https://github.com/rust-windowing/winit)
