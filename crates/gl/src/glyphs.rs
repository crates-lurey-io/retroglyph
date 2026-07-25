//! Glyph source + slot cache: maps characters to grid-packed atlas slots (issue #367).
//!
//! Two sources back the same [`GlyphCache`] API:
//!
//! - [`FontSource::Bitmap`]: the embedded/1-bit [`BitmapFont`]. Every glyph is uploaded once at
//!   init, the character-to-slot map is the font's own fixed index, and nothing is rasterized at
//!   run time.
//! - [`FontSource::Ttf`]: a TrueType/OpenType font rasterized on demand with `fontdue` (pure Rust,
//!   so it runs identically on native and wasm -- no `Canvas2D`/`FontFace` round trip). Glyphs are
//!   rasterized into a fixed-capacity, LRU-managed atlas the first time they're seen; the coverage
//!   bytes are queued for the renderer to `texSubImage3D` into their slot.
//!
//! The renderer never sees characters: [`GlyphCache::resolve`] hands back a `u16` atlas slot that
//! goes straight into the instance buffer, and [`GlyphCache::take_pending`] yields the coverage the
//! renderer must upload before drawing.

#![allow(clippy::redundant_pub_crate)]

use crate::atlas::{AtlasData, AtlasGeometry, SLOTS_PER_LAYER};
use retroglyph_window::font::BitmapFont;
use std::collections::HashMap;

/// Dynamic-atlas capacity in glyph slots (`SLOTS_PER_LAYER * layers`). 32 layers is far below the
/// 256-layer GL floor and holds a large working set; a grid with more distinct glyphs on screen
/// than this would thrash the LRU (documented limit).
const DYNAMIC_CAPACITY: u32 = SLOTS_PER_LAYER * 32;

/// Where glyph coverage comes from.
enum FontSource {
    /// A static bitmap font: all glyphs uploaded once, fixed character-to-slot map.
    Bitmap(BitmapFont),
    /// A TrueType font rasterized on demand by `fontdue`.
    Ttf(TtfFont),
    /// A test-only source that fills each non-space cell with full coverage, so a dynamic atlas can
    /// be exercised on the GPU (grid packing across layers, per-glyph upload) without a font file.
    #[cfg(test)]
    Synthetic { cell_w: u32, cell_h: u32 },
}

/// A `fontdue`-backed TrueType font rasterized at a fixed pixel size into monospace cells.
pub(crate) struct TtfFont {
    font: fontdue::Font,
    /// Rasterization size in pixels (the cell height).
    px: f32,
    /// Cell width in pixels (a representative monospace advance).
    cell_w: u32,
    /// Cell height in pixels.
    cell_h: u32,
    /// Baseline offset from the cell top, in pixels (the font's ascent).
    ascent: f32,
}

impl TtfFont {
    /// Parses `bytes` and derives monospace cell metrics for rasterization at `px` pixels tall.
    ///
    /// # Errors
    ///
    /// Returns a message if `fontdue` cannot parse the font.
    pub(crate) fn new(bytes: &[u8], px: f32) -> Result<Self, String> {
        let font = fontdue::Font::from_bytes(
            bytes,
            fontdue::FontSettings {
                scale: px,
                ..fontdue::FontSettings::default()
            },
        )
        .map_err(|e| format!("parse TTF: {e}"))?;

        let line = font
            .horizontal_line_metrics(px)
            .ok_or_else(|| "font has no horizontal line metrics".to_owned())?;
        // Cell height spans ascent..descent; width is a representative advance ('M' is a good
        // monospace stand-in and is present in essentially every text font).
        let cell_h = (line.ascent - line.descent).ceil().max(1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cell_w = font.metrics('M', px).advance_width.ceil().max(1.0) as u32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Ok(Self {
            font,
            px,
            cell_w,
            cell_h: cell_h as u32,
            ascent: line.ascent,
        })
    }

    /// Rasterizes `ch` into a `cell_w * cell_h` row-major coverage cell (top row first), placing
    /// the glyph on the baseline and clipping anything outside the cell.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn rasterize_cell(&self, ch: char) -> Vec<u8> {
        let mut cell = vec![0u8; (self.cell_w * self.cell_h) as usize];
        let (m, bitmap) = self.font.rasterize(ch, self.px);
        if m.width == 0 || m.height == 0 {
            return cell; // space and other zero-area glyphs.
        }
        // Glyph top-left in cell pixels: x by the left side bearing, y so the glyph sits on the
        // baseline (`ascent` below the cell top). `ymin` is the bottom of the glyph above the
        // baseline (y-up), so the top is `ymin + height`.
        let gx = m.xmin;
        let gy = (self.ascent - (m.ymin as f32 + m.height as f32)).round() as i32;

        for row in 0..m.height {
            let cy = gy + row as i32;
            if cy < 0 || cy >= self.cell_h as i32 {
                continue;
            }
            for col in 0..m.width {
                let cx = gx + col as i32;
                if cx < 0 || cx >= self.cell_w as i32 {
                    continue;
                }
                let src = bitmap[row * m.width + col];
                if src != 0 {
                    let idx = (cy as u32 * self.cell_w + cx as u32) as usize;
                    cell[idx] = src;
                }
            }
        }
        cell
    }
}

