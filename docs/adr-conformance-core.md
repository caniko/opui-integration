# ADR: Conformance Core Boundary

- Status: Accepted
- Scope: certification architecture

## Decision

Keep backend-independent package audits, gate folding, provenance, and semantic
snapshot logic in the `opui-integration` library. Keep orchestration and process
exit behavior in `opui-certify`. Do not create a separate
`opui-conformance-core` crate until a second independent consumer needs the
same Rust API.

Renderer adapters may provide captures and runtime observations, but they do
not own verdict policy. Frozen OPUI v1 remains the contract boundary; adapter
metadata must not become a lifecycle callback protocol.

## Consequence

This avoids a speculative crate and public API while preserving a clean
extraction boundary. A second consumer is the trigger for extraction, with the
existing library tests serving as the migration check.
