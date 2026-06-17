# Research: Linux Framebuffer / DRM/KMS Backend for Rust Terminal Rendering

## Summary

A Linux framebuffer or DRM/KMS backend enables direct pixel output to the display without X11 or
Wayland, which is relevant for embedded systems, kiosk mode, Raspberry Pi, and Linux console
rendering. The two main approaches are the legacy `/dev/fb0` framebuffer interface (simple
mmap-based pixel writes) and the modern DRM/KMS subsystem (mode setting + dumb buffers via
`/dev/dri/card0`). Rust has good crate coverage for both: the `framebuffer` crate wraps `/dev/fb0`,
and `drm-rs` (Smithay) wraps the full DRM/KMS API. Input handling without a display server uses the
`evdev` crate (pure Rust reimplementation of libevdev), and font rendering requires CPU-side
rasterization via crates like `fontdue` or `ab_glyph`.

## Findings

### 1. Linux Framebuffer Device (`/dev/fb0`)

1. **Framebuffer basics** -- The Linux framebuffer is a character device (`/dev/fb0`, major 29) that
   abstracts graphics hardware. It exposes the display as a memory-mapped region. You can `read()`,
   `write()`, `seek()`, and (most importantly) `mmap()` it. The kernel documentation describes it as
   "the frame buffer of some video hardware" with a well-defined interface so software doesn't need
   to know hardware register details.
   [Kernel Docs](https://www.kernel.org/doc/html/latest/fb/framebuffer.html)

2. **Key ioctls** -- Two critical ioctl structs control the framebuffer:
   - `FBIOGET_VSCREENINFO` / `FBIOPUT_VSCREENINFO` (`fb_var_screeninfo`): resolution (`xres`,
     `yres`), virtual resolution (`xres_virtual`, `yres_virtual`), bit depth (`bits_per_pixel`),
     color channel layout (`red`, `green`, `blue`, `transp` bitfields with offset/length), and
     timing parameters.
   - `FBIOGET_FSCREENINFO` (`fb_fix_screeninfo`): fixed info including `smem_start` (physical
     address), `smem_len` (total framebuffer size), `line_length` (bytes per scanline, may include
     padding), and `fb_type` (packed pixels, planes, etc.).
   - `FBIOPAN_DISPLAY`: pans the display to a different offset within virtual framebuffer memory,
     enabling double buffering.
     [rust-framebuffer source](https://github.com/Roysten/rust-framebuffer)

3. **Pixel formats** -- The `VarScreeninfo` structure defines pixel format via `Bitfield` structs
   for R, G, B, and transparency channels. Each bitfield has `offset`, `length`, and `msb_right`.
   Common formats:
   - 32bpp XRGB8888: blue at offset 0 (8 bits), green at 8, red at 16, unused at 24
   - 16bpp RGB565: blue at 0 (5 bits), green at 5 (6 bits), red at 11 (5 bits)
   - The format is not fixed; code must read the bitfield descriptors and pack pixels accordingly.

4. **Double buffering** -- Achieved via virtual resolution. Set `yres_virtual = 2 * yres`, then
   alternate between `yoffset = 0` and `yoffset = yres` using `FBIOPAN_DISPLAY`. Draw to the back
   buffer while the front is displayed, then pan to swap. This avoids tearing without vsync support.

5. **mmap workflow** (Rust):

   ```rust
   // Pseudocode based on rust-framebuffer crate
   let device = File::open("/dev/fb0")?;
   let var_info = ioctl(FBIOGET_VSCREENINFO);  // get resolution, bpp
   let fix_info = ioctl(FBIOGET_FSCREENINFO);  // get line_length, total size
   let len = fix_info.line_length * var_info.yres_virtual;
   let mmap = unsafe { MmapMut::map_mut(&device) };  // memory-mapped pixel buffer
   // Write pixels: offset = y * line_length + x * (bpp / 8)
   ```

6. **KD_GRAPHICS mode** -- When rendering directly to the framebuffer, you should set the console to
   `KD_GRAPHICS` mode via `ioctl(ttyfd, KDSETMODE, KD_GRAPHICS)` to suppress the kernel's text
   cursor and console output. Must restore `KD_TEXT` on exit.
   [rust-framebuffer source](https://github.com/Roysten/rust-framebuffer)

### 2. DRM/KMS for Modern Direct Rendering

7. **DRM/KMS architecture** -- The Direct Rendering Manager is a kernel subsystem for userspace GPU
   access via `/dev/dri/card*`. KMS (Kernel Mode Setting) is the modesetting component. The key
   abstractions are:
   - **Connector**: physical output (HDMI, DP, VGA)
   - **Encoder**: converts pixel data for a connector
   - **CRTC**: scanout engine that reads from a plane and sends to a connector
   - **Plane**: memory object containing a buffer (Primary, Cursor, Overlay types)
   - **Framebuffer**: a DRM object wrapping a buffer for display
     [drm-rs docs](https://docs.rs/drm/latest/drm/control/index.html)

8. **drm-rs crate (Smithay)** -- Safe Rust bindings for the DRM subsystem. Requires implementing
   `AsFd` and `drm::Device` traits on a file wrapper. Provides both legacy and atomic modesetting
   APIs. Part of the Smithay Wayland compositor ecosystem.
   [GitHub](https://github.com/Smithay/drm-rs), [crates.io](https://crates.io/crates/drm)

9. **Dumb buffers** -- The simplest buffer type, always available on any DRM driver. Created via
   `card.create_dumb_buffer((width, height), DrmFourcc::Xrgb8888, 32)`. Can be memory-mapped for CPU
   access via `card.map_dumb_buffer(&mut db)`. Returns a `&mut [u8]` slice for direct pixel writes.
   Slow compared to GBM/GPU buffers, but requires no GPU-specific setup. Fine for 2D rendering at
   moderate resolutions.
   [drm-rs DumbBuffer](https://docs.rs/drm/latest/drm/control/dumbbuffer/struct.DumbBuffer.html)

10. **Legacy modesetting workflow** (from drm-rs examples):

    ```rust
    let card = Card::open("/dev/dri/card0");
    let res = card.resource_handles()?;
    // Find connected connector
    let con = res.connectors().iter()
        .flat_map(|c| card.get_connector(*c, true))
        .find(|i| i.state() == connector::State::Connected)?;
    let mode = con.modes().first()?;  // best mode
    let (w, h) = mode.size();
    // Create dumb buffer + map + fill
    let mut db = card.create_dumb_buffer((w.into(), h.into()), DrmFourcc::Xrgb8888, 32)?;
    { let mut map = card.map_dumb_buffer(&mut db)?;
      for b in map.as_mut() { *b = 128; } }  // grey fill
    let fb = card.add_framebuffer(&db, 24, 32)?;
    // Activate
    card.set_crtc(crtc.handle(), Some(fb), (0, 0), &[con.handle()], Some(mode))?;
    ```

    [legacy_modeset.rs](https://github.com/Smithay/drm-rs/blob/develop/examples/legacy_modeset.rs)

11. **Atomic modesetting** -- The modern API. Requires enabling `ClientCapability::Atomic` and
    `ClientCapability::UniversalPlanes`. Properties are set on connectors, CRTCs, and planes via an
    `AtomicModeReq`, then committed atomically. More complex to set up but enables tear-free page
    flips, multi-plane compositing, and atomic property changes.
    [atomic_modeset.rs](https://github.com/Smithay/drm-rs/blob/develop/examples/atomic_modeset.rs)

12. **Page flipping (DRM)** -- Use `card.page_flip(crtc, fb, PageFlipFlags::EVENT, None)` to
    schedule a buffer swap at the next vblank. The `PageFlipEvent` is received via
    `card.receive_events()`. This provides tear-free double buffering without busy-waiting.

13. **DRM vs legacy framebuffer** -- DRM/KMS is the modern replacement. The legacy `/dev/fb0`
    interface is considered deprecated in the kernel, though it remains widely available. DRM
    provides proper mode setting, multi-monitor support, page flip events, and hardware plane
    compositing. On modern kernels, `/dev/fb0` may actually be a compatibility shim over DRM
    (`simpledrm` or `efifb`).

### 3. How Notcurses Uses the Linux Framebuffer

14. **Notcurses framebuffer rendering** -- Added in v2.4.0 (Sep 2021). On the Linux console,
    notcurses detects `/dev/fb0` via `is_linux_framebuffer()`, opens it `O_RDWR`, then calls
    `FBIOGET_VSCREENINFO` to get pixel geometry. The framebuffer is `mmap()`ed with
    `PROT_READ|PROT_WRITE` and `MAP_SHARED`.
    [notcurses linux.c](https://github.com/dankamongmen/notcurses/blob/master/src/lib/linux.c)

15. **Pixel blitting in notcurses** -- The `fbcon_draw()` function writes sprite pixel data directly
    into the mmap'd framebuffer. It iterates row by row, computing offsets as
    `(row * pixx + col) * 4` (assuming 4 bytes per pixel), and copies RGBA data with transparency
    checks (`rgba_trans_p()`). Transparent pixels are skipped, allowing compositing. Pixel format
    conversion is done inline: notcurses swaps R and B channels (`src[2]->dst[0]`, `src[0]->dst[2]`)
    to convert between RGBA and the framebuffer's BGRA/XRGB format.

16. **Console font reprogramming** -- Notcurses goes further than basic pixel blitting. It reads the
    kernel's console font via `KDFONTOP` ioctl, inspects the glyph table, and injects missing
    Unicode block-drawing characters (quadrant blocks, eighth blocks, line-drawing characters). It
    replaces unused font slots with custom bitmap glyphs, enabling higher-fidelity text rendering on
    the 256/512-glyph limited console font.
    [notcurses linux.c](https://github.com/dankamongmen/notcurses/blob/master/src/lib/linux.c)

17. **Scrolling** -- `fbcon_scroll()` implements pixel-level scrolling by `memmove()`ing rows upward
    in the framebuffer and `memset()`ing cleared rows to zero. This is a CPU-bound operation over
    the entire visible pixel area.

18. **Console limitations noted by notcurses** -- The Linux console has "particularly limited fonts,
    and most characters beyond ASCII are not reliable." Only 256 glyphs (512 with reduced color),
    max 16 colors natively. The framebuffer pixel blitter bypasses these limitations for
    image/sprite rendering but text still goes through the console font system.

### 4. Input Handling Without a Display Server

19. **evdev crate** -- Pure Rust reimplementation of libevdev. Opens `/dev/input/event*` devices
    directly. Provides typed event structs (`KeyCode`, `AbsoluteAxisCode`, `RelativeAxisCode`),
    synchronization handling (`SYN_DROPPED` recovery), and async support via tokio's `EventStream`.
    Supports keyboard, mouse, touchscreen, and gamepad input. Requires read permissions on
    `/dev/input/` devices (typically `input` group membership or root).
    [crates.io/crates/evdev](https://crates.io/crates/evdev), [docs.rs/evdev](https://docs.rs/evdev)

20. **evdev usage pattern**:

    ```rust
    use evdev::{Device, KeyCode, EventSummary};
    let mut device = Device::open("/dev/input/event0")?;
    loop {
        for event in device.fetch_events()? {
            match event.destructure() {
                EventSummary::Key(_, KeyCode::KEY_A, 1) => { /* A pressed */ },
                EventSummary::Key(_, key, 0) => { /* key released */ },
                EventSummary::AbsoluteAxis(_, axis, val) => { /* touch/joystick */ },
                _ => {}
            }
        }
    }
    ```

21. **input-linux crate** -- Alternative evdev/uinput wrapper. Last updated ~2024, 3.2K SLoC. More
    low-level than `evdev`. [crates.io/crates/input-linux](https://crates.io/crates/input-linux)

22. **Device enumeration** -- The `evdev` crate provides `evdev::enumerate()` to crawl `/dev/input/`
    and discover all input devices. Each device can be queried for supported event types, key codes,
    and properties. For a framebuffer application, you'd enumerate devices at startup to find
    keyboards and mice.

23. **Multiplexing input** -- Without a display server, the application must poll/select across
    multiple input device fds. The `evdev` crate exposes `AsRawFd` for integration with `epoll`,
    `poll`, or async runtimes. This replaces the event dispatching that X11/Wayland would normally
    handle.

### 5. Font Rendering (CPU Rasterization)

24. **fontdue** -- A `no_std` Rust font parser and rasterizer. Parses TrueType/OpenType fonts and
    rasterizes glyphs to coverage bitmaps on the CPU. Includes a layout engine. Very fast for a CPU
    rasterizer, suitable for embedded use. Returns `(Metrics, Vec<u8>)` per glyph where the
    `Vec<u8>` is an alpha coverage map. [docs.rs/fontdue](https://docs.rs/fontdue)

25. **ab_glyph** -- Another pure-Rust font rasterizer (successor to `rusttype`). Supports outline
    fonts, provides glyph positioning and rasterization. More widely used than fontdue in the
    ecosystem.

26. **Rendering pipeline for a framebuffer terminal**:
    1. Parse font file (TTF/OTF) with `fontdue` or `ab_glyph`
    2. For each character cell in the grid, rasterize the glyph at the target pixel size
    3. Cache rasterized glyphs in a `HashMap<(char, style), GlyphBitmap>`
    4. For each dirty cell, composite the glyph bitmap into the framebuffer:
       - Fill the cell rectangle with the background color
       - Blend the foreground color using the glyph's alpha coverage
    5. Write the result to the mmap'd buffer (or dumb buffer) No GPU involvement; all blending is
       done in CPU with direct memory writes.

27. **Performance considerations for font rendering** -- Glyph caching is essential. A typical
    terminal font at 16px has ~95 printable ASCII glyphs; rasterizing all upfront takes microseconds
    with fontdue. The bottleneck is compositing into the framebuffer, which at 1920x1080x4bpp is
    ~8MB per frame. Dirty-rectangle tracking (only updating changed cells) is critical for
    performance.

### 6. Resolution and Mode Setting

28. **Framebuffer mode setting** -- Via `FBIOPUT_VSCREENINFO` ioctl. Set `xres`, `yres`,
    `bits_per_pixel`, and timing parameters. The kernel driver may round values to match hardware
    capabilities. The `fbset` utility does this from userspace. Not all framebuffer drivers support
    mode changes; many (like `efifb`, `simpledrm`) are fixed at the boot-time resolution set by the
    bootloader/EFI.

29. **DRM/KMS mode setting** -- Query available modes from the connector: `connector.modes()`
    returns a list of `Mode` structs with resolution, refresh rate, and flags. Each mode includes
    the full timing specification. Select a mode and apply it via `set_crtc()` (legacy) or atomic
    commit (modern). DRM supports runtime mode switching, hot-plug detection, and multi-monitor
    configurations.

30. **Mode information** -- A DRM `Mode` contains: horizontal/vertical resolution, refresh rate,
    clock frequency, hsync/vsync timings, and flags (interlace, double scan, preferred, etc.). The
    first mode in the list is typically the preferred/native resolution.

### 7. Rust Crate Summary

| Crate          | Purpose                | Downloads   | Notes                                                                                    |
| -------------- | ---------------------- | ----------- | ---------------------------------------------------------------------------------------- |
| `drm` (drm-rs) | DRM/KMS safe bindings  | High        | Smithay project. Full modesetting + dumb buffers. Active.                                |
| `framebuffer`  | `/dev/fb0` wrapper     | ~150K total | Simple mmap abstraction. Thin. Less active.                                              |
| `evdev`        | Input device handling  | High        | Pure Rust libevdev. Keyboard/mouse/touch. Active. Async support.                         |
| `input-linux`  | Alternative evdev      | ~112K total | Lower-level. Less active.                                                                |
| `fontdue`      | CPU font rasterization | Moderate    | `no_std`. Fast. Layout engine included.                                                  |
| `ab_glyph`     | CPU font rasterization | High        | rusttype successor. Widely used.                                                         |
| `gbm` (gbm.rs) | GPU buffer management  | Moderate    | Smithay project. For hardware-accelerated buffers (not needed for dumb buffer approach). |

### 8. Performance Characteristics

31. **Direct memory-mapped writes** -- Writing to a mmap'd framebuffer is essentially a memory
    write. For `/dev/fb0` with `MAP_SHARED`, writes are visible immediately (no syscall per pixel).
    The kernel/GPU will scan out from this memory at the display refresh rate. Write bandwidth
    depends on memory bus speed; a full 1080p frame at 32bpp is ~8MB. Modern CPUs can push this at
    multiple GB/s, so a full-screen redraw takes <2ms in raw memcpy terms.

32. **Cache effects** -- The framebuffer memory may be mapped as write-combining (WC) or uncacheable
    depending on the hardware. WC memory has good sequential write performance but poor
    random-access performance. Write in scanline order when possible. Avoid reading from the
    framebuffer (reads from WC/UC memory are extremely slow, 10-100x slower than writes).

33. **DRM dumb buffer performance** -- Dumb buffers are kernel-allocated and CPU-accessible.
    Performance is similar to framebuffer mmap for the CPU side. The `map_dumb_buffer()` call
    returns a regular mmap'd region. The actual display update happens when the buffer is committed
    as a framebuffer to a CRTC (via `set_crtc` or `page_flip`).

34. **Vsync and tearing** -- Without page flipping, writes directly to the displayed buffer will
    cause tearing when the scanout overlaps a write. DRM page flipping solves this. The legacy
    framebuffer's `FBIOPAN_DISPLAY` can also provide vsync'd updates if the driver supports it, but
    many don't.

### 9. Use Cases

35. **Embedded systems** -- Framebuffer/DRM backends are standard for embedded Linux (Yocto,
    Buildroot). No display server overhead. Direct pixel control. Common on ARM SoCs with simple
    display controllers. The `drm-rs` dumb buffer path works well here since these devices rarely
    need GPU acceleration for 2D.

36. **Kiosk mode** -- Single-application display without window management. DRM/KMS is preferred:
    open the DRM device, set mode, render to dumb buffer, page flip. No window decorations, no
    compositor overhead. Used in digital signage, ATMs, point-of-sale terminals.

37. **Linux console (no display server)** -- Running on a VT/TTY without X11/Wayland. Notcurses
    demonstrates this works: detect the console, open the framebuffer, render pixels directly.
    Useful for server administration UIs, boot-time applications, and recovery consoles.

38. **Raspberry Pi** -- The Pi's VideoCore GPU exposes both `/dev/fb0` and DRM (`vc4-drm`). The DRM
    path is preferred on modern Raspberry Pi OS (Bookworm+). The `drm-rs` crate works with the Pi's
    DRM device. Dumb buffers are sufficient for terminal rendering; the Pi 4/5's memory bandwidth
    handles 1080p easily.

### 10. Trade-offs vs X11/Wayland

| Aspect             | Framebuffer/DRM Direct                                                        | X11/Wayland                                                                 |
| ------------------ | ----------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| **Complexity**     | Lower for single-app; must handle input, font rendering, compositing manually | Higher to set up, but handles window management, input routing, compositing |
| **Dependencies**   | Minimal (kernel only)                                                         | Requires display server + libraries                                         |
| **Multi-app**      | Single app owns the display exclusively                                       | Multiple windows, apps, overlays                                            |
| **Input**          | Must manage evdev devices directly                                            | Display server handles input routing, focus                                 |
| **Performance**    | Zero compositor overhead; direct scanout                                      | Extra copy through compositor (though DRM leasing can bypass)               |
| **Portability**    | Linux only                                                                    | Cross-platform (X11/Wayland/macOS/Windows through abstraction)              |
| **Font rendering** | Must do CPU rasterization                                                     | Can use system font stack, GPU-accelerated text                             |
| **Multi-monitor**  | Must enumerate connectors and manage manually                                 | Display server handles layout, scaling                                      |
| **Accessibility**  | None built in                                                                 | Screen readers, magnifiers integrate with display server                    |
| **Security**       | Requires video/render group or root                                           | Display server provides privilege separation                                |

### 11. Recommended Architecture for a Rust Framebuffer Backend

A practical backend for a terminal/grid rendering library would:

1. **Prefer DRM/KMS over legacy framebuffer** -- Use `drm-rs` with dumb buffers. Falls back to
   `/dev/fb0` via the `framebuffer` crate if DRM is unavailable.

2. **Double-buffer in userspace** -- Maintain a `Vec<u32>` (or `Vec<u8>`) as a shadow buffer. Render
   all changes there (with dirty tracking), then blit to the dumb buffer/framebuffer in one pass.

3. **Use fontdue for glyph rasterization** -- `no_std` compatible, fast, minimal. Cache all glyphs
   at initialization.

4. **Use evdev for input** -- Enumerate `/dev/input/event*`, select keyboard devices, run an event
   loop on a dedicated thread or async task.

5. **Set KD_GRAPHICS on the console TTY** -- Suppress kernel text output. Restore on exit (critical:
   install signal handlers for SIGTERM, SIGINT, etc.).

6. **Page flip for vsync** -- If using DRM, use atomic commits with page flip events. If using
   legacy fb, use `FBIOPAN_DISPLAY` where supported.

## Sources

### Kept

- **Linux Kernel Framebuffer Documentation**
  (<https://www.kernel.org/doc/html/latest/fb/framebuffer.html>) -- Official kernel docs on /dev/fb\*
  interface, ioctls, pixel format, timing
- **drm-rs crate** (<https://github.com/Smithay/drm-rs>) -- Primary Rust DRM binding. Examined README,
  docs, and both example files
- **drm-rs legacy_modeset.rs example**
  (<https://github.com/Smithay/drm-rs/blob/develop/examples/legacy_modeset.rs>) -- Complete working
  example of DRM dumb buffer rendering
- **drm-rs atomic_modeset.rs example**
  (<https://github.com/Smithay/drm-rs/blob/develop/examples/atomic_modeset.rs>) -- Atomic API example
  with planes and properties
- **drm::control docs** (<https://docs.rs/drm/latest/drm/control/index.html>) -- KMS resource type
  documentation (Connector, CRTC, Plane, etc.)
- **rust-framebuffer source (lib.rs)** (<https://github.com/Roysten/rust-framebuffer>) -- Complete
  source of the `framebuffer` crate showing ioctl usage, mmap, VarScreeninfo/FixScreeninfo structs
- **notcurses linux.c** (<https://github.com/dankamongmen/notcurses/blob/master/src/lib/linux.c>) --
  Full source of notcurses' framebuffer rendering, font reprogramming, scrolling
- **notcurses wiki/dankwiki** (<https://nick-black.com/dankwiki/index.php/Notcurses>) -- Detailed
  documentation on blitters, pixel rendering, console support, and release notes
- **evdev crate docs** (<https://docs.rs/evdev/latest/evdev/>) -- Full API documentation for Rust
  evdev input handling
- **fontdue crate docs** (<https://docs.rs/fontdue/latest/fontdue/>) -- CPU font rasterizer API

### Dropped

- **Wikipedia DRM article** -- Fetched (127K chars) but too large/generic; the drm-rs docs and
  kernel docs are more directly useful
- **Linux Kernel KMS documentation** (<https://www.kernel.org/doc/html/latest/gpu/drm-kms.html>) --
  Fetched (648K chars) but too large for this research scope; the drm-rs examples cover practical
  usage adequately
- **input-linux crate** -- Less actively maintained than `evdev`, less documentation; not
  recommended over `evdev`
- **rusttype crate** -- Superseded by `ab_glyph`; not worth covering separately

## Gaps

1. **Actual benchmarks** -- No real-world benchmark data was found for framebuffer terminal
   rendering in Rust specifically. The performance claims (sub-2ms full frame) are theoretical based
   on memory bandwidth; real numbers with font rendering + dirty tracking would be valuable.

2. **Multi-monitor with DRM** -- The research covers single-output scenarios. Handling multiple
   CRTCs/connectors for multi-monitor setups in a terminal context needs further investigation.

3. **GPU-accelerated 2D via DRM** -- Using GBM buffers + OpenGL ES for hardware-accelerated glyph
   rendering is possible but adds significant complexity. Worth exploring if CPU rendering proves
   too slow at 4K resolutions.

4. **Wayland-less compositing** -- If multiple overlapping widgets need compositing (popups, menus),
   doing this in software over a single framebuffer requires implementing z-ordering and clipping
   manually. No ready-made Rust crate for this.

5. **Color management** -- Framebuffer color profiles, gamma correction, and HDR support on Linux
   are handled differently than under a display server. Not researched here.

6. **Touch input** -- The evdev crate supports absolute axis events for touchscreens, but the actual
   protocol for multi-touch gesture recognition on a framebuffer app needs more investigation.
