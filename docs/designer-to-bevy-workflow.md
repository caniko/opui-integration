# Designer-to-Bevy workflow contract

## Canonical lifecycle

1. A designer opens or creates the canonical `.op` file with OpenPencil.
2. In the native property panel's Interact tab, semantic controls receive explicit `runtimeId`, role, accessible label, focus order, enabled state, and authored visual-state layer ids. Runtime ids are durable application keys and do not change when display names, text, hierarchy, or styling change.
3. The document declares a named runtime entrypoint. Duplicate/paste/page-copy clears document-unique runtime ids and visual-state links while retaining semantic metadata.
4. OpenPencil exports to a temporary sibling package, runs the frozen OPUI checker there, installs content-addressed sidecars, and atomically promotes the manifest only after validation succeeds.
5. In development, OpenPencil watches the `.op` file with debounce and repeats step 4. It emits one actionable success or failure diagnostic per settled edit.
6. Bevy's normal file watcher notices only a promoted valid manifest. `bevy_openpencil` reconciles it by source id, rebuilds runtime-id lookup, restores semantic widget bindings, and restores focus by runtime id when an entity had to be replaced.
7. `opui-bindings` deterministically generates typed Rust node and entrypoint enums from the validated manifest. The checked-in output must pass `just designer-bindings-check`.
8. The application handles native Bevy widget activation through generated ids and updates its own resources/components. It reapplies dynamic values and screen visibility after `OpenPencilUiReconciled`.
9. Production builds package the already-exported `.opui` manifest and sidecars. OpenPencil is absent at runtime.

The development entrypoint is `just designer-ui-dev`. Regenerate bindings with `just designer-bindings`. The clean automated proof is `just designer-workflow-test`, which rejects stale generated bindings against a fresh temporary export.

OpenPencil can author the metadata through `op runtime-ui:metadata --file design.op --spec runtime-ui.json`. Specs target the stable `.op` `nodeId` and may pin `sourceSha256` so stale automation fails before mutating the document. Name/type selectors remain available only as a warned legacy path.

## Ownership

| Data or behavior | Owner |
|---|---|
| Layout, typography, colors, imagery, static visual-state layers | OpenPencil `.op` source |
| Stable runtime ids, entrypoint names, semantic role, accessible label, focus order, visual-state links | OpenPencil `.op` source |
| Package shape and compatibility validation | Frozen OPUI v1 + checker |
| Native UI entities, semantic widget components, accessibility components, reconciliation | `bevy_openpencil` |
| Screen transitions, Play/Settings/Back/Apply/Quit/pause actions | Bevy application |
| Score, health, player name, settings values, and other live data | Bevy application |
| Development orchestration and workflow evidence | Integration repository |

Rust must not recreate designer-authored colors, borders, typography, spacing, or visual-state styling. Runtime code may select among authored layers and replace dynamic text/value content.

## Failure model

- A malformed `.op`, exporter error, checker error, duplicate/invalid runtime id, missing entrypoint, unsupported strict node, or sidecar failure does not replace the last known-good package.
- Watch mode reports the error and remains alive for the next edit.
- Bevy continues rendering the last promoted asset and preserves application resources, app-owned components on source-compatible entities, and current screen state.
- Each mount reports package SHA-256, generation, asset id, entrypoint, diagnostics, and last successful reconciliation sequence. Failed reloads distinguish stale last-good display from failure before any successful generation.
- A compatible source node keeps its entity and app-owned ECS components. If a focused semantic node is removed and reinserted with the same runtime id, the adapter restores focus to the new entity after reconciliation.
- Missing runtime ids or visual-state targets are actionable diagnostics. They never fall back to display-name lookup.
- Duplicate runtime ids fail validation; the runtime registry never silently overwrites the first entity.
- Version/schema/entrypoint incompatibility is reported with the offending package and required value before application behavior is bound.

## Runtime metadata rules

- `runtimeId` is explicit and stable. Display `name`, text, generated source id, and hierarchy are not behavior keys.
- Entrypoint names and runtime ids use the frozen OPUI identifier character set.
- A button's visual-state map may contain `default`, `hover`, `pressed`, `disabled`, and `focused`. Values are runtime ids of designer-authored layers.
- Focus order is authored as an integer and maps to Bevy `TabIndex`; hierarchy supplies the deterministic tie-break.
- `enabled: false` maps to Bevy `InteractionDisabled`, which also updates accessibility state.

## Deployment

Commit the `.op` source, metadata manifest, generated package, sidecars, and generated Rust bindings used by the application. `just designer-workflow-test` regenerates into a temporary location, compares package bytes, and checks bindings against that fresh manifest. Shipping includes only the `.opui` manifest and its `.opui.assets` directory; bindings compile into the application.
