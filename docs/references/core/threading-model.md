# Threading Model Design for a Rust Terminal/Grid Rendering Library

## Summary

The recommended approach for `rg` is a **single-threaded event loop on the main thread** (required by winit on macOS) with game logic running on a separate thread, connected via channels. The terminal grid buffer is shared using `Arc<FairMutex<Buffer>>` (following Alacritty's proven pattern), with the option to evolve toward double/triple buffering if profiling reveals contention. For WASM, all logic collapses to a single thread with `requestAnimationFrame`-driven rendering.

## 1. Single-Threaded Model (Simplest)

winit requires the event loop to run on the main thread on macOS (Cocoa/AppKit constraint). The simplest architecture runs everything in the winit event loop callback:

```rust
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};

struct App {
    buffer: Buffer,   // owned directly, no Arc/Mutex needed
    renderer: Renderer,
}

impl ApplicationHandler for App {
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::RedrawRequested => {
                // Game logic tick
                self.buffer.update();
                // Render from the same buffer
                self.renderer.draw(&self.buffer);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Continuous rendering: request another frame
        self.window.request_redraw();
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll); // game loop style
    let mut app = App::new();
    event_loop.run_app(&mut app).unwrap();
}
```

**Tradeoffs:**
- Pro: No synchronization, no Send/Sync requirements, simplest mental model.
- Pro: Works identically on native and WASM.
- Con: Game logic blocks rendering (and vice versa). If game tick is expensive, frame rate drops.
- Con: Cannot saturate multiple CPU cores.

**When to use:** Prototyping, simple games, WASM targets, or when game logic is trivially fast (< 1ms per tick).

## 2. Render Thread + Game Logic Thread with Shared Grid Buffer

The standard multi-threaded pattern: winit event loop stays on the main thread (handles input, drives rendering), game logic runs on a spawned thread, and they share the grid buffer.

```rust
use std::sync::Arc;
use parking_lot::Mutex;

struct SharedState {
    buffer: Buffer,
    dirty: bool,
}

fn main() {
    let shared = Arc::new(Mutex::new(SharedState {
        buffer: Buffer::new(80, 24),
        dirty: false,
    }));

    // Game logic thread
    let game_shared = Arc::clone(&shared);
    std::thread::spawn(move || {
        loop {
            {
                let mut state = game_shared.lock();
                state.buffer.tick();
                state.dirty = true;
            }
            // Don't hold the lock while sleeping
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    });

    // Main thread: winit event loop + rendering
    let event_loop = EventLoop::new().unwrap();
    let mut app = RenderApp { shared };
    event_loop.run_app(&mut app).unwrap();
}

impl ApplicationHandler for RenderApp {
    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::RedrawRequested = event {
            let state = self.shared.lock();
            if state.dirty {
                self.renderer.draw(&state.buffer);
                // Note: can't set dirty=false with shared ref
            }
        }
    }
}
```

**Tradeoffs:**
- Pro: Game logic doesn't block rendering (if lock hold times are short).
- Con: Lock contention if game thread holds the lock for a full tick.
- Con: Renderer blocks waiting for game thread to release lock (and vice versa).

## 3. Double-Buffering the Cell Grid

Double-buffering eliminates contention: the game thread writes to a "back" buffer while the render thread reads from a "front" buffer. An atomic swap exchanges them.

### Manual double buffer

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;

struct DoubleBuffer {
    buffers: [Mutex<Buffer>; 2],
    /// Index of the buffer currently used for reading (front buffer).
    front: AtomicUsize,
}

impl DoubleBuffer {
    fn new(cols: usize, rows: usize) -> Self {
        DoubleBuffer {
            buffers: [
                Mutex::new(Buffer::new(cols, rows)),
                Mutex::new(Buffer::new(cols, rows)),
            ],
            front: AtomicUsize::new(0),
        }
    }

    /// Game thread: get exclusive access to the back buffer.
    fn back_buffer(&self) -> MutexGuard<'_, Buffer> {
        let front = self.front.load(Ordering::Acquire);
        self.buffers[1 - front].lock()
    }

    /// Swap front and back. Call after game thread finishes writing.
    fn swap(&self) {
        let old = self.front.load(Ordering::Acquire);
        self.front.store(1 - old, Ordering::Release);
    }

    /// Render thread: get shared access to the front buffer.
    fn front_buffer(&self) -> MutexGuard<'_, Buffer> {
        let front = self.front.load(Ordering::Acquire);
        self.buffers[front].lock()
    }
}
```

**Caution:** This naive double-buffer still needs synchronization to prevent the swap from happening while the renderer is reading. A better approach is triple buffering.

### Triple buffering (lock-free, single producer / single consumer)

The `triple_buffer` crate provides exactly this pattern:

```rust
use triple_buffer::triple_buffer;

