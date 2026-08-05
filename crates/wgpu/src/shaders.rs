//! WGSL sources, composed from a shared prelude plus one body per pipeline family.
//!
//! WGSL has no `#include`, and every pipeline in this crate reads the same uniform block and the
//! same pair of atlas bindings. Rather than repeat those declarations in each `.wgsl` file (where
//! the copies would drift the first time a uniform field moved), `common.wgsl` holds them once and
//! [`source`] concatenates it with the requested body. Both shader modules therefore see one
//! definition of [`Uniforms`](crate::renderer::Uniforms)'s layout, by construction.
//!
//! Concatenation happens at build time, not run time: the two `include_str!`s are `const`, so a
//! [`Shader`]'s full source is a compile-time-known pair of `&'static str`s that
//! [`GpuResources`](crate::renderer::GpuResources) joins once when it creates the module.

// `redundant_pub_crate` fires on `pub(crate)` items in this private module; the module boundary is
// intentional, so it's allowed crate-locally.
#![allow(clippy::redundant_pub_crate)]

/// Declarations every pipeline shares: the uniform block, the atlas bindings, the compositing flag
/// constants, and the quad-corner and atlas-sampling helpers.
const COMMON: &str = include_str!("common.wgsl");

/// The two glyph pipelines (`vs_background`/`fs_background` and `vs_glyph`/`fs_glyph`).
const CELLS: &str = include_str!("cells.wgsl");

/// The sprite pipeline (`vs_sprite`/`fs_sprite`).
#[cfg(feature = "tilesets")]
const SPRITES: &str = include_str!("sprites.wgsl");

/// Which shader module to build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shader {
    /// The per-cell background and glyph pipelines.
    Cells,
    /// The sprite pipeline.
    #[cfg(feature = "tilesets")]
    Sprites,
}

/// The complete WGSL source for `shader`: the shared prelude followed by that shader's body.
pub(crate) fn source(shader: Shader) -> String {
    let body = match shader {
        Shader::Cells => CELLS,
        #[cfg(feature = "tilesets")]
        Shader::Sprites => SPRITES,
    };
    let mut out = String::with_capacity(COMMON.len() + body.len() + 1);
    out.push_str(COMMON);
    out.push('\n');
    out.push_str(body);
    out
}

#[cfg(test)]
mod tests {
    use super::{COMMON, Shader, source};
    use crate::renderer::Uniforms;

    #[test]
    fn every_module_carries_the_shared_declarations() {
        // A body that declared its own `Uniforms` would compile and then read the wrong offsets;
        // the point of the prelude is that there is exactly one declaration.
        let mut shaders = vec![Shader::Cells];
        #[cfg(feature = "tilesets")]
        shaders.push(Shader::Sprites);
        for shader in shaders {
            let src = source(shader);
            assert!(
                src.contains("struct Uniforms {"),
                "{shader:?} is missing the shared uniform block"
            );
            assert_eq!(
                src.matches("struct Uniforms {").count(),
                1,
                "{shader:?} declares the uniform block twice"
            );
            assert!(
                src.contains("@group(0) @binding(0) var<uniform> u: Uniforms;"),
                "{shader:?} is missing the uniform binding"
            );
        }
    }

    #[test]
    fn uniform_block_matches_the_rust_struct() {
        // WGSL's uniform address space requires the block's size to be a multiple of 16; the Rust
        // struct is what gets written into the buffer, so the two must agree on that size or every
        // field past the first mismatch is read from the wrong offset.
        assert_eq!(size_of::<Uniforms>(), 48);
        assert_eq!(size_of::<Uniforms>() % 16, 0);
        // The trailing `pad` field exists only to reach that size; naming it here keeps a future
        // edit that drops it from silently shrinking the block.
        assert!(COMMON.contains("pad: u32,"));
    }

    #[test]
    fn cells_declares_both_passes() {
        let src = source(Shader::Cells);
        for entry in [
            "fn vs_background(",
            "fn vs_glyph(",
            "fn fs_background(",
            "fn fs_glyph(",
        ] {
            assert!(src.contains(entry), "cells.wgsl is missing `{entry}`");
        }
    }

    #[test]
    fn only_the_glyph_pass_applies_the_sub_cell_offset() {
        // The background pass passes 0.0 for the offset scale and the glyph pass 1.0. If both
        // passed the same value, backgrounds would either move with their glyph (breaking the
        // "background fill is always the full, unshifted cell" half of the spill contract) or
        // glyphs would stop moving at all.
        let src = source(Shader::Cells);
        assert!(src.contains("place(vertex_index, instance_index, cell, 0.0)"));
        assert!(src.contains("place(vertex_index, instance_index, cell, 1.0)"));
    }

    #[test]
    fn compositing_flags_gate_a_cell_through_alpha_not_discard() {
        let src = source(Shader::Cells);
        // Both flags must still reach the fragment stage, or a higher grid layer's transparent
        // cells would paint over the layer beneath.
        assert!(src.contains("in.flags & FLAG_HAS_BG"));
        assert!(src.contains("in.flags & FLAG_HAS_GLYPH"));
        // `discard` makes a fragment's visibility undecidable before the fragment stage runs, which
        // costs a tile-based deferred renderer its early-Z and hidden-surface removal for the whole
        // draw. Every "draws nothing" case here is an alpha-0 write that source-over blending
        // resolves to a no-op instead.
        assert!(
            !src.contains("discard;"),
            "cells.wgsl reintroduced discard; gate the cell through alpha instead"
        );
    }

    #[cfg(feature = "tilesets")]
    #[test]
    fn the_sprite_stage_avoids_discard_too() {
        assert!(!source(Shader::Sprites).contains("discard;"));
    }

    #[cfg(feature = "tilesets")]
    #[test]
    fn sprites_recolour_in_exact_integer_math() {
        // The CPU rasterizer applies `Tint` in `u8` arithmetic. A float approximation here would
        // be off by one on some channels and break the parity test in `headless`.
        let src = source(Shader::Sprites);
        assert!(src.contains("fn multiply_u8(a: u32, b: u32) -> u32"));
        assert!(src.contains("fn mix_u8(a: i32, b: i32, t: i32) -> i32"));
        assert!(src.contains("round(src.rgb * 255.0)"));
    }
}