/// A dynamic-atlas slot assignment plus its LRU recency.
struct DynSlot {
    slot: u32,
    last_used: u64,
}

/// Assigns atlas slots to characters, growing to `capacity` then evicting the least-recently-used
/// entry. Font-agnostic (no rasterization), so the eviction policy is unit-testable on its own.
struct SlotAllocator {
    capacity: u32,
    map: HashMap<char, DynSlot>,
    /// Next never-used slot, until the atlas fills and the LRU starts evicting.
    next_slot: u32,
    /// Monotonic access counter for recency.
    tick: u64,
}

impl SlotAllocator {
    fn new(capacity: u32) -> Self {
        Self {
            capacity,
            map: HashMap::new(),
            next_slot: 0,
            tick: 0,
        }
    }

    /// Returns `(slot, was_miss)` for `ch`: a hit bumps recency; a miss assigns a fresh slot
    /// (growing, then LRU-evicting) that the caller must rasterize into.
    fn get_or_assign(&mut self, ch: char) -> (u32, bool) {
        self.tick += 1;
        let tick = self.tick;
        if let Some(entry) = self.map.get_mut(&ch) {
            entry.last_used = tick;
            return (entry.slot, false);
        }
        let slot = if self.next_slot < self.capacity {
            let s = self.next_slot;
            self.next_slot += 1;
            s
        } else {
            self.evict_lru()
        };
        self.map.insert(
            ch,
            DynSlot {
                slot,
                last_used: tick,
            },
        );
        (slot, true)
    }

    /// Removes the least-recently-used entry and returns its freed slot.
    fn evict_lru(&mut self) -> u32 {
        let victim = self
            .map
            .iter()
            .min_by_key(|(_, e)| e.last_used)
            .map(|(&ch, _)| ch)
            .expect("a full allocator has at least one entry to evict");
        self.map.remove(&victim).expect("victim just found").slot
    }

    /// Every currently-cached `(char, slot)`.
    fn entries(&self) -> Vec<(char, u32)> {
        self.map.iter().map(|(&ch, e)| (ch, e.slot)).collect()
    }
}

/// Maps characters to atlas slots, rasterizing on demand for the dynamic (TTF) source.
pub(crate) struct GlyphCache {
    source: FontSource,
    geometry: AtlasGeometry,
    /// Glyph cell size in unscaled pixels (equals the atlas cell size; drives on-screen geometry).
    cell_w: u32,
    cell_h: u32,
    /// The slot for the space glyph (used for blank base cells).
    space_slot: u32,
    /// Dynamic-atlas slot assignment (unused for the bitmap source, whose map is the font index).
    alloc: SlotAllocator,
    /// Slots rasterized since the last [`take_pending`](Self::take_pending), awaiting GPU upload.
    pending: Vec<(u32, Vec<u8>)>,
}

impl GlyphCache {
    /// A cache over a static bitmap font: the atlas is fully populated at init and the map is the
    /// font's fixed glyph index.
    pub(crate) fn bitmap(font: BitmapFont) -> Self {
        let cell_w = u32::from(font.glyph_width);
        let cell_h = u32::from(font.glyph_height);
        let geometry = AtlasGeometry::new(cell_w, cell_h, u32::from(font.glyph_count()));
        let space_slot = u32::from(font.char_to_index(' '));
        Self {
            source: FontSource::Bitmap(font),
            geometry,
            cell_w,
            cell_h,
            space_slot,
            alloc: SlotAllocator::new(0),
            pending: Vec::new(),
        }
    }

