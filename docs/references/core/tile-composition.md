# Tile and Sprite Composition Reference

Research for a Rust terminal/grid rendering library covering tile stacking, alpha blending, file
formats, codepage mapping, and multi-cell sprites.

---

## 1. BearLibTerminal's Multi-Tile-Per-Cell Composition Model

BearLibTerminal provides the reference model for grid-based tile composition in roguelike rendering.
Its API has four key mechanisms:

### Layers (0-255)

```c
void terminal_layer(int layer);
```

Layers are numbered 0..255. Layer 0 is the base layer and is the **only** layer that has a
background color. Layers 1+ are transparent overlays. The scene is rendered bottom-to-top by layer
index, providing strict Z-ordering. Layers can be cleared independently with `terminal_clear_area`
(affects only the current layer), while `terminal_clear` wipes all layers.

Key use cases:

- **Large tiles**: tiles bigger than one cell need layers to prevent left-to-right, top-to-bottom
  draw order from overwriting them.
- **Logical separation**: keep dungeon terrain on layer 0, items on layer 1, creatures on layer 2,
  UI on layer 3. Update each independently.

### Composition Mode (tile stacking within a cell)

```c
void terminal_composition(int mode); // TK_ON or TK_OFF
```

When composition is **off** (default), placing a tile replaces the cell contents. When composition
is **on**, the tile is _pushed onto the cell's tile stack_. There is no enforced limit on stack
depth. Each stacked tile retains its own:

- Foreground color
- Pixel offset (dx, dy)
- Per-corner colors (for gradients)

The `print` function supports inline composition with `[+]` tags:

```c
terminal_printf("a[+][color=red]^[/color]"); // 'a' with red '^' overlaid
```

### Per-Tile Offsets and Corner Colors

```c
void terminal_put_ext(int x, int y, int dx, int dy, int code, color_t* corners);
```

- `dx`, `dy`: pixel offset from the tile's natural cell position. Each tile in a composition stack
  has its own offset. Offsets do **not** change draw order; a tile shifted visually into a
  neighbor's cell is still drawn in its own cell's turn. Use layers if you need it to render on top.
- `corners`: array of 4 `color_t` values (top-left, bottom-left, bottom-right, top-right,
  counter-clockwise). Enables smooth color gradients across tiles. Pass NULL to use the current
  foreground color uniformly.

### Color Model

Colors are 32-bit BGRA (`0xAARRGGBB`). The alpha channel is meaningful: semi-transparent foreground
tiles blend with whatever is behind them. Background color only applies to layer 0.

### Tileset Configuration

Tilesets are loaded via the `terminal_set` configuration string:

```c
// Bitmap tileset at Unicode offset 0xE000
terminal_set("0xE000: tileset.png, size=16x16, spacing=2x1");

// TrueType font
terminal_set("font: UbuntuMono-R.ttf, size=12");

// Named font for [font=italic] tags in print
terminal_set("italic font: italic.ttf, size=12");
```

Important parameters:

- `size`: tile dimensions in the source image (e.g., `16x16`)
- `spacing`: how many grid cells a single tile occupies (e.g., `2x1` for a tile that spans 2 cells
  wide, 1 cell tall)
- `codepage`: codepage mapping for the tileset (e.g., `437` for CP437 layout)
- `align`: tile alignment within its cell area (`center`, `top-left`, etc.)
- `transparent`: background color key for formats without alpha

