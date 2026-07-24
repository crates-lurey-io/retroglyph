//! GLSL shader sources for the instanced cell renderer, and version-prefixing so the exact same
//! shader bodies compile on both desktop GL 3.3 core (`#version 330 core`) and WebGL2 / GL ES 3.0
//! (`#version 300 es`).
//!
//! Rendering model (instanced quads, one draw call per frame):
//!
//! - A single unit quad (4 corners, 6 indices) is drawn `cols * rows` times via
//!   `draw_elements_instanced`.
//! - Per-instance attributes (divisor 1) carry the glyph's atlas layer plus foreground/background
//!   RGB. There is no per-instance position: the vertex shader derives `(col, row)` from
//!   `gl_InstanceID` and a `u_cols` uniform, so the instance buffer stays 12 bytes/cell.
//! - The glyph atlas is a `sampler2DArray` (`R8` coverage, one layer per glyph). The fragment
//!   shader samples coverage and does `mix(bg, fg, coverage)`.

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

/// Vertex shader body (no `#version` line -- that is prepended by [`source`]).
const VERTEX_BODY: &str = r"
layout(location = 0) in vec2  a_corner; // unit-quad corner in [0,1], also the in-cell glyph UV
layout(location = 1) in uint  a_glyph;  // atlas layer (glyph id), per instance
layout(location = 2) in vec3  a_fg;     // foreground RGB (normalized u8), per instance
layout(location = 3) in vec3  a_bg;     // background RGB (normalized u8), per instance
layout(location = 4) in ivec2 a_offset; // sub-cell (dx, dy) in unscaled font pixels, per instance

uniform vec2 u_screen;     // surface size in physical pixels
uniform vec2 u_cell;       // cell size in physical pixels (glyph size * scale)
uniform vec2 u_glyph;      // glyph size in unscaled font pixels, to scale (dx, dy) into pixels
uniform int  u_cols;       // grid columns, to unpack gl_InstanceID into (col, row)
uniform int  u_draw_glyph; // 0 = background pass (no offset), 1 = glyph pass (apply offset)

flat out uint v_glyph;
flat out vec3 v_fg;
flat out vec3 v_bg;
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
}
";

/// Fragment shader body (no `#version` line, no precision qualifiers -- both are prepended by
/// [`source`] for the ES flavor).
const FRAGMENT_BODY: &str = r"
uniform highp sampler2DArray u_atlas;
uniform int u_draw_glyph; // 0 = background pass, 1 = glyph pass

flat in uint v_glyph;
flat in vec3 v_fg;
flat in vec3 v_bg;
in vec2 v_uv;

out vec4 frag;

void main() {
    if (u_draw_glyph == 0) {
        // Background pass: the cell's opaque background.
        frag = vec4(v_bg, 1.0);
    } else {
        // Glyph pass: foreground with atlas coverage as alpha, so non-glyph texels are transparent
        // and the background (or a neighbor's spilled glyph) shows through when blended.
        float coverage = texture(u_atlas, vec3(v_uv, float(v_glyph))).r;
        frag = vec4(v_fg, coverage);
    }
}
";

/// Builds a complete shader source string for `flavor`, prepending the right `#version` line (and,
/// for ES, the precision qualifiers a fragment shader needs).
pub(crate) fn source(flavor: GlslFlavor, body: Shader) -> String {
    let mut out = String::new();
    match flavor {
        GlslFlavor::Desktop330 => out.push_str("#version 330 core\n"),
        GlslFlavor::Es300 => {
            out.push_str("#version 300 es\n");
            // ES requires explicit default precision. The fragment shader also samples an integer
            // array texture, so give both float and the sampler a high precision default.
            out.push_str("precision highp float;\nprecision highp int;\n");
            if matches!(body, Shader::Fragment) {
                out.push_str("precision highp sampler2DArray;\n");
            }
        }
    }
    out.push_str(match body {
        Shader::Vertex => VERTEX_BODY,
        Shader::Fragment => FRAGMENT_BODY,
    });
    out
}

/// Which of the two shader stages to emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shader {
    /// The vertex stage ([`VERTEX_BODY`]).
    Vertex,
    /// The fragment stage ([`FRAGMENT_BODY`]).
    Fragment,
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

    #[test]
    fn fragment_splits_background_and_glyph_passes() {
        // Pass 0 emits the opaque background; pass 1 emits fg with coverage as alpha so glyphs
        // blend over (and spill onto) the backgrounds drawn in pass 0.
        let fs = source(GlslFlavor::Es300, Shader::Fragment);
        assert!(fs.contains("u_draw_glyph == 0"));
        assert!(fs.contains("vec4(v_bg, 1.0)"));
        assert!(fs.contains("vec4(v_fg, coverage)"));
    }
}
