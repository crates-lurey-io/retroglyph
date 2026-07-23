//! 19: Overworld
//!
//! A scrollable camera over a large, procedurally generated high-fantasy map.
//!
//! [`Camera`] onto a hand-rolled terrain generator: domain-warped value noise builds elevation,
//! moisture, and temperature fields, calibrated by quantile and classified into two dozen
//! biomes (from jungle and savanna through taiga, glacier, darkwood, and rare enchanted or
//! blighted blobs), rendered with hillshaded relief, depth-graded animated water, and coastal
//! foam. Rivers carve downhill to the sea (pooling into lakes, widening into estuaries), lava
//! spills from volcanoes, trade roads and bridges link procedurally named villages and cities,
//! and named regions, dragon lairs, wizard spires, faerie rings, and dark spires round out the
//! high fantasy. No RNG crate anywhere -- every value comes from an integer hash of its
//! coordinates, so the same seed always produces the same world (see the `noise`/`world`
//! modules below).
//!
//! `T` cycles the main map through five views. Beyond the per-cell terrain above, two strategic
//! "tiled" views (square provinces and a staggered hex honeycomb) aggregate each 16x8-cell
//! block of the same world into one bevel-shaded tile -- majority biome, the most notable
//! landmark, and road/river connections toward like neighbors survive the zoom-out; per-cell
//! detail deliberately doesn't. That's the abstraction bargain a strategy game makes: a tile is
//! "mostly forest, has a village, road east", not 128 individually animated cells. Two "grid"
//! overlay views make the opposite bargain: they keep the terrain at full per-cell fidelity and
//! draw the boundaries of the same square/hex provinces over it as background-tinted lines (one
//! cell thick, hex edges as clean staircases), with the reticle's province outlined and its
//! neighbors ringed -- the "detailed map with a hex grid" look. Hex layout and adjacency
//! (offset-to-axial conversion, the six neighbor directions for connectors, hex distance for
//! the reticle's ring highlight) come from the `hexal` crate in all four.
//!
//! At or above [`BP_SIDEBAR`] columns, an info sidebar opens: coordinates, the biome/landmark
//! under the reticle, elevation, a live minimap (rendered at double vertical resolution via
//! [`retroglyph_core::subcell::quantize_half_block`], the same subcell technique
//! `16_subcell_image` uses for raster images, applied here to a proc-gen color field instead) and
//! a glyph legend. Below it, chrome collapses to a single status line so the map still reads on a
//! narrow terminal.
//!
//! ```sh
//! cargo run --example 19_overworld --features crossterm
//! cargo run --example 19_overworld --features software
//! cargo run --example 19_overworld  # headless fallback, prints a few frames to stdout
//! ```
//!
//! # Controls
//!
//! - Arrow keys / WASD: pan the camera one cell (one tile in the tile views); hold Shift to
//!   pan 8 cells (4 tiles) at a time
//! - `T`: cycle the map view: terrain cells / square tiles / square grid / hex tiles / hex grid
//! - Drag the map with the mouse, or scroll the wheel: pan the camera
//! - Click or drag the sidebar minimap: jump the camera straight to that point on the map
//! - `R`: regenerate the world with a new seed
//! - `Home`: recenter on the world's origin
//! - `Q` / Escape: quit

#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    // Terrain-generation formulas here are written to read as formulas (the whole point of a
    // hand-rolled noise/hydrology demo) -- mechanically rewriting every `a * b + c` into
    // `a.mul_add(b, c)` trades that readability for a marginal precision/perf win nothing here
    // needs.
    clippy::suboptimal_flops,
    // `dzdx`/`dzdy` (hillshade), `nx`/`ny` (noise coordinates): short, paired names that are
    // more readable next to each other than any longer alternative would be.
    clippy::similar_names,
    // `Poi::weight`'s rarity table intentionally repeats small integers across unrelated kinds
    // (city/dragon lair both 1, several kinds share 3 or 4) -- that's the table being read, not
    // an accident to collapse into or-patterns.
    clippy::match_same_arms,
    // `Poi::fits`'s biome-eligibility grid is laid out one `(kind, biomes)` pair per line
    // deliberately, so it reads as a table; nesting the or-patterns would defeat that.
    clippy::unnested_or_patterns,
    // `mix`'s bit-mixer constants (`^ 61`, `>> 16`, ...) are the well-known Wang/Jenkins
    // reference values; hex-ifying the small ones would just make it harder to check against
    // that reference.
    clippy::decimal_bitwise_operands,
    // `noise`/`world`'s items are crate-internal, so `pub(crate)` is the correct visibility
    // (`unreachable_pub` agrees). The nursery `redundant_pub_crate` lint disagrees only because
    // these modules aren't themselves `pub`; the two lints conflict for this private-module
    // pattern, same as `crates/software/src/surface_native.rs`, and `pub(crate)` is the honest
    // choice.
    clippy::redundant_pub_crate
)]

mod noise {
    //! Deterministic, RNG-free value noise.
    //!
    //! Every function here is a pure function of its integer/float coordinates and a `seed`: no
    //! `rand` crate, no mutable generator state, nothing that could make the same seed produce a
    //! different world on two different runs (or two different platforms). [`fbm`] layers a
    //! handful of octaves of [`value_noise2`] the usual way (each octave halves in amplitude and
    //! doubles in frequency), which is all [`super::world`] needs for elevation/moisture/
    //! temperature fields that read as smooth, continuous terrain rather than per-cell static.

    /// A cheap integer hash (a Wang/Jenkins-style bit-mixer): scrambles `x` well enough that
    /// adjacent inputs produce uncorrelated outputs, without needing a real hashing crate.
    const fn mix(mut x: u32) -> u32 {
        x = (x ^ 61) ^ (x >> 16);
        x = x.wrapping_add(x << 3);
        x ^= x >> 4;
        x = x.wrapping_mul(0x27d4_eb2d);
        x ^= x >> 15;
        x
    }

    /// A deterministic pseudo-random value in `[-1, 1]` for one integer lattice point, varying by
    /// `seed` so different fields (elevation, moisture, ...) sampled at the same coordinates
    /// don't correlate.
    fn lattice(xi: i32, yi: i32, seed: u32) -> f32 {
        let h = mix((xi as u32).wrapping_mul(0x1f1f_1f1f)
            ^ (yi as u32).wrapping_mul(0x9E37_79B9)
            ^ seed.wrapping_mul(0x85EB_CA6B));
        (h as f32 / f32::from(u16::MAX) / f32::from(u16::MAX)) * 2.0 - 1.0
    }

    /// Smoothstep (3t² - 2t³): the usual interpolation curve for value noise, giving flat
    /// tangents at lattice points so the field has no visible creases at integer boundaries.
    fn smoothstep(t: f32) -> f32 {
        t * t * (3.0 - 2.0 * t)
    }

    /// Bilinear-interpolated value noise at `(x, y)`, in roughly `[-1, 1]`.
    fn value_noise2(x: f32, y: f32, seed: u32) -> f32 {
        let (x0, y0) = (x.floor(), y.floor());
        let (xi, yi) = (x0 as i32, y0 as i32);
        let tx = smoothstep(x - x0);
        let ty = smoothstep(y - y0);

        let v00 = lattice(xi, yi, seed);
        let v10 = lattice(xi + 1, yi, seed);
        let v01 = lattice(xi, yi + 1, seed);
        let v11 = lattice(xi + 1, yi + 1, seed);

        let a = v00 + (v10 - v00) * tx;
        let b = v01 + (v11 - v01) * tx;
        a + (b - a) * ty
    }

    /// Fractal Brownian motion: `octaves` layers of [`value_noise2`], each doubling frequency and
    /// halving amplitude, normalized back to roughly `[-1, 1]`.
    pub(crate) fn fbm(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
        let mut amp = 1.0;
        let mut freq = 1.0;
        let mut sum = 0.0;
        let mut norm = 0.0;
        for o in 0..octaves {
            sum += value_noise2(x * freq, y * freq, seed.wrapping_add(o.wrapping_mul(1013))) * amp;
            norm += amp;
            amp *= 0.5;
            freq *= 2.0;
        }
        if norm > 0.0 { sum / norm } else { 0.0 }
    }

    /// Ridged noise: `1 - |fbm|`, folded so valleys become sharp ridges instead of smooth bumps
    /// -- used for mountain spines, which plain `fbm` renders as rounded hills, not ranges.
    pub(crate) fn ridge(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
        1.0 - fbm(x, y, octaves, seed).abs()
    }

    /// Domain-warped fbm: perturbs the sample point by two auxiliary fbm fields before sampling.
    /// Plain fbm reads as "lumpy static" at continent scale; warping shears and swirls the field
    /// so coastlines get peninsulas, inlets, and curved island arcs instead of round blobs. The
    /// standard trick from Inigo Quilez's "domain warping" writeup, done with one level of warp.
    pub(crate) fn warped_fbm(x: f32, y: f32, octaves: u32, seed: u32, warp: f32) -> f32 {
        let qx = fbm(x + 17.3, y + 41.1, 3, seed ^ 0x9E37_79B9);
        let qy = fbm(x + 91.2, y + 57.7, 3, seed ^ 0x85EB_CA6B);
        fbm(x + qx * warp, y + qy * warp, octaves, seed)
    }

    /// A stable per-cell hash in `[0, 1)`, for placement decisions (rivers sources, points of
    /// interest, decorative texture) that should be reproducible but not spatially smooth the way
    /// [`fbm`] is.
    pub(crate) fn hash01(x: i32, y: i32, seed: u32) -> f32 {
        let h =
            mix((x as u32).wrapping_mul(0xC2B2_AE35) ^ (y as u32).wrapping_mul(0x27D4_EB2F) ^ seed);
        h as f32 / u32::MAX as f32
    }

    #[cfg(test)]
    // Exact float equality is the point of these determinism checks: the same inputs must
    // produce bit-identical output, not merely "close" output.
    #[allow(clippy::float_cmp)]
    mod tests {
        use super::*;

        #[test]
        fn same_seed_same_coords_is_deterministic() {
            assert_eq!(fbm(3.7, 1.2, 4, 42), fbm(3.7, 1.2, 4, 42));
            assert_eq!(hash01(10, 20, 5), hash01(10, 20, 5));
        }

        #[test]
        fn different_seeds_diverge() {
            assert_ne!(fbm(3.7, 1.2, 4, 42), fbm(3.7, 1.2, 4, 43));
        }

        #[test]
        fn fbm_stays_in_bounds() {
            for i in 0..200_i32 {
                let x = i as f32 * 0.37;
                let y = i as f32 * 0.91;
                let v = fbm(x, y, 5, 7);
                assert!(
                    (-1.5..=1.5).contains(&v),
                    "fbm({x}, {y}) = {v} out of range"
                );
            }
        }

        #[test]
        fn hash01_stays_in_unit_range() {
            for i in 0..500_i32 {
                let v = hash01(i, -i, 99);
                assert!((0.0..1.0).contains(&v), "hash01 = {v} out of range");
            }
        }
    }
}

mod world {
    //! Procedural world generation and rendering: domain-warped elevation, quantile-calibrated
    //! biome bands, hillshaded relief rendering, downhill-traced rivers (with lakes and widened
    //! estuaries), lava flows, roads and bridges linking settlements, scattered points of
    //! interest, and procedurally named regions and settlements.
    //!
    //! Everything expensive is computed once in [`World::generate`] and cached in flat `Vec`s
    //! indexed by [`idx`] -- the per-frame [`World::render_cell`] call (one per visible camera
    //! cell) only ever does array lookups plus a couple of cheap per-cell hash calls for
    //! decorative texture, so panning stays fast regardless of how large [`WORLD_W`]/[`WORLD_H`]
    //! are.
    //!
    //! Biome thresholds aren't fixed constants: after the elevation field is built, sea level,
    //! hill line, mountain line, and peak line are read off the field's own quantiles, so every
    //! seed lands near the same land/water/mountain proportions no matter what the noise happens
    //! to produce (see [`World::generate`]).

    use std::collections::HashMap;

    use retroglyph_core::subcell::{Glyph, Rgb, quantize_half_block};
    use retroglyph_core::{Color, Pos, Style};

    use super::noise::{fbm, hash01, ridge, warped_fbm};

