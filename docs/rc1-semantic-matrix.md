# RC1 semantic matrix

## Visual states

| State | Trigger | Precedence | Visibility | Activation | Reload | Evidence | Status |
|---|---|---|---|---|---|---|---|
| default | no focus, hover, press, or disable | fallback | exactly one state visible | none | identity retained | `bevy_openpencil::tests::semantic_button_maps_accessibility_order_and_authored_states` | Green |
| hover | mouse hit | below focus | hover only | none | identity retained | `direct_input::pointer_and_touch_follow_native_button_semantics` | Green |
| pressed | mouse or touch press | below disabled | pressed only | release over target | press state is cleared | `direct_input::pointer_and_touch_follow_native_button_semantics` | Green |
| disabled | hidden/inactive screen | highest | disabled only | rejected | identity retained | `direct_input::pointer_and_touch_follow_native_button_semantics` | Green |
| focused | pointer, keyboard, or gamepad focus | above hover | focused only | Enter, Space, or South | invalid focus rejected | `direct_input::focused_keyboard_dispatches_navigation_activation_and_back`; `direct_input::gamepad_uses_processed_state_and_rejects_invalid_focus` | Green |

## Runtime lifecycle

| Requested phase | Runtime evidence | Authored callback preservation | Status |
|---|---|---|---|
| onStart | Bevy `Startup` mounts the root | OpenPencil/Jian calls this `onLaunch`; it is not represented by frozen OPUI v1 | Blocked |
| onMount | `OpenPencilUiStatus::Loading` is inserted with the root | OpenPencil's `.op` loader discards authored node lifecycle data | Blocked |
| onReady | `OpenPencilUiStatus::Ready` and `OpenPencilUiReconciled` identify a successful generation | frozen OPUI v1 has no typed ready callback | Blocked |
| onEnter | application state controls screen visibility and focus | page lifecycle is not exported into frozen OPUI v1 | Blocked |
| onExit | application state disables hidden controls and rejects activation | Jian calls this `onLeave`; it is not exported into frozen OPUI v1 | Blocked |
| onUnmount | removing `OpenPencilUiRoot` clears status, registry entries, and owned children | frozen OPUI v1 has no typed unmount callback | Blocked |

The mount-status lifecycle itself is green under
`bevy_openpencil::tests::lifecycle_transitions_preserve_last_good_tree`. The
blocked result is limited to executing authored callback action lists. Adding
that behavior would require a new, versioned OPUI contract surface; silently
putting actions in `extensions` would not define portable runtime semantics.

## Mutation and input

| Path | Covered behavior | Evidence | Status |
|---|---|---|---|
| mutation/reload | reorder retains entity and application components; remove/reinsert retains application state | `showcase::reload_and_remove_reinsert_preserve_application_state` | Green |
| reconciliation stress | 1,000 mutations retain identity and entity count | `runtime::reconciliation_stress_keeps_identity_and_entity_count` | Green |
| mouse | hover, leave, press, release, drag-out cancellation | `direct_input::pointer_and_touch_follow_native_button_semantics` | Green |
| keyboard | tab order, reverse tab, hierarchy tie-break, Enter, Space, Escape, repeat rejection | `direct_input::focused_keyboard_dispatches_navigation_activation_and_back` | Green |
| synthetic gamepad | directional focus, opposite-input cancellation, South, East, Start, held-input rejection, disconnect | `direct_input::gamepad_uses_processed_state_and_rejects_invalid_focus` | Green |
| synthetic touch | press, release, cancel | `direct_input::pointer_and_touch_follow_native_button_semantics`; `direct_input::touch_cancel_clears_native_pressed_state` | Green |
| physical gamepad | unavailable on the headless certification host | none | Blocked |
| physical touch | unavailable on the headless certification host | none | Blocked |