fn main() {
    let (mut input, mut output) = triple_buffer(&Buffer::new(80, 24));

    // Game thread (producer)
    std::thread::spawn(move || {
        loop {
            // Get mutable access to the input buffer, modify in place
            {
                let buf = input.input_buffer_mut();
                buf.clear();
                buf.tick();
            }
            input.publish(); // atomic swap, never blocks
            std::thread::sleep(Duration::from_millis(16));
        }
    });

    // Render thread (consumer) - on main thread via winit
    // In RedrawRequested:
    output.update(); // atomic load of latest published buffer, never blocks
    let buf = output.output_buffer();
    renderer.draw(buf);
}
```

**Tradeoffs:**
- Pro: Zero contention. Producer and consumer never block each other.
- Pro: Consumer always reads the latest complete frame.
- Con: 3x memory for the buffer. For an 80x24 grid of ~20-byte cells, that's ~115 KB (trivial). For 300x100 it's ~1.8 MB (still fine).
- Con: Single producer, single consumer only. Multiple writers need a different pattern.
- Con: The `triple_buffer` crate requires `T: Send`. Your `Buffer`/`Cell` types must be `Send`.

## 4. Send/Sync Design for Buffer/Cell Types

For any multi-threaded pattern, your core types must satisfy Rust's thread-safety traits.

### Design rules

```rust
/// A single cell in the grid. All fields are plain data, no Rc/Cell/RefCell.
#[derive(Clone, Debug, Default)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags, // bitflags, Copy
}
// Cell is automatically Send + Sync (all fields are Send + Sync).

/// The grid buffer. Vec<Cell> is Send + Sync when Cell is.
#[derive(Clone, Debug)]
pub struct Buffer {
    cells: Vec<Cell>,
    width: usize,
    height: usize,
}
// Buffer is automatically Send + Sync.
```

**What breaks Send/Sync:**
- `Rc<T>` (not Send, not Sync) - use `Arc<T>` instead.
- `Cell<T>` / `RefCell<T>` (not Sync) - use `AtomicXxx` or `Mutex<T>` instead.
- Raw pointers (`*const T`, `*mut T`) - not Send/Sync by default; requires `unsafe impl`.
- Platform handles (GPU textures, window handles) - typically `!Send`.

**Key principle:** Keep `Buffer` and `Cell` as plain data (POD-like). No reference-counted pointers, no interior mutability, no platform handles. Rendering resources (GPU textures, pipelines) stay on the render thread and are never shared.

```rust
// This compiles only if Buffer is Send + Sync
fn assert_send_sync<T: Send + Sync>() {}
fn check() {
    assert_send_sync::<Buffer>();
    assert_send_sync::<Cell>();
}
```

## 5. Channel-Based Communication

Instead of sharing mutable state, use channels: the game thread sends commands/snapshots, the render thread receives and acts.

### Command-based (game sends diffs)

```rust
use std::sync::mpsc;