    const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::Rgb { r, g, b }
    }

    /// Extracts the `(r, g, b)` triple from a [`Color::Rgb`] -- every color this module hands to
    /// [`quantize_half_block`] is one, but the fallback keeps this total instead of panicking if
    /// that ever changes.
    const fn to_rgb(color: Color) -> Rgb {
        match color {
            Color::Rgb { r, g, b } => (r, g, b),
            _ => (0, 0, 0),
        }
    }

    /// World width, in cells.
    ///
    /// Deliberately much larger than any terminal or desktop window is likely to show at once (a
    /// maximized window at a small font on a 4K display might show on the order of 300-400
    /// columns) so panning always has somewhere new to go, on every backend.
    pub const WORLD_W: u16 = 420;
    /// World height, in cells; see [`WORLD_W`].
    pub const WORLD_H: u16 = 230;

    const _: () = assert!(
        WORLD_W as u32 * WORLD_H as u32 > 60_000,
        "world must stay comfortably larger than any real terminal/window or panning has nowhere \
         to go once the window fills the screen"
    );

    /// The near-black the whole scene fades toward: cell backgrounds are the biome color lerped
    /// most of the way to this, so the map reads as a lit painting rather than glyphs on void.
    const NIGHT: Color = rgb(6, 8, 13);

    // ── Biomes ───────────────────────────────────────────────────────────────

    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub(crate) enum Biome {
        DeepOcean,
        Shallows,
        River,
        Lake,
        Lava,
        Beach,
        Desert,
        Savanna,
        Plains,
        Swamp,
        Jungle,
        Forest,
        DarkForest,
        EnchantedForest,
        Taiga,
        Tundra,
        Glacier,
        Hills,
        Mountains,
        SnowPeak,
        VolcanicPeak,
        Ashland,
        Blight,
    }

    impl Biome {
        pub(crate) const fn label(self) -> &'static str {
            match self {
                Self::DeepOcean => "Deep ocean",
                Self::Shallows => "Coastal waters",
                Self::River => "River",
                Self::Lake => "Lake",
                Self::Lava => "Molten flow",
                Self::Beach => "Shore",
                Self::Desert => "Desert",
                Self::Savanna => "Savanna",
                Self::Plains => "Plains",
                Self::Swamp => "Swamp",
                Self::Jungle => "Jungle",
                Self::Forest => "Forest",
                Self::DarkForest => "Darkwood",
                Self::EnchantedForest => "Enchanted wood",
                Self::Taiga => "Taiga",
                Self::Tundra => "Tundra",
                Self::Glacier => "Glacier",
                Self::Hills => "Hills",
                Self::Mountains => "Mountains",
                Self::SnowPeak => "Snowcap",
                Self::VolcanicPeak => "Volcano",
                Self::Ashland => "Ashland",
                Self::Blight => "Blighted land",
            }
        }

        const fn is_water(self) -> bool {
            matches!(
                self,
                Self::DeepOcean | Self::Shallows | Self::River | Self::Lake
            )
        }

        /// Base color, before hillshading and per-cell decorative variance (see
        /// [`World::render_cell`]).
        const fn color(self) -> Color {
            match self {
                Self::DeepOcean => rgb(14, 32, 74),
                Self::Shallows => rgb(40, 92, 146),
                Self::River => rgb(62, 132, 194),
                Self::Lake => rgb(50, 118, 178),
                Self::Lava => rgb(228, 86, 26),
                Self::Beach => rgb(216, 198, 148),
                Self::Desert => rgb(206, 176, 108),
                Self::Savanna => rgb(172, 160, 76),
                Self::Plains => rgb(112, 156, 74),
                Self::Swamp => rgb(76, 94, 60),
                Self::Jungle => rgb(28, 124, 62),
                Self::Forest => rgb(46, 102, 56),
                Self::DarkForest => rgb(26, 58, 40),
                Self::EnchantedForest => rgb(124, 90, 198),
                Self::Taiga => rgb(54, 98, 84),
                Self::Tundra => rgb(168, 178, 176),
                Self::Glacier => rgb(198, 224, 238),
                Self::Hills => rgb(126, 126, 80),
                Self::Mountains => rgb(120, 114, 108),
                Self::SnowPeak => rgb(238, 242, 248),
                Self::VolcanicPeak => rgb(112, 66, 58),
                Self::Ashland => rgb(78, 70, 70),
                Self::Blight => rgb(74, 40, 84),
            }
        }

        /// Picks a glyph/color pair for one land cell, given a stable `[0, 1)` decorative hash
        /// and (for biomes with an animated flourish) the running clock. Water is handled
        /// separately in [`World::render_cell`], where depth and shoreline context are available.
        fn glyph(self, texture: f32, time: f64) -> (char, Color) {
            let c = self.color();
            match self {
                // Water fallbacks -- render_cell normally intercepts these with depth shading.
                Self::DeepOcean | Self::Shallows | Self::Lake => ('≈', c),
                Self::River => ('~', c),
                Self::Lava => {
                    let glow = ((time * 3.1 + f64::from(texture) * 11.0).sin() * 0.5 + 0.5) as f32;
                    ('~', Color::lerp(c, rgb(255, 206, 66), glow * 0.55))
                }
                Self::Beach => (if texture < 0.12 { ':' } else { '.' }, c),
                Self::Desert => {
                    if texture < 0.02 {
                        ('↑', rgb(108, 140, 70)) // rare cactus
                    } else if texture < 0.2 {
                        ('~', Color::lerp(c, Color::WHITE, 0.12)) // wind-rippled dune crest
                    } else if texture < 0.4 {
                        ('░', Color::lerp(c, Color::WHITE, 0.06))
                    } else {
                        ('.', c)
                    }
                }
                Self::Savanna => {
                    if texture < 0.035 {
                        ('τ', rgb(96, 118, 52)) // lone acacia
                    } else if texture < 0.5 {
                        ('"', c)
                    } else {
                        (',', Color::lerp(c, rgb(140, 120, 60), 0.3))
                    }
                }
                Self::Plains => {
                    if texture < 0.02 {
                        // scattered wildflowers, tinted by their own hash bits
                        let bloom = if texture < 0.007 {
                            rgb(226, 170, 210)
                        } else {
                            rgb(232, 214, 120)
                        };
                        ('*', bloom)
                    } else if texture < 0.07 {
                        ('.', Color::lerp(c, rgb(96, 76, 40), 0.5))
                    } else if texture < 0.5 {
                        ('"', c)
                    } else {
                        (',', Color::lerp(c, Color::BLACK, 0.12))
                    }
                }
                Self::Swamp => {
                    if texture < 0.14 {
                        ('~', Color::lerp(c, Color::BLACK, 0.35))
                    } else if texture < 0.55 {
                        (':', c)
                    } else {
                        ('"', Color::lerp(c, rgb(132, 128, 60), 0.3))
                    }
                }
                Self::Jungle => {
                    if texture < 0.08 {
                        ('§', rgb(60, 160, 70)) // hanging vines
                    } else if texture < 0.6 {
                        ('♣', Color::lerp(c, rgb(70, 190, 90), texture * 0.4))
                    } else {
                        ('♠', Color::lerp(c, rgb(16, 88, 44), 0.4))
                    }
                }
                Self::Forest => (
                    if texture < 0.08 {
                        ','
                    } else if texture < 0.55 {
                        '♠'
                    } else {
                        '♣'
                    },
                    Color::lerp(c, rgb(76, 152, 82), texture * 0.3),
                ),
                Self::DarkForest => {
                    if texture < 0.05 {
                        ('.', rgb(70, 66, 58)) // bare ground between old trunks
                    } else {
                        ('♠', Color::lerp(c, rgb(12, 30, 22), texture * 0.5))
                    }
                }
                Self::EnchantedForest => {
                    let twinkle =
                        ((time * 1.7 + f64::from(texture) * 23.0).sin() * 0.5 + 0.5) as f32;
                    if texture < 0.06 && twinkle > 0.82 {
                        ('☼', rgb(230, 200, 250))
                    } else if texture < 0.5 {
                        ('♣', Color::lerp(c, rgb(90, 220, 210), twinkle * 0.3))
                    } else {
                        ('♠', c)
                    }
                }
                Self::Taiga => {
                    if texture < 0.1 {
                        ('♠', Color::lerp(c, Color::WHITE, 0.35)) // snow-dusted crown
                    } else {
                        (
                            if texture < 0.55 { '♠' } else { '♣' },
                            Color::lerp(c, rgb(22, 72, 70), texture * 0.3),
                        )
                    }
                }
                Self::Tundra => (if texture < 0.2 { ',' } else { '.' }, c),
                Self::Glacier => {
                    let gleam = ((time * 0.9 + f64::from(texture) * 31.0).sin() * 0.5 + 0.5) as f32;
                    if texture < 0.04 && gleam > 0.85 {
                        ('∙', Color::WHITE)
                    } else {
                        (
                            if texture < 0.6 { '░' } else { '▒' },
                            Color::lerp(c, Color::WHITE, texture * 0.25),
                        )
                    }
                }
                Self::Hills => (
                    if texture < 0.35 { '∩' } else { '"' },
                    Color::lerp(c, rgb(96, 100, 54), texture * 0.3),
                ),
                Self::Mountains => ('▲', Color::lerp(c, rgb(92, 88, 92), texture * 0.35)),
                Self::SnowPeak => ('▲', Color::lerp(c, rgb(200, 214, 236), texture * 0.3)),
                Self::VolcanicPeak => ('▲', Color::lerp(c, rgb(210, 90, 30), texture * 0.4)),
                Self::Ashland => {
                    let ember = ((time * 2.2 + f64::from(texture) * 17.0).sin() * 0.5 + 0.5) as f32;
                    if texture < 0.05 {
                        (
                            '∙',
                            Color::lerp(rgb(120, 60, 40), rgb(255, 140, 40), ember * 0.8),
                        )
                    } else {
                        (
                            if texture < 0.5 { '░' } else { '.' },
                            Color::lerp(c, rgb(40, 36, 38), texture * 0.4),
                        )
                    }
                }
                Self::Blight => {
                    let pulse = ((time * 1.1 + f64::from(texture) * 13.0).sin() * 0.5 + 0.5) as f32;
                    if texture < 0.05 {
                        ('∙', Color::lerp(rgb(150, 40, 90), rgb(230, 80, 150), pulse))
                    } else if texture < 0.2 {
                        ('~', Color::lerp(c, rgb(120, 30, 70), pulse * 0.5))
                    } else if texture < 0.6 {
                        ('"', Color::lerp(c, rgb(30, 12, 40), texture * 0.5))
                    } else {
                        (',', Color::lerp(c, Color::BLACK, 0.3))
                    }
                }
            }
        }
    }

    // ── Points of interest ────────────────────────────────────────────────────

    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub(crate) enum Poi {
        City,
        Village,
        Watchtower,
        WizardTower,
        Ruins,
        StandingStone,
        FaerieRing,
        GemVein,
        DragonLair,
        DarkSpire,
        Waystone,
    }

    impl Poi {
        pub(crate) const fn label(self) -> &'static str {
            match self {
                Self::City => "City",
                Self::Village => "Village",
                Self::Watchtower => "Watchtower",
                Self::WizardTower => "Wizard's spire",
                Self::Ruins => "Ancient ruins",
                Self::StandingStone => "Standing stone",
                Self::FaerieRing => "Faerie ring",
                Self::GemVein => "Gem vein",
                Self::DragonLair => "Dragon's lair",
                Self::DarkSpire => "Dark spire",
                Self::Waystone => "Waystone",
            }
        }

        pub(crate) const fn glyph_color(self) -> (char, Color) {
            match self {
                Self::City => ('■', rgb(246, 214, 120)),
                Self::Village => ('⌂', rgb(224, 176, 96)),
                Self::Watchtower => ('T', rgb(248, 198, 90)),
                Self::WizardTower => ('Φ', rgb(140, 160, 255)),
                Self::Ruins => ('Ω', rgb(200, 196, 180)),
                Self::StandingStone => ('π', rgb(190, 182, 210)),
                Self::FaerieRing => ('○', rgb(240, 170, 220)),
                Self::GemVein => ('♦', rgb(120, 230, 220)),
                Self::DragonLair => ('δ', rgb(240, 110, 60)),
                Self::DarkSpire => ('I', rgb(190, 70, 160)),
                Self::Waystone => ('Θ', rgb(110, 210, 190)),
            }
        }

        /// Whether `biome` is ground this landmark can sit on.
        const fn fits(self, biome: Biome) -> bool {
            use Biome as B;
            matches!(
                (self, biome),
                (Self::City, B::Plains | B::Beach | B::Savanna)
                    | (Self::Village, B::Plains | B::Beach | B::Savanna)
                    | (Self::Watchtower, B::Hills | B::Mountains)
                    | (
                        Self::WizardTower,
                        B::Plains | B::Forest | B::Hills | B::Tundra
                    )
                    | (
                        Self::Ruins,
                        B::Plains
                            | B::Desert
                            | B::Forest
                            | B::Taiga
                            | B::Swamp
                            | B::Savanna
                            | B::Jungle
                    )
                    | (
                        Self::StandingStone,
                        B::Hills | B::Tundra | B::Plains | B::Savanna
                    )
                    | (Self::FaerieRing, B::EnchantedForest | B::Forest)
                    | (Self::GemVein, B::Mountains | B::Hills | B::Ashland)
                    | (
                        Self::DragonLair,
                        B::Mountains | B::VolcanicPeak | B::Ashland
                    )
                    | (Self::DarkSpire, B::Blight)
                    | (Self::Waystone, B::Plains | B::Hills | B::Tundra | B::Desert)
            )
        }

        /// Relative pick weight among the kinds that fit a cell's biome: cities and dragon lairs
        /// stay rare, villages and ruins common.
        const fn weight(self) -> u32 {
            match self {
                Self::City => 1,
                Self::Village => 8,
                Self::Watchtower => 4,
                Self::WizardTower => 2,
                Self::Ruins => 5,
                Self::StandingStone => 4,
                Self::FaerieRing => 3,
                Self::GemVein => 4,
                Self::DragonLair => 1,
                Self::DarkSpire => 3,
                Self::Waystone => 3,
            }
        }

        /// Whether this landmark is a settlement -- named, and linked into the road network.
        const fn is_settlement(self) -> bool {
            matches!(self, Self::City | Self::Village)
        }
    }

    /// The candidate pool [`World::scatter_pois`] draws from, weighted by [`Poi::weight`].
    const POI_KINDS: [Poi; 11] = [
        Poi::City,
        Poi::Village,
        Poi::Watchtower,
        Poi::WizardTower,
        Poi::Ruins,
        Poi::StandingStone,
        Poi::FaerieRing,
        Poi::GemVein,
        Poi::DragonLair,
        Poi::DarkSpire,
        Poi::Waystone,
    ];

    // ── Name generation ──────────────────────────────────────────────────────

    const NAME_PRE: [&str; 20] = [
        "Thorn", "Eld", "Vael", "Mor", "Cael", "Bryn", "Dun", "Hal", "Ker", "Wyn", "Ash", "Fen",
        "Gray", "Stone", "Oak", "Raven", "Wolf", "Ember", "Frost", "Salt",
    ];
    const NAME_SUF: [&str; 16] = [
        "holm", "wick", "mere", "fell", "stead", "ford", "haven", "bury", "gate", "march",
        "hollow", "crest", "watch", "field", "brook", "moor",
    ];
    const REGION_ADJ: [&str; 24] = [
        "Ashen",
        "Gilded",
        "Whispering",
        "Sunken",
        "Verdant",
        "Shattered",
        "Silent",
        "Amber",
        "Frozen",
        "Weeping",
        "Elder",
        "Howling",
        "Radiant",
        "Forgotten",
        "Thorned",
        "Misty",
        "Iron",
        "Pale",
        "Wild",
        "Starlit",
        "Bleak",
        "Emerald",
        "Crimson",
        "Sundered",
    ];
    const REGION_NOUN: [&str; 16] = [
        "Reach",
        "Wilds",
        "Expanse",
        "Marches",
        "Moors",
        "Vale",
        "Barrens",
        "Downs",
        "Fells",
        "Heath",
        "Lowlands",
        "Highlands",
        "Wold",
        "Steppe",
        "Hollows",
        "Coast",
    ];

    fn settlement_name(x: u16, y: u16, seed: u32) -> String {
        let a = hash01(i32::from(x), i32::from(y), seed ^ 0xD00D);
        let b = hash01(i32::from(y), i32::from(x), seed ^ 0xF00F);
        let pre = NAME_PRE[(a * NAME_PRE.len() as f32) as usize % NAME_PRE.len()];
        let suf = NAME_SUF[(b * NAME_SUF.len() as f32) as usize % NAME_SUF.len()];
        format!("{pre}{suf}")
    }

    fn region_name(k: usize, seed: u32) -> String {
        let a = hash01(k as i32, 71, seed ^ 0xABBA);
        let b = hash01(k as i32, 137, seed ^ 0xBEEF);
        let adj = REGION_ADJ[(a * REGION_ADJ.len() as f32) as usize % REGION_ADJ.len()];
        let noun = REGION_NOUN[(b * REGION_NOUN.len() as f32) as usize % REGION_NOUN.len()];
        format!("The {adj} {noun}")
    }

    // ── World ────────────────────────────────────────────────────────────────

    const fn idx(x: u16, y: u16) -> usize {
        y as usize * WORLD_W as usize + x as usize
    }

    /// The eight-neighborhood offsets used by the downhill walks and road builder.
    const NEIGHBORS8: [(i32, i32); 8] = [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ];

    /// Elevation quantiles the biome bands are read from, so land/water/mountain proportions stay
    /// stable across seeds regardless of what the raw noise range does.
    struct Thresholds {
        deep: f32,
        sea: f32,
        beach: f32,
        hills: f32,
        mountains: f32,
        peaks: f32,
    }

    /// A road-overlay cell: nothing, a dirt road, or a bridge where a road crosses water.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum RoadCell {
        None,
        Road,
        Bridge,
    }

    /// A fully generated, static world. Cheap to query per-cell (see [`World::render_cell`]);
    /// expensive-ish (though still well under a frame) to build, which is why [`World::generate`]
    /// runs once, not per draw.
    pub(crate) struct World {
        seed: u32,
        elevation: Vec<f32>,
        biome: Vec<Biome>,
        /// Precomputed hillshade multiplier per cell, ~`[0.72, 1.28]`, lit from the northwest.
        shade: Vec<f32>,
        road: Vec<RoadCell>,
        pois: HashMap<(u16, u16), Poi>,
        /// Names for settlements ([`Poi::is_settlement`]); other landmark kinds go unnamed.
        poi_names: HashMap<(u16, u16), String>,
        region_seeds: Vec<(u16, u16)>,
        region_names: Vec<String>,
        thresholds: Thresholds,
        e_min: f32,
        e_max: f32,
    }

    impl World {
        /// Builds a complete world from `seed`: every value is a pure function of `seed` and
        /// coordinates (see [`super::noise`]), so the same seed always regenerates byte-identical
        /// terrain.
        #[must_use]
        pub(crate) fn generate(seed: u32) -> Self {
            let n = WORLD_W as usize * WORLD_H as usize;
            let mut elevation = vec![0.0f32; n];
            for y in 0..WORLD_H {
                for x in 0..WORLD_W {
                    elevation[idx(x, y)] = Self::compute_elevation(x, y, seed);
                }
            }

            // Calibrate the biome bands against this world's actual elevation distribution.
            let mut sorted = elevation.clone();
            sorted.sort_by(f32::total_cmp);
            let q = |p: f32| sorted[(((sorted.len() - 1) as f32) * p) as usize];
            let thresholds = Thresholds {
                deep: q(0.14),
                sea: q(0.34),
                beach: q(0.365),
                hills: q(0.72),
                mountains: q(0.88),
                peaks: q(0.965),
            };
            let (e_min, e_max) = (sorted[0], sorted[sorted.len() - 1]);

            let mut biome = vec![Biome::Plains; n];
            for y in 0..WORLD_H {
                for x in 0..WORLD_W {
                    let i = idx(x, y);
                    biome[i] = Self::classify(x, y, elevation[i], &thresholds, seed);
                }
            }

            let mut world = Self {
                seed,
                elevation,
                biome,
                shade: Vec::new(),
                road: vec![RoadCell::None; n],
                pois: HashMap::new(),
                poi_names: HashMap::new(),
                region_seeds: Vec::new(),
                region_names: Vec::new(),
                thresholds,
                e_min,
                e_max,
            };
            world.trace_rivers();
            world.trace_lava();
            world.compute_shade(); // after rivers, so carved valleys read in the relief
            world.scatter_pois();
            world.build_roads();
            world.name_regions();
            world
        }

        #[must_use]
        pub(crate) const fn seed(&self) -> u32 {
            self.seed
        }

        #[must_use]
        pub(crate) fn biome_at(&self, pos: Pos) -> Biome {
            self.biome[idx(pos.x, pos.y)]
        }

        /// Elevation at `pos` as a percentage of this world's full range, for the sidebar
        /// readout.
        #[must_use]
        pub(crate) fn elevation_pct(&self, pos: Pos) -> f32 {
            let e = self.elevation[idx(pos.x, pos.y)];
            ((e - self.e_min) / (self.e_max - self.e_min).max(1e-6) * 100.0).clamp(0.0, 100.0)
        }

        #[must_use]
        pub(crate) fn poi_at(&self, pos: Pos) -> Option<Poi> {
            self.pois.get(&(pos.x, pos.y)).copied()
        }

        /// The name of the region `pos` falls in -- nearest-seed Voronoi over a jittered grid of
        /// named region seeds, so every part of the map belongs to exactly one "The Ashen Reach".
        #[must_use]
        pub(crate) fn region_at(&self, pos: Pos) -> &str {
            let k = self
                .region_seeds
                .iter()
                .enumerate()
                .min_by_key(|&(_, &(x, y))| dist_sq(pos.x, pos.y, x, y))
                .map_or(0, |(k, _)| k);
            self.region_names.get(k).map_or("Uncharted", String::as_str)
        }

        /// A reasonable place to point the camera on startup: the city (else village, else any
        /// landmark) nearest the world's center, or failing all of that the nearest dry land
        /// found by an expanding box search -- so the very first frame shows *something* rather
        /// than risking the world's exact center, which is just as likely to be open ocean.
        #[must_use]
        pub(crate) fn starting_view(&self) -> Pos {
            let center = (WORLD_W / 2, WORLD_H / 2);
            for want in [Some(Poi::City), Some(Poi::Village), None] {
                let best = self
                    .pois
                    .iter()
                    .filter(|&(_, &poi)| want.is_none_or(|w| poi == w))
                    .map(|(&pos, _)| pos)
                    .min_by_key(|&(x, y)| dist_sq(x, y, center.0, center.1));
                if let Some((x, y)) = best {
                    return Pos::new(x, y);
                }
            }
            for radius in 0..center.0.max(center.1) {
                let (lo_x, hi_x) = (
                    center.0.saturating_sub(radius),
                    (center.0 + radius).min(WORLD_W - 1),
                );
                let (lo_y, hi_y) = (
                    center.1.saturating_sub(radius),
                    (center.1 + radius).min(WORLD_H - 1),
                );
                for y in lo_y..=hi_y {
                    for x in lo_x..=hi_x {
                        let on_ring = y == lo_y || y == hi_y || x == lo_x || x == hi_x;
                        if on_ring && !self.biome[idx(x, y)].is_water() {
                            return Pos::new(x, y);
                        }
                    }
                }
            }
            Pos::new(center.0, center.1)
        }

        /// The label for whatever occupies `pos`: a named settlement ("Thornholm (Village)"), an
        /// unnamed landmark's kind, a road or bridge, or the underlying biome.
        #[must_use]
        pub(crate) fn label_at(&self, pos: Pos) -> String {
            if let Some(poi) = self.poi_at(pos) {
                return self.poi_names.get(&(pos.x, pos.y)).map_or_else(
                    || poi.label().to_owned(),
                    |name| format!("{name} ({})", poi.label()),
                );
            }
            match self.road[idx(pos.x, pos.y)] {
                RoadCell::Road => format!("Trade road ({})", self.biome_at(pos).label()),
                RoadCell::Bridge => "Bridge".to_owned(),
                RoadCell::None => self.biome_at(pos).label().to_owned(),
            }
        }

        /// The flat color used to represent `pos` on the minimap: a landmark's color if one sits
        /// there, depth-shaded water, or the biome's base color under this cell's hillshade --
        /// so the minimap reads as a little shaded-relief map rather than flat biome fills.
        fn swatch_color(&self, pos: Pos) -> Color {
            if let Some(poi) = self.poi_at(pos) {
                return poi.glyph_color().1;
            }
            let i = idx(pos.x, pos.y);
            let biome = self.biome[i];
            if biome.is_water() {
                return self.water_color(i, biome);
            }
            shade_color(biome.color(), self.shade[i])
        }

        /// One minimap cell at `(col, row)` of a `cols`x`rows` minimap, doubled to `rows * 2`
        /// vertical samples via [`quantize_half_block`] -- see `retroglyph_core::subcell` -- so a
        /// tiny sidebar minimap still resolves roughly twice the vertical detail a plain
        /// one-glyph-per-cell sampling would show.
        #[must_use]
        pub(crate) fn minimap_swatch(&self, col: u16, row: u16, cols: u16, rows: u16) -> Glyph {
            let sample = |mx: u16, my_half: u16| -> Rgb {
                let wx = (u32::from(mx) * u32::from(WORLD_W) / u32::from(cols))
                    .min(u32::from(WORLD_W) - 1);
                let wy = (u32::from(my_half) * u32::from(WORLD_H) / (u32::from(rows) * 2))
                    .min(u32::from(WORLD_H) - 1);
                to_rgb(self.swatch_color(Pos::new(wx as u16, wy as u16)))
            };
            quantize_half_block([sample(col, row * 2), sample(col, row * 2 + 1)])
        }

        /// A curated legend of glyph/color/label triples, ordered common-first so the entries
        /// that survive a short sidebar are the ones the player is most likely staring at.
        #[must_use]
        pub(crate) fn legend() -> Vec<(char, Color, &'static str)> {
            let biomes = [
                Biome::DeepOcean,
                Biome::River,
                Biome::Plains,
                Biome::Forest,
                Biome::Hills,
                Biome::Mountains,
                Biome::SnowPeak,
                Biome::Desert,
                Biome::Savanna,
                Biome::Jungle,
                Biome::Swamp,
                Biome::Taiga,
                Biome::Tundra,
                Biome::Glacier,
                Biome::DarkForest,
                Biome::EnchantedForest,
                Biome::Blight,
                Biome::Ashland,
                Biome::VolcanicPeak,
                Biome::Lava,
            ];
            let mut out: Vec<_> = biomes
                .into_iter()
                .map(|b| {
                    let (glyph, color) = b.glyph(0.6, 0.0);
                    (glyph, color, b.label())
                })
                .collect();
            for poi in POI_KINDS {
                let (glyph, color) = poi.glyph_color();
                out.push((glyph, color, poi.label()));
            }
            out
        }

        // ── Rendering ──────────────────────────────────────────────────────────

        /// Depth-graded water color for cell `i`: coastal shallows fade toward deep-ocean blue as
        /// elevation falls further below sea level, giving continuous bathymetry instead of two
        /// flat bands. Rivers and lakes stay their brighter fixed hue so they read against it.
        fn water_color(&self, i: usize, biome: Biome) -> Color {
            match biome {
                Biome::River | Biome::Lake => biome.color(),
                _ => {
                    let depth = ((self.thresholds.sea - self.elevation[i])
                        / (self.thresholds.sea - self.e_min).max(1e-6))
                    .clamp(0.0, 1.0)
                    .powf(0.65);
                    Color::lerp(Biome::Shallows.color(), Biome::DeepOcean.color(), depth)
                }
            }
        }

        /// Whether any 4-neighbor of `(x, y)` is dry land -- used to draw breaking foam along
        /// coastlines.
        fn touches_land(&self, x: u16, y: u16) -> bool {
            let mut any = false;
            if x > 0 {
                any |= !self.biome[idx(x - 1, y)].is_water();
            }
            if x + 1 < WORLD_W {
                any |= !self.biome[idx(x + 1, y)].is_water();
            }
            if y > 0 {
                any |= !self.biome[idx(x, y - 1)].is_water();
            }
            if y + 1 < WORLD_H {
                any |= !self.biome[idx(x, y + 1)].is_water();
            }
            any
        }

        /// The glyph and style to draw for `pos` at `time` (seconds since start, for animated
        /// flourishes: rolling swell, breaking foam, ember glow, the enchanted wood's twinkle).
        #[must_use]
        pub(crate) fn render_cell(&self, pos: Pos, time: f64) -> (char, Style) {
            let i = idx(pos.x, pos.y);
            let biome = self.biome[i];
            let texture = hash01(i32::from(pos.x), i32::from(pos.y), self.seed ^ 0xABCD);

            if let Some(poi) = self.poi_at(pos) {
                let (glyph, color) = poi.glyph_color();
                let ground = if biome.is_water() {
                    self.water_color(i, biome)
                } else {
                    shade_color(biome.color(), self.shade[i])
                };
                return (
                    glyph,
                    Style::new().fg(color).bg(Color::lerp(ground, NIGHT, 0.6)),
                );
            }

            match self.road[i] {
                RoadCell::Road => {
                    let ground = shade_color(biome.color(), self.shade[i]);
                    let bg = Color::lerp(Color::lerp(ground, NIGHT, 0.7), rgb(190, 168, 120), 0.16);
                    return ('·', Style::new().fg(rgb(200, 180, 138)).bg(bg));
                }
                RoadCell::Bridge => {
                    let bg = Color::lerp(self.water_color(i, biome), NIGHT, 0.55);
                    return ('=', Style::new().fg(rgb(158, 118, 74)).bg(bg));
                }
                RoadCell::None => {}
            }

            if biome.is_water() {
                let base = self.water_color(i, biome);
                // A slow diagonal swell rolling across all water, plus shimmer on rivers.
                let phase = time * 1.1
                    + f64::from(pos.x) * 0.35
                    + f64::from(pos.y) * 0.6
                    + f64::from(texture) * 6.0;
                let swell = (phase.sin() * 0.5 + 0.5) as f32;
                let mut fg = Color::lerp(base, rgb(178, 214, 244), 0.18 + swell * 0.22);
                let mut ch = if biome == Biome::River {
                    '~'
                } else if texture < 0.5 {
                    '≈'
                } else {
                    '~'
                };
                // Breaking foam where open water meets the shore.
                if biome != Biome::River && self.touches_land(pos.x, pos.y) {
                    let foam = ((time * 1.6 + f64::from(texture) * 40.0).sin() * 0.5 + 0.5) as f32;
                    if foam > 0.6 {
                        ch = '≈';
                        fg = Color::lerp(fg, Color::WHITE, (foam - 0.6) * 1.6);
                    }
                }
                let bg = Color::lerp(base, NIGHT, 0.55);
                return (ch, Style::new().fg(fg).bg(bg));
            }

            let (glyph, fg) = biome.glyph(texture, time);
            let shade = self.shade[i];
            let bg = shade_color(Color::lerp(biome.color(), NIGHT, 0.74), shade);
            (glyph, Style::new().fg(shade_color(fg, shade)).bg(bg))
        }

        // ── Generation steps ───────────────────────────────────────────────────

        /// Domain-warped continents + detail + ridged mountain spines, all faded out toward the
        /// map edge so the world reads as a proper fantasy-map landmass ringed by open ocean
        /// rather than terrain sliced off arbitrarily at the border.
        fn compute_elevation(x: u16, y: u16, seed: u32) -> f32 {
            let nx = f32::from(x) * 0.02;
            let ny = f32::from(y) * 0.02;
            let continent = warped_fbm(nx * 0.4, ny * 0.4, 4, seed, 0.85);
            let detail = fbm(nx, ny, 5, seed.wrapping_add(1));
            let mut base = continent * 1.1 + detail * 0.45;

            // Mountain ridges only rise out of land that's already trending upward -- the mask
            // shapes ranges across continents instead of uniformly lifting the whole map.
            let land_mask = (base * 2.2 + 0.3).clamp(0.0, 1.0);
            let ridge_n = ridge(nx * 1.8, ny * 1.8, 4, seed.wrapping_add(2));
            let ridge_boost = ((ridge_n - 0.6).max(0.0) / 0.4).powf(1.5);
            base += ridge_boost * land_mask * 1.4;

            // Edge falloff: blend the outer ~10% of the map down to below any interior elevation,
            // so every border is guaranteed sea no matter where the quantile calibration lands.
            let margin = f32::from(WORLD_H.min(WORLD_W)) * 0.10;
            let d_edge = f32::from(x)
                .min(f32::from(WORLD_W - 1 - x))
                .min(f32::from(y))
                .min(f32::from(WORLD_H - 1 - y));
            let t = (d_edge / margin).clamp(0.0, 1.0);
            let t = t * t * (3.0 - 2.0 * t);
            -1.7 + (base + 1.7) * t
        }

        fn moisture_at(x: u16, y: u16, seed: u32) -> f32 {
            let nx = f32::from(x) * 0.02;
            let ny = f32::from(y) * 0.02;
            (fbm(nx * 0.9 + 50.0, ny * 0.9 + 50.0, 4, seed.wrapping_add(3)) * 0.5 + 0.5)
                .clamp(0.0, 1.0)
        }

        /// Cold in the far north, hot in the far south (the classic fantasy-map axis), cooled by
        /// altitude and wobbled by noise so climate bands don't run in ruler-straight stripes.
        fn temperature_at(x: u16, y: u16, elevation: f32, seed: u32) -> f32 {
            let nx = f32::from(x) * 0.02;
            let ny = f32::from(y) * 0.02;
            let lat = f32::from(y) / f32::from(WORLD_H);
            let wobble = fbm(nx * 0.6 + 900.0, ny * 0.6 + 900.0, 3, seed.wrapping_add(4));
            (lat * 1.05 - 0.02 + wobble * 0.16 - elevation.max(0.0) * 0.22).clamp(0.0, 1.0)
        }

        fn classify(x: u16, y: u16, e: f32, t: &Thresholds, seed: u32) -> Biome {
            if e < t.sea {
                return if e < t.deep {
                    Biome::DeepOcean
                } else {
                    Biome::Shallows
                };
            }
            if e < t.beach {
                return Biome::Beach;
            }

            let moisture = Self::moisture_at(x, y, seed);
            let temperature = Self::temperature_at(x, y, e, seed);
            let nx = f32::from(x) * 0.02;
            let ny = f32::from(y) * 0.02;

            // Corruption blobs override everything below the peaks: rare, but unmistakable.
            if e < t.peaks {
                let blight = fbm(
                    nx * 0.33 + 7000.0,
                    ny * 0.33 + 7000.0,
                    3,
                    seed.wrapping_add(7),
                );
                if blight > 0.52 {
                    return Biome::Blight;
                }
            }

            let volcanism = fbm(
                nx * 0.35 + 5000.0,
                ny * 0.35 + 5000.0,
                2,
                seed.wrapping_add(6),
            );
            if e >= t.peaks {
                return if volcanism > 0.28 {
                    Biome::VolcanicPeak
                } else if temperature < 0.55 {
                    Biome::SnowPeak
                } else {
                    Biome::Mountains
                };
            }
            if e >= t.mountains {
                return if volcanism > 0.34 {
                    Biome::Ashland
                } else if temperature < 0.18 {
                    Biome::SnowPeak
                } else {
                    Biome::Mountains
                };
            }
            if e >= t.hills {
                return Biome::Hills;
            }

            // Lowlands: moisture x temperature, Whittaker-diagram style, with rare magic-noise
            // overlays (enchanted and dark woods) for the high-fantasy flavor.
            if temperature < 0.07 {
                return Biome::Glacier;
            }
            if temperature < 0.15 {
                return Biome::Tundra;
            }
            if temperature < 0.34 && moisture > 0.45 {
                return Biome::Taiga;
            }
            if temperature > 0.62 && moisture > 0.62 {
                return Biome::Jungle;
            }
            if temperature > 0.55 && moisture < 0.38 {
                return Biome::Desert;
            }
            if temperature > 0.5 && moisture < 0.48 {
                return Biome::Savanna;
            }
            if moisture > 0.68 && e < t.hills * 0.4 + t.beach * 0.6 {
                return Biome::Swamp;
            }
            if moisture > 0.5 {
                let magic = fbm(
                    nx * 0.3 + 3000.0,
                    ny * 0.3 + 3000.0,
                    2,
                    seed.wrapping_add(5),
                );
                return if magic > 0.55 {
                    Biome::EnchantedForest
                } else if magic < -0.55 {
                    Biome::DarkForest
                } else {
                    Biome::Forest
                };
            }
            Biome::Plains
        }

        /// Hillshade from the elevation gradient, lit from the northwest -- the single biggest
        /// "looks like a real map" trick here: slopes facing the light brighten, slopes facing
        /// away darken, and ranges pop into relief. Water is left flat (shade 1.0) so it reads
        /// calm.
        fn compute_shade(&mut self) {
            let n = self.elevation.len();
            self.shade = vec![1.0; n];
            for y in 0..WORLD_H {
                for x in 0..WORLD_W {
                    let i = idx(x, y);
                    if self.biome[i].is_water() {
                        continue;
                    }
                    let e = |px: i32, py: i32| -> f32 {
                        let cx = px.clamp(0, i32::from(WORLD_W) - 1) as u16;
                        let cy = py.clamp(0, i32::from(WORLD_H) - 1) as u16;
                        self.elevation[idx(cx, cy)]
                    };
                    let dzdx =
                        e(i32::from(x) + 1, i32::from(y)) - e(i32::from(x) - 1, i32::from(y));
                    let dzdy =
                        e(i32::from(x), i32::from(y) + 1) - e(i32::from(x), i32::from(y) - 1);
                    self.shade[i] = (1.0 + (-dzdx - dzdy) * 1.5).clamp(0.72, 1.28);
                }
            }
        }

        /// Walks water downhill from a scatter of highland sources to the sea, marking every cell
        /// crossed [`Biome::River`], carving the channel slightly (so later rivers tend to merge
        /// into earlier ones' valleys), widening the mouth where it nears sea level, and pooling
        /// into a [`Biome::Lake`] when it bottoms out inland. Pure steepest descent rather than a
        /// real hydraulic simulation -- enough to read as rivers carving through terrain.
        fn trace_rivers(&mut self) {
            let stride = 17;
            let mut y = 3;
            while y < WORLD_H {
                let mut x = 3;
                while x < WORLD_W {
                    let jitter_x =
                        (hash01(i32::from(x), i32::from(y), self.seed ^ 0x1111) * 6.0) as i32 - 3;
                    let jitter_y =
                        (hash01(i32::from(x), i32::from(y), self.seed ^ 0x2222) * 6.0) as i32 - 3;
                    let sx = (i32::from(x) + jitter_x).clamp(0, i32::from(WORLD_W) - 1) as u16;
                    let sy = (i32::from(y) + jitter_y).clamp(0, i32::from(WORLD_H) - 1) as u16;
                    let e = self.elevation[idx(sx, sy)];
                    let spawns = hash01(i32::from(sx), i32::from(sy), self.seed ^ 0x7777) < 0.34;
                    if e >= self.thresholds.hills && spawns {
                        self.trace_river(sx, sy);
                    }
                    x += stride;
                }
                y += stride;
            }
        }

        fn trace_river(&mut self, start_x: u16, start_y: u16) {
            let mouth_level = self.thresholds.sea + 0.08;
            let mut cur = (start_x, start_y);
            let mut uphill_run = 0u32;
            for _ in 0..2500 {
                let i = idx(cur.0, cur.1);
                if self.elevation[i] <= self.thresholds.sea || self.biome[i].is_water() {
                    break; // reached the sea, or merged into an existing river/lake
                }
                let e_here = self.elevation[i];
                self.biome[i] = Biome::River;
                self.elevation[i] -= 0.02; // carve: later rivers fall into this valley

                // Estuary: as the river nears sea level, spill into adjacent low land so the
                // mouth reads wider than the mountain stream that fed it.
                if e_here < mouth_level {
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let (nx, ny) = (i32::from(cur.0) + dx, i32::from(cur.1) + dy);
                        if nx < 0 || ny < 0 || nx >= i32::from(WORLD_W) || ny >= i32::from(WORLD_H)
                        {
                            continue;
                        }
                        let ni = idx(nx as u16, ny as u16);
                        if !self.biome[ni].is_water() && self.elevation[ni] < mouth_level {
                            self.biome[ni] = Biome::River;
                        }
                    }
                }

                // Step to the lowest neighbor, judged against the *pre-carve* elevation (or the
                // carve itself would turn every visited cell into an instant local minimum), with
                // a small uphill tolerance so the river can spill over minor bumps. Too many
                // uphill steps in a row means a real basin: pool into a lake and stop.
                match self.lowest_neighbor(cur) {
                    Some((next, ne)) if ne < e_here + 0.03 => {
                        uphill_run = if ne >= e_here { uphill_run + 1 } else { 0 };
                        if uphill_run > 4 {
                            self.fill_lake(cur);
                            break;
                        }
                        cur = next;
                    }
                    _ => {
                        self.fill_lake(cur);
                        break;
                    }
                }
            }
        }

        /// The lowest 8-neighbor of `(x, y)` and its elevation -- unconditionally, so callers
        /// decide what "low enough" means (rivers tolerate a slight rise; lava doesn't).
        fn lowest_neighbor(&self, (x, y): (u16, u16)) -> Option<((u16, u16), f32)> {
            let mut best: Option<((u16, u16), f32)> = None;
            for (dx, dy) in NEIGHBORS8 {
                let (nx, ny) = (i32::from(x) + dx, i32::from(y) + dy);
                if nx < 0 || ny < 0 || nx >= i32::from(WORLD_W) || ny >= i32::from(WORLD_H) {
                    continue;
                }
                let (nx, ny) = (nx as u16, ny as u16);
                let ne = self.elevation[idx(nx, ny)];
                if best.is_none_or(|(_, b)| ne < b) {
                    best = Some(((nx, ny), ne));
                }
            }
            best
        }

        /// Floods a small basin around an inland local minimum into a [`Biome::Lake`]: everything
        /// reachable from `origin` within a modest elevation band, capped so a flat plain can't
        /// flood into an inland sea.
        fn fill_lake(&mut self, origin: (u16, u16)) {
            let limit = self.elevation[idx(origin.0, origin.1)] + 0.045;
            let mut queue = vec![origin];
            let mut filled = 0;
            while let Some((x, y)) = queue.pop() {
                let i = idx(x, y);
                if self.biome[i] == Biome::Lake
                    || self.elevation[i] > limit
                    || self.biome[i].is_water() && self.biome[i] != Biome::River
                {
                    continue;
                }
                self.biome[i] = Biome::Lake;
                filled += 1;
                if filled >= 80 {
                    break;
                }
                for (dx, dy) in NEIGHBORS8 {
                    let (nx, ny) = (i32::from(x) + dx, i32::from(y) + dy);
                    if nx >= 0 && ny >= 0 && nx < i32::from(WORLD_W) && ny < i32::from(WORLD_H) {
                        queue.push((nx as u16, ny as u16));
                    }
                }
            }
        }

        /// Short lava flows from volcanic peaks -- the same downhill walk as rivers, capped much
        /// shorter since lava should pool near its source, not snake across the map.
        fn trace_lava(&mut self) {
            for y in 0..WORLD_H {
                for x in 0..WORLD_W {
                    if self.biome[idx(x, y)] != Biome::VolcanicPeak {
                        continue;
                    }
                    // Only a scatter of peak cells vent: a volcano should trail a few glowing
                    // rivulets, not drown its whole flank in lava.
                    if hash01(i32::from(x), i32::from(y), self.seed ^ 0x6666) > 0.12 {
                        continue;
                    }
                    let mut cur = (x, y);
                    for step in 0..25 {
                        let i = idx(cur.0, cur.1);
                        if self.elevation[i] <= self.thresholds.sea || self.biome[i] == Biome::Lava
                        {
                            break;
                        }
                        // The peak itself stays a peak; lava marks only the slopes below it.
                        if step > 0 {
                            self.biome[i] = Biome::Lava;
                        }
                        match self.lowest_neighbor(cur) {
                            Some((next, ne)) if ne < self.elevation[i] => cur = next,
                            _ => break,
                        }
                    }
                }
            }
        }

        /// Scatters landmarks in two passes over a jittered grid: settlements first (their own,
        /// generous pass -- civilization is the anchor the road network and starting view hang
        /// off), then everything else, with spacing rejection so landmarks read as scattered, not
        /// clumped. Settlements get generated names.
        fn scatter_pois(&mut self) {
            let mut placed: Vec<(u16, u16)> = Vec::new();

            // Pass 1: settlements on hospitable ground.
            self.grid_scan(15, 0x3333, |world, px, py, roll| {
                if roll > 0.5 {
                    return;
                }
                let biome = world.biome[idx(px, py)];
                if !Poi::Village.fits(biome) {
                    return;
                }
                if placed
                    .iter()
                    .any(|&(ox, oy)| dist_sq(px, py, ox, oy) < 10 * 10)
                {
                    return;
                }
                let kind = if roll < 0.06 { Poi::City } else { Poi::Village };
                world.pois.insert((px, py), kind);
                world
                    .poi_names
                    .insert((px, py), settlement_name(px, py, world.seed));
                placed.push((px, py));
            });

            // Pass 2: everything else, weighted by rarity.
            self.grid_scan(13, 0x4343, |world, px, py, roll| {
                if roll > 0.13 {
                    return; // most sampled cells stay empty ground
                }
                let biome = world.biome[idx(px, py)];
                if placed
                    .iter()
                    .any(|&(ox, oy)| dist_sq(px, py, ox, oy) < 7 * 7)
                {
                    return;
                }
                if let Some(kind) = pick_poi(biome, roll / 0.13) {
                    world.pois.insert((px, py), kind);
                    placed.push((px, py));
                }
            });
        }

        /// Visits a jittered grid of sample cells with a per-cell roll in `[0, 1)` -- the shared
        /// skeleton of both [`Self::scatter_pois`] passes.
        fn grid_scan(
            &mut self,
            stride: u16,
            salt: u32,
            mut visit: impl FnMut(&mut Self, u16, u16, f32),
        ) {
            let mut y = 2;
            while y < WORLD_H {
                let mut x = 2;
                while x < WORLD_W {
                    let jx = (hash01(i32::from(x), i32::from(y), self.seed ^ salt)
                        * f32::from(stride)) as i32;
                    let jy = (hash01(i32::from(x), i32::from(y), self.seed ^ salt.rotate_left(7))
                        * f32::from(stride)) as i32;
                    let px = (i32::from(x) + jx).clamp(0, i32::from(WORLD_W) - 1) as u16;
                    let py = (i32::from(y) + jy).clamp(0, i32::from(WORLD_H) - 1) as u16;
                    let roll = hash01(
                        i32::from(px),
                        i32::from(py),
                        self.seed ^ salt.rotate_left(13),
                    );
                    if !self.pois.contains_key(&(px, py)) {
                        visit(self, px, py, roll);
                    }
                    x += stride;
                }
                y += stride;
            }
        }

        /// Links each settlement to its nearest already-connected neighbor with a greedy
        /// slope-averse walk, bridging rivers and skipping pairs separated by too much open sea
        /// -- so plains fill in with a believable web of trade roads rather than isolated dots.
        fn build_roads(&mut self) {
            let mut settlements: Vec<(u16, u16)> = self
                .pois
                .iter()
                .filter(|&(_, poi)| poi.is_settlement())
                .map(|(&pos, _)| pos)
                .collect();
            settlements.sort_unstable();
            for i in 1..settlements.len() {
                let from = settlements[i];
                let Some(&to) = settlements[..i]
                    .iter()
                    .min_by_key(|&&(x, y)| dist_sq(from.0, from.1, x, y))
                else {
                    continue;
                };
                if dist_sq(from.0, from.1, to.0, to.1) > 90 * 90 {
                    continue; // too remote: some frontier villages just aren't on the network
                }
                self.walk_road(from, to);
            }
        }

        /// Whether a straight line between two settlements crosses enough open ocean that a road
        /// makes no sense (different islands); rivers and lakes are fine, those get bridges.
        fn sea_blocks(&self, a: (u16, u16), b: (u16, u16)) -> bool {
            let steps = i32::from(a.0.abs_diff(b.0).max(a.1.abs_diff(b.1))).max(1);
            let mut sea = 0;
            for s in 0..=steps {
                let x = i32::from(a.0) + (i32::from(b.0) - i32::from(a.0)) * s / steps;
                let y = i32::from(a.1) + (i32::from(b.1) - i32::from(a.1)) * s / steps;
                if matches!(
                    self.biome[idx(x as u16, y as u16)],
                    Biome::DeepOcean | Biome::Shallows
                ) {
                    sea += 1;
                    if sea > 10 {
                        return true;
                    }
                }
            }
            false
        }

        fn walk_road(&mut self, from: (u16, u16), to: (u16, u16)) {
            if self.sea_blocks(from, to) {
                return;
            }
            let mut cur = from;
            let mut guard = 4 * u32::from(from.0.abs_diff(to.0).max(from.1.abs_diff(to.1))) + 8;
            while cur != to && guard > 0 {
                guard -= 1;
                let cheb = |p: (u16, u16)| i32::from(p.0.abs_diff(to.0).max(p.1.abs_diff(to.1)));
                let here = cheb(cur);
                let mut best: Option<((u16, u16), f32)> = None;
                for (dx, dy) in NEIGHBORS8 {
                    let (nx, ny) = (i32::from(cur.0) + dx, i32::from(cur.1) + dy);
                    if nx < 0 || ny < 0 || nx >= i32::from(WORLD_W) || ny >= i32::from(WORLD_H) {
                        continue;
                    }
                    let next = (nx as u16, ny as u16);
                    if cheb(next) >= here {
                        continue; // only ever step closer, so the walk always terminates
                    }
                    let i = idx(next.0, next.1);
                    let slope = (self.elevation[i] - self.elevation[idx(cur.0, cur.1)]).abs();
                    let water_pen = match self.biome[i] {
                        Biome::DeepOcean => 40.0,
                        Biome::Shallows => 14.0,
                        Biome::River | Biome::Lake => 2.5,
                        _ => 0.0,
                    };
                    // Reusing an existing road is nearly free, so networks share trunk routes.
                    let reuse = if self.road[i] == RoadCell::None {
                        0.0
                    } else {
                        -1.5
                    };
                    let cost =
                        slope * 9.0 + water_pen + reuse + hash01(nx, ny, self.seed ^ 0x8888) * 0.4; // jitter: no ruler-straight roads
                    if best.is_none_or(|(_, b)| cost < b) {
                        best = Some((next, cost));
                    }
                }
                let Some((next, _)) = best else { break };
                cur = next;
                if cur == to {
                    break;
                }
                let i = idx(cur.0, cur.1);
                if !self.pois.contains_key(&cur) {
                    self.road[i] = if self.biome[i].is_water() {
                        RoadCell::Bridge
                    } else {
                        RoadCell::Road
                    };
                }
            }
        }

        /// Seeds a jittered 6x3 grid of named regions; [`Self::region_at`] resolves any position
        /// to its nearest seed, Voronoi-style.
        fn name_regions(&mut self) {
            const COLS: u16 = 6;
            const ROWS: u16 = 3;
            for ry in 0..ROWS {
                for rx in 0..COLS {
                    let k = (ry * COLS + rx) as usize;
                    let cx = (u32::from(rx) * 2 + 1) * u32::from(WORLD_W) / (u32::from(COLS) * 2);
                    let cy = (u32::from(ry) * 2 + 1) * u32::from(WORLD_H) / (u32::from(ROWS) * 2);
                    let jx = (hash01(k as i32, 3, self.seed ^ 0x9999) - 0.5) * f32::from(WORLD_W)
                        / f32::from(COLS)
                        * 0.7;
                    let jy = (hash01(k as i32, 5, self.seed ^ 0xAAAA) - 0.5) * f32::from(WORLD_H)
                        / f32::from(ROWS)
                        * 0.7;
                    let x = ((cx as f32 + jx) as i32).clamp(0, i32::from(WORLD_W) - 1) as u16;
                    let y = ((cy as f32 + jy) as i32).clamp(0, i32::from(WORLD_H) - 1) as u16;
                    self.region_seeds.push((x, y));
                    self.region_names.push(region_name(k, self.seed));
                }
            }
        }
    }

    // ── Strategic tiles ──────────────────────────────────────────────────────

    /// Width of one strategic tile, in world cells; see [`TileMap`].
    pub(crate) const TILE_W: u16 = 16;
    /// Height of one strategic tile, in world cells. Half of [`TILE_W`]: terminal glyphs are
    /// roughly twice as tall as wide, so a 16x8 world block reads as a square-ish province
    /// however the tile itself is drawn.
    pub(crate) const TILE_H: u16 = 8;

    /// One strategic tile: a [`TILE_W`]x[`TILE_H`] block of world cells collapsed to what a
    /// strategy-game player needs at province scale -- the majority biome, at most one landmark,
    /// and whether roads or rivers pass through. Everything (colors included) is resolved at
    /// build time, so rendering a tile is pure lookups, the same bargain [`World::render_cell`]
    /// makes per cell.
    #[derive(Clone, Copy, PartialEq, Debug)]
    pub(crate) struct Tile {
        /// Majority biome of the block: the most common water biome if the block is mostly
        /// water, otherwise the most common land biome (so a coastal province reads as its land,
        /// not as "technically 45% shallows").
        pub(crate) biome: Biome,
        /// Flat face color: the average of the block's per-cell map colors (hillshaded land,
        /// depth-graded water), so tiles match the minimap's palette and coastal tiles blend
        /// toward blue instead of snapping to a binary land/water color.
        pub(crate) base: Color,
        /// Dominant-biome glyph, sampled with a fixed decorative hash -- tiles don't twinkle;
        /// the strategic views trade cell mode's ambience for a stable counter-like read.
        pub(crate) glyph: char,
        /// Foreground color for [`Self::glyph`].
        pub(crate) glyph_color: Color,
        /// The most notable landmark in the block, if any (see [`poi_rank`]).
        pub(crate) poi: Option<Poi>,
        /// Whether any river cell falls in the block.
        pub(crate) river: bool,
        /// Whether any road or bridge cell falls in the block.
        pub(crate) road: bool,
        /// Whether the block is mostly water.
        pub(crate) water: bool,
    }

    /// The strategic-tile grid over a [`World`]: [`World::tile_map`] aggregates every
    /// [`TILE_W`]x[`TILE_H`] block once, and the per-frame tile renderers only ever index into
    /// the result. `staggered` builds the hex variant, where odd rows sample half a tile
    /// further right so each hex aggregates the world cells actually under its staggered
    /// on-screen footprint.
    pub(crate) struct TileMap {
        cols: u16,
        rows: u16,
        staggered: bool,
        tiles: Vec<Tile>,
    }

    impl TileMap {
        #[must_use]
        pub(crate) const fn cols(&self) -> u16 {
            self.cols
        }

        #[must_use]
        pub(crate) const fn rows(&self) -> u16 {
            self.rows
        }

        /// The tile at `(col, row)`, or `None` outside the grid -- signed so callers can probe
        /// neighbors without pre-clamping.
        #[must_use]
        pub(crate) fn get(&self, col: i32, row: i32) -> Option<&Tile> {
            if col < 0 || row < 0 || col >= i32::from(self.cols) || row >= i32::from(self.rows) {
                return None;
            }
            self.tiles
                .get(row as usize * self.cols as usize + col as usize)
        }

        /// The `(col, row)` of the tile whose sample block contains world position `pos`.
        #[must_use]
        pub(crate) fn tile_of(&self, pos: Pos) -> (u16, u16) {
            let row = (pos.y / TILE_H).min(self.rows - 1);
            let stag = if self.staggered && row % 2 == 1 {
                TILE_W / 2
            } else {
                0
            };
            let col = (pos.x.saturating_sub(stag) / TILE_W).min(self.cols - 1);
            (col, row)
        }
    }

    impl World {
        /// Aggregates this world into a [`TileMap`] -- one pass over the cached per-cell arrays,
        /// so it's cheap enough to rebuild whenever the seed or the stagger changes.
        #[must_use]
        pub(crate) fn tile_map(&self, staggered: bool) -> TileMap {
            let cols = WORLD_W.div_ceil(TILE_W);
            let rows = WORLD_H.div_ceil(TILE_H);
            let mut tiles = Vec::with_capacity(usize::from(cols) * usize::from(rows));
            for row in 0..rows {
                let stag = if staggered && row % 2 == 1 {
                    TILE_W / 2
                } else {
                    0
                };
                for col in 0..cols {
                    // Blocks that hang past the world edge (the last column/row, and staggered
                    // odd rows) clamp their sample range; the border is all sea, so a partial
                    // edge sample still aggregates to an honest ocean tile.
                    let x0 = (col * TILE_W + stag).min(WORLD_W - 1);
                    let y0 = (row * TILE_H).min(WORLD_H - 1);
                    let x1 = (x0 + TILE_W).min(WORLD_W);
                    let y1 = (y0 + TILE_H).min(WORLD_H);
                    tiles.push(self.build_tile(x0, x1, y0, y1));
                }
            }
            TileMap {
                cols,
                rows,
                staggered,
                tiles,
            }
        }

        /// Aggregates one block of world cells into a [`Tile`]; see [`Tile`]'s field docs for
        /// exactly what survives the zoom-out.
        fn build_tile(&self, x0: u16, x1: u16, y0: u16, y1: u16) -> Tile {
            // Insertion-ordered counts, not a HashMap: ties on the majority vote must break
            // deterministically, or the same seed could render different tiles run to run.
            let mut counts: Vec<(Biome, u32)> = Vec::new();
            let mut water_cells = 0u32;
            let mut total = 0u32;
            let (mut r_sum, mut g_sum, mut b_sum) = (0.0f32, 0.0f32, 0.0f32);
            let mut river = false;
            let mut road = false;
            let mut poi: Option<Poi> = None;
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = idx(x, y);
                    let biome = self.biome[i];
                    match counts.iter_mut().find(|(b, _)| *b == biome) {
                        Some((_, n)) => *n += 1,
                        None => counts.push((biome, 1)),
                    }
                    total += 1;
                    river |= biome == Biome::River;
                    road |= self.road[i] != RoadCell::None;
                    let color = if biome.is_water() {
                        water_cells += 1;
                        self.water_color(i, biome)
                    } else {
                        shade_color(biome.color(), self.shade[i])
                    };
                    let (r, g, b) = to_rgb(color);
                    r_sum += f32::from(r);
                    g_sum += f32::from(g);
                    b_sum += f32::from(b);
                    if let Some(&kind) = self.pois.get(&(x, y))
                        && poi.is_none_or(|cur| poi_rank(kind) < poi_rank(cur))
                    {
                        poi = Some(kind);
                    }
                }
            }
            let water = water_cells * 2 > total;
            let biome = counts
                .iter()
                .filter(|(b, _)| b.is_water() == water)
                .max_by_key(|(_, n)| *n)
                .map_or(Biome::DeepOcean, |(b, _)| *b);
            let scale = 1.0 / total.max(1) as f32;
            let base = rgb(
                (r_sum * scale) as u8,
                (g_sum * scale) as u8,
                (b_sum * scale) as u8,
            );
            let (glyph, glyph_color) = biome.glyph(0.3, 0.0);
            Tile {
                biome,
                base,
                glyph,
                glyph_color,
                poi,
                river,
                road,
                water,
            }
        }
    }

    /// Display priority for the single landmark a [`Tile`] keeps: cities over villages over
    /// everything else, then rarer kinds (lower [`Poi::weight`]) over common ones -- a dragon's
    /// lair should beat a waystone for a province's one icon slot.
    const fn poi_rank(poi: Poi) -> u32 {
        match poi {
            Poi::City => 0,
            Poi::Village => 1,
            _ => 2 + poi.weight(),
        }
    }

    /// Applies a hillshade multiplier to a color: `s < 1` darkens toward black, `s > 1` lightens
    /// toward white (attenuated, so lit slopes glow rather than wash out).
    fn shade_color(c: Color, s: f32) -> Color {
        if s < 1.0 {
            Color::lerp(c, Color::BLACK, (1.0 - s).min(1.0))
        } else {
            Color::lerp(c, Color::WHITE, ((s - 1.0) * 0.7).min(1.0))
        }
    }

    fn dist_sq(ax: u16, ay: u16, bx: u16, by: u16) -> i32 {
        let dx = i32::from(ax) - i32::from(bx);
        let dy = i32::from(ay) - i32::from(by);
        dx * dx + dy * dy
    }

    /// Weighted pick among the non-settlement [`POI_KINDS`] that fit `biome`, keyed off `roll` in
    /// `[0, 1)` so the choice stays deterministic per-cell. Weights keep dragon lairs rare while
    /// ruins stay common (see [`Poi::weight`]); settlements are placed by their own earlier pass.
    fn pick_poi(biome: Biome, roll: f32) -> Option<Poi> {
        let candidates: Vec<Poi> = POI_KINDS
            .into_iter()
            .filter(|k| !k.is_settlement() && k.fits(biome))
            .collect();
        let total: u32 = candidates.iter().map(|k| k.weight()).sum();
        if total == 0 {
            return None;
        }
        let mut pick = (roll * total as f32) as u32;
        for kind in candidates {
            if pick < kind.weight() {
                return Some(kind);
            }
            pick -= kind.weight();
        }
        None
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn generation_is_deterministic() {
            let a = World::generate(7);
            let b = World::generate(7);
            assert_eq!(a.biome, b.biome);
            assert_eq!(a.pois.len(), b.pois.len());
            assert_eq!(a.poi_names, b.poi_names);
            assert_eq!(a.region_names, b.region_names);
        }

        #[test]
        fn different_seeds_produce_different_worlds() {
            let a = World::generate(1);
            let b = World::generate(2);
            assert_ne!(a.biome, b.biome);
        }

        #[test]
        fn world_has_water_and_several_land_biomes() {
            let world = World::generate(2);
            let mut seen = std::collections::HashSet::new();
            for &b in &world.biome {
                seen.insert(b);
            }
            assert!(
                seen.contains(&Biome::DeepOcean) || seen.contains(&Biome::Shallows),
                "expected some water in the world"
            );
            assert!(
                seen.len() >= 10,
                "expected a diverse world, only saw {} biome kinds: {seen:?}",
                seen.len()
            );
        }

        #[test]
        fn map_borders_are_all_sea() {
            let world = World::generate(2);
            for x in 0..WORLD_W {
                for y in [0, WORLD_H - 1] {
                    assert!(
                        world.biome[idx(x, y)].is_water(),
                        "expected water at border ({x}, {y}), got {:?}",
                        world.biome[idx(x, y)]
                    );
                }
            }
            for y in 0..WORLD_H {
                for x in [0, WORLD_W - 1] {
                    assert!(
                        world.biome[idx(x, y)].is_water(),
                        "expected water at border ({x}, {y}), got {:?}",
                        world.biome[idx(x, y)]
                    );
                }
            }
        }

        #[test]
        fn world_has_rivers_roads_and_points_of_interest() {
            let world = World::generate(2);
            assert!(
                world.biome.contains(&Biome::River),
                "expected at least one river tile"
            );
            assert!(
                !world.pois.is_empty(),
                "expected at least one point of interest"
            );
            assert!(
                world.road.contains(&RoadCell::Road),
                "expected a road network between settlements"
            );
        }

        #[test]
        fn settlements_are_named() {
            let world = World::generate(2);
            for (&(x, y), &poi) in &world.pois {
                if poi.is_settlement() {
                    let name = world.poi_names.get(&(x, y));
                    assert!(
                        name.is_some_and(|n| !n.is_empty()),
                        "unnamed settlement at ({x}, {y})"
                    );
                }
            }
            assert!(
                world.pois.values().any(|p| p.is_settlement()),
                "expected at least one settlement to exercise naming"
            );
        }

        #[test]
        fn every_position_resolves_to_a_named_region() {
            let world = World::generate(2);
            for &pos in &[
                Pos::new(0, 0),
                Pos::new(WORLD_W - 1, WORLD_H - 1),
                Pos::new(WORLD_W / 2, WORLD_H / 2),
            ] {
                let name = world.region_at(pos);
                assert!(name.starts_with("The "), "unexpected region name {name:?}");
            }
        }

        #[test]
        fn pois_only_sit_on_eligible_biomes() {
            let world = World::generate(3);
            for (&(x, y), &poi) in &world.pois {
                let biome = world.biome[idx(x, y)];
                assert!(
                    poi.fits(biome),
                    "{poi:?} placed on {biome:?} at ({x}, {y}), which it doesn't fit"
                );
            }
        }

        #[test]
        fn land_water_split_is_calibrated() {
            // The quantile thresholds should hold the water fraction near the 34% target
            // regardless of seed (rivers/lakes push it slightly above).
            for seed in [1, 2, 9, 42] {
                let world = World::generate(seed);
                let water = world.biome.iter().filter(|b| b.is_water()).count();
                let frac = water as f64 / world.biome.len() as f64;
                assert!(
                    (0.30..0.48).contains(&frac),
                    "seed {seed}: water fraction {frac:.2} out of expected band"
                );
            }
        }

        #[test]
        fn tile_map_covers_the_world_and_is_deterministic() {
            let world = World::generate(2);
            let a = world.tile_map(false);
            let b = world.tile_map(false);
            assert_eq!(a.cols, WORLD_W.div_ceil(TILE_W));
            assert_eq!(a.rows, WORLD_H.div_ceil(TILE_H));
            assert_eq!(a.tiles.len(), usize::from(a.cols) * usize::from(a.rows));
            assert_eq!(a.tiles, b.tiles, "same seed must aggregate identically");
            let hexes = world.tile_map(true);
            assert_eq!(hexes.tiles.len(), a.tiles.len());
        }

        #[test]
        fn tile_map_keeps_the_strategic_features() {
            let world = World::generate(2);
            for staggered in [false, true] {
                let map = world.tile_map(staggered);
                for col in 0..map.cols {
                    let top = map.get(i32::from(col), 0).expect("top tile");
                    let bottom = map
                        .get(i32::from(col), i32::from(map.rows - 1))
                        .expect("bottom tile");
                    assert!(top.water && bottom.water, "map borders are open sea");
                }
                assert!(map.tiles.iter().any(|t| !t.water), "expected land tiles");
                assert!(map.tiles.iter().any(|t| t.road), "expected road tiles");
                assert!(map.tiles.iter().any(|t| t.river), "expected river tiles");
                assert!(
                    map.tiles.iter().any(|t| t.poi.is_some()),
                    "expected landmark tiles"
                );
            }
        }

        #[test]
        fn tile_of_stays_in_bounds_everywhere() {
            let world = World::generate(2);
            for staggered in [false, true] {
                let map = world.tile_map(staggered);
                for &pos in &[
                    Pos::new(0, 0),
                    Pos::new(WORLD_W - 1, 0),
                    Pos::new(0, WORLD_H - 1),
                    Pos::new(WORLD_W - 1, WORLD_H - 1),
                    Pos::new(WORLD_W / 2, WORLD_H / 2),
                ] {
                    let (col, row) = map.tile_of(pos);
                    assert!(col < map.cols && row < map.rows, "({col}, {row}) in bounds");
                    assert!(map.get(i32::from(col), i32::from(row)).is_some());
                }
                assert_eq!(map.tile_of(Pos::new(0, 0)), (0, 0));
            }
        }
    }
}

