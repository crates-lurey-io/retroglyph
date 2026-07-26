//! GLSL shader sources for the instanced cell renderer, and version-prefixing so the exact same
//! shader bodies compile on both desktop GL 3.3 core (`#version 330 core`) and WebGL2 / GL ES 3.0
//! (`#version 300 es`).
//!
//! Rendering model (instanced quads, one draw call per frame):
//!
//! - A single unit quad (4 corners, 6 indices) is drawn `cols * rows` times via
//!   `draw_elements_instanced`.
//! - Per-instance attributes (divisor 1) carry the glyph's atlas *slot*, foreground/background
//!   RGB, the sub-cell offset, and compositing flags. There is no per-instance position: the
//!   vertex shader derives `(col, row)` from `gl_InstanceID` and a `u_cols` uniform.
//! - The glyph atlas is a `sampler2DArray` (`R8` coverage) with glyphs grid-packed
//!   `u_atlas_cols`x`u_atlas_rows` per layer (issue #367). The fragment shader unpacks the
//!   per-instance slot id into a `(layer, column, row)` sub-rect, samples its coverage, and blends
//!   foreground over background.
//! - The per-instance `a_flags` bits (has-background, has-glyph) drive a `discard` in each pass, so
//!   the same shader composites multiple grid layers back-to-front: a transparent background or an
//!   empty glyph in a higher layer is discarded and the layer beneath shows through (issue #368).

// `pub(crate)` items in this private module are the crate-internal shader API; the nursery
// `redundant_pub_crate` lint conflicts with keeping the module boundary explicit.
#![allow(clippy::redundant_pub_crate)]

/// Whether to emit a WebGL2 / GL ES 3.0 header (`#version 300 es` + precision qualifiers) or a
/// desktop GL 3.3 core header (`#version 330 core`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GlslFlavor {
    /// Desktop OpenGL 3.3 core profile (`#version 330 core`).
    ///
    /// Never constructed on wasm (WebGL2 is always `Es300`), hence the target-gated `dead_code`
    /// allow.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Desktop330,
    /// WebGL2 / OpenGL ES 3.0 (`#version 300 es`, explicit precision qualifiers required).
    Es300,
}

/// Vertex shader body (no `#version` line, that is prepended by [`source`]).
const VERTEX_BODY: &str = r"
layout(location = 0) in vec2  a_corner; // unit-quad corner in [0,1], also the in-cell glyph UV
layout(location = 1) in uint  a_glyph;  // atlas layer (glyph id), per instance
layout(location = 2) in vec3  a_fg;     // foreground RGB (normalized u8), per instance
layout(location = 3) in vec3  a_bg;     // background RGB (normalized u8), per instance
layout(location = 4) in ivec2 a_offset; // sub-cell (dx, dy) in unscaled font pixels, per instance
layout(location = 5) in uint  a_flags;  // compositing flags: bit0 = has bg, bit1 = has glyph

uniform vec2 u_screen;     // surface size in physical pixels
uniform vec2 u_cell;       // cell size in physical pixels (glyph size * scale)
uniform vec2 u_glyph;      // glyph size in unscaled font pixels, to scale (dx, dy) into pixels
uniform int  u_cols;       // grid columns, to unpack gl_InstanceID into (col, row)
uniform int  u_draw_glyph; // 0 = background pass (no offset), 1 = glyph pass (apply offset)

flat out uint v_glyph;
flat out vec3 v_fg;
flat out vec3 v_bg;
flat out uint v_flags;
out vec2 v_uv;