enum GameCommand {
    SetCell { x: usize, y: usize, cell: Cell },
    Clear,
    Resize { width: usize, height: usize },
    /// Send a complete buffer snapshot for the next frame.
    Frame(Buffer),
    Shutdown,
}

fn main() {
    let (tx, rx) = mpsc::channel::<GameCommand>();

    // Game thread
    std::thread::spawn(move || {
        let mut buffer = Buffer::new(80, 24);
        loop {
            buffer.tick();
            // Send a clone of the full buffer each frame
            tx.send(GameCommand::Frame(buffer.clone())).unwrap();
            std::thread::sleep(Duration::from_millis(16));
        }
    });

    // Render thread (main, in winit event loop)
    // In RedrawRequested handler:
    let mut render_buffer = Buffer::new(80, 24);
    // Drain all pending commands, keep only the latest frame
    while let Ok(cmd) = rx.try_recv() {
        match cmd {
            GameCommand::Frame(buf) => render_buffer = buf,
            GameCommand::SetCell { x, y, cell } => render_buffer.set(x, y, cell),
            GameCommand::Clear => render_buffer.clear(),
            GameCommand::Resize { width, height } => render_buffer.resize(width, height),
            GameCommand::Shutdown => { /* exit */ }
        }
    }
    renderer.draw(&render_buffer);
}
```

### Using crossbeam channels (bounded, for backpressure)

```rust
use crossbeam_channel::{bounded, select, Receiver, Sender};

let (frame_tx, frame_rx): (Sender<Buffer>, Receiver<Buffer>) = bounded(2);

// Game thread
std::thread::spawn(move || {
    let mut buffer = Buffer::new(80, 24);
    loop {
        buffer.tick();
        // bounded(2): if renderer is 2 frames behind, game thread blocks
        // This provides natural backpressure
        frame_tx.send(buffer.clone()).unwrap();
    }
});

// Render thread: drain to latest
fn get_latest_frame(rx: &Receiver<Buffer>) -> Option<Buffer> {
    let mut latest = None;
    while let Ok(buf) = rx.try_recv() {
        latest = Some(buf);
    }
    latest
}
```

**Tradeoffs:**
- Pro: Clean separation. No shared mutable state. Easy to reason about.
- Pro: Backpressure with bounded channels.
- Con: Cloning the buffer each frame. For an 80x24 grid (~3840 cells), this is ~77 KB per clone, trivially fast. For larger grids, use `Arc<Buffer>` to share immutable snapshots.
- Con: Latency: one frame of delay between game state and display.

### Avoiding clones with Arc snapshots

```rust
use std::sync::Arc;

// Game thread produces immutable snapshots
let buffer = Arc::new(buffer.clone()); // freeze current state
frame_tx.send(buffer).unwrap();

// Render thread reads the Arc without cloning the data
let frame: Arc<Buffer> = frame_rx.recv().unwrap();
renderer.draw(&frame);
```

## 6. How Terminal Emulators Handle Threading

### Alacritty's Architecture

Alacritty uses three logical threads, confirmed by source analysis:

1. **Main thread (event loop):** Runs `winit::EventLoop::run_app()`. Handles window events, input, and triggers rendering. Implements `ApplicationHandler`. The `Processor` struct owns all `WindowContext` instances.

2. **PTY reader thread:** Spawned per terminal via `EventLoop::spawn()` (named "PTY reader"). Reads bytes from the PTY using `polling::Poller`, parses VT sequences via `vte::ansi::Processor`, and writes the parsed output into the terminal state. Uses `std::sync::mpsc` channel to receive `Msg::Input`, `Msg::Resize`, `Msg::Shutdown` from the main thread.

3. **Config monitor thread:** Watches config files for changes, sends reload events.

**Shared state:** The terminal (`Term<T>`) is wrapped in `Arc<FairMutex<Term<T>>>`. Both the main thread (for rendering/input) and the PTY reader thread (for writing parsed output) access it through this mutex.

**FairMutex design** (from `alacritty_terminal/src/sync.rs`):

```rust
/// Uses an extra lock to ensure that if one thread is waiting,
/// it will get the lock before a single thread can re-lock it.
pub struct FairMutex<T> {
    data: Mutex<T>,
    next: Mutex<()>,
}

