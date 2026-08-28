# Veritasium dependency

## Supported lanes

The canonical RC1 lane is Bevy 0.19.1 from
`https://codeberg.org/caniko/rs-veritasium.git` at exact revision
`7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0`. `Cargo.lock` must contain one
Bevy identity and every official Bevy 0.19.1 crate must use that source and
revision.

The advisory lane is crates.io Bevy 0.19.1, the latest explicitly supported
upstream 0.19 patch release. Runtime package compilation is green in this lane.
Typography measures MAE 1.75, RMSE 12.73, max 240, exact 0.0461, and
thresholded 0.0394, compared with canonical MAE 1.58, RMSE 11.25, max 218,
exact 0.0457, and thresholded 0.0387. The upstream lane therefore fails only
the immutable maximum-channel threshold. It remains non-blocking for RC1
and does not change the canonical pin.

## OPUI-required patch

| Commit | Bevy crates | Behavior | Proving OPUI gate | Generic | Upstream | Adapter replacement | Removal condition |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `bb905ff09` | `bevy_text`, `bevy_ui` | Includes Parley's synthetic skew in glyph-atlas identity and applies it during Swash rasterization for normal and text-input paths. | `synthetic_italic_skews_the_glyph_top_to_the_right`; `typography-fidelity` 1280x720. | Yes, for fonts without an `ital` or `slnt` axis. | No issue or PR recorded. File before RC1. | No. Atlas identity and rasterization are Bevy internals. | An upstream revision carries equivalent synthesis behavior and the complete canonical conformance matrix passes against it. |

`31d730ff9` is the associated test-layout-only cleanup; it changes no behavior.

## Remaining fork delta

The pinned revision also contains observer-ordering, mediated-access,
replication-substrate, and fork-maintenance layers used by Regicide. OPUI does
not call those APIs and has no test that requires them. Cargo cannot select only
the synthetic-italic commit from one Git revision, so they remain part of the
canonical source identity but not part of OPUI's behavioral requirement.

The upstream-exit sequence is:

1. File and link the generic synthetic-italic upstream issue or PR.
2. Add an advisory upstream visual run without changing the canonical lane.
3. Record the first upstream revision with equivalent atlas and raster behavior.
4. Run the full clean capsule against that exact revision.
5. Replace the canonical pin only when all structural, semantic, raster, and
   visual gates pass unchanged.

No branch, moving tag, or floating Git dependency may replace the exact pin.

## Pointer cancellation promotion

Commit `7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0` dispatches `Pointer<Cancel>` against the previous hover target before clearing per-pointer state. Codeberg `trunk`, Cargo HTTPS resolution, both consumer manifests, and every active Bevy lock entry use that revision. `bevy_picking`, `bevy_ui_widgets`, and the integration cancellation regression pass against it.