void main() {
    int col = gl_InstanceID % u_cols;
    int row = gl_InstanceID / u_cols;
    vec2 origin = vec2(float(col), float(row)) * u_cell;
    vec2 px = origin + a_corner * u_cell;
    // On the glyph pass, shift the whole quad by the sub-cell offset so the glyph is free to spill
    // past the cell edge into neighbors. dx/dy are in unscaled font pixels; u_cell / u_glyph is the
    // integer scale, so this converts them to physical pixels. The background pass leaves the quad
    // pinned to the cell, so backgrounds never move.
    if (u_draw_glyph == 1) {
        px += vec2(a_offset) * (u_cell / u_glyph);
    }
    // Pixel space (y-down, origin top-left) -> clip space (y-up). Flipping y here means the atlas
    // can store glyph row 0 first and sample with v_uv.y = a_corner.y (0 at the cell's top).
    vec2 clip = vec2(px.x / u_screen.x * 2.0 - 1.0, 1.0 - px.y / u_screen.y * 2.0);
    gl_Position = vec4(clip, 0.0, 1.0);
    v_uv = a_corner;
    v_glyph = a_glyph;
    v_fg = a_fg;
    v_bg = a_bg;
    v_flags = a_flags;
}
";

/// Fragment shader body (no `#version` line, no precision qualifiers, both are prepended by
/// [`source`] for the ES flavor).
const FRAGMENT_BODY: &str = r"
uniform highp sampler2DArray u_atlas;
uniform int u_draw_glyph; // 0 = background pass, 1 = glyph pass
uniform int u_atlas_cols; // glyph columns packed per atlas layer
uniform int u_atlas_rows; // glyph rows packed per atlas layer

flat in uint v_glyph;
flat in vec3 v_fg;
flat in vec3 v_bg;
flat in uint v_flags;
in vec2 v_uv;

out vec4 frag;

void main() {
    if (u_draw_glyph == 0) {
        // Background pass: the cell's opaque background. A cell with no background (a transparent
        // cell in a higher layer) is discarded so the layer beneath shows through.
        if ((v_flags & 1u) == 0u) {
            discard;
        }
        frag = vec4(v_bg, 1.0);
    } else {
        // Glyph pass: foreground with atlas coverage as alpha, so non-glyph texels are transparent
        // and the background (or a neighbor's spilled glyph) shows through when blended. An empty
        // cell (no glyph) is discarded so it can't erase the layer beneath.
        if ((v_flags & 2u) == 0u) {
            discard;
        }
        // The glyph id is a flat atlas slot; unpack it into a (layer, column, row) sub-rect within
        // the grid-packed TEXTURE_2D_ARRAY (issue #367) and sample that cell with the in-cell UV.
        uint perLayer = uint(u_atlas_cols * u_atlas_rows);
        uint layer = v_glyph / perLayer;
        uint within = v_glyph % perLayer;
        float gcol = float(within % uint(u_atlas_cols));
        float grow = float(within / uint(u_atlas_cols));
        vec2 uv = (vec2(gcol, grow) + v_uv) / vec2(float(u_atlas_cols), float(u_atlas_rows));
        float coverage = texture(u_atlas, vec3(uv, float(layer))).r;
        frag = vec4(v_fg, coverage);
    }
}
";

/// Vertex shader body for the RGBA sprite pass (issue #366). Unlike the glyph shader, sprite
/// instances carry an explicit grid cell (sprite cells are sparse, so `gl_InstanceID` can't derive
/// it) and a sprite pixel size, so the quad scales to the sprite, which may exceed one cell and
/// spill into neighbours, exactly like `retroglyph-software`'s sprite blit.
// Integer attribute signedness must match the vertex-array data type or WebGL2/SwiftShader raises
// INVALID_OPERATION at draw: `a_cell`/`a_layer`/`a_sprite` are fed UNSIGNED_SHORT so they are
// `uvec`, and `a_offset` is fed signed SHORT so it stays `ivec2`.
#[cfg(feature = "tilesets")]
const VERTEX_SPRITE_BODY: &str = r"
layout(location = 0) in vec2  a_corner; // unit-quad corner in [0,1]
layout(location = 1) in uvec2 a_cell;   // grid (col, row) of the sprite's top-left cell
layout(location = 2) in uint  a_layer;  // sprite atlas array layer
layout(location = 3) in uvec2 a_sprite; // sprite size in unscaled pixels (may exceed a cell)
layout(location = 4) in ivec2 a_offset; // sub-cell (dx, dy) in unscaled pixels