impl<T> FairMutex<T> {
    /// Acquire a lease to reserve the mutex lock.
    /// Prevents others from acquiring a lock, but blocks
    /// if someone else already holds a lease.
    pub fn lease(&self) -> MutexGuard<'_, ()> {
        self.next.lock()
    }

    /// Fair lock: acquire the "next" lock first.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        let _next = self.next.lock();
        self.data.lock()
    }

    /// Unfair lock: skip the fairness queue.
    pub fn lock_unfair(&self) -> MutexGuard<'_, T> {
        self.data.lock()
    }

    /// Try to lock without blocking (unfair).
    pub fn try_lock_unfair(&self) -> Option<MutexGuard<'_, T>> {
        self.data.try_lock()
    }
}
```

**Why FairMutex:** The PTY reader can produce data faster than the renderer consumes it. Without fairness, the PTY reader could starve the renderer by re-acquiring the lock immediately. The lease mechanism ensures the renderer gets its turn.

**PTY reader locking strategy** (from `event_loop.rs`):

```rust
// Reserve the next terminal lock for PTY reading
let _terminal_lease = Some(self.terminal.lease());
let mut terminal = None;

loop {
    // Read bytes from PTY into buf
    match self.pty.reader().read(&mut buf[unprocessed..]) { ... }

    // Try to lock the terminal (non-blocking)
    let terminal = match &mut terminal {
        Some(terminal) => terminal,
        None => terminal.insert(match self.terminal.try_lock_unfair() {
            // Force block if buffer is full
            None if unprocessed >= READ_BUFFER_SIZE => self.terminal.lock_unfair(),
            None => continue, // keep reading, try lock later
            Some(terminal) => terminal,
        }),
    };

    // Parse bytes into terminal state while holding the lock
    state.parser.advance(&mut **terminal, &buf[..unprocessed]);
}
```

This is a key optimization: the PTY reader accumulates bytes without holding the lock, then acquires it to flush parsed data in batch. If the lock is contended, it keeps reading into the buffer until either the lock becomes available or the buffer fills up (1 MB).

**Communication flow:**
```
Input events (keyboard/mouse)
    -> main thread handles them
    -> sends Msg::Input(bytes) via mpsc channel to PTY thread
    -> PTY thread writes bytes to PTY fd

PTY output:
    PTY fd -> PTY reader thread reads bytes
    -> acquires FairMutex<Term>
    -> parses VT sequences, updates terminal grid
    -> sends Event::Wakeup via EventLoopProxy to main thread
    -> main thread requests redraw
    -> acquires FairMutex<Term>, reads grid, renders
