# Coordinates

## Sub-cell offsets aren't shared code across backends

`retroglyph-window`'s `Presenter` trait specifies the sub-cell offset (`Tile::dx`/`dy`) and spill
contract once, so the CPU rasterizer (`retroglyph-software`) and the GPU ones (`retroglyph-gl`,
`retroglyph-wgpu`) produce the same pixels without mirrored per-backend comments that reference each
other and drift when only one is touched. What the contract does not specify is a shared
implementation: the GPU backends shift a quad's vertex position in their vertex shader, while
`retroglyph-software` shifts `origin_x`/`origin_y` in a CPU blit. These are irreducibly different
mechanics that must nonetheless agree on the same four points (offsets are unscaled font pixels,
backgrounds stay unshifted, spill is uniform in all four directions, and a two-pass
background-then-glyph draw is what makes that uniformity possible); see the `Presenter` trait's own
doc comment for those four points.
