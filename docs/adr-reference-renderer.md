# ADR: Reference Renderer Role

- Status: Accepted
- Scope: RC1 and stable-v1 certification

## Decision

The pinned software reference renderer is the visual oracle. OpenPencil is the
authoring/export path and `bevy_openpencil` is the implementation under test;
neither may accept its own output as the sole reference. Lavapipe/software
rendering is the reproducible required lane, while hardware rendering is
supplemental.

Golden changes require an independently explained source or policy change and
must preserve structural, semantic, raster, and accessibility evidence. A
renderer disagreement is retained and minimized rather than accepted by
updating both sides together.

## Consequence

The exact reference renderer output and SHA-256 stay in the capsule. Replacing
the renderer requires a paired clean comparison and an explicit ADR update.
