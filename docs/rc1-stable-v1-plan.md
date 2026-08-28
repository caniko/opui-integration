# OPUI RC1 and stable-v1 plan

Alpha.2 closes exporter/runtime conformance and release reproducibility. RC1
hardens the harness without adding OPUI v2 behavior to the frozen v1 contract.
This roadmap is not an alpha.2 release gate.

## RC1 milestones

1. Keep package audits, gate folding, provenance, and backend-independent
   semantic snapshots in the integration library; extract a separate crate only
   when a second consumer exists, per `adr-conformance-core.md`.
2. Define a backend-neutral adapter protocol for package load, identity,
   reconciliation, computed geometry, capabilities, and captures.
3. Negotiate capability profiles explicitly and compare the selected profile
   with package diagnostics before runtime mounting.
4. Generate valid and invalid OPUI packages with property-based tests while
   preserving deterministic seeds and minimized failures.
5. Fuzz parser, checker, asset loader, sidecar resolver, and reconciliation
   boundaries with bounded corpora.
6. Minimize renderer disagreements into standalone `.op` and `.opui` fixtures
   before accepting any semantic or visual golden change.
7. Add an independent software or browser reference renderer so OpenPencil and
   Bevy do not form the only oracle pair.
8. Make Lavapipe the canonical software-rendered visual lane; retain RADV as an
   additional hardware lane, never the sole release oracle.
9. Produce a signed Nix release capsule with SBOM, source identities, package
   checksums, and in-toto/SLSA-compatible provenance.
10. Set reconciliation and rendering budgets for allocations, changed-node
    work, frame time, and asset reload latency.
11. Certify structural behavior on Linux, macOS, and Windows before expanding
    hardware-specific visual matrices.
12. Migrate from Veritasium to an exact upstream Bevy revision using the exit
    gates in `veritasium-dependency.md`.

## Dependencies and exit criteria

| Milestone | Depends on | Exit criterion |
| --- | --- | --- |
| 1 | alpha.2 capsule | Existing cases use the library boundary with unchanged verdicts. |
| 2 | 1 | OpenPencil and Bevy implement the same versioned adapter contract. |
| 3 | 2 | Unsupported capabilities fail before mount with deterministic diagnostics. |
| 4 | 1 | Seeded generators reproduce and minimize every failure. |
| 5 | 1 | Bounded corpora run in CI with retained crashing inputs. |
| 6 | 2, 4 | Cross-renderer failures emit standalone minimized fixtures. |
| 7 | 2 | A third renderer passes the structural and semantic matrix. |
| 8 | 7 | Lavapipe is reproducible and required; RADV remains supplemental. |
| 9 | 1 | Capsule includes signatures, SBOM, hashes, and provenance attestations. |
| 10 | 1, 2 | Enforced budgets have stable measurements and documented thresholds. |
| 11 | 1, 2 | Linux, macOS, and Windows pass the structural matrix. |
| 12 | 7, 8 | Exact upstream Bevy passes the complete clean matrix unchanged. |

## Stable-v1 gates

`release-profiles/stable-v1.toml` evaluates the unchanged RC1 candidate for
promotion readiness; it does not bump or publish `0.1.0`. Verdict vocabulary is:

- `RELEASE`: every owned and external stable-v1 gate passes;
- `DO NOT RELEASE` with blocked external issues: engineering may be green, but
  required native, physical, or signing evidence is unavailable;
- `DO NOT RELEASE` with failed or missing issues: evidence or implementation is
  invalid and must be fixed before reassessment.

Run `just assess-stable-v1`. The assessment reads the immutable certified RC1
report and overlays strict external evidence; it does not weaken the source-tree
lock or recertify from a dirty development checkout.

- Freeze the backend protocol and capability profile.
- Require deterministic property and fuzz corpora in CI.
- Require Lavapipe, package preflight, signed capsule provenance, and
  multi-platform structural certification.
- Publish performance budgets and support policy.
- Remove the fork or document a time-bounded exception with an owner.

Accessibility, localization, actions, focus, typed bindings, animation, and
incremental updates remain extension RFCs until stable v1. They must not alter
the frozen v1 schema or existing semantics during RC1.