use hexal::{Direction, OddR, OffsetHex};
use retroglyph_core::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use retroglyph_core::{Backend, Camera, Color, Frame, Pos, Rect, Size, Style, Terminal};
use retroglyph_examples::Example;
use retroglyph_widgets::{Constraint, Panel, Widget, split_h, truncate};

use world::{TILE_H, TILE_W, TileMap, World};
pub use world::{WORLD_H, WORLD_W};

/// The world seed used on startup, chosen (by eyeballing histogram dumps over the first dozen
/// seeds) for showing off every biome family -- including the rare ones (enchanted wood,
/// blight, a volcano, jungle, at least one city) -- without having to press `R` first.
const DEFAULT_SEED: u32 = 2;

/// Terminal width, in columns, at or above which the info sidebar opens. Below it, chrome
/// collapses to a single status line so the map still reads on a narrow terminal -- the same
/// "layout changes shape, not just size" idea `15_outpost_dashboard` uses for its own
/// sheet/sidebar split.
const BP_SIDEBAR: u16 = 74;
/// Terminal height, in rows, below which the sidebar's minimap/legend don't have room and are
/// skipped even when [`BP_SIDEBAR`] is met.
const BP_TALL: u16 = 18;

const SIDEBAR_W: u16 = 30;
const MINIMAP_H: u16 = 11;