    /// A cache over a dynamic TTF font: an empty, LRU-managed atlas that rasterizes glyphs on first
    /// use. The space glyph is pre-assigned (its blank cell needs no upload).
    pub(crate) fn ttf(ttf: TtfFont) -> Self {
        let (cell_w, cell_h) = (ttf.cell_w, ttf.cell_h);
        Self::dynamic(FontSource::Ttf(ttf), cell_w, cell_h, DYNAMIC_CAPACITY)
    }

    /// Shared constructor for the dynamic sources (TTF and, in tests, synthetic).
    fn dynamic(source: FontSource, cell_w: u32, cell_h: u32, capacity: u32) -> Self {
        let geometry = AtlasGeometry::new(cell_w, cell_h, capacity);
        let mut cache = Self {
            source,
            geometry,
            cell_w,
            cell_h,
            space_slot: 0,
            alloc: SlotAllocator::new(geometry.capacity()),
            pending: Vec::new(),
        };
        // Reserve a slot for space (blank, so its queued upload is all-zero and harmless).
        cache.space_slot = cache.resolve(' ');
        cache
    }

    /// Glyph cell size in unscaled pixels.
    pub(crate) const fn cell_size(&self) -> (u32, u32) {
        (self.cell_w, self.cell_h)
    }

    /// The atlas slot of the space glyph.
    pub(crate) const fn space_slot(&self) -> u32 {
        self.space_slot
    }

