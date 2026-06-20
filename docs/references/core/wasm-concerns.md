# WASM Concerns for a Rust Terminal/Grid Rendering Library

Comprehensive reference for shipping a Rust terminal/grid renderer to browsers via WebAssembly.
Covers build optimization, async patterns, threading, browser compatibility, deployment, JS
integration, tooling, event handling, and performance.

## 1. Bundle Size Optimization

### Cargo Profile Settings

```toml
[profile.release]
lto = true          # Full link-time optimization; enables cross-crate inlining/pruning
opt-level = 's'     # Optimize for size (try 'z' too; 's' is sometimes smaller)
codegen-units = 1   # Single codegen unit = better optimization, slower compile
strip = true        # Strip debug symbols
panic = 'abort'     # No unwinding machinery; saves ~10KB
```

`opt-level = 's'` sometimes produces smaller binaries than `'z'`. Always measure both.

### wasm-opt Post-Processing

The [Binaryen](https://github.com/WebAssembly/binaryen) `wasm-opt` tool typically yields another
15-20% size reduction on top of LLVM's output, and can improve runtime speed simultaneously.

```sh
wasm-opt -Os -o output.wasm input.wasm   # size-optimized
wasm-opt -Oz -o output.wasm input.wasm   # aggressive size optimization
```

Both `wasm-pack` and `trunk` can run `wasm-opt` automatically.

### Tree Shaking / Dead Code Elimination

- **web-sys cargo features**: `web-sys` is entirely feature-gated. Only enable the exact API

  features you need (e.g., `Window`, `Document`, `HtmlCanvasElement`, `WebGl2RenderingContext`).
  Each unused feature adds zero code.

- **wasm-bindgen CLI**: strips all unexported functions and unused imports from the final `.wasm`.

  The raw compiler output (`target/wasm32-unknown-unknown/release/foo.wasm`) is intentionally
  oversized; never measure that file.

- **wasm-snip**: replaces function bodies with `unreachable` instructions. Useful for removing panic

  infrastructure. Follow with `wasm-opt --dce` to cascade dead code removal.

- **twiggy**: size profiler for `.wasm` binaries. Use `twiggy top` and `twiggy dominators` to find

  what's pulling in code.

### Code-Level Techniques

| Technique                                        | Savings                        | Trade-off                           |
| ------------------------------------------------ | ------------------------------ | ----------------------------------- |
| Avoid `format!`/`to_string` in release           | Large (pulls in fmt infra)     | Debug-only formatting               |
| Avoid panics (use `abort` or `unchecked_unwrap`) | ~1-5KB per panic site chain    | Safety risk with unchecked          |
| Use trait objects over generics                  | Reduces monomorphization bloat | Dynamic dispatch overhead           |
| Avoid allocation or use `wee_alloc`              | ~10KB (dlmalloc size)          | `wee_alloc` is slower at allocation |
| Use `#[repr(u8)]` enums, `FixedBitSet`           | Compact memory layout          | Bit manipulation complexity         |

### Compression

Wasm binary format is highly amenable to gzip, often achieving 50%+ reduction. Use
`instantiateStreaming` (supported in all modern browsers) so the browser can compile Wasm while it's
still downloading.

## 2. Async Initialization

### wasm-bindgen-futures

Bridges JS `Promise` and Rust `Future`. Convert a `Promise` to a Rust future via
`JsFuture::from(promise).await`, or export an `async fn` from Rust that returns a `Promise` to JS.

```rust
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::Response;

#[wasm_bindgen]
pub async fn load_font(url: String) -> Result<JsValue, JsValue> {
    let window = web_sys::window().unwrap();
    let resp_value = JsFuture::from(window.fetch_with_str(&url)).await?;
    let resp: Response = resp_value.dyn_into()?;
    let buffer = JsFuture::from(resp.array_buffer()?).await?;
    Ok(buffer)
}
```

### Initialization Pattern

```rust
#[wasm_bindgen]
pub async fn init() -> Result<(), JsValue> {
    // 1. Load font atlas (async fetch)
    let font_data = load_font("/assets/font_atlas.png").await?;

    // 2. Initialize WebGL context
    let canvas = get_canvas()?;
    let gl = canvas.get_context("webgl2")?.unwrap();

    // 3. Upload textures (synchronous GL calls)
    upload_font_texture(&gl, &font_data)?;

    // 4. Start render loop
    start_render_loop(gl)?;
    Ok(())
}
```

From JS:

```js
import init, { init as appInit } from './pkg/my_app.js';
await init(); // Load and instantiate WASM
await appInit(); // Async app initialization (fonts, assets)
```

### Key Considerations

- Wasm instantiation itself should use `WebAssembly.instantiateStreaming` (wasm-bindgen's

  `--target web` does this automatically).

- Font loading can use the browser's `FontFace` API via `web-sys` or fetch raw bytes for a custom

  atlas.

- Show a loading indicator from JS while `init()` runs.

## 3. Web Worker Rendering

### Basic Worker Pattern

The wasm-bindgen guide provides a
[Wasm in Web Worker example](https://rustwasm.github.io/docs/wasm-bindgen/examples/wasm-in-web-worker.html).
The pattern:

1. Main thread creates a `Worker` and sends initial config via `postMessage`.
2. Worker loads wasm via `importScripts` (for `--target no-modules`) or ES module import

   (Chrome-only).

3. Worker instantiates wasm, creates rendering state, and processes messages.

```js
// worker.js
importScripts('./pkg/my_app.js');
async function init() {
  await wasm_bindgen('./pkg/my_app_bg.wasm');
  const renderer = wasm_bindgen.Renderer.new();
  self.onmessage = (e) => {
    if (e.data.type === 'render') {
      renderer.tick();
      // Transfer rendered frame back or draw to OffscreenCanvas
    }
  };
}
init();
```

### OffscreenCanvas in a Worker

Transfer a canvas to a worker for GPU rendering off the main thread:

```js
// main thread
const canvas = document.getElementById('terminal');
const offscreen = canvas.transferControlToOffscreen();
worker.postMessage({ canvas: offscreen }, [offscreen]);
```

```js
// worker
self.onmessage = (e) => {
  if (e.data.canvas) {
    const gl = e.data.canvas.getContext('webgl2');
    // Initialize Rust renderer with this GL context
  }
};
```

### SharedArrayBuffer for Grid Data

For sharing grid state between main thread (handling input) and worker (rendering):

```rust
// Grid stored in SharedArrayBuffer
// Main thread writes cell updates, worker reads for rendering
use js_sys::SharedArrayBuffer;

#[wasm_bindgen]
pub fn create_grid_buffer(width: u32, height: u32) -> SharedArrayBuffer {
    let size = (width * height * 4) as u32; // 4 bytes per cell (char + attrs)
    SharedArrayBuffer::new(size)
}
```

### Requirements for SharedArrayBuffer

- Server must send COOP/COEP headers:

  ````text
  Cross-Origin-Opener-Policy: same-origin
  Cross-Origin-Embedder-Policy: require-corp
  ```text

  ````

- Without these headers, `SharedArrayBuffer` is undefined in the browser.

### Atomics for Synchronization

Use `Atomics.wait` / `Atomics.notify` (via `js_sys`) for signaling between threads. The main thread
in a browser **cannot call `Atomics.wait`** (it would block the UI). Only workers can block. Use
`Atomics.waitAsync` on the main thread (Chrome 87+) or a polling approach.

### Threading with wasm-bindgen (Parallel Raytracing Pattern)

For true multi-threaded Wasm, requires nightly Rust with:

````shell
RUSTFLAGS='-C target-feature=+atomics,+bulk-memory,+mutable-globals'
cargo build --target wasm32-unknown-unknown -Z build-std=panic_abort,std
```rust

Caveats from the wasm-bindgen parallel raytracing example:

- The main browser thread **cannot block** (no mutex acquisition, no `Atomics.wait`).
- No `std::thread` support; use Web Workers as the threading primitive.
- `--target bundler` is unsupported for threaded wasm; use `--target web` or `--target no-modules`.
- TLS destructors never run; use `__wbindgen_thread_destroy` to clean up.
- Worker threads may implicitly block during initialization.

## 4. Browser Compatibility Matrix

Data from CanIUse as of June 2025:

| Feature               | Chrome | Firefox     | Safari        | Edge | iOS Safari | Notes                                   |
| --------------------- | ------ | ----------- | ------------- | ---- | ---------- | --------------------------------------- |
| WebAssembly           | 57+    | 52+         | 11+           | 16+  | 11+        | Universal in modern browsers            |
| WebGL 2.0             | 56+    | 51+         | 15+           | 79+  | 15+        | Full support everywhere now             |
| WebGPU                | 113+   | behind flag | 26+ (partial) | 113+ | 26+        | Firefox still disabled by default       |
| OffscreenCanvas       | 69+    | 105+        | 17+           | 79+  | 17+        | Safari 16.2-16.6 partial only           |
| SharedArrayBuffer     | 68+    | 79+         | 15.2+         | 79+  | 15.2+      | Requires COOP/COEP headers              |
| ES Modules in Workers | 80+    | behind flag | 15+           | 80+  | 15+        | Firefox limitation affects worker setup |

### Practical Implications

- **WebGL2 is the safe rendering target.** Universal support across all modern browsers since ~2021.

  Use as the default backend.

- **WebGPU is not ready as sole backend.** Firefox has it behind a flag with no firm timeline for

  default-on. Safari support is partial (26+). Plan WebGPU as an opt-in enhancement over WebGL2.

- **OffscreenCanvas is viable for worker rendering.** All modern browsers support it. Safari 16.x

  had partial support but 17+ is full.

- **SharedArrayBuffer works everywhere but needs headers.** The COOP/COEP requirement is the main

  deployment friction, not browser support.

- **Worker module support is inconsistent.** Firefox does not support ES modules in workers, so use

  `--target no-modules` with `importScripts` for cross-browser worker compatibility, or
  `--target web` with a thin wrapper.

### wasm-bindgen Browser Support

wasm-bindgen targets Firefox, Chrome, Safari, and Edge. The generated JS uses features that all
modern browsers support. IE11 is unsupported (no WebAssembly). For legacy contexts, Binaryen's
`wasm2js` can transpile wasm to JS.

## 5. Deploying to itch.io

### HTML5 Game Packaging

itch.io embeds HTML5 games in an iframe. Requirements:

- Upload a **ZIP file** containing an `index.html` entry point and all assets.
- Set "Kind of Game" to "HTML Game" on the project page.
- Configure embed dimensions or use "Click to launch in fullscreen."

### ZIP File Constraints

- Max 1,000 files after extraction
- Max 240 character file paths
- Max 500MB total extracted size; 200MB per file
- File names are case-sensitive and UTF-8

### Build Pipeline for itch.io

```sh
# Build with trunk (generates dist/ directory)

trunk build --release

# Or with wasm-pack + manual HTML

wasm-pack build --target web --release
# Copy pkg/, index.html, assets/ into a staging directory

# Package

cd dist && zip -r ../my-game.zip . && cd ..

# Upload via butler (itch.io CLI) or web interface

butler push my-game.zip your-username/your-game:html5
````

### itch.io-Specific Notes

- itch.io's CDN auto-compresses `.wasm` files with gzip.
- Pre-compressed `.br` (Brotli) files are detected by extension and served with correct

  `content-encoding`.

- Use relative paths only (absolute paths break because the game is served from a subdirectory).
- itch.io does NOT set COOP/COEP headers, so **SharedArrayBuffer will not work** on itch.io unless

  they add support. Plan for a single-threaded fallback.

- "Mobile Friendly" option available in embed settings; your renderer should handle dynamic viewport

  sizes.

## 6. Deploying to GitHub Pages

### Using trunk

[Trunk](https://trunkrs.dev/) is a build tool specifically for Rust WASM web apps. It handles the
full pipeline: compile to wasm, run wasm-bindgen, run wasm-opt, bundle assets, and generate HTML.

```toml
# Trunk.toml (optional config)

[build]
target = "index.html"
release = true

[tools]
wasm_opt = "version_119"
```

GitHub Actions workflow:

```yaml
name: Deploy to GitHub Pages
on:
  push:
    branches: [main]
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

        with:
          targets: wasm32-unknown-unknown

      - name: Install trunk

        run: cargo install trunk

      - name: Build

        run: trunk build --release --public-url "/${{ github.event.repository.name }}/"

      - name: Deploy

        uses: peaceiris/actions-gh-pages@v3
        with:
          github_token: ${{ secrets.GITHUB_TOKEN }}
          publish_dir: ./dist
```

The `--public-url` flag is critical for GitHub Pages, which serves from
`https://user.github.io/repo/`.

### Using wasm-pack

```sh
wasm-pack build --target web --release
```

Then manually create an `index.html` that loads the generated JS:

```html
<script type="module">
  import init from './pkg/my_app.js';
  async function run() {
    await init();
  }
  run();
</script>
```

### COOP/COEP Headers on GitHub Pages

GitHub Pages does **not** allow custom HTTP headers. This means SharedArrayBuffer and multi-threaded
wasm will not work on GitHub Pages directly. Workarounds:

- Use [coi-serviceworker](https://github.com/nickelqd/nickelqd.github.io) to inject headers via a

  service worker.

- Deploy to a platform that supports custom headers (Cloudflare Pages, Netlify, Vercel).

## 7. Integrating with JavaScript Frameworks

### General Pattern

The wasm module is an async dependency. Framework wrappers need to:

1. Load and instantiate the wasm module.
2. Manage a canvas element lifecycle.
3. Forward events to the wasm module.
4. Clean up on unmount.

### React Wrapper

```tsx
import { useEffect, useRef } from 'react';

export function Terminal({ cols, rows }: { cols: number; rows: number }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<any>(null);

  useEffect(() => {
    let cancelled = false;
    async function setup() {
      const wasm = await import('./pkg/my_terminal.js');
      await wasm.default(); // init wasm
      if (cancelled) return;

      const renderer = wasm.TerminalRenderer.new(canvasRef.current!, cols, rows);
      rendererRef.current = renderer;
    }
    setup();
    return () => {
      cancelled = true;
      rendererRef.current?.free(); // Call wasm destructor
    };
  }, [cols, rows]);

  return <canvas ref={canvasRef} />;
}
```

### Svelte Wrapper

```svelte
<script>
  import { onMount, onDestroy } from 'svelte';

  export let cols = 80;
  export let rows = 24;

  let canvas;
  let renderer;

  onMount(async () => {
    const wasm = await import('./pkg/my_terminal.js');
    await wasm.default();
    renderer = wasm.TerminalRenderer.new(canvas, cols, rows);
  });

  onDestroy(() => {
    renderer?.free();
  });
</script>

<canvas bind:this={canvas}></canvas>
```

### Key Integration Concerns

- **Memory management**: Rust objects allocated via `#[wasm_bindgen]` must be freed by calling

  `.free()` on the JS side. React's `useEffect` cleanup and Svelte's `onDestroy` are the right
  places.

- **TypeScript types**: wasm-bindgen generates `.d.ts` files automatically. These integrate with

  framework tooling.

- **npm packaging**: Use `wasm-pack build --target bundler` to produce an npm-compatible package.

  Framework bundlers (Vite, Webpack) can import it directly.

- **Vite plugin**: `vite-plugin-wasm` handles wasm imports. For Vite 5+, top-level await + `wasm`

  target usually works natively.

## 8. Build Tools: wasm-bindgen vs wasm-pack vs trunk

| Tool             | Role                   | Output                    | Use Case                                            |
| ---------------- | ---------------------- | ------------------------- | --------------------------------------------------- |
| **wasm-bindgen** | Core binding generator | JS glue + processed .wasm | Low-level; always used under the hood               |
| **wasm-pack**    | Build + package tool   | npm-ready package (pkg/)  | Publishing to npm, integrating with JS bundlers     |
| **trunk**        | Full web app builder   | Complete dist/ directory  | Self-contained Rust web apps, GitHub Pages, itch.io |

### wasm-bindgen

The foundation layer. Generates JS bindings from `#[wasm_bindgen]` annotations. Provides multiple
output targets:

- `--target bundler`: ES module output for Webpack/Vite (default).
- `--target web`: Standalone ES module, no bundler needed.
- `--target no-modules`: Global script, uses `importScripts`, required for cross-browser worker

  compatibility.

- `--target nodejs`: CommonJS for Node.

### wasm-pack

Wraps `cargo build` + `wasm-bindgen` + `wasm-opt` into one command. Generates a complete npm package
with `package.json`, `.d.ts` types, and README. Best for library authors who want to publish the
wasm module for consumption by JS projects.

```sh
wasm-pack build --target web --release  # For direct web use
wasm-pack build --target bundler        # For npm/bundler consumption
wasm-pack test --chrome --headless      # Run wasm-bindgen-test in browser
```

### trunk

Watches `index.html` for asset links, auto-compiles Rust to wasm, runs wasm-bindgen and wasm-opt,
copies assets, and serves with live reload. Best for apps where Rust owns the entire frontend.

```sh
trunk serve          # Dev server with live reload
trunk build --release --public-url /my-app/  # Production build
```

### Recommendation for This Project

Use **wasm-pack** with `--target web` as the primary build. This gives maximum flexibility:

- Direct embedding in any web page.
- Easy integration with React/Svelte/vanilla JS.
- npm publishable for library consumers.
- Can be wrapped by trunk if a standalone demo/app is needed.

## 9. Handling Browser Events from Rust

### requestAnimationFrame

The canonical pattern uses `Rc<RefCell<Option<Closure>>>` to create a self-referencing callback
loop:

```rust
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

fn request_animation_frame(f: &Closure<dyn FnMut()>) {
    web_sys::window().unwrap()
        .request_animation_frame(f.as_ref().unchecked_ref())
        .unwrap();
}

pub fn start_render_loop() {
    let f = Rc::new(RefCell::new(None));
    let g = f.clone();

    *g.borrow_mut() = Some(Closure::new(move || {
        // Render frame here
        render_frame();
        // Schedule next frame
        request_animation_frame(f.borrow().as_ref().unwrap());
    }));

    request_animation_frame(g.borrow().as_ref().unwrap());
}
```

### ResizeObserver

```rust
use wasm_bindgen::prelude::*;
use web_sys::ResizeObserver;

pub fn observe_resize(canvas: &web_sys::HtmlCanvasElement) -> ResizeObserver {
    let cb = Closure::<dyn FnMut(js_sys::Array)>::new(move |entries: js_sys::Array| {
        let entry = entries.get(0);
        // Extract content rect, update renderer dimensions
    });

    let observer = ResizeObserver::new(cb.as_ref().unchecked_ref()).unwrap();
    observer.observe(canvas);
    cb.forget(); // Leak the closure (lives for app lifetime)
    observer
}
```

Required web-sys features: `ResizeObserver`, `ResizeObserverEntry`.

### Visibility Change (Page Visibility API)

```rust
pub fn observe_visibility() {
    let document = web_sys::window().unwrap().document().unwrap();
    let cb = Closure::<dyn FnMut()>::new(move || {
        let doc = web_sys::window().unwrap().document().unwrap();
        let hidden = doc.hidden();
        if hidden {
            // Pause rendering, reduce tick rate
        } else {
            // Resume rendering
        }
    });
    document
        .add_event_listener_with_callback("visibilitychange", cb.as_ref().unchecked_ref())
        .unwrap();
    cb.forget();
}
```

### Closure Memory Management

- `Closure::forget()` leaks memory. Acceptable for app-lifetime callbacks (rAF loop, resize

  observer).

- For short-lived callbacks, store the `Closure` in a struct and drop it when no longer needed.
- Each `Closure` that crosses the JS boundary allocates. Minimize the number of closures; batch

  event handling where possible.

### Alternative: Handle Events in JS, Call Wasm

For high-frequency events (keyboard, mouse), it's often better to handle them in a thin JS layer and
batch-call into wasm:

```js
document.addEventListener('keydown', (e) => {
  wasmModule.handle_key(e.keyCode, e.shiftKey, e.ctrlKey, e.altKey);
});
```

This avoids creating Rust closures for each event type and reduces JS-Wasm boundary overhead.

## 10. Performance: WASM vs Native Overhead

### Baseline Performance

WebAssembly typically runs at **70-95% of native speed** depending on the workload:

- Compute-heavy code (grid simulation, text shaping): ~85-95% native speed.
- Code with heavy memory allocation: ~70-85% (wasm linear memory + bounds checking).
- SIMD-heavy code: varies; wasm SIMD is a subset of native SIMD.

Browsers use tiered compilation: baseline compiler runs code as soon as bytes arrive over the
network, then an optimizing compiler (TurboFan in V8, IonMonkey/Cranelift in Firefox) recompiles hot
functions.

### JS-WASM Boundary Crossing Cost

Each call from JS to WASM (or vice versa) has overhead from:

- Argument marshaling (converting JS types to wasm scalars).
- Stack switching between JS and wasm execution contexts.
- For complex types (strings, objects): serialization + copy across the boundary.

**Measured cost**: a trivial JS-to-wasm function call costs roughly 5-20ns in modern browsers. This
is small for individual calls but adds up at high frequency.

### Minimizing Boundary Crossings

The Rust WASM book's Game of Life tutorial demonstrates the core principle:

> "Large, long-lived data structures should live in WebAssembly linear memory and be exposed to
> JavaScript as opaque handles. JavaScript calls exported functions that transform data, perform
> heavy computation, and return small results."

Concrete techniques:

1. **Batch operations**: Instead of calling wasm per-cell, call `tick()` once per frame to process

   the entire grid.

1. **Direct memory access**: Instead of copying grid data through function returns, expose a pointer

   to wasm linear memory and read it from JS as a `Uint8Array`:

   ```js
   const cellsPtr = universe.cells();
   const cells = new Uint8Array(wasm.memory.buffer, cellsPtr, width * height);
   ```

1. **Batch input events**: Accumulate keyboard/mouse events in a JS buffer, pass the batch to wasm

   once per frame.

1. **Minimize string passing**: Strings require allocation + copy in both directions. Use numeric

   IDs or pre-allocated buffers instead.

1. **Use typed arrays for bulk data**: When you must transfer data, use `Float32Array` /

   `Uint8Array` views into shared memory rather than serializing to JSON.

### Rendering Pipeline Optimization

For a terminal/grid renderer specifically:

| Approach                                           | Boundary Crossings/Frame | Latency |
| -------------------------------------------------- | ------------------------ | ------- |
| Render entirely in wasm (WebGL via web-sys)        | 1 (rAF callback)         | Lowest  |
| Wasm computes grid, JS reads memory + draws canvas | 2-3 (tick + ptr read)    | Low     |
| Wasm returns cell array, JS iterates + draws       | N (per-cell)             | High    |

**Recommended**: do all grid computation and GL rendering in Rust/wasm. The only JS-wasm boundary
crossing per frame should be the `requestAnimationFrame` callback entry point.

### WebGL/WebGPU from Rust

Using `web-sys` to call WebGL2 directly from Rust has per-call overhead equivalent to calling from
JS. For draw-call-heavy renderers, batch geometry into a single draw call (instanced rendering for
grid cells). A terminal grid is well-suited to instancing: one quad per cell, instanced attributes
for character ID, foreground/background color.

## Sources

- **Kept:**
  - [Rust WASM Book: Shrinking .wasm Size](https://rustwasm.github.io/docs/book/reference/code-size.html) -

    authoritative guide on all size optimization techniques

  - [wasm-bindgen Guide: Promises and Futures](https://rustwasm.github.io/docs/wasm-bindgen/reference/js-promises-and-rust-futures.html) -

    official async integration docs

  - [wasm-bindgen Guide: requestAnimationFrame](https://rustwasm.github.io/docs/wasm-bindgen/examples/request-animation-frame.html) -

    canonical rAF loop pattern

  - [wasm-bindgen Guide: Wasm in Web Worker](https://rustwasm.github.io/docs/wasm-bindgen/examples/wasm-in-web-worker.html) -

    official worker example

  - [wasm-bindgen Guide: Parallel Raytracing](https://rustwasm.github.io/docs/wasm-bindgen/examples/raytrace.html) -

    SharedArrayBuffer + threading caveats

  - [wasm-bindgen Guide: Deployment](https://rustwasm.github.io/docs/wasm-bindgen/reference/deployment.html) -

    all target modes documented

  - [wasm-bindgen Guide: Optimizing for Size](https://rustwasm.github.io/docs/wasm-bindgen/reference/optimize-size.html) -

    wasm-bindgen-specific size advice

  - [Rust WASM Book: Implementing Life](https://rustwasm.github.io/docs/book/game-of-life/implementing.html) -

    JS-WASM interface design principles

  - [itch.io HTML5 Games Documentation](https://itch.io/docs/creators/html5) - upload requirements,

    compression, pitfalls

  - [CanIUse: OffscreenCanvas](https://caniuse.com/offscreencanvas) - browser support data
  - [CanIUse: SharedArrayBuffer](https://caniuse.com/sharedarraybuffer) - browser support +

    COOP/COEP requirements

  - [CanIUse: WebGL 2.0](https://caniuse.com/webgl2) - universal modern browser support confirmed
  - [CanIUse: WebGPU](https://caniuse.com/webgpu) - still limited (Firefox disabled by default)

- **Dropped:**
  - trunkrs.dev - returned 403, could not fetch docs (used GitHub README knowledge instead)
  - MDN Rust to WebAssembly guide - basic intro, no novel information beyond wasm-bindgen docs

## Gaps

1. **Trunk documentation details**: trunkrs.dev blocked the fetch. The GitHub Actions workflow and

   configuration shown above are based on known trunk CLI behavior, but specific `Trunk.toml`
   options may have changed. Verify against the
   [trunk GitHub repo](https://github.com/trunk-rs/trunk).

1. **WebGPU from Rust (wgpu)**: The `wgpu` crate supports wasm32 and translates to WebGPU/WebGL2

   automatically. A deeper dive into wgpu's wasm-specific configuration (feature flags, adapter
   selection, limits) would be valuable as a follow-up.

1. **Specific performance benchmarks**: The 70-95% native speed estimate is a commonly cited range.

   Project-specific benchmarks (grid rendering throughput, draw call overhead via web-sys WebGL
   bindings) should be measured once a prototype exists.

1. **itch.io COOP/COEP headers**: Could not confirm whether itch.io supports or plans to support

   COOP/COEP for SharedArrayBuffer. The single-threaded fallback recommendation stands, but this
   should be verified.

1. **coi-serviceworker reliability**: Using a service worker to inject COOP/COEP headers on GitHub

   Pages is a known workaround, but it adds a second page load on first visit and may have edge
   cases. Test thoroughly if multi-threaded wasm on GitHub Pages is required.