// ── Strategic tile geometry ────────────────────────────────────────────────

/// Screen width of one square-view tile, in terminal cells; with [`SQ_H`] rows it draws as an
/// 8x4 block, roughly square at the typical 1:2 glyph aspect.
const SQ_W: u16 = 8;
/// Screen height of one square-view tile, in terminal cells.
const SQ_H: u16 = 4;

/// Horizontal pitch between hex tiles in a row, in terminal cells -- also the width of a hex's
/// middle row. See [`HEX_ROW_H`] for how the 3-row hexes interlock.
const HEX_W: u16 = 8;
/// Vertical pitch between hex rows, in terminal cells.
///
/// Each hex is drawn 3 rows tall (a 4-cell taper row, the 8-cell middle row, another taper),
/// but consecutive rows share their taper rows: a hex's bottom-taper screen row is also the
/// next row's top-taper screen row, with the odd-row stagger keeping the two sets of taper
/// cells disjoint. The result partitions the plane exactly -- every map cell belongs to one
/// hex -- which is what makes the honeycomb read without drawing outline glyphs.
const HEX_ROW_H: u16 = 2;

/// How the main map renders the world; `T` cycles.
///
/// [`View::Cells`] is the classic per-cell terrain. The two *tile* views aggregate
/// [`TILE_W`]x[`TILE_H`] world-cell blocks into province-sized counters (see
/// [`world::TileMap`]) -- the abstraction level a strategy game would run its turns on. The two
/// *grid* views keep the per-cell terrain at full fidelity and instead draw the boundaries of
/// those same provinces over it as background-tinted border lines (see [`grid_region`]) -- the
/// "detailed terrain with a hex grid" look, rather than the "board-game counters" look.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum View {
    /// One glyph per world cell (the original view).
    #[default]
    Cells,
    /// Square strategic tiles, [`SQ_W`]x[`SQ_H`] screen cells each.
    Squares,
    /// Per-cell terrain with the square-tile boundaries overlaid.
    SquareGrid,
    /// Hexagonal strategic tiles on a staggered (odd rows shifted right) honeycomb, laid out
    /// and reasoned about with [`hexal`]'s `OddR` offset coordinates.
    Hexes,
    /// Per-cell terrain with the hex boundaries overlaid.
    HexGrid,
}

