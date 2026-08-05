# Research: Compositing Color Space for Sprite Alpha Blending

## Summary

Retroglyph composites sprites in **sRGB-encoded (gamma) space**, on every backend, deliberately.
This is not the physically correct way to composite: source-over should interpolate in linear light,
and interpolating encoded values makes a partially transparent edge come out darker than it should.
The deviation is kept because the alternative is worse for this library's use case, not because
nobody noticed.

Three things drive the verdict. Retroglyph's sprites are pixel art authored in tools that themselves
blend in gamma space, so today's output reproduces what the artist saw while choosing an alpha
value. The 2D ecosystem is uniformly gamma-space, and the one comparable Rust project to try linear
reverted it. And the change would cost bit-exact cross-backend parity, which is enforced by tests
and is worth more than the correction is.

Text is unaffected entirely, and by construction rather than by luck. See
[Text is exempt](#text-is-exempt).

Verdict: **rejected**, tracked in retroglyph#1178. Revisit only if the assumptions in
[When to revisit](#when-to-revisit) stop holding.

## 1. The defect, stated precisely

Source-over with straight alpha is

```text
out = src * a + dst * (1 - a)
```

That interpolation is only physically meaningful when `src` and `dst` are proportional to light
intensity. sRGB values are not: they carry a roughly 2.2-power transfer function. Interpolating them
directly weights the darker endpoint too heavily, so a half-transparent bright pixel over a dark
background lands lower than the true half-intensity point.

Where it happens:

| Crate                 | Site                                                                         |
| --------------------- | ---------------------------------------------------------------------------- |
| `retroglyph-software` | `U8x4Rgba::source_over` from the sprite blit in `src/lib.rs`                 |
| `retroglyph-gl`       | the sprite pass's `glBlendFuncSeparate(SRC_ALPHA, ONE_MINUS_SRC_ALPHA, ...)` |
| `retroglyph-wgpu`     | `SPRITE_BLEND` in `src/renderer.rs`                                          |

All three are non-sRGB render targets, so no hardware transfer happens either. The three agree with
each other exactly, which is the point.

## 2. Measured magnitude

Against the bundled example assets, comparing each intermediate-alpha pixel blended in gamma space
against the same pixel blended in linear light:

| Asset                         | Pixels with `0 < a < 255` | Distinct alpha levels |
| ----------------------------- | ------------------------- | --------------------- |
| `examples/assets/chest.png`   | 0 of 1024 (0.00%)         | none, fully binary    |
| `examples/assets/tileset.png` | 74 of 512 (14.45%)        | 2 (`160`, `230`)      |

For those 74 pixels:

| Background | Max channel delta | Mean channel delta |
| ---------- | ----------------- | ------------------ |
| black      | 46/255            | 16.4/255           |
| mid-gray   | 10/255            | 3.5/255            |
| white      | 37/255            | 8.2/255            |

Two things to take from this. The error is real and can be visible against dark backgrounds, which
is the common roguelike case. And it is confined to assets that use intermediate alpha at all:
`chest.png`, a typical hand-drawn pixel-art sprite, has none, and `tileset.png` uses exactly two
hand-picked levels rather than a soft gradient.

Reproduce with `python3` and Pillow: decode each PNG to RGBA, blend every `0 < a < 255` pixel over a
constant background both ways, and diff.

## 3. What comparable projects do

| Project                                                     | Direction       | Outcome                                                                          |
| ----------------------------------------------------------- | --------------- | -------------------------------------------------------------------------------- |
| [egui](https://github.com/emilk/egui/pull/2071)             | linear to gamma | Reverted. All color operations moved back to gamma space.                        |
| [Ghostty](https://github.com/ghostty-org/ghostty/pull/4686) | sRGB to linear  | Draft, marked DO NOT MERGE.                                                      |
| Kitty                                                       | linear          | Shipped, and had to add a gamma/contrast setting to undo the thinning it caused. |
| Bevy                                                        | linear          | Added a per-camera option rather than picking one.                               |
| HTML Canvas, CSS, SVG                                       | gamma           | Specified: canvas surfaces are sRGB and compositing happens there.               |
| Photoshop, Aseprite                                         | gamma           | Default. Aseprite blends straight on stored bytes.                               |

The pattern is that linear-light compositing is standard in 3D rendering and film (OpenEXR, ACES),
and gamma-space compositing is standard in 2D authoring and UI. Retroglyph is squarely in the second
group.

Ghostty's draft records the practical problem directly: correct linear blending makes glyphs look
thinner than users expect, enough that Kitty needed a compensating knob. That specific failure does
not apply here, because retroglyph's text never carries intermediate alpha, but it shows that
"physically correct" and "looks right" diverge in exactly this area.

## 4. Why not just fix it

### Authoring fidelity points the other way

`tileset.png` uses two alpha levels, `160` and `230`. Those were chosen by a person looking at a
preview in an editor that blends in gamma space. Retroglyph currently reproduces that preview
exactly. Moving to linear light would make the library disagree with the tool the art was drawn in,
which for a pixel-art library is a regression in the property that actually matters.

### It costs bit-exact cross-backend parity

`crates/gl/src/headless.rs` and `crates/wgpu/src/headless.rs` compare GPU output against the
software rasterizer pixel for pixel, exactly. A transfer function cannot be evaluated identically on
a CPU and an arbitrary GPU:

- A 256-entry decode table is exact, but the reverse direction is not: Khronos's `EXT_texture_sRGB`
  states that mapping linear values back to all 256 sRGB encodings needs at least 4096 entries.
- Evaluating the curve analytically instead makes results depend on operation ordering. Rounding
  once at the end versus rounding per step differs on 203 of 256 decode entries by 1 ULP, purely
  from non-commuting rounding.
- GPU `pow`/`exp`/`log` are not required to be correctly rounded and may differ across vendors
  (`ARB_shader_precision`, the CUDA floating-point appendix). Hardware sRGB render targets are a
  third independent implementation with their own error budget.

So a naive linear port replaces a small consistent error with a tolerance-based parity suite, and
1-LSB backend bugs stop being detectable.

There is a route that would preserve exactness, for the record: ship the transfer function as data
rather than math. Decode is exact at 256 entries, the blend can run in 16-bit fixed-point linear,
and encode can go through a 65536-entry table (64 KB on the CPU, a 256x256 texture on the GPU). Both
backends would index identical tables and no `pow` would be evaluated anywhere. It is viable and it
is not cheap, and it is only worth building if the underlying change is worth making, which it
currently isn't.

### `alpha-blend` is the wrong home for it

`retroglyph-software` blends through
[`alpha-blend`](https://github.com/crates-lurey-io/alpha-blend), whose design point is exact
zero-tolerance integer arithmetic (it tests all 130k `div255` inputs). A lossy, platform-dependent
transcendental step conflicts with that contract. Any linear-light code retroglyph ever needs should
live in retroglyph.

## Text is exempt

Glyph coverage is strictly `0x00` or `0xFF`. `AtlasData` writes only those two values, the atlas is
sampled `Nearest` with multisampling off, and quads are integer aligned at every supported scale,
including with a sub-cell `dx`/`dy` offset (the offset is unscaled font pixels times an integer
scale). The alpha reaching the blend unit for a glyph or a background is therefore only ever exactly
0 or 1, source-over resolves to exactly one endpoint, and every color space agrees bit for bit.

This is load-bearing, so it is pinned by `coverage_is_strictly_binary` in
`crates/window/src/atlas.rs` rather than left as an observation. An antialiased or grayscale-AA
glyph source would fail that test, which is the intended outcome: it would pull text into this
problem across all three backends at once and needs to be a deliberate decision.

## When to revisit

- Antialiased or grayscale-AA glyph rasterization becomes a goal. Text stops being exempt and the
  calculus changes completely.
- Sprites with genuinely soft alpha (photographic edges, smooth gradients, imported non-pixel-art)
  become a supported use case rather than an edge case.
- Someone reports a visible artifact from it in practice. Nobody has.

If it is revisited, the shared-lookup-table route above is the design to start from, because it is
the only one that keeps cross-backend exactness.
