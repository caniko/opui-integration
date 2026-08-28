# OPUI v1 RC1 convergence status

**LOCAL RC1 CERTIFICATION PASSES. PUBLICATION REMAINS AN OPERATOR ACTION.**

The paired exact clean capsules pass every locally owned RC1 technical,
packaging, supply-chain, visual, stress, performance, API, semver, provenance,
and reproducibility gate. Authored lifecycle callbacks are explicitly deferred
from frozen OPUI v1. Windows, macOS, physical gamepad/touch, and provenance
signing remain external `Blocked` results and are not fabricated as passes.

## Canonical identities

`repos.lock.toml` is authoritative. The current closure pins:

| Repository | Exact revision |
| --- | --- |
| Frozen OPUI v1 | `f4b6dc6df431efae9245be51b6c08c828339b007` |
| OPUI checker | `04fdda1c8a2dabd4fad3ee66dd9043f44ed8509c` |
| OpenPencil | `4c2a37e3d6632c89530f0edcfd7aec184e38383f` |
| Bevy adapter | see `repos.lock.toml` |
| Veritasium | `7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0` |

The reference renderer is built from the pinned OpenPencil
`reference-renderer` flake output and verified with SHA-256
`fb13d86f3f7941baaea1ca8dac9d829bf43d24175e65365633fbf39be55039ea`.

## Clean capsule

Run `just certify-rc1-clean`. Each capsule clones exact
revisions with `--no-local`, builds the pinned renderer and raster exporter,
uses fresh Cargo, target, XDG, and run roots, and atomically retains both
successful and failed evidence bundles. RC1 runs twice and requires identical
artifact-manifest SHA-256 values.

The newest successful `release/release-*/release-report.md` is authoritative.
It must report `RELEASE` for all implemented gates and cases. Earlier bundles
are immutable investigation evidence and are never reused.

The RC1 clean aggregate covers formatting, strict Clippy, tests, all targets,
examples, feature-off builds, hot-reload mutation stress, negative exports,
repository locks, deterministic exports, package archives, schema publish
dry-run, downstream package resolution, SPDX SBOM, advisories, license/source
policy, graphical showcase captures, semantic snapshots, accessibility,
runtime IDs, raster fallback, public API and semver compatibility, graphical
stress, performance budgets, artifact manifests, unsigned provenance, paired
reproducibility, and visual policy `opui-v1-visual-18-220`.

`just ci-rc1` is the provider-neutral automation entrypoint. It runs formatting,
tests, strict Clippy, exact-archive rehearsal, and immutable handoff preparation;
no hosted CI provider is configured or implied.

## External results

External authorities start from `external-results/rc1.toml`, keep its candidate
and package identity fields unchanged, and attach repository-relative evidence
paths with SHA-256 values for every `pass`. Import a completed result with
`just import-rc1-external RESULT`; unknown, missing, stale, malformed, or
mismatched evidence is a failure, while unavailable environments remain
explicitly `blocked`.

| Area | Status | Required action |
| --- | --- | --- |
| Authored lifecycle callbacks | `Deferred` | Frozen OPUI v1 does not include executable callbacks. A future versioned contract may add them; `extensions` must not become an implicit callback protocol. |
| External platforms | `Blocked` | Retain Windows and macOS structural smoke evidence when runners are available. |
| Physical input | `Blocked` | Retain physical gamepad and touch traces when hardware is available. |
| Upstream Bevy exit | `Blocked` | Upstream Bevy exceeds the immutable typography max-channel budget (`240 > 220`); keep the Veritasium pin until equivalent behavior lands upstream. |

## Publication hold

No crate, version, tag, hosted release, or push is authorized by this document.
If publication is explicitly authorized, publish in dependency order:
`openpencil_ui_schema`, then `bevy_openpencil` after the exact schema version
resolves from crates.io. This document does not authorize publication, tagging,
hosted releases, pushes, or signing.
