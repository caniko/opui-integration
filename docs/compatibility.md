# OPUI v1 compatibility

Recommendation: **RELEASE AS EXPERIMENTAL**

Structural interop is proven through public APIs: `op export` → `opui check` → `bevy_openpencil`. `just conformance` is green.

## Visual (1280×720, Linux, Vulkan RADV)

| Scene | Result |
| --- | --- |
| `--scene control` | nonblank (921600/921600). Host UI-to-texture works. |
| `--scene opui` | nonblank. vs `artboard.png`: **mae=23.59 max=255**. Budget 18 / 220. |

Do not raise the budget without a reviewed pair. max=255 is the missing ellipse orb (no raster-native) plus fill/gradient drift.

## Honest fixture notes

| Requested | What the fixture actually does |
| --- | --- |
| `.fig` source | `op export --file FILE.op --format opui`. `.fig` is import-only. |
| CSS grid | OpenPencil `LayoutMode` is `none` / `vertical` / `horizontal`. `inventory.grid` is a row flex. |
| Component variants | Two frames (`button_primary`, `button_secondary`). Empty `variant_properties`. |
| Raster fallback | `badge_orb` stays `opui.unsupported_native` until `just export-raster`. |

## Export

```
op export --file fixtures/runtime-ui.op --format opui --output generated/runtime-ui.opui --item artboard
```

Runtime entrypoint is `"default"`.

## Known gaps

- Bevy bakes opacity into colors (`bevy.opacity`).
- Raster: the earlier local Linux link failed with `mold: fatal: library not found: freetype`.
- `ComputedNode` sizes are 0 in Update when the tree is first sourced; screenshot still composites.
