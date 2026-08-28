# Designer-to-Bevy gap analysis

Baseline: OPUI alpha.2 evidence at `release/release-1787699135263/` proves structural export, validation, loading, rendering, and source-keyed reconciliation. This document covers the product workflow above that contract; it does not change frozen OPUI v1.

This table records the pre-closure baseline. Current contract-closure status is maintained in `designer-workflow-production-gap.md`.

| Capability | Existing proof | Product gap | Owner | Planned proof |
|---|---|---|---|---|
| Author `.op` without raw JSON editing | `op start [--headless] --file`, `op design`, `op update`, and `op save` use the public editor/MCP document model | No explicit stable runtime id or named runtime entrypoint in the `.op` model | Jian schema + OpenPencil exporter | Author the showcase through `op design`; inspect saved runtime metadata and exported entrypoints |
| Deterministic OPUI export | `op export --file ... --format opui`; alpha.2 determinism and checker gates | Export writes the destination before validation, and there is no watch mode | OpenPencil | Unit tests for atomic replacement/recovery plus a watched source mutation |
| Stable runtime lookup | `OpenPencilRuntimeIds`; duplicate ids fail closed | Existing export infers runtime ids from display names or source ids | Jian schema + OpenPencil exporter | Rename/reparent while explicit runtime ids remain unchanged |
| Interactive controls | `runtime_binding.rs` demonstrates inserting Bevy's legacy `Button` marker | Every consumer must rebuild role, label, order, disabled state, and visual-state glue | `bevy_openpencil` | Semantic metadata automatically installs Bevy 0.19 headless widgets and authored state bindings |
| Pointer/touch activation | Native Bevy UI/picking and `bevy_ui_widgets::Button` already provide press/release/click activation | Adapter does not opt semantic nodes into the widget system | `bevy_openpencil` | Pointer press/release and `Activate` tests |
| Keyboard focus/activation | Canonical Bevy provides `InputFocus`, `TabIndex`, `TabGroup`, `TabNavigationPlugin`, and focused Enter/Space activation | Exported hierarchy does not declare focus order; reload does not explicitly repair replaced focus targets | OpenPencil + `bevy_openpencil` | Tab order, Enter/Space activation, focused-node replacement test |
| Gamepad focus/activation | Canonical Bevy provides focused gamepad events and directional-navigation resources | Input mapping is application policy, not OPUI data | Integration application | D-pad navigation and South-button activation use the same focused semantic buttons |
| Accessibility | Native Bevy widgets provide button role; `AccessibleLabel`, `InteractionDisabled`, and `TabIndex` provide label/state/order | Exported semantic metadata is not mapped | OpenPencil + `bevy_openpencil` | Assert role, label, disabled state, and authored order |
| Authored visual states | Static state layers can already be authored and rendered | No exported relationship links default/hover/pressed/disabled/focused layers to a semantic control | Jian schema + OpenPencil exporter + `bevy_openpencil` | State transitions only change layer visibility; Rust contains no authored colors/styles |
| Dynamic values | Applications can mutate native Bevy `Text` through runtime-id lookup | Values must be reapplied after reconciliation, and this ownership rule is undocumented | Integration application + docs | Score, health, player name, Boolean and numeric settings survive reload |
| Hot reload preservation | Source-compatible nodes retain entity identity and app-owned ECS components | Removed/reinserted controls need semantic rebinding and deterministic focus restoration | `bevy_openpencil` | Reorder/reparent/restyle/remove/reinsert test with app state and custom components |
| Last-known-good recovery | Bevy keeps the currently loaded asset if no valid replacement appears | Export can partially replace the on-disk package before reporting failure | OpenPencil | Invalid watched edit leaves destination bytes and running UI unchanged |
| One-command development | Individual author/export/check/run commands exist | No canonical orchestration command or actionable diagnostics path | Integration | `just designer-ui-dev` |
| Production deployment | Bevy consumes a normal `.opui` package and sidecars | Pre-exported application package and guide are missing | Integration + `bevy_openpencil` docs | Run showcase from tracked assets with no OpenPencil process |
| Compatibility UX | Loader rejects invalid schema and missing entrypoints | Workflow does not summarize tool/schema/entrypoint/runtime-id compatibility | Integration | Workflow report with clear compatibility failures |

## Boundaries

- `.op` is the canonical design source. Generated `.opui` files are produced only by OpenPencil.
- OpenPencil owns design structure, static styling, runtime metadata authoring, export, validation, diagnostics, and watch behavior.
- Frozen OPUI v1 owns the engine-neutral package contract and checker. Its fixtures, policy, assertions, snapshots, and semantic goldens remain unchanged.
- `bevy_openpencil` owns native Bevy component mapping, semantic widget installation, runtime-id lookup, and reconciliation.
- The Bevy application owns routes/screens, game/application state, actions, input policy, and dynamic values.
- The integration repository owns the representative application, orchestration commands, and end-to-end evidence.
- Production consumes a pre-exported package; OpenPencil is not a runtime dependency.

## Non-goals

- No visual editor is added to Bevy.
- No behavior graph or game state is added to OPUI.
- No WebSocket, custom daemon, or second source of truth is introduced.
- No frozen alpha.2 evidence or generated package is hand-edited.