```

### Rio's Architecture

Rio follows a very similar pattern to Alacritty:

- **Main thread:** winit event loop, rendering via `Sugarloaf`.
- **PTY reader thread ("Machine"):** Spawned per terminal context. Same `Arc<FairMutex<Crosswords<T>>>` pattern for sharing terminal state (Rio uses the same `FairMutex` implementation).
- **Communication:** `corcovado::channel` (mio-based channel) for sending messages to the PTY thread. `EventListener` trait + event proxy for PTY -> main thread notifications.

Both Alacritty and Rio converge on the same architecture because the constraints are identical: macOS requires the event loop on the main thread, PTY I/O is inherently async/blocking, and the terminal grid must be shared between reader and renderer.

## 7. Arc<Mutex<Buffer>> vs Channel-Based vs Double-Buffer Swap

| Pattern | Contention | Memory | Complexity | Latency | Best For |
|---|---|---|---|---|---|
| `Arc<Mutex<Buffer>>` | Medium: lock held during read/write | 1x buffer | Low | Minimal (same frame) | Simple apps, proven pattern |
| `Arc<FairMutex<Buffer>>` | Low: fairness prevents starvation | 1x buffer + small overhead | Low-Medium | Minimal | Terminal emulators (Alacritty/Rio) |
| Channel (clone per frame) | None (no shared state) | 2x buffer (in flight) | Low | +1 frame | Clean separation, moderate grids |
| Channel (`Arc<Buffer>`) | None | 1x buffer (shared immutable) | Low | +1 frame | Large buffers where clone is expensive |
| Double buffer (manual) | Low (swap is atomic) | 2x buffer | Medium | +1 frame max | Fixed producer/consumer |
| Triple buffer | None (lock-free) | 3x buffer | Medium | Latest available | Real-time, no tolerance for jitter |
| `ArcSwap<Buffer>` | Near-zero (atomic pointer swap) | 2x buffer (old + new) | Low | Latest available | Read-heavy, infrequent updates |

### Recommendation for `rg`

Start with `Arc<Mutex<Buffer>>` (or a FairMutex variant). The grid sizes involved (typically < 200x100 = 20,000 cells) make lock contention negligible at 60fps. If profiling shows contention:

1. First try `ArcSwap<Buffer>`: game thread builds a new `Arc<Buffer>`, stores it atomically. Render thread loads the latest with near-zero overhead.
2. If allocation pressure from creating new `Arc<Buffer>` each frame matters, move to `triple_buffer`.

## 8. Lock-Free Approaches for Cell Grid Updates

### Per-cell atomics (not recommended for grids)

Making each cell atomic is technically possible but impractical:

```rust
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};

struct AtomicCell {
    ch: AtomicU32,   // char as u32
    fg: AtomicU32,   // packed RGBA
    bg: AtomicU32,
    flags: AtomicU8,
}

impl AtomicCell {
    fn store(&self, cell: &Cell) {
        self.ch.store(cell.ch as u32, Ordering::Relaxed);
        self.fg.store(cell.fg.to_u32(), Ordering::Relaxed);
        self.bg.store(cell.bg.to_u32(), Ordering::Relaxed);
        self.flags.store(cell.flags.bits(), Ordering::Release);
    }
}
```

**Problems:**
- No atomicity across multiple fields. Reader can see ch from frame N and fg from frame N+1 (tearing).
- 128-bit atomics (`AtomicU128`) exist on some platforms but aren't portable.
- Complexity explodes. Not worth it for a grid.

### Sequence lock (seqlock)

A seqlock provides lock-free reads with low overhead. The writer increments a sequence counter before and after writing. Readers check the counter; if it changed during their read, they retry.

```rust
use std::sync::atomic::{AtomicUsize, Ordering};

struct SeqLock<T> {
    seq: AtomicUsize,
    data: std::cell::UnsafeCell<T>,
}

unsafe impl<T: Send> Send for SeqLock<T> {}
unsafe impl<T: Send> Sync for SeqLock<T> {}

impl<T: Copy> SeqLock<T> {
    fn write(&self, val: T) {
        // Odd sequence = write in progress
        let s = self.seq.fetch_add(1, Ordering::Release);
        debug_assert!(s % 2 == 0);
        unsafe { *self.data.get() = val; }
        self.seq.fetch_add(1, Ordering::Release);
    }

    fn read(&self) -> T {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 % 2 != 0 { continue; } // write in progress
            let val = unsafe { std::ptr::read_volatile(self.data.get()) };
            let s2 = self.seq.load(Ordering::Acquire);
            if s1 == s2 { return val; }
            // Retry: data was modified during read
            std::hint::spin_loop();
        }
    }
}
```

**For grids:** You could put a seqlock around the entire buffer, but at that point a regular mutex is simpler and the performance difference for a ~100 KB buffer is negligible.

### RCU (Read-Copy-Update) via ArcSwap

The most practical "lock-free" pattern for grid updates:

```rust
use arc_swap::ArcSwap;
use std::sync::Arc;