impl View {
    const fn next(self) -> Self {
        match self {
            Self::Cells => Self::Squares,
            Self::Squares => Self::SquareGrid,
            Self::SquareGrid => Self::Hexes,
            Self::Hexes => Self::HexGrid,
            Self::HexGrid => Self::Cells,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Cells => "terrain",
            Self::Squares => "square tiles",
            Self::SquareGrid => "square grid",
            Self::Hexes => "hex tiles",
            Self::HexGrid => "hex grid",
        }
    }

    /// World cells per screen cell `(x, y)`, so mouse drags and wheel scrolls track the map 1:1
    /// in every view -- a tile view shows more world per screen cell, so a drag must pan more.
    /// The grid overlays render cells 1:1 like [`Self::Cells`], so they scale 1:1 too.
    const fn pan_scale(self) -> (i32, i32) {
        match self {
            Self::Cells | Self::SquareGrid | Self::HexGrid => (1, 1),
            Self::Squares => ((TILE_W / SQ_W) as i32, (TILE_H / SQ_H) as i32),
            Self::Hexes => ((TILE_W / HEX_W) as i32, (TILE_H / HEX_ROW_H) as i32),
        }
    }
}

// ── Palette ──────────────────────────────────────────────────────────────────

const BG: Color = Color::Rgb { r: 8, g: 9, b: 14 };
const PANEL_BG: Color = Color::Rgb {
    r: 16,
    g: 17,
    b: 26,
};
const BORDER: Color = Color::Rgb {
    r: 74,
    g: 68,
    b: 96,
};
const FG: Color = Color::Rgb {
    r: 214,
    g: 212,
    b: 226,
};
const DIM_FG: Color = Color::Rgb {
    r: 122,
    g: 118,
    b: 142,
};
const ACCENT: Color = Color::Rgb {
    r: 248,
    g: 198,
    b: 90,
};
const RETICLE: Color = Color::Rgb {
    r: 255,
    g: 236,
    b: 170,
};
/// Road connector marks in the tile views, matching cell mode's trade-road dots.
const TILE_ROAD_FG: Color = Color::Rgb {
    r: 216,
    g: 196,
    b: 150,
};
/// River connector marks in the tile views: cell mode's river blue, lightened to read against
/// a tile's darkened face color.
const TILE_RIVER_FG: Color = Color::Rgb {
    r: 120,
    g: 182,
    b: 235,
};
/// Grid-line tint for the overlay views: the sidebar [`BORDER`] family, lightened enough to
/// register on dark ocean without shouting over land terrain.
const GRID_LINE: Color = Color::Rgb {
    r: 148,
    g: 142,
    b: 178,
};

