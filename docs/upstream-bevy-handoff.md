# Upstream Bevy Handoff

No upstream issue or pull request was filed during RC1 certification. This is
ready-to-file material; it does not claim upstream acceptance.

## Synthetic Italic Issue

**Title:** Include synthetic skew in glyph atlas identity and Swash rasterization

Fonts without an `ital` or `slnt` axis can be synthetically italicized by
Parley, but Bevy 0.19.1 does not include the synthesis skew in glyph-atlas
identity or apply it consistently in normal and text-input raster paths. The
result is observable raster disagreement and possible atlas reuse across
different synthesized styles.

The generic fork patch is `bb905ff09` in `bevy_text` and `bevy_ui`; associated
commit `31d730ff9` only adjusts test layout. A minimal reproducer is the
`typography-fidelity` case:

```console
just certify-case typography-fidelity 1280x720
```

Canonical Veritasium measures MAE 1.58, RMSE 11.25, max channel 218, exact
0.0457, and thresholded 0.0387. Stock Bevy 0.19.1 measures MAE 1.75, RMSE
12.73, max channel 240, exact 0.0461, and thresholded 0.0394. The immutable
policy maximum is 220, so stock Bevy fails that gate.

Acceptance requires a focused Bevy test proving distinct atlas/raster output
for synthesized italic text in both normal and text-input paths. OPUI can leave
the fork only after the complete clean matrix passes unchanged against an exact
upstream revision.

## Previous-Target Cancellation

Veritasium revision `7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0` dispatches
`Pointer<Cancel>` against `PreviousHoverMap` before clearing pointer state. The
focused upstream handoff should preserve that ordering and include the existing
`bevy_picking` and `bevy_ui_widgets` cancellation regressions. Stock Bevy
0.19.1 does not satisfy this behavior.

## Advisory Lane

Stock Bevy remains advisory only. It may compile and collect comparison
evidence, but it cannot replace the canonical pin, weaken visual policy
`18 / 220`, or affect the release verdict. No moving branch or tag is an
acceptable replacement for an exact passing upstream revision.