static GRID: once_cell::sync::Lazy<ArcSwap<Buffer>> =
    once_cell::sync::Lazy::new(|| ArcSwap::from_pointee(Buffer::new(80, 24)));

// Game thread: read-copy-update
fn game_tick() {
    let old = GRID.load();
    let mut new_buf = (*old).clone();
    new_buf.tick();
    GRID.store(Arc::new(new_buf));
}

// Render thread: load is ~1 atomic op, no blocking
fn render() {
    let buf = GRID.load();
    renderer.draw(&buf);
}
```

`ArcSwap::load()` is extremely fast in read-heavy scenarios (optimized to avoid the atomic ref-count increment in most cases via hazard pointers/epoch-based reclamation).

## 9. WASM Single-Threaded Constraints

**Key constraints:**
- `wasm32-unknown-unknown`: No `std::thread::spawn`. No `Mutex` blocking (it would deadlock the single thread). No `mpsc::channel` (single thread, receiver would block forever).
- `wasm32-unknown-unknown` with `atomics` + `SharedArrayBuffer`: Threads via `web_sys::Worker`, but setup is complex and requires specific HTTP headers (`Cross-Origin-Opener-Policy`, `Cross-Origin-Embedder-Policy`).
- wgpu on WASM: Works, but the device is `!Send` on WASM (it wraps a JS object). All GPU operations must happen on the main thread.

**Practical approach: single-threaded on WASM, multi-threaded on native.**

```rust
#[cfg(not(target_arch = "wasm32"))]
mod threading {
    use std::sync::Arc;
    use parking_lot::Mutex;

    pub struct GameRunner {
        shared: Arc<Mutex<Buffer>>,
        handle: std::thread::JoinHandle<()>,
    }

    impl GameRunner {
        pub fn new(buffer: Arc<Mutex<Buffer>>) -> Self {
            let shared = Arc::clone(&buffer);
            let handle = std::thread::spawn(move || {
                loop {
                    shared.lock().tick();
                    std::thread::sleep(Duration::from_millis(16));
                }
            });
            GameRunner { shared: buffer, handle }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod threading {
    pub struct GameRunner {
        buffer: Buffer, // owned directly, no Arc/Mutex
    }

    impl GameRunner {
        pub fn new(buffer: Buffer) -> Self {
            GameRunner { buffer }
        }

        /// Called from requestAnimationFrame callback
        pub fn tick(&mut self) {
            self.buffer.tick();
        }

        pub fn buffer(&self) -> &Buffer {
            &self.buffer
        }
    }
}
```

**Abstraction pattern:**

```rust
/// Trait that abstracts over single-threaded and multi-threaded buffer access.
pub trait BufferAccess {
    fn with_buffer<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Buffer) -> R;

    fn with_buffer_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Buffer) -> R;
}

#[cfg(not(target_arch = "wasm32"))]
impl BufferAccess for Arc<Mutex<Buffer>> {
    fn with_buffer<F, R>(&self, f: F) -> R
    where F: FnOnce(&Buffer) -> R {
        f(&self.lock())
    }

    fn with_buffer_mut<F, R>(&self, f: F) -> R
    where F: FnOnce(&mut Buffer) -> R {
        f(&mut self.lock())
    }
}

#[cfg(target_arch = "wasm32")]
impl BufferAccess for RefCell<Buffer> {
    fn with_buffer<F, R>(&self, f: F) -> R
    where F: FnOnce(&Buffer) -> R {
        f(&self.borrow())
    }