// ── State ────────────────────────────────────────────────────────────────────

/// What a held mouse drag is currently doing -- panning the main map (tracking the last drag
/// position, so each move is a relative delta) or scrubbing the sidebar minimap (no last-position
/// tracking needed: every move jumps the camera straight to the point under the cursor, via
/// [`Overworld::jump_to_minimap`]).
enum Drag {
    Map(Pos),
    Minimap,
}

/// State for the overworld example: the generated [`World`], the [`Camera`] panned over it, and
/// the click/drag bookkeeping the mouse handlers below need.
pub struct Overworld {
    world: World,
    camera: Camera,
    /// World position the camera is centered on -- also where the sidebar's info readout and
    /// the on-map reticle point.
    pub cam_center: Pos,
    time: f64,
    /// The main map's screen [`Rect`] from the most recent draw, for click/drag picking.
    pub last_map_rect: Option<Rect>,
    /// The sidebar minimap's screen [`Rect`] from the most recent draw, for click/drag picking
    /// -- `None` whenever the minimap wasn't drawn this frame (narrow terminal, or too short for
    /// it to fit), so a stray click can't be misread as landing on it.
    pub last_minimap_rect: Option<Rect>,
    drag: Option<Drag>,
    /// Active map view; `T` cycles. `pub` so the sibling test file can assert the cycling.
    pub view: View,
    /// Cached [`TileMap`] keyed by `(seed, staggered)`, rebuilt lazily when either changes --
    /// aggregation is one pass over the world, cheap, but not something to redo every frame.
    tile_cache: Option<((u32, bool), TileMap)>,
    /// The world rect the most recent [`Self::draw_map`] showed, whichever view drew it -- the
    /// minimap's "you are here" box reads this instead of poking at view-specific camera state.
    visible_world: Rect,
}

impl Default for Overworld {
    fn default() -> Self {
        let world = World::generate(DEFAULT_SEED);
        let camera = Camera::new(
            Rect::new(0, 0, 10, 6),
            Size {
                width: WORLD_W,
                height: WORLD_H,
            },
        );
        let cam_center = world.starting_view();
        Self {
            world,
            camera,
            cam_center,
            time: 0.0,
            last_map_rect: None,
            last_minimap_rect: None,
            drag: None,
            view: View::Cells,
            tile_cache: None,
            visible_world: Rect::new(0, 0, 0, 0),
        }
    }
}

impl Overworld {
    fn pan(&mut self, dx: i32, dy: i32) {
        let x = (i32::from(self.cam_center.x) + dx).clamp(0, i32::from(WORLD_W) - 1);
        let y = (i32::from(self.cam_center.y) + dy).clamp(0, i32::from(WORLD_H) - 1);
        self.cam_center = Pos::new(x as u16, y as u16);
    }

    fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers) -> bool {
        // In the tile views one keypress moves one whole tile -- the strategic grid is the unit
        // that matters there, and sub-tile nudges wouldn't visibly move the reticle.
        let shift = mods.contains(KeyModifiers::SHIFT);
        let (step_x, step_y): (i32, i32) = match self.view {
            View::Cells | View::SquareGrid | View::HexGrid => {
                let s = if shift { 8 } else { 1 };
                (s, s)
            }
            View::Squares | View::Hexes => {
                let m = if shift { 4 } else { 1 };
                (i32::from(TILE_W) * m, i32::from(TILE_H) * m)
            }
        };
        match code {
            KeyCode::Char('q' | 'Q') | KeyCode::Escape => return false,
            KeyCode::Up | KeyCode::Char('w' | 'W') => self.pan(0, -step_y),
            KeyCode::Down | KeyCode::Char('s' | 'S') => self.pan(0, step_y),
            KeyCode::Left | KeyCode::Char('a' | 'A') => self.pan(-step_x, 0),
            KeyCode::Right | KeyCode::Char('d' | 'D') => self.pan(step_x, 0),
            KeyCode::Char('t' | 'T') => self.view = self.view.next(),
            KeyCode::Char('r' | 'R') => {
                self.world = World::generate(self.world.seed().wrapping_add(1));
                self.cam_center = self.world.starting_view();
            }
            KeyCode::Home => self.cam_center = self.world.starting_view(),
            _ => {}
        }
        true
    }

    /// Jumps the camera straight to whatever world point is under `pos` on the sidebar minimap
    /// -- a no-op if `pos` isn't over it (or it wasn't drawn this frame at all; see
    /// [`Self::last_minimap_rect`]). Unlike [`Self::pan`]'s relative deltas, this always sets an
    /// absolute position, so both a single click and every step of a drag land exactly where the
    /// pointer is, the way scrubbing a real minimap/radar should feel.
    fn jump_to_minimap(&mut self, pos: Pos) {
        let Some(rect) = self.last_minimap_rect else {
            return;
        };
        if !rect.contains_pos(pos) {
            return;
        }
        let (local_x, local_y) = (pos.x - rect.left(), pos.y - rect.top());
        let wx = (u32::from(local_x) * u32::from(WORLD_W) / u32::from(rect.width()))
            .min(u32::from(WORLD_W) - 1);
        let wy = (u32::from(local_y) * u32::from(WORLD_H) / u32::from(rect.height()))
            .min(u32::from(WORLD_H) - 1);
        self.cam_center = Pos::new(wx as u16, wy as u16);
    }

    fn handle_mouse(&mut self, kind: MouseEventKind, pos: Pos) {
        let on_map = self.last_map_rect.is_some_and(|r| r.contains_pos(pos));
        let on_minimap = self.last_minimap_rect.is_some_and(|r| r.contains_pos(pos));
        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if on_minimap {
                    self.jump_to_minimap(pos);
                    self.drag = Some(Drag::Minimap);
                } else if on_map {
                    self.drag = Some(Drag::Map(pos));
                }
            }
            MouseEventKind::Up(MouseButton::Left) => self.drag = None,
            MouseEventKind::Moved => match self.drag {
                Some(Drag::Minimap) => self.jump_to_minimap(pos),
                Some(Drag::Map(last)) => {
                    let (sx, sy) = self.view.pan_scale();
                    self.pan(
                        (i32::from(last.x) - i32::from(pos.x)) * sx,
                        (i32::from(last.y) - i32::from(pos.y)) * sy,
                    );
                    self.drag = Some(Drag::Map(pos));
                }
                None => {}
            },
            MouseEventKind::ScrollUp if on_map => self.pan(0, -2 * self.view.pan_scale().1),
            MouseEventKind::ScrollDown if on_map => self.pan(0, 2 * self.view.pan_scale().1),
            _ => {}
        }
    }

    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            match event {
                Event::Close => return false,
                Event::Key(k) if k.is_down() => {
                    if !self.handle_key(k.code, k.modifiers) {
                        return false;
                    }
                }
                Event::Mouse(m) => self.handle_mouse(m.kind, m.position),
                _ => {}
            }
        }
        true
    }

    // ── Drawing ──────────────────────────────────────────────────────────────

    fn draw_map<B: Backend>(&mut self, term: &mut Terminal<B>, area: Rect) {
        if area.width() == 0 || area.height() == 0 {
            return;
        }
        self.last_map_rect = Some(area);
        match self.view {
            View::Cells | View::SquareGrid | View::HexGrid => self.draw_cells(term, area),
            View::Squares => self.draw_tiles(term, area, false),
            View::Hexes => self.draw_tiles(term, area, true),
        }
    }

    /// The per-cell terrain view (see [`World::render_cell`]), which also serves the two grid
    /// overlays: those render every cell identically and then re-tint backgrounds along province
    /// boundaries (see [`grid_overlay`]).
    fn draw_cells<B: Backend>(&mut self, term: &mut Terminal<B>, area: Rect) {
        self.camera.set_viewport(area);
        self.camera.center_on(self.cam_center);
        self.visible_world = self.camera.visible_bounds();

        // `Some(hex?)` in the grid overlay views, `None` in plain Cells.
        let grid = match self.view {
            View::SquareGrid => Some(false),
            View::HexGrid => Some(true),
            _ => None,
        };
        let selected =
            grid.map(|hex| grid_region(hex, self.cam_center.x.into(), self.cam_center.y.into()));

        for (world_pos, screen_pos) in self.camera.cells() {
            let (glyph, mut style) = self.world.render_cell(world_pos, self.time);
            if let (Some(hex), Some(selected)) = (grid, selected) {
                style = grid_overlay(style, hex, world_pos, selected);
            }
            term.put_styled(screen_pos.x, screen_pos.y, glyph, style);
        }

        // The reticle: a soft highlight on the cell the sidebar/status line is describing, so
        // there's always a clear answer to "where, exactly, is that readout talking about".
        if let Some(screen) = self.camera.world_to_screen(self.cam_center) {
            let (glyph, style) = self.world.render_cell(self.cam_center, self.time);
            let highlighted = style.bg(Color::lerp(style.background(), RETICLE, 0.55));
            term.put_styled(screen.x, screen.y, glyph, highlighted);
        }
    }

    /// Shared driver for both strategic tile views: refreshes the cached [`TileMap`] if the
    /// seed or stagger changed, then hands off to the mode's renderer.
    fn draw_tiles<B: Backend>(&mut self, term: &mut Terminal<B>, area: Rect, hex: bool) {
        let key = (self.world.seed(), hex);
        if self.tile_cache.as_ref().is_none_or(|(k, _)| *k != key) {
            self.tile_cache = Some((key, self.world.tile_map(hex)));
        }
        let Some((_, map)) = self.tile_cache.as_ref() else {
            return;
        };
        let vis = if hex {
            draw_hex_tiles(term, area, map, self.cam_center, self.time)
        } else {
            draw_square_tiles(term, area, map, self.cam_center, self.time)
        };
        self.visible_world = vis;
    }

    fn draw_minimap<B: Backend>(&mut self, term: &mut Terminal<B>, area: Rect) {
        if area.width() == 0 || area.height() == 0 {
            return;
        }
        self.last_minimap_rect = Some(area);
        for row in 0..area.height() {
            for col in 0..area.width() {
                let glyph = self
                    .world
                    .minimap_swatch(col, row, area.width(), area.height());
                term.put_styled(
                    area.left() + col,
                    area.top() + row,
                    glyph.ch,
                    Style::new().fg(glyph.fg).bg(glyph.bg),
                );
            }
        }

        // Overlay the visible-world rectangle so the minimap doubles as a "you are here, and
        // this is how much of the map fits on screen" indicator, whichever view drew the map.
        let vis = self.visible_world;
        if vis.width() == 0 || vis.height() == 0 {
            term.reset_style();
            return;
        }
        let to_col = |x: u16| {
            (u32::from(x) * u32::from(area.width()) / u32::from(WORLD_W))
                .min(u32::from(area.width()) - 1) as u16
        };
        let to_row = |y: u16| {
            (u32::from(y) * u32::from(area.height()) / u32::from(WORLD_H))
                .min(u32::from(area.height()) - 1) as u16
        };
        let (x0, x1) = (
            to_col(vis.left()),
            to_col(vis.right().saturating_sub(1).max(vis.left())),
        );
        let (y0, y1) = (
            to_row(vis.top()),
            to_row(vis.bottom().saturating_sub(1).max(vis.top())),
        );
        let style = Style::new().fg(ACCENT);
        for x in x0..=x1 {
            term.put_styled(area.left() + x, area.top() + y0, '─', style);
            term.put_styled(area.left() + x, area.top() + y1, '─', style);
        }
        for y in y0..=y1 {
            term.put_styled(area.left() + x0, area.top() + y, '│', style);
            term.put_styled(area.left() + x1, area.top() + y, '│', style);
        }
        for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
            term.put_styled(area.left() + x, area.top() + y, '+', style);
        }
        term.reset_style();
    }

    fn draw_sidebar<B: Backend>(&mut self, term: &mut Terminal<B>, area: Rect) {
        Panel::new()
            .title(" OVERWORLD ")
            .border_style(Style::new().fg(BORDER).bg(PANEL_BG))
            .fill_style(Style::new().bg(PANEL_BG))
            .render(area, term);
        let inner = Rect::new(
            area.left() + 1,
            area.top() + 1,
            area.width().saturating_sub(2),
            area.height().saturating_sub(2),
        );
        if inner.width() == 0 || inner.height() == 0 {
            return;
        }
        let w = inner.width_usize();
        let mut y = inner.top();

        term.reset_style().fg(DIM_FG).bg(PANEL_BG);
        term.print(
            inner.left(),
            y,
            truncate(
                &format!(
                    "seed {}  ({}, {})",
                    self.world.seed(),
                    self.cam_center.x,
                    self.cam_center.y
                ),
                w,
            ),
        );
        y += 1;

        let region = self.world.region_at(self.cam_center).to_owned();
        term.reset_style().fg(FG).bg(PANEL_BG);
        term.print(inner.left(), y, truncate(&region, w));
        y += 1;

        let label = self.world.label_at(self.cam_center);
        term.reset_style().fg(ACCENT).bg(PANEL_BG);
        term.print(inner.left(), y, truncate(&label, w));
        y += 1;

        let elev_pct = self.world.elevation_pct(self.cam_center);
        term.reset_style().fg(DIM_FG).bg(PANEL_BG);
        term.print(
            inner.left(),
            y,
            truncate(&format!("elevation ~{elev_pct:.0}%"), w),
        );
        y += 1;

        term.print(
            inner.left(),
            y,
            truncate(&format!("view: {} [T]", self.view.label()), w),
        );
        y += 2;

        if inner.height() >= MINIMAP_H + 16 {
            self.draw_minimap(term, Rect::new(inner.left(), y, inner.width(), MINIMAP_H));
            y += MINIMAP_H + 1;
        }

        term.reset_style().fg(FG).bg(PANEL_BG);
        term.print(inner.left(), y, truncate("Legend", w));
        y += 1;
        for (glyph, color, name) in World::legend() {
            if y >= inner.bottom() - 2 {
                break;
            }
            term.reset_style().fg(color).bg(PANEL_BG);
            term.put(inner.left(), y, glyph);
            term.reset_style().fg(DIM_FG).bg(PANEL_BG);
            term.print(inner.left() + 2, y, truncate(name, w.saturating_sub(2)));
            y += 1;
        }

        term.reset_style().fg(DIM_FG).bg(PANEL_BG);
        term.print(
            inner.left(),
            inner.bottom() - 1,
            truncate("drag pans, T tiles, R reroll", w),
        );
        term.reset_style();
    }

    fn draw_status<B: Backend>(&self, term: &mut Terminal<B>, area: Rect) {
        if area.height() == 0 {
            return;
        }
        for x in area.left()..area.right() {
            term.put_styled(x, area.top(), ' ', Style::new().bg(PANEL_BG));
        }
        let label = self.world.label_at(self.cam_center);
        let text = format!(
            "({}, {})  {label}  -- arrows pan, T view, R rerolls, Q quits",
            self.cam_center.x, self.cam_center.y
        );
        term.reset_style().fg(FG).bg(PANEL_BG);
        term.print(
            area.left() + 1,
            area.top(),
            truncate(&text, area.width_usize().saturating_sub(1)),
        );
        term.reset_style();
    }

    /// Draws this frame and presents it. `pub` (unlike this example's other `draw_*` helpers) so
    /// the sibling test file can prime layout state (`last_map_rect`/`last_minimap_rect`) with a
    /// single draw before driving synthetic input at it.
    pub fn draw<B: Backend>(&mut self, term: &mut Terminal<B>) {
        let size = term.size();
        let screen = Rect::new(0, 0, size.width, size.height);
        // Cleared unconditionally and only re-set inside `draw_minimap` if it actually runs this
        // frame, so a resize that drops the sidebar (or just the minimap) can't leave a stale
        // rect around for `jump_to_minimap` to misfire against.
        self.last_minimap_rect = None;
        for y in 0..size.height {
            for x in 0..size.width {
                term.put_styled(x, y, ' ', Style::new().bg(BG));
            }
        }

        let wide = size.width >= BP_SIDEBAR && size.height >= BP_TALL;
        if wide {
            let cols = split_h(screen, &[Constraint::Fill(1), Constraint::Fixed(SIDEBAR_W)]);
            self.draw_map(term, cols[0]);
            self.draw_sidebar(term, cols[1]);
        } else if size.height >= 2 {
            let map_area = Rect::new(0, 1, size.width, size.height - 1);
            self.draw_map(term, map_area);
            self.draw_status(term, Rect::new(0, 0, size.width, 1));
        } else {
            self.draw_map(term, screen);
        }

        term.present().ok();
    }
}

// ── Strategic tile rendering ─────────────────────────────────────────────────

/// A `(dx, dy)` cell offset into a tile's screen footprint, marking where a road or river
/// connector is drawn.
type MarkCell = (i32, i32);

/// Square-view neighbor links: `(dcol, drow, road mark cell, river mark cell)` -- where in the
/// [`SQ_W`]x[`SQ_H`] block the connector toward that neighbor goes. Road and river marks for
/// the same edge sit on different cells so a tile that has both shows both.
const SQ_LINKS: [(i32, i32, MarkCell, MarkCell); 4] = [
    (0, -1, (4, 0), (3, 0)),
    (0, 1, (4, 3), (3, 3)),
    (-1, 0, (0, 1), (0, 2)),
    (1, 0, (7, 1), (7, 2)),
];

/// Hex-view neighbor links: `(direction, road mark cell, river mark cell)`, with cells given as
/// `(dx, dy)` offsets into the hex footprint (`dy` 0/2 are the taper rows, 1 the middle row).
/// The six [`Direction`]s come from `hexal`; roads sit toward the corner of each edge, rivers
/// just inside them. The cells are chosen so that a connector and its counterpart on the
/// neighbor land on adjacent screen cells (E/W share a row; the diagonal pairs meet on the
/// shared taper row), so a route reads as a continuous dotted line, not scattered specks.
const HEX_LINKS: [(Direction, MarkCell, MarkCell); 6] = [
    (Direction::E, (7, 1), (6, 1)),
    (Direction::W, (0, 1), (1, 1)),
    (Direction::NE, (5, 0), (4, 0)),
    (Direction::NW, (2, 0), (3, 0)),
    (Direction::SE, (5, 2), (4, 2)),
    (Direction::SW, (2, 2), (3, 2)),
];

/// [`Terminal::put_styled`] with clipping to `area`: tiles at the map edges legitimately hang
/// partly outside the viewport, so every tile-view write funnels through this.
fn put_clipped<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    x: i32,
    y: i32,
    glyph: char,
    style: Style,
) {
    if x >= i32::from(area.left())
        && x < i32::from(area.right())
        && y >= i32::from(area.top())
        && y < i32::from(area.bottom())
    {
        term.put_styled(x as u16, y as u16, glyph, style);
    }
}

/// Resolved face and glyph colors for one drawn tile: the flat face color (darkened toward the
/// scene background, with the reticle highlight already mixed in) and the glyph foreground
/// (with the per-tile water swell applied -- the tile views' one surviving animation).
fn tile_face(tile: &world::Tile, highlight: f32, col: u16, row: u16, time: f64) -> (Color, Color) {
    let mut face = Color::lerp(tile.base, BG, 0.34);
    if highlight > 0.0 {
        face = Color::lerp(face, RETICLE, highlight);
    }
    let mut glyph_fg = tile.glyph_color;
    if tile.water {
        let swell =
            ((time * 1.1 + f64::from(col) * 0.9 + f64::from(row) * 1.4).sin() * 0.5 + 0.5) as f32;
        glyph_fg = Color::lerp(glyph_fg, Color::WHITE, 0.10 + swell * 0.25);
    }
    (face, glyph_fg)
}

// ── Grid overlays ────────────────────────────────────────────────────────────────

/// Stagger, in world cells, of a hex-lattice row: odd rows shift right half a hex -- the same
/// `OddR` convention as everything else hex in this example. `rem_euclid` so out-of-world
/// fragment rows above the map keep a consistent identity.
fn hex_row_stagger(row: i32) -> i32 {
    if row.rem_euclid(2) == 1 {
        i32::from(TILE_W / 2)
    } else {
        0
    }
}

/// Which province of the overlay grid owns world cell `(x, y)` -- squares when `hex` is false,
/// the honeycomb otherwise.
///
/// The hex partition is the same lattice the aggregated hex view uses ([`TILE_W`]-wide,
/// [`TILE_H`]-row pitch, odd rows staggered right), but as the *exact* hexagon footprint
/// rather than that view's rectangular sampling approximation: scaled up from the screen-space
/// honeycomb [`HEX_ROW_H`] describes, each hex has a half-height taper band at its top and
/// bottom shared with its vertical neighbors (rows split there between the hex whose neck the
/// cell is under and the staggered one beside it), and every interior hex owns exactly
/// `TILE_W * TILE_H` cells. Returns lattice `(col, row)`, possibly outside the world for
/// border fragments -- callers only compare identities, so that's harmless.
fn grid_region(hex: bool, x: i32, y: i32) -> (i32, i32) {
    let w = i32::from(TILE_W);
    let h = i32::from(TILE_H);
    if !hex {
        return (x.div_euclid(w), y.div_euclid(h));
    }
    let indent = w / 4;
    let band = y.div_euclid(h / 2);
    if band.rem_euclid(2) == 1 {
        // The middle band: unambiguously this row's hex.
        let row = band.div_euclid(2);
        ((x - hex_row_stagger(row)).div_euclid(w), row)
    } else {
        // A shared taper band: cells under the neck of `below`'s hexes belong to them; the
        // staggered cells between necks belong to the previous row's bottom tapers.
        let below = band.div_euclid(2);
        let cx = x - hex_row_stagger(below) - indent;
        if cx.rem_euclid(w) < w / 2 {
            (cx.div_euclid(w), below)
        } else {
            let above = below - 1;
            ((x - hex_row_stagger(above) - indent).div_euclid(w), above)
        }
    }
}

