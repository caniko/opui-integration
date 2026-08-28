# Designer workflow production gap

Contract-closure baseline: integration `85c48413182c26cb5ca8c1ca02b42b23d07be94c`, OpenPencil `4c2a37e3d6632c89530f0edcfd7aec184e38383f`, adapter `28d8c3a6f3e7d4a0580c2a605006336544a7a53a`, canonical Veritasium `adeb8f5ce2d2563b326e55904cb42a1e544639ad`, and cancellation owner fix `7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0`. Alpha.3 closure promotes canonical Veritasium `7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0` and adapter `36388fb7be76f464e32586493e78c070960f9fa5`. The fresh `just designer-workflow-test` gate is green.

## Evidence classes

| Class | Meaning |
|---|---|
| Implementation present | Source exists and compiles, but the complete user path may not be exercised. |
| Directly tested | A deterministic test traverses the production plugin, event, resource, and observer path. |
| Inherited | Behavior is supplied by pinned Bevy and is not yet directly tested in this application. |
| Graphical absent | No retained proof that a real window/render graph produced the expected pixels. |
| Hardware absent | No matching physical device was available; synthetic evidence is reported separately. |
| Release blocker | Alpha.3 cannot be prepared while this owned gate is red or absent. |
| RC1 blocker | Alpha.3 may remain experimental, but an RC1 cannot be declared. |
| Stable-v1 blocker | Required before replacing Veritasium or declaring a stable workflow contract. |

## Current audit

| Capability | Current evidence | Class | Alpha.3 | RC1 | Stable v1 |
|---|---|---|---|---|---|
| Canonical `.op` authoring | Public `op start`, `op design`, save/reload, tracked showcase source | Directly tested | green | green | green |
| Explicit runtime metadata | Jian serialization and OpenPencil export tests | Directly tested | green | green | green |
| Metadata authoring UX | Native Interact inspector edits runtime id, role, accessible label, tab order, enabled state, and five visual-state targets through shared desktop/web history paths | Directly tested | green | green | green |
| Exact metadata targeting | `nodeId` selectors cover nodes and visual states; optional `sourceSha256` rejects stale specs; legacy names warn | Directly tested | green | green | green |
| Deterministic validated atomic export | Byte comparison, frozen checker, staged promotion tests | Directly tested | green | green | green |
| Invalid initial/edit recovery | Startup-invalid and edit-invalid watch tests | Directly tested | green after watcher-order fix | green | green |
| Native semantic mapping | Button, accessibility, tab index, disabled state, opt-in hover tracking, visual layers | Directly tested at component/state level | green | green | green |
| Pointer dispatch | `PointerInput` + backend hits exercise hover, press, drag-end, release, focus, activation, disabled controls, and authored layers | Directly tested | green | green | green |
| Synthetic touch dispatch | Touch press/release/cancel traverses native picking and clears `Pressed` against canonical Veritasium | Directly tested | green | green | green |
| Focused keyboard dispatch | Actual `KeyboardInput` messages exercise Tab, Shift+Tab, hierarchy tie-breaks, Enter, Space, Escape, repeat/release, and disabled focus | Directly tested | green | green | green |
| Synthetic gamepad | Actual connection/raw-button messages exercise all D-pad directions, South/East/Start, held and opposing input, disconnect/reconnect, absent/disabled/removed focus | Directly tested | green | green | green |
| Physical gamepad | No gamepad in `/proc/bus/input/devices` | Hardware absent | allowed only for experimental verdict | blocker | blocker |
| Physical touch | No touch device in `/proc/bus/input/devices` | Hardware absent | informational when synthetic touch is green | blocker if claimed | blocker |
| Reconciliation preservation | Reorder, compatible reuse, remove/reinsert, focus replacement | Directly tested | green | green | green |
| Dynamic application state | Screens/settings/text reapply after reconciliation | Directly tested | green | green | green |
| Typed runtime bindings | Deterministic manifest-to-Rust generation covers node ids and entrypoints; showcase state, lookup, focus, and mount code use generated enums; clean workflow rejects stale output | Directly tested | green | green | green |
| Per-mount lifecycle status | Required status distinguishes loading, ready generation, stale last-good, failure without last-good, recovery, removal, package identity, diagnostics, and reconciliation sequence per mount | Directly tested with three-sibling isolation | green | green | green |
| Fresh downstream package consumer | A fresh crate compiles against unpacked schema and runtime `.crate` artifacts; the generated runtime manifest retains no path dependency | Directly tested | green | green | green |
| Three headless viewports | Runtime probes at 1280x720, 1920x1080, 800x1280 | Directly tested structurally | green | insufficient alone | insufficient alone |
| Real graphical window | Three showcase viewports create Winit windows under isolated Weston and capture non-corrupt output through llvmpipe Vulkan | Directly tested | green | green | green |
| Accessibility tree snapshot | Runtime probes retain every AccessKit node with role, label, value, disabled state, tab order, bounds, and effective parent | Directly tested | green | green | green |
| Stress and performance | 1,000 reconciliations preserve runtime identity and 64 live entities; measured debug run completes in 2.429 seconds | Directly tested | green | green | green |
| Clean isolated capsule | Alpha.3 runs from exact clean clones with a fresh Cargo home and target, strict pins, package preflight, and the graphical matrix | Directly tested | green | green | green |
| Package publication readiness | Alpha.3 schema publish dry-run and packaged runtime/schema/downstream compilation pass without path dependencies | Directly tested | green | green | green |
| Upstream Bevy exit | Codeberg `trunk`, Cargo HTTPS, both consumer manifests, and all active Bevy lock entries resolve the owner-tested cancellation fix | Directly tested | green | green | green |

## Host boundaries

- The host exposes COSMIC Wayland (`wayland-1`), Xwayland (`:1`), RADV/Mesa 26.1.5, and an AMD Radeon RX 7900 XTX. This is capability inventory, not graphical acceptance evidence.
- Weston remains absent from the outer environment. The pinned Nix graphical shell supplies Weston, Mesa llvmpipe Vulkan, and libinput for reproducible window and device evidence.
- No physical gamepad or touch device is connected. Their gates remain explicitly blocked until device-backed traces exist.
- Veritasium commit `7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0` dispatches cancellation against `PreviousHoverMap` before clearing pointer state. Owner picking/widget suites and the integration regression pass, Codeberg advertises it, and Cargo resolves it over HTTPS.
- Casement dirt and five old untracked integration release directories are protected inputs, not alpha.3 evidence.

## Promotion rule

Alpha.3 production readiness requires every owned release blocker above to become directly tested in a clean isolated capsule. Missing physical gamepad hardware may produce an experimental verdict only after the synthetic matrix and all other owned gates are green. Headless probes, direct `Activate` calls, inherited Bevy behavior, or a developer desktop screenshot cannot substitute for the corresponding direct gate.