    fn with_buffer_mut<F, R>(&self, f: F) -> R
    where F: FnOnce(&mut Buffer) -> R {
        f(&mut self.borrow_mut())
    }
}
```

## 10. Concrete Rust Code Examples

### Complete: Game thread + render thread with crossbeam channel

```rust
use crossbeam_channel::{bounded, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

#[derive(Clone)]
struct Cell {
    ch: char,
    fg: [u8; 4],
    bg: [u8; 4],
}

#[derive(Clone)]
struct Buffer {
    cells: Vec<Cell>,
    width: usize,
    height: usize,
    frame: u64,
}

impl Buffer {
    fn new(width: usize, height: usize) -> Self {
        Buffer {
            cells: vec![Cell { ch: ' ', fg: [255; 4], bg: [0, 0, 0, 255] }; width * height],
            width,
            height,
            frame: 0,
        }
    }

    fn tick(&mut self) {
        self.frame += 1;
        // Game logic updates cells...
        let idx = (self.frame as usize) % self.cells.len();
        self.cells[idx].ch = '#';
    }
}

// Immutable snapshot shared via Arc (no clone of cell data)
type FrameSnapshot = Arc<Buffer>;

fn spawn_game_thread(tx: Sender<FrameSnapshot>) {
    std::thread::Builder::new()
        .name("game-logic".into())
        .spawn(move || {
            let mut buffer = Buffer::new(80, 24);
            let target_dt = Duration::from_millis(16); // ~60 ticks/sec

            loop {
                let start = Instant::now();
                buffer.tick();

                // Snapshot: clone the buffer, wrap in Arc
                let snapshot = Arc::new(buffer.clone());
                if tx.send(snapshot).is_err() {
                    break; // render thread dropped, shut down
                }

                let elapsed = start.elapsed();
                if elapsed < target_dt {
                    std::thread::sleep(target_dt - elapsed);
                }
            }
        })
        .expect("failed to spawn game thread");
}

struct App {
    window: Option<Window>,
    frame_rx: Receiver<FrameSnapshot>,
    current_frame: Option<FrameSnapshot>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            self.window = Some(
                event_loop
                    .create_window(Window::default_attributes())
                    .unwrap(),
            );
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                // Drain channel, keep only the latest frame
                while let Ok(frame) = self.frame_rx.try_recv() {
                    self.current_frame = Some(frame);
                }

                if let Some(frame) = &self.current_frame {
                    // Render using frame.cells, frame.width, frame.height
                    // (GPU upload, draw calls, etc.)
                    let _ = frame.frame; // use the data
                }

                // Request next frame
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {}
}

fn main() {
    let (tx, rx) = bounded::<FrameSnapshot>(3); // 3 frames of buffer

    spawn_game_thread(tx);

    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        window: None,
        frame_rx: rx,
        current_frame: None,
    };

    event_loop.run_app(&mut app).unwrap();
}
```

### Complete: FairMutex pattern (Alacritty-style)

```rust
use parking_lot::{Mutex, MutexGuard};
use std::sync::Arc;

/// Fair mutex that prevents starvation.
pub struct FairMutex<T> {
    data: Mutex<T>,
    next: Mutex<()>,
}

impl<T> FairMutex<T> {
    pub fn new(data: T) -> Self {
        FairMutex {
            data: Mutex::new(data),
            next: Mutex::new(()),
        }
    }

    /// Reserve the next lock acquisition. Holds a "ticket" that
    /// guarantees you'll get the data lock next.
    pub fn lease(&self) -> MutexGuard<'_, ()> {
        self.next.lock()
    }

    /// Fair lock: waits for the ticket, then acquires data.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        let _next = self.next.lock();
        self.data.lock()
    }

    /// Bypass fairness (use when you know contention is low).
    pub fn lock_unfair(&self) -> MutexGuard<'_, T> {
        self.data.lock()
    }

    /// Non-blocking try-lock (unfair).
    pub fn try_lock_unfair(&self) -> Option<MutexGuard<'_, T>> {
        self.data.try_lock()
    }
}

