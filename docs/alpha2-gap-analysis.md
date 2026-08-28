# OPUI 0.1.0-alpha.2 gap analysis

Inventory date: 2026-08-26.

Alpha.2 release closure is complete for every release-blocking case. The
authoritative clean capsule reports `RELEASE AS EXPERIMENTAL` because two
non-release-blocking cases retain computed-snapshot differences that require
separate semantic review.

## Closed gaps

| Requirement | Evidence |
| --- | --- |
| Exact clean repository state | `just certify-release-clean` clones every locked repository and submodule |
| Nix source closure | OpenPencil renderer and raster outputs build from initialized `path:` clones |
| Full aggregate | all cases and viewports execute inside one capsule |
| Visual parity | every release-blocking visual case passes `18 / 220` unchanged |
| Gradient intrinsic sizing | container fill items preserve intrinsic basis; leaf fills remain evenly weighted |
| Package order simulation | runtime `.crate` compiles against unpacked exact schema `.crate` |
| Publication metadata | both publishable crates are aligned at `0.1.0-alpha.2` |
| Bevy identity | all official Bevy 0.19.1 crates resolve to exact Veritasium revision `adeb8f5c...` |
| Operator dirt isolation | primary Casement changes do not contaminate release evidence |
| Stable provenance | promoted JSON points to retained `release/<release-id>/` artifacts |
| Provider-neutral CI | `just ci` runs the clean capsule and passes locally |

## Remaining non-blockers

| Gap | Evidence | RC1 action |
| --- | --- | --- |
| Diagnostics computed snapshot | non-release-blocking case is experimental | review generated clean geometry separately before any golden change |
| Native-core computed snapshot | visual metrics pass; computed snapshot differs | review generated clean geometry separately before any golden change |
| Upstream Bevy synthetic italics | crates.io Bevy compiles but typography max is 240 vs canonical 218 | file upstream issue/PR and keep advisory lane |
| Hosted CI | repositories have no configured provider workflow | select a provider before adding workflow syntax |
| Signed provenance / SBOM | not required for alpha.2 | RC1 capsule hardening milestone |
| Independent renderer | OpenPencil and Bevy remain the oracle pair | add third renderer and Lavapipe lane in RC1 |

No policy, fixture, assertion, mask, mapping snapshot, or semantic golden was
softened to close alpha.2.