    /// The backing bitmap font, if this is the static bitmap source (used by the Linux headless
    /// render test to compare against the font's own coverage bits).
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) const fn bitmap_font(&self) -> Option<&BitmapFont> {
        match &self.source {
            FontSource::Bitmap(font) => Some(font),
            _ => None,
        }
    }

    /// The initial full-atlas coverage to upload once at resource build: the whole bitmap atlas, or
    /// a zeroed dynamic atlas that per-glyph uploads then fill in.
    pub(crate) fn initial_atlas(&self) -> AtlasData {
        if let FontSource::Bitmap(font) = &self.source {
            AtlasData::build(font)
        } else {
            AtlasData {
                geometry: self.geometry,
                coverage: vec![
                    0u8;
                    (self.geometry.tex_w() * self.geometry.tex_h() * self.geometry.layers)
                        as usize
                ],
            }
        }
    }

    /// Resolves `ch` to its atlas slot, rasterizing and queuing an upload on a dynamic-atlas miss.
    pub(crate) fn resolve(&mut self, ch: char) -> u32 {
        if let FontSource::Bitmap(font) = &self.source {
            return u32::from(font.char_to_index(ch));
        }
        let (slot, was_miss) = self.alloc.get_or_assign(ch);
        if was_miss {
            let coverage = self.rasterize(ch);
            self.pending.push((slot, coverage));
        }
        slot
    }

    /// Rasterizes `ch` into a `cell_w * cell_h` coverage cell for the active dynamic source.
    fn rasterize(&self, ch: char) -> Vec<u8> {
        match &self.source {
            FontSource::Ttf(ttf) => ttf.rasterize_cell(ch),
            #[cfg(test)]
            FontSource::Synthetic { cell_w, cell_h } => {
                let fill = if ch == ' ' { 0 } else { 0xFF };
                vec![fill; (cell_w * cell_h) as usize]
            }
            FontSource::Bitmap(_) => unreachable!("rasterize is only reached for dynamic sources"),
        }
    }

    /// Drains the slots rasterized since the last call, for the renderer to upload. Empty for the
    /// bitmap source.
    pub(crate) fn take_pending(&mut self) -> Vec<(u32, Vec<u8>)> {
        core::mem::take(&mut self.pending)
    }

    /// Re-queues every currently-cached glyph for upload, e.g. after a WebGL2 context loss rebuilt
    /// the (now-empty) atlas texture. No-op for the bitmap source, whose atlas is rebuilt whole.
    pub(crate) fn requeue_all(&mut self) {
        if matches!(self.source, FontSource::Bitmap(_)) {
            return;
        }
        let mut repop: Vec<(u32, Vec<u8>)> = self
            .alloc
            .entries()
            .into_iter()
            .map(|(ch, slot)| (slot, self.rasterize(ch)))
            .collect();
        self.pending.append(&mut repop);
    }

    /// A dynamic cache backed by the test-only synthetic source (full-coverage cells).
    #[cfg(test)]
    pub(crate) fn synthetic(cell_w: u32, cell_h: u32, capacity: u32) -> Self {
        Self::dynamic(
            FontSource::Synthetic { cell_w, cell_h },
            cell_w,
            cell_h,
            capacity,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{FontSource, GlyphCache, SlotAllocator};

    #[test]
    fn slot_allocator_grows_then_evicts_least_recently_used() {
        let mut a = SlotAllocator::new(2);
        // Fresh assignments grow into slots 0, 1.
        assert_eq!(a.get_or_assign('x'), (0, true));
        assert_eq!(a.get_or_assign('y'), (1, true));
        // A hit does not allocate and reports no miss.
        assert_eq!(a.get_or_assign('x'), (0, false));
        // Now 'y' is least-recently-used; a new glyph evicts it and reuses its slot.
        assert_eq!(a.get_or_assign('z'), (1, true));
        // 'y' is gone (its next lookup is a fresh miss), 'x' survived.
        assert_eq!(a.get_or_assign('x'), (0, false));
        let (_, y_miss) = a.get_or_assign('y');
        assert!(y_miss, "evicted glyph should miss on re-lookup");
    }

    #[test]
    fn synthetic_cache_queues_one_upload_per_new_glyph_and_grows_slots() {
        // Space takes slot 0 during construction (blank upload).
        let mut cache = GlyphCache::synthetic(4, 4, 512);
        let _ = cache.take_pending(); // drop the space upload.

        let a = cache.resolve('A');
        let b = cache.resolve('B');
        assert_ne!(a, b, "distinct glyphs get distinct slots");
        // Re-resolving a cached glyph queues nothing new.
        assert_eq!(cache.resolve('A'), a);

        let pending = cache.take_pending();
        assert_eq!(pending.len(), 2, "one upload each for A and B");
        // Synthetic non-space cells are fully covered (4x4 of 0xFF).
        assert!(
            pending
                .iter()
                .all(|(_, cov)| cov.iter().all(|&b| b == 0xFF))
        );
        assert!(pending.iter().all(|(_, cov)| cov.len() == 16));
    }

    #[test]
    fn requeue_all_re_uploads_the_whole_working_set() {
        let mut cache = GlyphCache::synthetic(4, 4, 512);
        cache.resolve('A');
        cache.resolve('B');
        let _ = cache.take_pending();
        // After a context-loss rebuild, every cached glyph (space + A + B) must be re-queued.
        cache.requeue_all();
        assert_eq!(cache.take_pending().len(), 3);
    }

    #[cfg(feature = "default-font")]
    #[test]
    fn bitmap_cache_maps_char_to_font_index_without_pending() {
        use retroglyph_window::font::unscii16;
        let mut cache = GlyphCache::bitmap(unscii16::FONT);
        assert_eq!(
            cache.resolve('A'),
            u32::from(unscii16::FONT.char_to_index('A'))
        );
        // Static font: nothing is rasterized at run time.
        assert!(cache.take_pending().is_empty());
        assert!(matches!(cache.source, FontSource::Bitmap(_)));
    }

    #[test]
    fn ttf_rejects_invalid_font_bytes() {
        assert!(super::TtfFont::new(b"not a font", 16.0).is_err());
    }

    /// Tiny5 (SIL OFL 1.1); see `crates/gl/testdata/Tiny5-OFL.txt`.
    const TINY5: &[u8] = include_bytes!("../testdata/Tiny5-Regular.ttf");

    #[test]
    fn ttf_rasterizes_a_glyph_within_its_cell_and_space_is_blank() {
        let ttf = super::TtfFont::new(TINY5, 16.0).expect("Tiny5 parses");
        assert!(ttf.cell_w > 0 && ttf.cell_h > 0);
        let cell_len = (ttf.cell_w * ttf.cell_h) as usize;

        let a = ttf.rasterize_cell('A');
        assert_eq!(a.len(), cell_len, "coverage fills exactly one cell");
        assert!(a.iter().any(|&b| b != 0), "'A' must have visible coverage");

        let space = ttf.rasterize_cell(' ');
        assert!(space.iter().all(|&b| b == 0), "space must be blank");
    }

    #[test]
    fn ttf_cache_queues_one_upload_per_new_glyph() {
        let mut cache = GlyphCache::ttf(super::TtfFont::new(TINY5, 16.0).unwrap());
        let _ = cache.take_pending(); // drop the reserved space upload.
        let a = cache.resolve('A');
        let pending = cache.take_pending();
        assert_eq!(pending.len(), 1, "one upload for the newly-seen 'A'");
        assert_eq!(pending[0].0, a);
        assert!(pending[0].1.iter().any(|&b| b != 0));
        // Re-resolving is a cache hit: no new upload.
        assert_eq!(cache.resolve('A'), a);
        assert!(cache.take_pending().is_empty());
    }
}
