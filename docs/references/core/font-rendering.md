# Research: CPU Font/Glyph Rasterization in Rust for Terminal Grid Rendering

## Summary

For a terminal/grid renderer, **cosmic-text + swash + etagere** is the proven production stack. cosmic-text handles font discovery (fontdb), shaping (harfrust), fallback, and layout; swash rasterizes glyphs (including color emoji via COLR/CBDT/sbix); etagere's `BucketedAtlasAllocator` packs glyphs into a texture atlas with LRU eviction. This is exactly the stack used by glyphon (wgpu text renderer), iced, and the COSMIC desktop. For a simpler prototype that doesn't need shaping or fallback, fontdue offers the fastest raw rasterization with a trivial API, but it cannot handle complex scripts or emoji.

## 1. fontdue

### What it is

Pure Rust, `no_std`, TrueType/OpenType font rasterizer and layout tool. Claims lowest end-to-end latency for a font rasterizer. Parses fonts fully on creation (no lifetime dependencies). Depends on `ttf-parser` for font parsing.

**Crate:** [`fontdue`](https://crates.io/crates/fontdue) (0.9.x)

### API

```rust
use fontdue::{Font, FontSettings};

// Parse font (allocates, fully parses on creation)
let font = Font::from_bytes(
    include_bytes!("Roboto-Regular.ttf") as &[u8],
    FontSettings::default(),
).unwrap();

// Rasterize a single glyph: returns (Metrics, Vec<u8>)
// The Vec<u8> is an alpha coverage bitmap (0=transparent, 255=opaque)
let (metrics, bitmap) = font.rasterize('g', 17.0);

// metrics.width, metrics.height  -- bitmap dimensions
// metrics.xmin, metrics.ymin     -- glyph bearing (offset from origin)
// metrics.advance_width           -- horizontal advance

// Can also rasterize by glyph index:
let (metrics, bitmap) = font.rasterize_indexed(glyph_index, 17.0);

// Subpixel rasterization (width is 3x for RGB subpixels):
let (metrics, bitmap) = font.rasterize_subpixel('g', 17.0);

// Layout API (naive, no shaping):
use fontdue::layout::{Layout, CoordinateSystem, LayoutSettings, TextStyle};
let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
layout.reset(&LayoutSettings::default());
layout.append(&[font], &TextStyle::new("Hello world!", 35.0, 0));
for glyph in layout.glyphs() {
    // glyph.x, glyph.y, glyph.width, glyph.height, glyph.parent (char)
    let (_, bitmap) = font.rasterize(glyph.parent, glyph.key.px);
}
```

### Key characteristics

- **Performance**: Fastest pure-Rust rasterizer. Benchmarks show it outperforming ab_glyph at all sizes for both glyf and CFF outlines.
- **No shaping**: Layout is naive (character-by-character with basic kerning). No OpenType GSUB/GPOS. This means no ligatures, no complex script support.
- **No font discovery**: You must load font bytes yourself.
- **No color emoji**: No support for COLR, CBDT, or sbix tables.
- **No fallback**: Single font only per rasterization call.
- **no_std compatible**: Works in embedded/WASM contexts.
- **Subpixel rendering**: Built-in support via `rasterize_subpixel()`.

### When to use

Quick prototyping or simple ASCII-only rendering where shaping and emoji don't matter. For a terminal, fontdue alone is insufficient because terminals must handle CJK, emoji, and fallback fonts.

## 2. ab_glyph

### What it is

OpenType glyph loading, scaling, positioning, and rasterization. Successor to `rusttype`. Used by `glyph_brush` and previously by egui.

**Crate:** [`ab_glyph`](https://crates.io/crates/ab_glyph) (0.2.x)

### API

```rust
use ab_glyph::{Font, FontRef, Glyph, point};

// Load font (zero-copy from slice, or owned via FontVec)
let font = FontRef::try_from_slice(include_bytes!("Exo2-Light.otf"))?;

// Get a glyph with scale and position
let glyph: Glyph = font
    .glyph_id('q')
    .with_scale_and_position(24.0, point(100.0, 0.0));

// Outline and draw
if let Some(outlined) = font.outline_glyph(glyph) {
    outlined.draw(|x, y, coverage| {
        // x, y: pixel coordinates (u32)
        // coverage: 0.0..1.0 alpha value
    });
}

// Metrics via the Font trait
let advance = font.h_advance(glyph.id);
let ascent = font.ascent_unscaled();
let kern = font.kern(glyph_a.id, glyph_b.id);
```

### Comparison to fontdue

| Feature | fontdue | ab_glyph |
|---------|---------|----------|
| Rasterization speed | Faster (~2-4x at typical sizes) | Slower |
| API style | Returns `(Metrics, Vec<u8>)` | Callback-based `draw(\|x,y,c\|)` |
| Output format | Coverage bitmap (u8 array) | Per-pixel callback |
| Subpixel rendering | Built-in | Not built-in |
| Variable fonts | No | Yes (with `variable-fonts` feature) |
| `no_std` | Yes | Yes (with `libm` feature) |
| Shaping | No | No |
| Font discovery | No | No |
| Color emoji | No | No |
| Allocation model | Owned font, allocates on parse | Zero-copy or owned |

Both are "rasterizer-only" libraries. Neither does shaping or fallback. fontdue's bitmap output is more convenient for atlas construction; ab_glyph's callback requires you to allocate your own buffer.

## 3. cosmic-text (Full Stack)

### What it is

The text rendering stack for the COSMIC desktop (System76/Pop!_OS). Provides the full pipeline: font discovery, shaping, fallback, layout, and optional rasterization. Used by glyphon, iced, Bevy (as of 0.16), and many other Rust projects.

**Crate:** [`cosmic-text`](https://crates.io/crates/cosmic-text) (latest uses harfrust)

### Architecture

```
                    cosmic-text
                   /     |       \
           fontdb    harfrust     swash (optional)
        (discovery)  (shaping)   (rasterization)
```

- **fontdb**: Font database and discovery. Scans system font directories, supports `fontconfig` on Linux. Matches fonts by family, weight, stretch, style (CSS Fonts Level 3 rules).
- **harfrust**: Pure Rust port of HarfBuzz for OpenType text shaping (GSUB/GPOS). Handles ligatures, complex scripts (Arabic, Devanagari, etc.), kerning. Earlier versions used `rustybuzz`; current versions use `harfrust`.
- **swash**: Glyph rasterization with color emoji support (COLR/CPAL, CBDT/CBLC, sbix). Optional dependency (feature-gated).

### API Walkthrough

```rust
use cosmic_text::{
    Attrs, Buffer, Color, FontSystem, Metrics, Shaping, SwashCache,
    SwashContent, CacheKey, fontdb,
};

// --- Step 1: Create FontSystem (one per application) ---
// Loads system fonts automatically
let mut font_system = FontSystem::new();

// Or, create with a custom font database:
let mut db = fontdb::Database::new();
db.load_system_fonts();
db.load_font_file("path/to/custom/font.ttf").unwrap();
let mut font_system = FontSystem::new_with_locale_and_db("en-US".into(), db);

// --- Step 2: Create SwashCache (one per application) ---
let mut swash_cache = SwashCache::new();

// --- Step 3: Create a Buffer for text layout ---
let metrics = Metrics::new(14.0, 20.0); // font_size, line_height
let mut buffer = Buffer::new(&mut font_system, metrics);

// Set buffer size (width for wrapping, height for clipping)
buffer.set_size(&mut font_system, Some(800.0), Some(600.0));

// Set text with attributes
let attrs = Attrs::new()
    .family(cosmic_text::Family::Name("JetBrains Mono"))
    .weight(cosmic_text::Weight::NORMAL);
buffer.set_text(
    &mut font_system,
    "Hello, world! 🦀 مرحبا",
    &attrs,
    Shaping::Advanced, // use harfrust for full shaping
    None,              // alignment
);

// --- Step 4: Iterate layout runs and rasterize ---
for run in buffer.layout_runs() {
    let line_y = run.line_y;

    for glyph in run.glyphs.iter() {
        // Convert logical glyph to physical (snapped to pixel grid)
        let physical = glyph.physical((0.0, 0.0), 1.0); // (offset, scale)

        // physical.cache_key: CacheKey for atlas lookup
        // physical.x, physical.y: integer pixel position

        // Rasterize via SwashCache
        let image = swash_cache.get_image(&mut font_system, physical.cache_key);
        if let Some(image) = image {
            // image.placement: { width, height, left, top }
            // image.content: SwashContent::Mask | Color | SubpixelMask
            // image.data: Vec<u8> (alpha for Mask, RGBA for Color)

            let x = physical.x + image.placement.left;
            let y = physical.y - image.placement.top;
            // Upload image.data to atlas at computed position
        }
    }
}

// --- Alternative: Simple draw callback ---
let text_color = Color::rgb(0xFF, 0xFF, 0xFF);
buffer.draw(&mut swash_cache, text_color, |x, y, w, h, color| {
    // Draw a colored rectangle at (x, y) with size (w, h)
});
```

### CacheKey structure

```rust
pub struct CacheKey {
    pub font_id: fontdb::ID,      // Which font face
    pub glyph_id: u16,            // Glyph index within font
    pub font_size_bits: u32,      // f32::to_bits() of font size
    pub x_bin: SubpixelBin,       // Fractional X quantized to 4 bins
    pub y_bin: SubpixelBin,       // Fractional Y quantized to 4 bins
    pub font_weight: Weight,      // For synthetic bold
    pub flags: CacheKeyFlags,     // Synthetic italic, etc.
}

// SubpixelBin quantizes fractional offsets into 4 bins:
// Zero (0.0..0.25), One (0.25..0.5), Two (0.5..0.75), Three (0.75..1.0)
// This means each glyph at a given size can have up to 16 subpixel variants (4x * 4y)
```

### Font fallback

cosmic-text handles font fallback automatically during shaping. When a glyph is not found in the primary font, it walks a fallback chain:
1. User-specified font family
2. Monospace fallback (when `monospace_fallback` feature is enabled)
3. System fonts matched by script/language
4. Platform-specific emoji fonts
5. Last-resort fallback

The fallback is per-glyph: a single buffer can mix glyphs from multiple fonts.

### Shaping modes

```rust
Shaping::Advanced  // Full harfrust/rustybuzz shaping (ligatures, GSUB/GPOS)
Shaping::Basic     // Simple left-to-right, no ligatures (faster)
```

For a terminal, `Shaping::Advanced` is needed for ligature fonts (like Fira Code) and correct rendering of non-Latin scripts.

## 4. swash (Standalone Usage)

### What it is

Pure Rust font introspection, complex text shaping, and glyph rendering. Authored by the same developer who created the `parley` layout engine for the Linebender project. Supports TrueType, CFF, variable fonts, color emoji (COLR/CPAL, CBDT/CBLC, sbix).

**Crate:** [`swash`](https://crates.io/crates/swash) (0.2.x)

### Standalone API

```rust
use swash::{FontRef, CacheKey, scale::*, shape::*, GlyphId};
use zeno::{Format, Vector};

// --- Font Loading ---
let font_data = std::fs::read("JetBrainsMono-Regular.ttf").unwrap();
let font = FontRef {
    data: &font_data,
    offset: 0,
    key: CacheKey::new(),
};

// --- Scaling Context (reuse per thread) ---
let mut scale_context = ScaleContext::new();

// --- Build Scaler ---
let mut scaler = scale_context.builder(font)
    .size(14.0)
    .hint(true)
    .build();

// --- Render a glyph ---
let glyph_id = font.charmap().map('A');

// Simple approach: get an Image directly via Render
let image = Render::new(&[
    // Priority order of glyph sources
    Source::ColorOutline(0),        // COLR/CPAL with palette 0
    Source::ColorBitmap(StrikeWith::BestFit), // CBDT/sbix
    Source::Outline,                // Standard scalable outline
])
.format(Format::Alpha)             // or Format::Subpixel for LCD
.offset(Vector::new(0.25, 0.0))   // subpixel offset
.render(&mut scaler, glyph_id);

if let Some(image) = image {
    // image.placement: Placement { left, top, width, height }
    // image.data: Vec<u8> (alpha or RGBA depending on source)
    // image.content: Content::Mask, Color, or SubpixelMask
}

// --- Text Shaping (swash has its own shaper) ---
use swash::shape::ShapeContext;

let mut shape_context = ShapeContext::new();
let mut shaper = shape_context
    .builder(font)
    .size(14.0)
    .script(swash::text::Script::Latin)
    .build();

shaper.add_str("Hello");

shaper.shape_with(|cluster| {
    for glyph in cluster.glyphs {
        // glyph.id: GlyphId
        // glyph.x: f32 offset
        // glyph.y: f32 offset
        // glyph.data: u32 (pass-through user data)
    }
});
```

### Key characteristics

- **Zero transient heap allocations**: All scratch buffers maintained in contexts
- **Thread-friendly**: ScaleContext and ShapeContext are separate from font data
- **CacheKey**: Opaque identifier for font instances, used to key internal LRU caches
- **Full emoji support**: COLR/CPAL (Microsoft), CBDT/CBLC (Google), sbix (Apple)
- **Hinting**: TrueType and PostScript hinting supported
- **Variable fonts**: Full support including for shaping
- **Path effects**: Stroking, dashing, emboldening, affine transforms

### swash vs fontdue

| Feature | swash | fontdue |
|---------|-------|---------|
| Shaping | Full OpenType + AAT | None |
| Color emoji | COLR, CBDT, sbix | None |
| Variable fonts | Full | No |
| Hinting | TT + PS | None |
| Performance | Good, but heavier | Fastest pure rasterization |
| Dependency weight | Moderate | Minimal |

## 5. Building a Glyph Atlas

### Overview

A glyph atlas is a single large texture containing all rasterized glyphs. The GPU samples from this atlas when rendering text, reducing draw calls to one per atlas texture.

### Packing Strategies

#### Shelf Packing (etagere)

**Crate:** [`etagere`](https://crates.io/crates/etagere)

Glyphs are placed left-to-right on horizontal "shelves." When a glyph doesn't fit the current shelf, a new shelf is started. Two allocators:

- **`AtlasAllocator`**: Tracks individual allocations. Supports deallocation and shelf coalescing (merging adjacent empty shelves). Better fragmentation handling.
- **`BucketedAtlasAllocator`**: Groups items into buckets. Faster allocation/deallocation but only reclaims space when all items in a bucket are freed. Better for large numbers of small, similarly-sized items (i.e., glyphs).

```rust
use etagere::{BucketedAtlasAllocator, size2, Allocation};

// Create atlas allocator (1024x1024 pixels)
let mut packer = BucketedAtlasAllocator::new(size2(1024, 1024));

// Allocate space for a glyph (with 1px padding)
let alloc: Option<Allocation> = packer.allocate(size2(
    glyph_width + 2,  // 1px padding each side
    glyph_height + 2,
));

if let Some(alloc) = alloc {
    let x = alloc.rectangle.min.x + 1; // skip padding
    let y = alloc.rectangle.min.y + 1;
    // Upload glyph bitmap to texture at (x, y)
}

// Deallocate when evicting
packer.deallocate(alloc.id);

// Grow the atlas (preserves existing allocations)
packer.grow(size2(2048, 2048));
```

**Why glyphon and others prefer etagere**: Shelf packing is simple, fast, and works well for glyphs which tend to be similar heights (within a given font size). `BucketedAtlasAllocator` is the sweet spot for glyph atlases.

#### Guillotine Packing (guillotiere)

**Crate:** [`guillotiere`](https://crates.io/crates/guillotiere)

Tracks rectangle splits in a tree structure. Finds and merges neighboring free rectangles in constant time. Better space utilization than shelf packing for varied rectangle sizes, but slower allocation.

```rust
use guillotiere::{AtlasAllocator, size2};

let mut allocator = AtlasAllocator::new(size2(1024, 1024));

let alloc = allocator.allocate(size2(64, 48));
if let Some(alloc) = alloc {
    // alloc.id: AllocId
    // alloc.rectangle: Rectangle { min, max }
}

allocator.deallocate(alloc.id);
```

Originally developed for Firefox WebRender. Slightly better packing density, but etagere's shelf packing is faster and "good enough" for glyphs.

#### Recommendation

**Use `etagere::BucketedAtlasAllocator`** for a glyph atlas. Glyphs at a given font size have similar heights, which is the ideal case for shelf packing. This is what glyphon uses in production.

### Cache Key Design

The cache key must encode everything that affects the rasterized output:

```rust
// cosmic-text's CacheKey is the gold standard:
struct GlyphCacheKey {
    font_id: fontdb::ID,        // which font face
    glyph_id: u16,              // glyph index (NOT codepoint - ligatures map multiple chars to one glyph)
    font_size_bits: u32,        // f32::to_bits() for exact equality
    x_bin: SubpixelBin,         // quantized fractional X (4 bins)
    y_bin: SubpixelBin,         // quantized fractional Y (4 bins)
    font_weight: Weight,        // for synthetic bold
    flags: CacheKeyFlags,       // synthetic italic, etc.
}
```

**Terminal simplification**: In a terminal grid, glyphs snap to cell boundaries. You can often skip subpixel binning entirely (all glyphs at integer positions), reducing cache key to just `(font_id, glyph_id, font_size_bits)`. This dramatically reduces atlas entries.

However, if you want fractional positioning (for proportional fonts in UI elements), keep the subpixel bins. The 4-bin quantization (Zero/One/Two/Three representing 0.0..0.25, 0.25..0.5, 0.5..0.75, 0.75..1.0) is the standard approach, giving at most 16 variants per glyph per size.

### Eviction Strategies

Three approaches used in practice:

1. **LRU eviction (glyphon)**: Track which glyphs are used each frame via a `glyphs_in_use: HashSet`. On atlas full, evict least-recently-used entries that aren't in the current frame's set. Uses `lru::LruCache`.

2. **Frame-based eviction (macroquad example)**: Track `used_this_frame: HashSet<CacheKey>`. At end of frame, deallocate any glyph not used. Simple but aggressive; glyphs that appear every other frame get thrashed.

3. **Recreate on overflow (WezTerm/Alacritty)**: When atlas is full, create a new atlas with 2x dimensions and re-rasterize all cached glyphs. No eviction tracking needed. Works well for terminals where the glyph set is bounded.

**Recommendation for terminal**: Start with strategy 3 (grow-and-rebuild). A typical terminal uses ~100-200 unique glyphs (ASCII + common symbols). Even at 2x DPI with subpixel variants, this fits comfortably in a 1024x1024 atlas. If you need more sophistication, add LRU eviction.

### Complete Atlas Example (cosmic-text + etagere)

```rust
use cosmic_text::{CacheKey, SwashCache, SwashContent, FontSystem, Buffer};
use etagere::{BucketedAtlasAllocator, size2, AllocId};
use std::collections::HashMap;

struct GlyphEntry {
    alloc_id: AllocId,
    atlas_x: u32,
    atlas_y: u32,
    width: u32,
    height: u32,
    offset_x: i32,  // bearing
    offset_y: i32,
    is_color: bool,
}

struct GlyphAtlas {
    packer: BucketedAtlasAllocator,
    cache: HashMap<CacheKey, GlyphEntry>,
    // Two textures: one R8 for masks, one RGBA8 for color emoji
    mask_pixels: Vec<u8>,    // R8
    color_pixels: Vec<u8>,   // RGBA
    size: u32,
}

impl GlyphAtlas {
    fn new(size: u32) -> Self {
        Self {
            packer: BucketedAtlasAllocator::new(size2(size as i32, size as i32)),
            cache: HashMap::new(),
            mask_pixels: vec![0; (size * size) as usize],
            color_pixels: vec![0; (size * size * 4) as usize],
            size,
        }
    }

    fn get_or_rasterize(
        &mut self,
        key: CacheKey,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
    ) -> Option<&GlyphEntry> {
        if self.cache.contains_key(&key) {
            return self.cache.get(&key);
        }

        let image = swash_cache.get_image(font_system, key)?;
        let w = image.placement.width;
        let h = image.placement.height;
        if w == 0 || h == 0 { return None; }

        // Allocate with 1px padding
        let alloc = self.packer.allocate(size2(w as i32 + 2, h as i32 + 2))?;
        let ax = (alloc.rectangle.min.x + 1) as u32;
        let ay = (alloc.rectangle.min.y + 1) as u32;

        let is_color = matches!(image.content, SwashContent::Color);

        if is_color {
            // Copy RGBA data into color atlas
            for row in 0..h {
                let src_start = (row * w * 4) as usize;
                let src_end = src_start + (w * 4) as usize;
                let dst_start = ((ay + row) * self.size * 4 + ax * 4) as usize;
                self.color_pixels[dst_start..dst_start + (w * 4) as usize]
                    .copy_from_slice(&image.data[src_start..src_end]);
            }
        } else {
            // Copy alpha data into mask atlas
            for row in 0..h {
                let src_start = (row * w) as usize;
                let src_end = src_start + w as usize;
                let dst_start = ((ay + row) * self.size + ax) as usize;
                self.mask_pixels[dst_start..dst_start + w as usize]
                    .copy_from_slice(&image.data[src_start..src_end]);
            }
        }

        self.cache.insert(key, GlyphEntry {
            alloc_id: alloc.id,
            atlas_x: ax,
            atlas_y: ay,
            width: w,
            height: h,
            offset_x: image.placement.left,
            offset_y: image.placement.top,
            is_color,
        });

        self.cache.get(&key)
    }
}
```

## 6. Font Fallback Chains

### How cosmic-text handles fallback

cosmic-text's `FontSystem` wraps a `fontdb::Database` and performs per-glyph fallback during shaping:

1. **Primary font**: User-specified family (e.g., "JetBrains Mono")
2. **Attribute match**: If the glyph is missing, search fontdb for fonts matching the requested weight/style/stretch
3. **Script-based fallback**: Identify the Unicode script of the missing codepoint and search for fonts known to cover that script
4. **Monospace fallback**: When the `monospace_fallback` feature is enabled, prefer monospace fonts in fallback (critical for terminals)
5. **Emoji fallback**: Platform-specific emoji font detection (Apple Color Emoji on macOS, Noto Color Emoji on Linux)
6. **Last resort**: `.notdef` glyph (tofu box)

### Manual fallback (without cosmic-text)

```rust
// If using fontdue or swash directly, implement your own fallback:
struct FontStack {
    fonts: Vec<swash::FontRef<'static>>,
}

impl FontStack {
    fn find_glyph(&self, c: char) -> Option<(usize, GlyphId)> {
        for (idx, font) in self.fonts.iter().enumerate() {
            let glyph_id = font.charmap().map(c);
            if glyph_id != 0 {
                return Some((idx, glyph_id));
            }
        }
        None
    }
}
```

### Terminal-specific considerations

- **Monospace constraint**: Fallback fonts may be proportional. You need to scale/clip glyphs to fit the cell grid. WezTerm does this with sophisticated scaling logic based on glyph aspect ratio.
- **Double-width characters**: CJK characters occupy two cells. The renderer must detect `East_Asian_Width` property and allocate two cells.
- **Nerd Fonts / Powerline**: Many terminal users install patched fonts. These should be the primary font, not a fallback.

## 7. Color Emoji Handling

### Emoji font table formats

| Format | Used by | Data type | swash support |
|--------|---------|-----------|---------------|
| **COLR/CPAL** v0 | Microsoft, Google (Noto Color Emoji v2) | Vector layers with color palettes | Yes |
| **COLR** v1 | Google (Noto Color Emoji v3) | Vector with gradients, compositing | Partial |
| **CBDT/CBLC** | Google (Noto Color Emoji bitmap) | Embedded PNG bitmaps | Yes (but see caveat) |
| **sbix** | Apple (Apple Color Emoji) | Embedded PNG/JPEG bitmaps | Yes |
| **SVG** | Mozilla, Adobe | SVG documents per glyph | No (requires SVG renderer) |

### swash's source priority

```rust
// The Render builder accepts a priority list of sources:
Render::new(&[
    Source::ColorOutline(0),              // COLR with palette 0
    Source::ColorBitmap(StrikeWith::BestFit), // CBDT/sbix, scaled to requested size
    Source::Outline,                       // Monochrome outline fallback
])
```

swash handles compositing of COLR layers internally. For CBDT/sbix, it selects the best bitmap strike for the requested size and scales it.

### Known issues

- **swash CBDT panics**: There are known issues with swash panicking on some CBDT fonts (swash#48). The `bevy_emoji` crate works around this by using `ttf-parser` to extract CBDT bitmaps directly, bypassing swash.
- **COLRv1 support**: Partial. Complex gradient fills and compositing modes may not render correctly.
- **SVG emoji**: Not supported by swash. Would require integrating `resvg` or similar.

### Atlas implications for color emoji

Color emoji require an RGBA texture, not an alpha-only texture. This means you need either:
- **Two atlas textures**: One R8 for monochrome glyphs, one RGBA8 for color (glyphon's approach)
- **Single RGBA atlas**: Store monochrome glyphs as (255, 255, 255, alpha), color glyphs as-is. Wastes 3x memory for monochrome glyphs but simplifies the pipeline.

glyphon uses two separate `InnerAtlas` instances (mask and color) with separate `BucketedAtlasAllocator` each. The shader samples from the appropriate atlas based on a flag in the vertex data.

## 8. Production Reference: glyphon

[glyphon](https://github.com/grovesNL/glyphon) is the canonical example of this entire stack assembled for wgpu. Architecture:

```
cosmic-text (FontSystem + Buffer)
    ↓ layout_runs() → LayoutGlyph → PhysicalGlyph → CacheKey
SwashCache
    ↓ get_image(CacheKey) → SwashImage { data, placement, content }
etagere::BucketedAtlasAllocator
    ↓ allocate(size) → Allocation { id, rectangle }
wgpu Texture (R8Unorm for mask, Rgba8Unorm for color)
    ↓ write_texture()
Vertex buffer (screen_rect, atlas_rect, color, content_type)
    ↓ render pass with instanced quads
```

Key implementation details from glyphon's `text_atlas.rs`:
- Initial atlas size: 256x256, grows by 2x up to GPU's `max_texture_dimension_2d`
- Uses `LruCache` (from `lru` crate) with `FxHasher` for fast glyph lookup
- Tracks `glyphs_in_use: HashSet` per frame for LRU eviction
- On atlas full: evicts LRU entries that aren't in current frame's use set, retrying allocation
- If all sized glyphs are in use: grows the atlas (re-rasterizes all cached glyphs into new texture)
- Separate mask and color atlases

## 9. Performance Comparison

### Rasterization speed (relative, not absolute)

| Library | Relative speed | Notes |
|---------|---------------|-------|
| **fontdue** | Fastest (1x) | Pure rasterization only, no overhead |
| **ab_glyph** | ~2-4x slower | Callback-based, variable font support adds overhead |
| **swash** | ~3-5x slower | Full-featured: hinting, color, variable fonts |
| **cosmic-text + swash** | ~5-8x slower | Adds shaping, fallback, layout overhead |

These ratios are for the rasterization step only. In practice, rasterization is done once per glyph per size (cached in the atlas), so the speed difference is negligible for steady-state rendering. The initial "cold start" populating the atlas is where it matters.

### Where time is actually spent in a terminal renderer

1. **Shaping** (~40% of text processing): HarfBuzz/harfrust is the bottleneck for complex text
2. **Rasterization** (~30%): One-time cost per glyph, amortized by caching
3. **Atlas upload** (~20%): GPU texture upload for new glyphs
4. **Layout** (~10%): Line breaking, bidi, cursor positioning

For a terminal with mostly ASCII text and a monospace font, shaping is fast (basic kerning only) and the glyph set is small (~200 glyphs). Cold start to fill the atlas takes <1ms on modern hardware.

### Memory usage

| Atlas size | R8 (mask) | RGBA8 (color) |
|------------|-----------|---------------|
| 512x512 | 256 KB | 1 MB |
| 1024x1024 | 1 MB | 4 MB |
| 2048x2048 | 4 MB | 16 MB |

A 1024x1024 atlas comfortably holds ~1000+ ASCII glyphs at 14px. For terminals, you rarely need more than 1024x1024 for the mask atlas. Color emoji may need a separate 512x512 atlas.

## 10. Recommendation for a Terminal Grid Renderer

### Recommended stack

```toml
[dependencies]
cosmic-text = { version = "0.16", default-features = false, features = ["std", "swash", "monospace_fallback"] }
etagere = "0.4"
```

### Why this stack

1. **cosmic-text** provides everything needed out of the box: font discovery, shaping (for ligature fonts), per-glyph fallback (for emoji and CJK), and rasterization.
2. **`monospace_fallback` feature** ensures fallback fonts prefer monospace variants, critical for terminal grid alignment.
3. **swash** (via cosmic-text) handles color emoji properly with COLR/CBDT/sbix support.
4. **etagere** is the battle-tested atlas allocator, used by glyphon and others.
5. This is the same stack as glyphon, iced, and the COSMIC desktop, so it's well-maintained.

### Why not fontdue alone

- No shaping = no ligatures (Fira Code, JetBrains Mono ligatures won't work)
- No fallback = emoji and CJK characters won't render
- No color emoji support
- You'd have to build all the infrastructure cosmic-text already provides

### Implementation approach

1. Create `FontSystem` and `SwashCache` at startup
2. When terminal content changes, create/update a `Buffer` with the cell text
3. Iterate `layout_runs()`, convert each `LayoutGlyph` to `PhysicalGlyph`
4. For each `PhysicalGlyph`, look up `cache_key` in your atlas HashMap
5. On cache miss: rasterize via `SwashCache::get_image()`, pack into atlas via etagere
6. Build a vertex buffer of quads (screen position + atlas UV) and render in one draw call

### Terminal-specific optimization: skip subpixel binning

For a monospace grid, all glyphs align to cell boundaries (integer pixel positions). Set subpixel bins to `SubpixelBin::Zero` or use cosmic-text's `PhysicalGlyph` with integer offsets. This means each glyph at a given size needs only ONE atlas entry, not up to 16 subpixel variants. Cache key reduces to `(font_id, glyph_id, font_size)`.

### Alternative: fontdue for the fast path

If you want maximum rasterization speed for ASCII and don't need shaping:
- Use **fontdue** for ASCII glyphs (fast path, ~95% of terminal content)
- Use **cosmic-text + swash** for non-ASCII fallback (emoji, CJK, complex scripts)
- This hybrid approach adds complexity but gives best-of-both performance

Most terminals (Alacritty, WezTerm) don't take this hybrid approach. They use a single text stack throughout. WezTerm uses FreeType + HarfBuzz (C libraries); a pure-Rust equivalent is cosmic-text + swash.

## Sources

- Kept: [fontdue docs](https://docs.rs/fontdue) and [GitHub](https://github.com/mooman219/fontdue) - primary API docs and benchmarks
- Kept: [ab_glyph docs](https://docs.rs/ab_glyph/latest/ab_glyph/) - API reference for Font trait
- Kept: [cosmic-text docs](https://docs.rs/cosmic-text/latest/cosmic_text/) - full API reference, CacheKey/SubpixelBin/Buffer/FontSystem
- Kept: [swash docs](https://docs.rs/swash/latest/swash/scale/index.html) and [GitHub](https://github.com/dfrg/swash) - scale module walkthrough, Render builder, Source enum
- Kept: [etagere docs](https://docs.rs/etagere/latest/etagere/) - AtlasAllocator and BucketedAtlasAllocator API
- Kept: [guillotiere GitHub](https://github.com/nical/guillotiere) - guillotine algorithm implementation
- Kept: [glyphon source (text_atlas.rs)](https://docs.rs/glyphon/latest/x86_64-pc-windows-msvc/src/glyphon/text_atlas.rs.html) - production atlas implementation with LRU eviction
- Kept: [cosmic-text macroquad gist](https://gist.github.com/caspark/b88108696d0e7678b2e6768da32f1be2) - complete working example of cosmic-text + guillotiere atlas
- Kept: [WezTerm DeepWiki - Glyph Cache](https://deepwiki.com/wezterm/wezterm/3.2.1-glyph-cache-and-texture-atlas) - production terminal glyph cache architecture
- Kept: [Mozilla Gfx Blog - Atlas Allocation](https://mozillagfx.wordpress.com/2021/02/04/improving-texture-atlas-allocation-in-webrender/) - original rationale for etagere vs guillotiere
- Kept: [bevy_emoji](https://crates.io/crates/bevy_emoji) - documents swash CBDT panic issue
- Kept: [resvg color fonts issue](https://github.com/RazrFalcon/resvg/issues/487) - comprehensive breakdown of emoji font table formats
- Dropped: fontcore crate - too early-stage, not relevant
- Dropped: typf-render-color - niche, low downloads
- Dropped: text-typeset - undocumented, low maturity
- Dropped: Alacritty renderer PRs - interesting but Alacritty's approach (crossfont + FreeType) isn't pure Rust

## Gaps

1. **Exact benchmark numbers**: fontdue's README shows graphs but not raw numbers. The Exa rate limit prevented finding independent benchmark comparisons. The relative performance claims are directionally correct based on fontdue's benchmark graphs and design (no hinting, no color, no variable font overhead).

2. **harfrust maturity**: cosmic-text recently switched from rustybuzz to harfrust. harfrust is a full Rust port of HarfBuzz rather than a C-to-Rust translation. Could not find independent benchmarks comparing the two. Both are believed to pass the HarfBuzz test suite.

3. **COLRv1 completeness in swash**: The exact coverage of COLRv1 features (gradients, compositing operators, sweep gradients) is unclear from docs alone. For terminal use, COLRv0 support (flat color layers) is sufficient for most emoji.

4. **Subpixel rendering on macOS**: macOS deprecated subpixel antialiasing in Mojave. For a cross-platform terminal, you likely want to support both alpha-only and subpixel modes, but macOS should default to grayscale AA.

5. **Multi-threaded rasterization**: swash's `ScaleContext` is designed for per-thread use. For initial atlas population with hundreds of glyphs, parallel rasterization could help. No benchmarks found on the actual speedup.