uniform vec2 u_screen;     // surface size in physical pixels
uniform vec2 u_cell;       // cell size in physical pixels (glyph size * scale)
uniform vec2 u_glyph;      // glyph size in unscaled pixels (u_cell / u_glyph = integer scale)
uniform vec2 u_sprite_tex; // sprite atlas layer size in texels (the max sprite dims)

out vec2 v_uv;
flat out uint v_layer;
flat out vec2 v_uv_scale; // maps the [0,1] quad onto the sprite's sub-rect within its layer

void main() {
    vec2 scale = u_cell / u_glyph;
    vec2 origin = vec2(a_cell) * u_cell + vec2(a_offset) * scale;
    vec2 px = origin + a_corner * (vec2(a_sprite) * scale);
    vec2 clip = vec2(px.x / u_screen.x * 2.0 - 1.0, 1.0 - px.y / u_screen.y * 2.0);
    gl_Position = vec4(clip, 0.0, 1.0);
    v_uv = a_corner;
    v_layer = a_layer;
    v_uv_scale = vec2(a_sprite) / u_sprite_tex;
}
";

/// Fragment shader body for the RGBA sprite pass. Samples the sprite's sub-rect (top-left of its
/// layer, the rest of the layer is transparent padding) and outputs straight-alpha RGBA; the caller
/// enables source-over blending, so a sprite's transparent pixels let the layers below show through.
#[cfg(feature = "tilesets")]
const FRAGMENT_SPRITE_BODY: &str = r"
uniform highp sampler2DArray u_sprites;

in vec2 v_uv;
flat in uint v_layer;
flat in vec2 v_uv_scale;

out vec4 frag;

void main() {
    vec2 uv = v_uv * v_uv_scale;
    frag = texture(u_sprites, vec3(uv, float(v_layer)));
}
";

/// Builds a complete shader source string for `flavor`, prepending the right `#version` line (and,
/// for ES, the precision qualifiers a fragment shader needs).
pub(crate) fn source(flavor: GlslFlavor, body: Shader) -> String {
    let mut out = String::new();
    let is_fragment = match body {
        Shader::Fragment => true,
        #[cfg(feature = "tilesets")]
        Shader::SpriteFragment => true,
        _ => false,
    };
    match flavor {
        GlslFlavor::Desktop330 => out.push_str("#version 330 core\n"),
        GlslFlavor::Es300 => {
            out.push_str("#version 300 es\n");
            // ES requires explicit default precision. The fragment shaders also sample an array
            // texture, so give both float and the sampler a high precision default.
            out.push_str("precision highp float;\nprecision highp int;\n");
            if is_fragment {
                out.push_str("precision highp sampler2DArray;\n");
            }
        }
    }
    out.push_str(match body {
        Shader::Vertex => VERTEX_BODY,
        Shader::Fragment => FRAGMENT_BODY,
        #[cfg(feature = "tilesets")]
        Shader::SpriteVertex => VERTEX_SPRITE_BODY,
        #[cfg(feature = "tilesets")]
        Shader::SpriteFragment => FRAGMENT_SPRITE_BODY,
    });
    out
}

/// Which shader stage to emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shader {
    /// The glyph vertex stage ([`VERTEX_BODY`]).
    Vertex,
    /// The glyph fragment stage ([`FRAGMENT_BODY`]).
    Fragment,
    /// The sprite vertex stage ([`VERTEX_SPRITE_BODY`], issue #366).
    #[cfg(feature = "tilesets")]
    SpriteVertex,
    /// The sprite fragment stage ([`FRAGMENT_SPRITE_BODY`], issue #366).
    #[cfg(feature = "tilesets")]
    SpriteFragment,
}

#[cfg(test)]
mod tests {
    use super::{GlslFlavor, Shader, source};

    #[test]
    fn desktop_header_is_330_core() {
        let vs = source(GlslFlavor::Desktop330, Shader::Vertex);
        assert!(vs.starts_with("#version 330 core\n"));
        assert!(!vs.contains("precision"));
    }

