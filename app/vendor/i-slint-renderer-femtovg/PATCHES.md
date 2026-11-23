# Retained Slint FemtoVG renderer patch

Source: i-slint-renderer-femtovg 1.15.1 from crates.io, matching the locked Slint release.
Original source and license texts are retained. Local renderer changes are in
`itemrenderer.rs`, `images.rs` and `lib.rs`.

Backported [Slint PR #11335](https://github.com/slint-ui/slint/pull/11335),
commit 120159a0dad0addabb2018cca68cbff7557fb53e, to the 1.15.1 glyph API.
Align translation-only canvas origins to physical pixels for text and its
decorations, then restore the original transform. Rotations and scaling retain
their existing behavior. This avoids half-pixel nearest-neighbor text tearing.
No framework or renderer version is upgraded.

The Windows settings sheet asks the renderer to blur the workbench layer for
the current frame. FemtoVG filters the live workbench texture before drawing
the settings controls. An externally updated material texture invalidates the
layer through its revision. The layer releases its cached textures when the
settings transition ends.

The explicit font_probe example exercises fractional physical-pixel origins
using the actual native GPU renderer. Source licenses remain under LICENSES/.