/// Applies the grid-overlay tint for one world cell: terrain glyph and colors stay, only the
/// background shifts. A cell is a border cell iff its north or west neighbor lies in a
/// different province -- checking just those two sides keeps every boundary one cell thick, and
/// gives hex edges clean staircases with no line glyphs. The selected province's border gets
/// the reticle color, its neighbors' borders (hexal distance 1, or the 4-neighborhood for
/// squares) a fainter ring, and its interior a whisper of tint so the territory reads as a
/// whole.
fn grid_overlay(style: Style, hex: bool, pos: Pos, selected: (i32, i32)) -> Style {
    let (x, y) = (i32::from(pos.x), i32::from(pos.y));
    let region = grid_region(hex, x, y);
    let border = (x > 0 && grid_region(hex, x - 1, y) != region)
        || (y > 0 && grid_region(hex, x, y - 1) != region);
    let dist = if hex {
        let a = OffsetHex::<i32, OddR>::new(region.0, region.1).to_hex();
        let b = OffsetHex::<i32, OddR>::new(selected.0, selected.1).to_hex();
        a.distance(b)
    } else {
        (region.0 - selected.0).abs() + (region.1 - selected.1).abs()
    };
    let bg = style.background();
    if border {
        let (line, strength) = match dist {
            0 => (RETICLE, 0.70),
            1 => (RETICLE, 0.30),
            _ => (GRID_LINE, 0.30),
        };
        style.bg(Color::lerp(bg, line, strength))
    } else if dist == 0 {
        style.bg(Color::lerp(bg, RETICLE, 0.10))
    } else {
        style
    }
}

/// A stable per-tile hash for decorative choices, mirroring what `noise::hash01` does for world
/// cells: keyed on absolute tile coordinates so the pattern doesn't crawl when the view pans.
/// Both tile renderers use its low bits to scatter dim copies of the tile's glyph -- enough
/// texture that a province reads as "made of forest" rather than a flat slab, dim enough
/// (foreground pulled most of the way to the face color) not to compete with the real markers.
const fn tile_hash(col: u16, row: u16) -> u32 {
    (col as u32).wrapping_mul(0x9E37_79B9) ^ (row as u32).wrapping_mul(0x85EB_CA6B)
}

/// Top-left visible tile and the world rect that view implies -- the shared "camera" math of
/// both tile views: center the reticle's tile, clamp to the tile grid the way
/// [`Camera::center_on`] clamps to world cells.
fn tile_view_origin(map: &TileMap, center: Pos, fit_x: u16, fit_y: u16) -> (u16, u16, Rect) {
    let (fit_x, fit_y) = (fit_x.min(map.cols()), fit_y.min(map.rows()));
    let (ccol, crow) = map.tile_of(center);
    let origin_col = ccol
        .saturating_sub(fit_x / 2)
        .min(map.cols().saturating_sub(fit_x));
    let origin_row = crow
        .saturating_sub(fit_y / 2)
        .min(map.rows().saturating_sub(fit_y));
    let x = origin_col * TILE_W;
    let y = origin_row * TILE_H;
    let vis = Rect::new(
        x,
        y,
        (fit_x * TILE_W).min(WORLD_W.saturating_sub(x)),
        (fit_y * TILE_H).min(WORLD_H.saturating_sub(y)),
    );
    (origin_col, origin_row, vis)
}

/// Beveled face color for one cell of a square tile: lit top edge, shadowed bottom, faint
/// left/right relief -- the same northwest-light convention as the terrain hillshade, and what
/// separates two adjacent tiles of the same biome without spending cells on border glyphs.
fn square_bevel(face: Color, dx: i32, dy: i32) -> Color {
    let mut c = face;
    if dy == 0 {
        c = Color::lerp(c, Color::WHITE, 0.16);
    } else if dy == i32::from(SQ_H) - 1 {
        c = Color::lerp(c, Color::BLACK, 0.34);
    }
    if dx == 0 {
        c = Color::lerp(c, Color::WHITE, 0.08);
    } else if dx == i32::from(SQ_W) - 1 {
        c = Color::lerp(c, Color::BLACK, 0.20);
    }
    c
}

/// Beveled face color for one cell of a hex tile: lit top taper, shadowed bottom taper, faintly
/// shadowed middle-row ends. Same job as [`square_bevel`], shaped to the hex footprint.
fn hex_bevel(face: Color, dx: i32, dy: i32) -> Color {
    match dy {
        0 => Color::lerp(face, Color::WHITE, 0.16),
        2 => Color::lerp(face, Color::BLACK, 0.34),
        _ if dx == 0 || dx == i32::from(HEX_W) - 1 => Color::lerp(face, Color::BLACK, 0.18),
        _ => face,
    }
}

/// Draws the square strategic-tile view into `area` and returns the visible world rect (for
/// the minimap overlay). The reticle's tile gets a strong highlight and its 4-neighborhood a
/// faint one -- the square counterpart of the hex view's adjacency ring.
fn draw_square_tiles<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    map: &TileMap,
    center: Pos,
    time: f64,
) -> Rect {
    let (ccol, crow) = map.tile_of(center);
    let (origin_col, origin_row, vis) = tile_view_origin(
        map,
        center,
        (area.width() / SQ_W).max(1),
        (area.height() / SQ_H).max(1),
    );
    let row_end = map.rows().min(origin_row + area.height() / SQ_H + 1);
    let col_end = map.cols().min(origin_col + area.width() / SQ_W + 1);
    for row in origin_row..row_end {
        for col in origin_col..col_end {
            let Some(tile) = map.get(i32::from(col), i32::from(row)) else {
                continue;
            };
            let sx = i32::from(area.left()) + i32::from(col - origin_col) * i32::from(SQ_W);
            let sy = i32::from(area.top()) + i32::from(row - origin_row) * i32::from(SQ_H);
            let dist =
                (i32::from(col) - i32::from(ccol)).abs() + (i32::from(row) - i32::from(crow)).abs();
            let highlight = match dist {
                0 => 0.55,
                1 => 0.18,
                _ => 0.0,
            };
            let (face, glyph_fg) = tile_face(tile, highlight, col, row, time);

            for dy in 0..i32::from(SQ_H) {
                for dx in 0..i32::from(SQ_W) {
                    let bg = square_bevel(face, dx, dy);
                    put_clipped(term, area, sx + dx, sy + dy, ' ', Style::new().bg(bg));
                }
            }
            let dim = Color::lerp(tile.glyph_color, face, 0.45);
            let hash = tile_hash(col, row);
            for (bit, (dx, dy)) in [(1, 2), (6, 1)].into_iter().enumerate() {
                if (hash >> bit) & 1 == 1 {
                    let style = Style::new().fg(dim).bg(square_bevel(face, dx, dy));
                    put_clipped(term, area, sx + dx, sy + dy, tile.glyph, style);
                }
            }

            for (dc, dr, road_at, river_at) in SQ_LINKS {
                let Some(neighbor) = map.get(i32::from(col) + dc, i32::from(row) + dr) else {
                    continue;
                };
                if tile.river && neighbor.river {
                    let (dx, dy) = river_at;
                    let bg = Color::lerp(square_bevel(face, dx, dy), TILE_RIVER_FG, 0.40);
                    let style = Style::new().fg(TILE_RIVER_FG).bg(bg);
                    put_clipped(term, area, sx + dx, sy + dy, '~', style);
                }
                if tile.road && neighbor.road {
                    let (dx, dy) = road_at;
                    let bg = Color::lerp(square_bevel(face, dx, dy), TILE_ROAD_FG, 0.40);
                    let style = Style::new().fg(TILE_ROAD_FG).bg(bg);
                    put_clipped(term, area, sx + dx, sy + dy, '·', style);
                }
            }

            let style = Style::new().fg(glyph_fg).bg(square_bevel(face, 3, 1));
            put_clipped(term, area, sx + 3, sy + 1, tile.glyph, style);
            if let Some(poi) = tile.poi {
                let (glyph, color) = poi.glyph_color();
                let style = Style::new().fg(color).bg(square_bevel(face, 4, 1));
                put_clipped(term, area, sx + 4, sy + 1, glyph, style);
            } else if tile.road {
                // A road tile with no landmark carries a center dot, so a route reads as a
                // continuous line through the tile instead of stubs at its edges.
                let style = Style::new().fg(TILE_ROAD_FG).bg(square_bevel(face, 4, 1));
                put_clipped(term, area, sx + 4, sy + 1, '·', style);
            }
        }
    }
    vis
}

/// Draws the hex strategic-tile view into `area` and returns the visible world rect (for the
/// minimap overlay).
///
/// Layout is the interlocking honeycomb [`HEX_ROW_H`] describes; coordinates go through
/// `hexal`: each `(col, row)` converts to an axial [`hexal::Hex`] via the [`OddR`] offset
/// scheme, road/river connectors probe the six [`Direction`] neighbors, and the reticle
/// highlight is hex distance (the selected hex strong, its ring of six faint).
fn draw_hex_tiles<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    map: &TileMap,
    center: Pos,
    time: f64,
) -> Rect {
    let (ccol, crow) = map.tile_of(center);
    let (origin_col, origin_row, vis) = tile_view_origin(
        map,
        center,
        (area.width() / HEX_W).max(1),
        (area.height() / HEX_ROW_H).max(1),
    );
    let selected = OffsetHex::<i32, OddR>::new(i32::from(ccol), i32::from(crow)).to_hex();
    // One extra row/column past every edge: staggered odd rows poke half a hex into the margins,
    // and shared taper rows mean the row above the origin still owns cells on the top screen row.
    let row_end = map.rows().min(origin_row + area.height() / HEX_ROW_H + 2);
    let col_end = map.cols().min(origin_col + area.width() / HEX_W + 1);
    for row in origin_row.saturating_sub(1)..row_end {
        let stag = if row % 2 == 1 {
            i32::from(HEX_W / 2)
        } else {
            0
        };
        for col in origin_col.saturating_sub(1)..col_end {
            let Some(tile) = map.get(i32::from(col), i32::from(row)) else {
                continue;
            };
            let sx = i32::from(area.left())
                + (i32::from(col) - i32::from(origin_col)) * i32::from(HEX_W)
                + stag;
            let sy = i32::from(area.top())
                + (i32::from(row) - i32::from(origin_row)) * i32::from(HEX_ROW_H);
            let hex = OffsetHex::<i32, OddR>::new(i32::from(col), i32::from(row)).to_hex();
            let highlight = match hex.distance(selected) {
                0 => 0.55,
                1 => 0.18,
                _ => 0.0,
            };
            let (face, glyph_fg) = tile_face(tile, highlight, col, row, time);

            // Footprint: taper rows above and below the 8-cell middle row (see [`HEX_ROW_H`]).
            for dx in 2..6 {
                put_clipped(
                    term,
                    area,
                    sx + dx,
                    sy,
                    ' ',
                    Style::new().bg(hex_bevel(face, dx, 0)),
                );
                let bottom = Style::new().bg(hex_bevel(face, dx, 2));
                put_clipped(term, area, sx + dx, sy + 2, ' ', bottom);
            }
            for dx in 0..i32::from(HEX_W) {
                let bg = hex_bevel(face, dx, 1);
                put_clipped(term, area, sx + dx, sy + 1, ' ', Style::new().bg(bg));
            }
            let dim = Color::lerp(tile.glyph_color, face, 0.45);
            let hash = tile_hash(col, row);
            for (bit, (dx, dy)) in [(1, 1), (6, 1)].into_iter().enumerate() {
                if (hash >> bit) & 1 == 1 {
                    let style = Style::new().fg(dim).bg(hex_bevel(face, dx, dy));
                    put_clipped(term, area, sx + dx, sy + dy, tile.glyph, style);
                }
            }

            for (dir, road_at, river_at) in HEX_LINKS {
                let offset = hex.neighbor(dir).to_offset::<OddR>();
                let Some(neighbor) = map.get(offset.col, offset.row) else {
                    continue;
                };
                if tile.river && neighbor.river {
                    let (dx, dy) = river_at;
                    let bg = Color::lerp(hex_bevel(face, dx, dy), TILE_RIVER_FG, 0.40);
                    let style = Style::new().fg(TILE_RIVER_FG).bg(bg);
                    put_clipped(term, area, sx + dx, sy + dy, '~', style);
                }
                if tile.road && neighbor.road {
                    let (dx, dy) = road_at;
                    let bg = Color::lerp(hex_bevel(face, dx, dy), TILE_ROAD_FG, 0.40);
                    let style = Style::new().fg(TILE_ROAD_FG).bg(bg);
                    put_clipped(term, area, sx + dx, sy + dy, '·', style);
                }
            }

            let style = Style::new().fg(glyph_fg).bg(hex_bevel(face, 3, 1));
            put_clipped(term, area, sx + 3, sy + 1, tile.glyph, style);
            if let Some(poi) = tile.poi {
                let (glyph, color) = poi.glyph_color();
                let style = Style::new().fg(color).bg(hex_bevel(face, 4, 1));
                put_clipped(term, area, sx + 4, sy + 1, glyph, style);
            } else if tile.road {
                // Same center-dot trick as the square view: routes read as lines, not stubs.
                let style = Style::new().fg(TILE_ROAD_FG).bg(hex_bevel(face, 4, 1));
                put_clipped(term, area, sx + 4, sy + 1, '·', style);
            }
        }
    }
    vis
}

impl Example for Overworld {
    const NAME: &'static str = "19_overworld";

    #[cfg(feature = "software")]
    fn fill_viewport() -> bool {
        true
    }

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, frame: &Frame) -> bool {
        self.time += frame.delta.as_secs_f64();
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(Overworld);

#[cfg(test)]
mod grid_tests {
    use super::*;

    #[test]
    fn hex_regions_partition_the_plane_into_equal_areas() {
        let mut counts = std::collections::HashMap::new();
        for y in 0..220 {
            for x in 0..220 {
                *counts.entry(grid_region(true, x, y)).or_insert(0u32) += 1;
            }
        }
        // Interior hexes (fully inside the sampled window) must each own exactly one tile's
        // worth of cells -- the partition neither overlaps nor leaks.
        for col in 2..10 {
            for row in 2..24 {
                assert_eq!(
                    counts.get(&(col, row)).copied(),
                    Some(u32::from(TILE_W) * u32::from(TILE_H)),
                    "hex ({col}, {row}) area"
                );
            }
        }
    }

    #[test]
    fn hex_region_borders_touch_only_hexal_neighbors() {
        // Any two 4-adjacent cells in different provinces must straddle hexes at hexal
        // distance 1 -- the geometric partition and hexal's adjacency model must agree, or the
        // ring highlight and the connector logic would be lying.
        for y in 0..200 {
            for x in 0..200 {
                let a = grid_region(true, x, y);
                for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                    let b = grid_region(true, nx, ny);
                    if a != b {
                        let ha = OffsetHex::<i32, OddR>::new(a.0, a.1).to_hex();
                        let hb = OffsetHex::<i32, OddR>::new(b.0, b.1).to_hex();
                        assert_eq!(
                            ha.distance(hb),
                            1,
                            "cells ({x}, {y})/({nx}, {ny}) straddle non-adjacent hexes {a:?}/{b:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn square_regions_match_tile_blocks() {
        assert_eq!(grid_region(false, 0, 0), (0, 0));
        assert_eq!(
            grid_region(false, i32::from(TILE_W) - 1, i32::from(TILE_H) - 1),
            (0, 0)
        );
        assert_eq!(
            grid_region(false, i32::from(TILE_W), i32::from(TILE_H)),
            (1, 1)
        );
        assert_eq!(
            grid_region(false, i32::from(TILE_W) * 3 + 5, i32::from(TILE_H) * 2 + 3),
            (3, 2)
        );
    }
}