    #[test]
    fn es_fragment_has_precision_and_sampler_precision() {
        let fs = source(GlslFlavor::Es300, Shader::Fragment);
        assert!(fs.starts_with("#version 300 es\n"));
        assert!(fs.contains("precision highp float;"));
        assert!(fs.contains("precision highp sampler2DArray;"));
    }

    #[test]
    fn es_vertex_omits_sampler_precision() {
        let vs = source(GlslFlavor::Es300, Shader::Vertex);
        assert!(vs.contains("precision highp float;"));
        assert!(!vs.contains("sampler2DArray"));
    }

    #[test]
    fn vertex_offsets_the_glyph_quad_only_on_the_glyph_pass() {
        for flavor in [GlslFlavor::Desktop330, GlslFlavor::Es300] {
            let vs = source(flavor, Shader::Vertex);
            // The offset attribute (location 4), the glyph-size uniform, and the pass selector must
            // be present, and the offset must be applied to the quad position under the glyph pass.
            assert!(
                vs.contains("in ivec2 a_offset"),
                "{flavor:?} vertex missing a_offset"
            );
            assert!(
                vs.contains("uniform vec2 u_glyph"),
                "{flavor:?} vertex missing u_glyph"
            );
            assert!(
                vs.contains("u_draw_glyph == 1"),
                "{flavor:?} vertex does not gate the offset on the glyph pass"
            );
            assert!(
                vs.contains("px += vec2(a_offset) * (u_cell / u_glyph)"),
                "{flavor:?} vertex does not offset the quad position"
            );
        }
    }

    /// Sprite alignment inside a multi-cell span (retroglyph#412) is folded into `a_offset` on
    /// the CPU, in unscaled pixels, precisely so no shader or vertex-stride change is needed.
    /// The sprite vertex stage must therefore keep applying `a_offset` scaled by
    /// `u_cell / u_glyph`, or every aligned sprite silently renders in the wrong place.
    #[cfg(feature = "tilesets")]
    #[test]
    fn sprite_vertex_applies_the_scaled_sub_cell_offset() {
        for flavor in [GlslFlavor::Desktop330, GlslFlavor::Es300] {
            let vs = source(flavor, Shader::SpriteVertex);
            assert!(
                vs.contains("in ivec2 a_offset"),
                "{flavor:?} sprite vertex missing a_offset"
            );
            assert!(
                vs.contains("vec2 scale = u_cell / u_glyph"),
                "{flavor:?} sprite vertex no longer derives the render scale"
            );
            assert!(
                vs.contains("vec2(a_offset) * scale"),
                "{flavor:?} sprite vertex no longer offsets the quad origin"
            );
        }
    }

    #[test]
    fn fragment_splits_background_and_glyph_passes() {
        // Pass 0 emits the opaque background; pass 1 emits fg with coverage as alpha so glyphs
        // blend over (and spill onto) the backgrounds drawn in pass 0.
        let fs = source(GlslFlavor::Es300, Shader::Fragment);
        assert!(fs.contains("u_draw_glyph == 0"));
        assert!(fs.contains("vec4(v_bg, 1.0)"));
        assert!(fs.contains("vec4(v_fg, coverage)"));
    }

    #[test]
    fn compositing_flags_discard_transparent_and_empty_cells() {
        // The flags attribute must reach the fragment shader and gate each pass with a `discard`,
        // or higher grid layers can't be transparent (issue #368).
        let vs = source(GlslFlavor::Es300, Shader::Vertex);
        assert!(vs.contains("in uint  a_flags"), "vertex missing a_flags");
        assert!(
            vs.contains("v_flags = a_flags"),
            "vertex does not forward a_flags"
        );
        let fs = source(GlslFlavor::Es300, Shader::Fragment);
        assert!(
            fs.contains("(v_flags & 1u) == 0u"),
            "fragment missing has-bg discard"
        );
        assert!(
            fs.contains("(v_flags & 2u) == 0u"),
            "fragment missing has-glyph discard"
        );
        assert!(fs.contains("discard"), "fragment missing discard");
    }
}