// Usage in a terminal grid renderer:
fn setup() {
    let grid = Arc::new(FairMutex::new(Buffer::new(80, 24)));

    // Game/PTY thread
    let game_grid = Arc::clone(&grid);
    std::thread::spawn(move || {
        loop {
            // Reserve a lease to prevent renderer starvation
            let _lease = game_grid.lease();
            // Try to lock without blocking
            if let Some(mut buf) = game_grid.try_lock_unfair() {
                buf.tick();
                // Lock is released when `buf` drops
            }
            // If lock is held by renderer, drop the lease and retry next tick
            std::thread::sleep(Duration::from_millis(16));
        }
    });

    // Render thread (main)
    // In RedrawRequested:
    {
        let buf = grid.lock(); // fair lock, guaranteed to not be starved
        renderer.draw(&buf);
    }
}
```

### Complete: ArcSwap for lock-free grid sharing

```rust
use arc_swap::ArcSwap;
use std::sync::Arc;

fn main() {
    let grid = Arc::new(ArcSwap::from_pointee(Buffer::new(80, 24)));

    // Game thread: read-copy-update
    let game_grid = Arc::clone(&grid);
    std::thread::spawn(move || {
        loop {
            // Load current, clone, modify, store
            let current = game_grid.load();
            let mut next = (*current).clone();
            next.tick();
            game_grid.store(Arc::new(next));
            std::thread::sleep(Duration::from_millis(16));
        }
    });

    // Render thread (main, in winit)
    // In RedrawRequested:
    let frame = grid.load(); // near-zero cost, no lock
    renderer.draw(&frame);
    // `frame` is a Guard that keeps the Arc alive until dropped
}
```

## Sources

- **Kept:**
  - [Alacritty main.rs](https://github.com/alacritty/alacritty/blob/master/alacritty/src/main.rs) - Entry point showing winit event loop setup
  - [Alacritty event.rs](https://github.com/alacritty/alacritty/blob/master/alacritty/src/event.rs) - Event processor with ApplicationHandler, shows main thread architecture
  - [Alacritty event_loop.rs](https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/src/event_loop.rs) - PTY reader thread with FairMutex locking strategy, channel communication
  - [Alacritty sync.rs](https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/src/sync.rs) - FairMutex implementation
  - [Alacritty grid/mod.rs](https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/src/grid/mod.rs) - Grid<T> data structure design
  - [Rio context/mod.rs](https://github.com/raphamorim/rio/blob/main/frontends/rioterm/src/context/mod.rs) - Confirms same Arc<FairMutex> pattern
  - [winit docs](https://docs.rs/winit/latest/winit/) - Event loop API, macOS main thread requirement
  - [triple_buffer docs](https://docs.rs/triple_buffer/latest/triple_buffer/) - Lock-free single-producer/single-consumer buffer
  - [arc_swap docs](https://docs.rs/arc_swap/latest/arc_swap/) - Atomic Arc swapping for read-heavy scenarios

- **Dropped:**
  - wgpu docs - Fetched but contained mostly API surface, not threading guidance. WASM constraints are well-documented elsewhere.
  - Rio application.rs - 88K chars, mostly UI event handling, not threading-relevant.

## Gaps

1. **Benchmarks of contention under load.** No empirical data on FairMutex vs ArcSwap vs triple_buffer at different grid sizes and frame rates. Would require building a harness with criterion.

2. **wgpu-specific threading constraints.** wgpu's `Device` and `Queue` are `Send` on native but `!Send` on WASM. The exact implications for buffer upload parallelism aren't fully explored here.

3. **Resize handling across threads.** When the window resizes, both the render thread and game thread need to know the new dimensions. The exact synchronization protocol (who reallocates the buffer, how to avoid rendering a partially-resized grid) deserves its own design doc.

4. **Input latency measurement.** Alacritty's approach (PTY reader batches bytes, renderer acquires lock) is optimized for throughput. Whether this tradeoff is right for a game (where input latency matters more than throughput) is an open question.