[Source: BearLibTerminal Reference](http://foo.wyrd.name/en:bearlibterminal:reference)
[Source: BearLibTerminal Configuration](http://foo.wyrd.name/en:bearlibterminal:reference:configuration)

---

## 2. Alpha Blending Order for Stacked Tiles

### Painter's Algorithm

Grid renderers use painter's algorithm: draw back-to-front. The standard order is:

1. **Background fill** (layer 0 background color)
2. **Layer 0 tile stack** (bottom of stack first)
3. **Layer 1 tile stack**
4. ...up through layer N

Within each layer, tiles in the composition stack are drawn in push order (first pushed = drawn
first = furthest back).

Within a single layer, cells are drawn left-to-right, top-to-bottom. This is why large tiles that
extend rightward/downward get clipped: subsequent cells overwrite them. Layers solve this.

### Standard Alpha Blending (straight alpha)

The standard "over" operator for straight (non-premultiplied) alpha:

```
out.rgb = src.rgb * src.a + dst.rgb * (1 - src.a)
out.a   = src.a + dst.a * (1 - src.a)
```

### Pre-multiplied Alpha

Pre-multiplied alpha stores `(r*a, g*a, b*a, a)` instead of `(r, g, b, a)`. The blend formula
simplifies to:

```
out.rgb = src.rgb + dst.rgb * (1 - src.a)
out.a   = src.a + dst.a * (1 - src.a)
```

Advantages of pre-multiplied alpha:

- Compositing is associative: `A over (B over C) == (A over B) over C`
- Filtering/mipmapping works correctly (no dark halos at transparent edges)
- GPU blend mode is simpler: `ONE, ONE_MINUS_SRC_ALPHA`
- Supports additive blending naturally (set alpha to 0 but keep RGB for glow)

For a tile renderer, pre-multiplied alpha is strongly preferred. Most GPU APIs (wgpu, OpenGL)
default to or work best with pre-multiplied textures. PNG files store straight alpha, so convert on
load:

```rust
fn premultiply(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
    let af = a as f32 / 255.0;
    [
        (r as f32 * af) as u8,
        (g as f32 * af) as u8,
        (b as f32 * af) as u8,
        a,
    ]
}
```

### Practical Compositing Pipeline

For a software-rendered grid (no GPU):

```rust
fn composite_cell(layers: &[Layer], x: usize, y: usize) -> Rgba {
    let mut result = layers[0].background(x, y); // opaque base

    for layer in layers {
        for tile in layer.cell(x, y).tile_stack() {
            let src = tile.sample_with_color(); // tile pixel * fg color
            result = alpha_over(src, result);
        }
    }
    result
}

fn alpha_over(src: Rgba, dst: Rgba) -> Rgba {
    // Pre-multiplied "over" operation
    let inv_a = 1.0 - src.a;
    Rgba {
        r: src.r + dst.r * inv_a,
        g: src.g + dst.g * inv_a,
        b: src.b + dst.b * inv_a,
        a: src.a + dst.a * inv_a,
    }
}
```

---

## 3. Popular Roguelike Tileset Formats and Sources

### Oryx Design Lab

- **URL**: <https://www.oryxdesignlab.com/>
- **Style**: Clean 16x16 and 24x24 pixel art, consistent palette
- **Sets**: "16-bit Fantasy" (1,700+ tiles), "Tiny Dungeon" (legacy free set)
- **Format**: PNG sprite sheets, grid-aligned, no padding
- **License**: Commercial (paid), some older sets were CC

### Kenney Assets

- **URL**: <https://kenney.nl/assets>
- **Style**: Various sizes (16x16, 32x32, 64x64), clean flat style
- **Sets**: "1-Bit Pack" (over 1,000 monochrome tiles), "Roguelike/RPG Pack", "Micro Roguelike",
  "Tiny Dungeon"
- **Format**: Individual PNGs + sprite sheets. Sheets use consistent grid with no padding. Often
  includes a JSON/XML atlas file.
- **License**: CC0 (public domain), free for commercial use

### DawnLike (DragonDePlatino)

- **URL**: <https://opengameart.org/content/dawnlike-16x16-universal-rogue-like-tileset-v181>
- **Style**: 16x16, rich pixel art inspired by NetHack and Dungeon Crawl
- **Sets**: Complete roguelike tileset: terrain, characters, items, GUI, effects
- **Format**: Multiple PNG sheets organized by category (Characters, Objects, GUI, etc.). Standard
  16x16 grid, no padding.
- **License**: CC-BY 4.0 (credit required)

### Curses/CP437 Tilesets

Classic roguelike "font" tilesets that map 256 CP437 glyphs to a 16x16 grid of tiles. Standard
dimensions include 8x8, 10x10, 12x12, 16x16, and 20x20 pixels per glyph.

Popular sources:

- **Dwarf Fortress tilesets**: community-maintained collection at
  <https://dwarffortresswiki.org/Tileset_repository>
- **REXPaint fonts**: ships with multiple CP437 fonts at various sizes
- **libtcod/bracket-lib built-in fonts**: terminal8x8, vga8x16, etc.

Layout convention: 16 columns x 16 rows = 256 glyphs. The glyph at position `(col, row)` maps to
CP437 index `row * 16 + col`. Row-major order, top-left is index 0 (NUL/space), reading
left-to-right then top-to-bottom.

---

## 4. REXPaint .xp File Format

REXPaint is the standard ASCII art editor for roguelike development. Its `.xp` format is compact
(gzip-compressed binary) and widely supported.

### File Structure

The file is a gzip stream. After decompression, the binary layout is:

```
┌─────────────────────────────────┐
│ xp_version: i32                 │  Format version (negative to distinguish
│                                 │  from old files that started with layer count)
├─────────────────────────────────┤
│ num_layers: i32                 │  1..9
├─────────────────────────────────┤  ← repeated for each layer:
│ width: i32                      │
│ height: i32                     │
│ cells: [XpCell; width * height] │  Column-major order!
├─────────────────────────────────┤
│ ... next layer ...              │
└─────────────────────────────────┘
```

Each `XpCell` is 10 bytes:

```
┌──────────────────────┐
│ glyph_code: i32 (LE) │  CP437/Unicode code point
│ fg_r: u8             │
│ fg_g: u8             │
│ fg_b: u8             │
│ bg_r: u8             │
│ bg_g: u8             │
│ bg_b: u8             │
└──────────────────────┘
```

### Important Details

- **Column-major order**: cells are stored column-by-column. For position (x, y), the index is
  `x * height + y`.
- **Transparency**: background color `(255, 0, 255)` (hot pink) marks a transparent cell. When
  rendering, skip these cells on upper layers; convert visible transparent cells on the base layer
  to black.
- **No alpha channel**: colors are RGB only (3 bytes each). Transparency is binary via the magic
  pink background.
- **Width/height repeated**: each layer redundantly stores width and height (always the same across
  all layers).
- **Extended glyphs**: glyph codes can exceed 255 if the tileset has more than 16 rows (custom
  extended fonts in REXPaint).

### Rust Implementation

The `rexpaint` crate (crates.io) provides read/write support:

```rust
use rexpaint::XpFile;
use std::fs::File;
use std::io::BufReader;

// Load
let f = File::open("map.xp")?;
let xp = XpFile::read(&mut BufReader::new(f))?;

// Access
for layer in &xp.layers {
    let width = layer.width;
    let height = layer.height;
    for x in 0..width {
        for y in 0..height {
            let cell = &layer.cells[x * height + y];
            // cell.ch      -> u32 (glyph code)
            // cell.fg      -> XpColor { r, g, b }
            // cell.bg      -> XpColor { r, g, b }
            let is_transparent = cell.bg.r == 255
                && cell.bg.g == 0
                && cell.bg.b == 255;
        }
    }
}
```

[Source: REXPaint Manual, Appendix B](https://www.gridsagegames.com/rexpaint/)

---

## 5. Tiled .tmx/.tsx Format Basics

Tiled is the most popular 2D map editor. Its XML-based formats are well-supported in the Rust
ecosystem via the `tiled` crate.

### TMX (Map) Structure

```xml
<map version="1.8" orientation="orthogonal"
     width="40" height="30" tilewidth="16" tileheight="16">

  <tileset firstgid="1" source="dungeon.tsx"/>

  <layer id="1" name="Floor" width="40" height="30">
    <data encoding="csv">
      1,2,1,3,...
    </data>
  </layer>

  <layer id="2" name="Walls" width="40" height="30">
    <data encoding="base64" compression="zlib">
      eJztwTEBACAMA7...
    </data>
  </layer>

  <objectgroup id="3" name="Entities">
    <object id="1" name="spawn" x="128" y="64"/>
  </objectgroup>
</map>
```

Key attributes for roguelikes:

- `orientation`: typically `"orthogonal"` for roguelikes
- `tilewidth`/`tileheight`: grid cell size in pixels
- `renderorder`: `"right-down"` (default) matches standard roguelike rendering
- Layer ordering matches render ordering (first layer = bottom)

### TSX (Tileset) Structure

```xml
<tileset name="dungeon" tilewidth="16" tileheight="16"
         tilecount="256" columns="16">
  <image source="dungeon.png" width="256" height="256"/>
  <tile id="42">
    <properties>
      <property name="blocking" type="bool" value="true"/>
    </properties>
  </tile>
</tileset>
```

Relevant features:

- `spacing`: pixels between tiles in the source image
- `margin`: pixels around the edge of the tileset image
- `tileoffset`: pixel offset applied when rendering
- `<tile>` elements: per-tile properties (blocking, animation frames, etc.)
- `<animation>`: frame-based tile animation with per-frame duration

### Data Encoding

Tile layer data can be:

- **CSV**: comma-separated Global Tile IDs (simplest to parse)
- **Base64**: optionally compressed with zlib, gzip, or zstd
- Global Tile IDs (GIDs) are u32 values. The top 3 bits encode flip flags:
  - Bit 31: horizontal flip
  - Bit 30: vertical flip
  - Bit 29: diagonal flip (rotation)

### Rust Crate: `tiled`

```rust
use tiled::Loader;

let mut loader = Loader::new();
let map = loader.load_tmx_map("dungeon.tmx")?;

for layer in map.layers() {
    if let Some(tile_layer) = layer.as_tile_layer() {
        for y in 0..map.height {
            for x in 0..map.width {
                if let Some(tile) = tile_layer.get_tile(x as i32, y as i32) {
                    let tileset = tile.get_tileset();
                    let tile_id = tile.id(); // local ID within tileset
                }
            }
        }
    }
}
```

[Source: TMX Map Format Specification](https://doc.mapeditor.org/en/stable/reference/tmx-map-format/)
[Source: rs-tiled crate](https://github.com/mapeditor/rs-tiled)

---

## 6. LDtk Level Editor Format

LDtk (Level Designer Toolkit) is a modern 2D level editor with strong tileset support, auto-tiling,
and entity placement. Created by Sebastien Benard (creator of Dead Cells).

### JSON Structure Overview

```
Project (.ldtk)
├── jsonVersion: string
├── worldLayout: "Free" | "GridVania" | "LinearHorizontal" | "LinearVertical"
├── defs: Definitions
│   ├── layers[]        -- layer definitions (IntGrid, Tiles, Entities, AutoLayer)
│   ├── entities[]      -- entity definitions with field schemas
│   ├── tilesets[]      -- tileset definitions (path, tile size, padding, spacing)
│   └── enums[]         -- enum definitions (can tag tiles)
└── levels[]
    ├── identifier: string
    ├── worldX, worldY: int  -- position in world
    ├── pxWid, pxHei: int    -- pixel dimensions
    └── layerInstances[]
        ├── __type: "IntGrid" | "Tiles" | "Entities" | "AutoLayer"
        ├── __gridSize: int
        ├── gridTiles[]       -- for Tiles/AutoLayer
        │   ├── px: [x, y]   -- pixel position in level
        │   ├── src: [x, y]  -- pixel position in tileset source
        │   ├── f: int       -- flip flags (0=none, 1=horiz, 2=vert, 3=both)
        │   └── t: int       -- tile ID
        ├── entityInstances[] -- for Entity layers
        │   ├── __identifier: string
        │   ├── px: [x, y]
        │   └── fieldInstances[]
        └── intGridCsv[]      -- for IntGrid layers
```

### Key Features for Roguelikes

- **IntGrid layers**: cells contain integer values (wall type, terrain flags). Directly maps to
  roguelike tile maps.
- **Auto-layers**: auto-tile rules generate tiles from IntGrid values. Rules are resolved by the
  editor; game code just reads the output `gridTiles`.
- **Entity layers**: positioned entity instances with typed fields (spawn points, doors, NPCs with
  custom properties).
- **Enum tags on tiles**: tag tiles with enums in the tileset definition, then query tiles by tag at
  runtime.
- **Separate level files**: large projects can store each level in its own `.ldtkl` file.

### Rust Crate: `ldtk_rust`

```rust
use ldtk_rust::Project;

let project = Project::new("world.ldtk");

for level in &project.levels {
    for layer in level.layer_instances.as_ref().unwrap() {
        match layer.__type.as_str() {
            "Tiles" | "AutoLayer" => {
                for tile in &layer.grid_tiles {
                    let px_x = tile.px[0]; // pixel x in level
                    let px_y = tile.px[1];
                    let src_x = tile.src[0]; // source rect in tileset
                    let src_y = tile.src[1];
                    let flip = tile.f;
                }
            }
            "IntGrid" => {
                // layer.int_grid_csv contains flat array of values
            }
            "Entities" => {
                for entity in layer.entity_instances.as_ref().unwrap_or(&vec![]) {
                    // entity.__identifier, entity.px, entity.field_instances
                }
            }
            _ => {}
        }
    }
}
```

[Source: LDtk JSON Overview](https://ldtk.io/docs/general/json-overview/)
[Source: ldtk_rust crate](https://docs.rs/ldtk_rust/)

---

## 7. Sprite Sheet Conventions

### Grid Layout

Standard sprite sheet layout for roguelike tilesets:

```
┌────┬────┬────┬────┐
│ 0  │ 1  │ 2  │ 3  │  Row 0
├────┼────┼────┼────┤
│ 4  │ 5  │ 6  │ 7  │  Row 1
├────┼────┼────┼────┤
│ 8  │ 9  │ 10 │ 11 │  Row 2
└────┴────┴────┴────┘
       4 columns
```

Tile index to pixel coordinates:

```rust
fn tile_rect(index: u32, tile_w: u32, tile_h: u32, columns: u32,
             margin: u32, spacing: u32) -> (u32, u32, u32, u32) {
    let col = index % columns;
    let row = index / columns;
    let x = margin + col * (tile_w + spacing);
    let y = margin + row * (tile_h + spacing);
    (x, y, tile_w, tile_h)
}
```

### Padding and Spacing

- **Margin**: pixels from the image edge to the first tile
- **Spacing**: pixels between adjacent tiles
- **Extrusion/bleeding border**: some engines duplicate edge pixels by 1px around each tile to
  prevent texture filtering artifacts (sub-pixel bleeding). This adds 2px per tile dimension.

Common configurations: | Convention | Margin | Spacing | Notes |
|-----------|--------|---------|-------| | No padding | 0 | 0 | Simplest; most roguelike tilesets |
| 1px spacing | 0 | 1 | Prevents bleed with bilinear filtering | | 1px extrude | 0 | 2 | Each tile
edge pixel duplicated outward | | Tiled default | 0 | 0 | But supports arbitrary margin/spacing |

### Power-of-Two Sizes

GPU textures historically required power-of-two (POT) dimensions for mipmapping and some older
hardware. Modern GPUs handle NPOT textures, but POT is still preferred for:

- Efficient GPU memory allocation (no wasted padding)
- Mipmap generation
- Better cache behavior

Common tileset image sizes: 256x256, 512x512, 1024x1024, 2048x2048. Common tile sizes: 8x8, 16x16,
24x24, 32x32.

A 256x256 sheet with 16x16 tiles = 16 columns x 16 rows = 256 tiles (exactly one CP437 codepage).

### Multiple Sheets

Large tilesets split across multiple PNG files (DawnLike, Oryx). Group by category: terrain,
characters, items, effects. The renderer maintains a texture atlas or array of textures, with tile
IDs mapping to (sheet_index, local_index).

---

## 8. Codepage Mapping (CP437 Index to Unicode)

### The CP437 Standard

Code Page 437 is the character set of the original IBM PC. It defines 256 glyphs in a 16x16 grid.
For roguelike rendering, CP437 provides a complete set of:

- ASCII printable characters (0x20-0x7E)
- Box-drawing characters (0xB0-0xDF): lines, corners, intersections, shading
- Greek letters and math symbols (0xE0-0xFE): used for monsters/items
- Card suits, faces, arrows, and misc symbols (0x00-0x1F)

### Mapping Table (excerpt of non-ASCII ranges)

The full official mapping is maintained by the Unicode Consortium at
`https://www.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/PC/CP437.TXT`.

Key ranges for roguelike rendering:

```
CP437  Unicode  Description
-----  -------  -----------
0x01   U+263A   ☺ White smiling face
0x02   U+263B   ☻ Black smiling face
0x03   U+2665   ♥ Heart suit
0x04   U+2666   ♦ Diamond suit
0x05   U+2663   ♣ Club suit
0x06   U+2660   ♠ Spade suit
0x0E   U+266A   ♪ Eighth note
0x0F   U+263C   ☼ Sun
0x18   U+2191   ↑ Up arrow
0x19   U+2193   ↓ Down arrow
0x1A   U+2192   → Right arrow
0x1B   U+2190   ← Left arrow
0xB0   U+2591   ░ Light shade
0xB1   U+2592   ▒ Medium shade
0xB2   U+2593   ▓ Dark shade
0xB3   U+2502   │ Box vertical
0xC4   U+2500   ─ Box horizontal
0xC5   U+253C   ┼ Box cross
0xDA   U+250C   ┌ Box down-right
0xBF   U+2510   ┐ Box down-left
0xC0   U+2514   └ Box up-right
0xD9   U+2518   ┘ Box up-left
0xDB   U+2588   █ Full block
0xDC   U+2584   ▄ Lower half block
0xDF   U+2580   ▀ Upper half block
0xFE   U+25A0   ■ Black square
```

### Rust Implementation

```rust
/// Complete CP437 to Unicode mapping table.
/// Index with the CP437 byte value to get the Unicode code point.
const CP437_TO_UNICODE: [char; 256] = {
    let mut table = ['\0'; 256];
    // 0x00-0x1F: control char region with graphical glyphs
    table[0x00] = '\0';      // NUL (rendered as space)
    table[0x01] = '\u{263A}'; // ☺
    table[0x02] = '\u{263B}'; // ☻
    table[0x03] = '\u{2665}'; // ♥
    table[0x04] = '\u{2666}'; // ♦
    table[0x05] = '\u{2663}'; // ♣
    table[0x06] = '\u{2660}'; // ♠
    table[0x07] = '\u{2022}'; // •
    table[0x08] = '\u{25D8}'; // ◘
    table[0x09] = '\u{25CB}'; // ○
    table[0x0A] = '\u{25D9}'; // ◙
    table[0x0B] = '\u{2642}'; // ♂
    table[0x0C] = '\u{2640}'; // ♀
    table[0x0D] = '\u{266A}'; // ♪ (note: actual 0x0E)
    // ... (0x20-0x7E map 1:1 to ASCII/Unicode)
    // ... (0x80-0xFF: see full table from Unicode Consortium)
    table
};

/// Convert a Unicode char to its CP437 index, if one exists.
fn unicode_to_cp437(ch: char) -> Option<u8> {
    CP437_TO_UNICODE.iter()
        .position(|&c| c == ch)
        .map(|i| i as u8)
}
```

For production use, the `codepage_437` crate on crates.io provides a complete bidirectional mapping.
bracket-lib also includes a built-in `to_cp437` function.

### Custom Codepages

For tilesets that don't follow CP437 layout:

- Define a custom mapping file (REXPaint supports this via `_utf8.txt`)
- Store the mapping as a `HashMap<char, u16>` or a lookup table
- BearLibTerminal's `codepage` parameter in tileset config accepts custom files

[Source: Unicode CP437 Mapping](https://www.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/PC/CP437.TXT)
[Source: Wikipedia Code Page 437](https://en.wikipedia.org/wiki/Code_page_437)

---

## 9. Handling Tiles Larger Than One Cell

### The Problem

In a strict grid renderer, each cell is drawn independently in row-major order (left-to-right,
top-to-bottom). A 2x2 tile placed at cell (3, 2) needs to cover cells (3,2), (4,2), (3,3), and
(4,3). But when cell (4,2) is drawn, it overwrites the right half of the large tile.

### Approach 1: BearLibTerminal Layers + Spacing

BearLibTerminal solves this with the `spacing` tileset parameter:

```c
terminal_set("0xE000: bigmonsters.png, size=32x32, spacing=2x2");
```

This tells the renderer that tiles from this set occupy a 2x2 cell area. The tile is placed at
`put(x, y, 0xE000)` and the renderer knows to reserve that area. Combined with layers, the large
tile renders on a higher layer so it won't be overwritten by adjacent cells on lower layers.

### Approach 2: Anchor Cell + Neighbor Markers

The grid stores the large tile only in one "anchor" cell (typically top-left). Neighboring cells
that the tile overlaps store a marker (e.g., `OccupiedBy(anchor_x, anchor_y)`).

```rust
enum CellContent {
    Empty,
    Tile(TileId),
    /// This cell is visually occupied by a large tile anchored elsewhere.
    OccupiedBy { anchor_x: u32, anchor_y: u32 },
}
```

During rendering:

1. When encountering an `OccupiedBy` cell, skip drawing its own content
2. When encountering the anchor cell, draw the full multi-cell tile

### Approach 3: Separate Sprite Layer

Render multi-cell sprites on a dedicated overlay layer, completely separate from the grid. The grid
only stores gameplay data; the sprite is drawn at arbitrary pixel coordinates on top.

```rust
struct LargeSprite {
    tile_id: TileId,
    grid_x: u32,
    grid_y: u32,
    width_cells: u32,
    height_cells: u32,
}

fn render_large_sprite(sprite: &LargeSprite, cell_w: u32, cell_h: u32) {
    let px_x = sprite.grid_x * cell_w;
    let px_y = sprite.grid_y * cell_h;
    let px_w = sprite.width_cells * cell_w;
    let px_h = sprite.height_cells * cell_h;
    draw_tile_scaled(sprite.tile_id, px_x, px_y, px_w, px_h);
}
```

### Approach 4: Multi-Tile Decomposition

Split the large source tile into cell-sized sub-tiles at load time. A 2x2 tile becomes 4 separate
1x1 tiles. Each sub-tile is placed in its respective cell.

```rust
fn decompose_large_tile(
    sheet: &Image,
    src_x: u32, src_y: u32,
    tile_w: u32, tile_h: u32,  // cell size
    span_w: u32, span_h: u32,  // e.g., 2x2
) -> Vec<SubTile> {
    let mut subs = Vec::new();
    for dy in 0..span_h {
        for dx in 0..span_w {
            subs.push(SubTile {
                pixels: sheet.crop(
                    src_x + dx * tile_w,
                    src_y + dy * tile_h,
                    tile_w, tile_h,
                ),
                offset_x: dx,
                offset_y: dy,
            });
        }
    }
    subs
}
```

This is the simplest approach for a pure grid renderer since it doesn't require any special
rendering logic. The tradeoff is that the tile can't be animated or transformed as a unit.

---

## 10. Rust Code Examples

### Loading a Sprite Sheet and Extracting Tiles

```rust
use image::{GenericImageView, RgbaImage};

struct TileAtlas {
    texture: RgbaImage,
    tile_width: u32,
    tile_height: u32,
    columns: u32,
    margin: u32,
    spacing: u32,
}

impl TileAtlas {
    fn from_file(path: &str, tile_w: u32, tile_h: u32) -> Self {
        let img = image::open(path).unwrap().to_rgba8();
        let columns = img.width() / tile_w;
        TileAtlas {
            texture: img,
            tile_width: tile_w,
            tile_height: tile_h,
            columns,
            margin: 0,
            spacing: 0,
        }
    }

    fn tile_rect(&self, index: u32) -> (u32, u32, u32, u32) {
        let col = index % self.columns;
        let row = index / self.columns;
        let x = self.margin + col * (self.tile_width + self.spacing);
        let y = self.margin + row * (self.tile_height + self.spacing);
        (x, y, self.tile_width, self.tile_height)
    }

    fn get_tile(&self, index: u32) -> RgbaImage {
        let (x, y, w, h) = self.tile_rect(index);
        self.texture.view(x, y, w, h).to_image()
    }
}
```

### Software Tile Compositing

```rust
use image::{Rgba, RgbaImage};

#[derive(Clone)]
struct TileInstance {
    tile_index: u32,
    fg_color: [u8; 4],  // RGBA tint
    offset_x: i32,      // pixel offset
    offset_y: i32,
}

struct GridCell {
    bg_color: [u8; 4],
    tiles: Vec<TileInstance>,  // composition stack
}

fn render_grid(
    grid: &[Vec<GridCell>],  // grid[y][x]
    atlas: &TileAtlas,
    cell_w: u32,
    cell_h: u32,
) -> RgbaImage {
    let grid_h = grid.len() as u32;
    let grid_w = grid[0].len() as u32;
    let mut output = RgbaImage::new(grid_w * cell_w, grid_h * cell_h);

    for gy in 0..grid_h {
        for gx in 0..grid_w {
            let cell = &grid[gy as usize][gx as usize];
            let px = gx * cell_w;
            let py = gy * cell_h;

            // Fill background
            for dy in 0..cell_h {
                for dx in 0..cell_w {
                    output.put_pixel(px + dx, py + dy, Rgba(cell.bg_color));
                }
            }

            // Composite each tile in stack order
            for tile_inst in &cell.tiles {
                let tile_img = atlas.get_tile(tile_inst.tile_index);
                for dy in 0..cell_h {
                    for dx in 0..cell_w {
                        let tx = (dx as i32 + tile_inst.offset_x) as u32;
                        let ty = (dy as i32 + tile_inst.offset_y) as u32;
                        if tx < cell_w && ty < cell_h {
                            let src = tile_img.get_pixel(tx, ty);
                            let tinted = tint_pixel(*src, tile_inst.fg_color);
                            let dst = output.get_pixel(px + dx, py + dy);
                            output.put_pixel(px + dx, py + dy,
                                alpha_blend(tinted, *dst));
                        }
                    }
                }
            }
        }
    }
    output
}

fn tint_pixel(src: Rgba<u8>, tint: [u8; 4]) -> Rgba<u8> {
    Rgba([
        ((src[0] as u16 * tint[0] as u16) / 255) as u8,
        ((src[1] as u16 * tint[1] as u16) / 255) as u8,
        ((src[2] as u16 * tint[2] as u16) / 255) as u8,
        ((src[3] as u16 * tint[3] as u16) / 255) as u8,
    ])
}

fn alpha_blend(src: Rgba<u8>, dst: Rgba<u8>) -> Rgba<u8> {
    let sa = src[3] as f32 / 255.0;
    let inv_sa = 1.0 - sa;
    Rgba([
        (src[0] as f32 * sa + dst[0] as f32 * inv_sa) as u8,
        (src[1] as f32 * sa + dst[1] as f32 * inv_sa) as u8,
        (src[2] as f32 * sa + dst[2] as f32 * inv_sa) as u8,
        (src[3] as f32 + dst[3] as f32 * inv_sa).min(255.0) as u8,
    ])
}
```

### Loading REXPaint Files and Converting to Grid

```rust
use rexpaint::{XpFile, XpColor};

const TRANSPARENT_BG: XpColor = XpColor { r: 255, g: 0, b: 255 };

fn load_rexpaint_to_grid(path: &str) -> Vec<Vec<GridCell>> {
    let file = std::fs::File::open(path).unwrap();
    let xp = XpFile::read(&mut std::io::BufReader::new(file)).unwrap();

    let layer0 = &xp.layers[0];
    let w = layer0.width;
    let h = layer0.height;

    let mut grid = vec![vec![GridCell {
        bg_color: [0, 0, 0, 255],
        tiles: Vec::new(),
    }; w]; h];

    for (layer_idx, layer) in xp.layers.iter().enumerate() {
        for x in 0..w {
            for y in 0..h {
                let cell = &layer.cells[x * h + y]; // column-major!
                let is_transparent = cell.bg == TRANSPARENT_BG;

                if is_transparent && layer_idx > 0 {
                    continue; // skip transparent cells on upper layers
                }

                let gc = &mut grid[y][x];

                if layer_idx == 0 && !is_transparent {
                    gc.bg_color = [cell.bg.r, cell.bg.g, cell.bg.b, 255];
                }

                if cell.ch != 0 && cell.ch != 32 {
                    gc.tiles.push(TileInstance {
                        tile_index: cell.ch as u32,
                        fg_color: [cell.fg.r, cell.fg.g, cell.fg.b, 255],
                        offset_x: 0,
                        offset_y: 0,
                    });
                }
            }
        }
    }

    grid
}
```

### CP437 Codepage Lookup

```rust
/// Full CP437-to-Unicode mapping. Indices 0x00..0xFF map to the corresponding
/// Unicode code point as defined by the Microsoft/Unicode Consortium mapping.
pub const CP437: [char; 256] = [
    // 0x00-0x0F
    '\u{0000}', '\u{263A}', '\u{263B}', '\u{2665}',
    '\u{2666}', '\u{2663}', '\u{2660}', '\u{2022}',
    '\u{25D8}', '\u{25CB}', '\u{25D9}', '\u{2642}',
    '\u{2640}', '\u{266A}', '\u{266B}', '\u{263C}',
    // 0x10-0x1F
    '\u{25BA}', '\u{25C4}', '\u{2195}', '\u{203C}',
    '\u{00B6}', '\u{00A7}', '\u{25AC}', '\u{21A8}',
    '\u{2191}', '\u{2193}', '\u{2192}', '\u{2190}',
    '\u{221F}', '\u{2194}', '\u{25B2}', '\u{25BC}',
    // 0x20-0x7E: standard ASCII (1:1 with Unicode)
    ' ', '!', '"', '#', '$', '%', '&', '\'',
    '(', ')', '*', '+', ',', '-', '.', '/',
    '0', '1', '2', '3', '4', '5', '6', '7',
    '8', '9', ':', ';', '<', '=', '>', '?',
    '@', 'A', 'B', 'C', 'D', 'E', 'F', 'G',
    'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O',
    'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W',
    'X', 'Y', 'Z', '[', '\\', ']', '^', '_',
    '`', 'a', 'b', 'c', 'd', 'e', 'f', 'g',
    'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o',
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w',
    'x', 'y', 'z', '{', '|', '}', '~', '\u{2302}',
    // 0x80-0x8F
    '\u{00C7}', '\u{00FC}', '\u{00E9}', '\u{00E2}',
    '\u{00E4}', '\u{00E0}', '\u{00E5}', '\u{00E7}',
    '\u{00EA}', '\u{00EB}', '\u{00E8}', '\u{00EF}',
    '\u{00EE}', '\u{00EC}', '\u{00C4}', '\u{00C5}',
    // 0x90-0x9F
    '\u{00C9}', '\u{00E6}', '\u{00C6}', '\u{00F4}',
    '\u{00F6}', '\u{00F2}', '\u{00FB}', '\u{00F9}',
    '\u{00FF}', '\u{00D6}', '\u{00DC}', '\u{00A2}',
    '\u{00A3}', '\u{00A5}', '\u{20A7}', '\u{0192}',
    // 0xA0-0xAF
    '\u{00E1}', '\u{00ED}', '\u{00F3}', '\u{00FA}',
    '\u{00F1}', '\u{00D1}', '\u{00AA}', '\u{00BA}',
    '\u{00BF}', '\u{2310}', '\u{00AC}', '\u{00BD}',
    '\u{00BC}', '\u{00A1}', '\u{00AB}', '\u{00BB}',
    // 0xB0-0xBF: shade + box drawing
    '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}',
    '\u{2524}', '\u{2561}', '\u{2562}', '\u{2556}',
    '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}',
    '\u{255D}', '\u{255C}', '\u{255B}', '\u{2510}',
    // 0xC0-0xCF
    '\u{2514}', '\u{2534}', '\u{252C}', '\u{251C}',
    '\u{2500}', '\u{253C}', '\u{255E}', '\u{255F}',
    '\u{255A}', '\u{2554}', '\u{2569}', '\u{2566}',
    '\u{2560}', '\u{2550}', '\u{256C}', '\u{2567}',
    // 0xD0-0xDF: more box drawing + blocks
    '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}',
    '\u{2558}', '\u{2552}', '\u{2553}', '\u{256B}',
    '\u{256A}', '\u{2518}', '\u{250C}', '\u{2588}',
    '\u{2584}', '\u{258C}', '\u{2590}', '\u{2580}',
    // 0xE0-0xEF: Greek + math
    '\u{03B1}', '\u{00DF}', '\u{0393}', '\u{03C0}',
    '\u{03A3}', '\u{03C3}', '\u{00B5}', '\u{03C4}',
    '\u{03A6}', '\u{0398}', '\u{03A9}', '\u{03B4}',
    '\u{221E}', '\u{03C6}', '\u{03B5}', '\u{2229}',
    // 0xF0-0xFF
    '\u{2261}', '\u{00B1}', '\u{2265}', '\u{2264}',
    '\u{2320}', '\u{2321}', '\u{00F7}', '\u{2248}',
    '\u{00B0}', '\u{2219}', '\u{00B7}', '\u{221A}',
    '\u{207F}', '\u{00B2}', '\u{25A0}', '\u{00A0}',
];
```

---

## Sources

- **Kept**:
  - [BearLibTerminal Reference](http://foo.wyrd.name/en:bearlibterminal:reference) -- primary source
    for composition, layers, put_ext API
  - [BearLibTerminal Configuration](http://foo.wyrd.name/en:bearlibterminal:reference:configuration)
    -- tileset loading, codepage, spacing parameters
  - [REXPaint Manual](https://www.gridsagegames.com/rexpaint/) -- definitive .xp format
    specification (Appendix B)
  - [TMX Map Format](https://doc.mapeditor.org/en/stable/reference/tmx-map-format/) -- official
    Tiled format documentation
  - [rs-tiled crate](https://github.com/mapeditor/rs-tiled) -- Rust TMX/TSX loader
  - [LDtk JSON Overview](https://ldtk.io/docs/general/json-overview/) -- LDtk format documentation
  - [ldtk_rust crate](https://docs.rs/ldtk_rust/) -- Rust LDtk loader
  - [Unicode CP437 Mapping](https://www.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/PC/CP437.TXT) --
    authoritative codepage mapping
  - [Wikipedia: Code Page 437](https://en.wikipedia.org/wiki/Code_page_437) -- comprehensive
    reference with Unicode equivalences
  - [bracket-lib](https://github.com/amethyst/bracket-lib) -- Rust roguelike toolkit with tile
    rendering, REXPaint support
  - [rexpaint crate](https://docs.rs/rexpaint/) -- Rust .xp file reader/writer

- **Dropped**:
  - Generic gamedev tutorial sites -- too shallow, no primary specifications
  - BearLibTerminal output subpage -- 404, content already covered in main reference

## Gaps

- **GPU-accelerated tile compositing**: this document covers the conceptual model and software
  rendering. For wgpu/OpenGL vertex-batched rendering with texture atlases, additional research into
  instanced quad rendering and texture array approaches would be useful.
- **Animation**: tile animation (frame cycling, duration) is mentioned in Tiled's `<animation>`
  element but not explored in depth. REXPaint has no animation.
- **Texture atlas packing**: runtime atlas packing (bin packing algorithms like rectpack) for
  combining multiple sprite sheets into one GPU texture is not covered.
- **DawnLike/Oryx exact sheet layouts**: the specific sheet organization of these tilesets (which
  file contains which category) would need their actual distribution files to document precisely.
