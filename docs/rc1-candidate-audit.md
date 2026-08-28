# RC1 Candidate Audit

Audit date: 2026-08-28

This audit is read-only with respect to the certified candidate. It does not
authorize publication, tagging, signing, pushing, or a hosted release.

## Source Candidate

The publishable source candidate is the exact `bevy_openpencil` repository at
`1a00b72b6bd333e67c1c5224a07d8d81881ca30d`, containing
`openpencil_ui_schema` and `bevy_openpencil` version `0.1.0-rc.1`.

The certified source closure is:

| Source | Revision |
| --- | --- |
| Frozen OPUI v1 contract | `f4b6dc6df431efae9245be51b6c08c828339b007` |
| OPUI checker | `04fdda1c8a2dabd4fad3ee66dd9043f44ed8509c` |
| OpenPencil | `4c2a37e3d6632c89530f0edcfd7aec184e38383f` |
| Jian | `ba334d27edf05b7e4c7a2746fc3c664d9ed24f28` |
| Bevy schema and adapter | `1a00b72b6bd333e67c1c5224a07d8d81881ca30d` |
| Certification harness | Private RC1 evidence; not part of the public source history |
| Veritasium | `7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0` |
| Agent | `5c0f9506e27be5b4f29cd2c1093e2858ad0fa20c` |
| Casement | `451173eb3353a4166d2d0f241f2e0606051064bd` |

The primary OpenPencil checkout contains protected Casement work and is not
strict evidence. The authoritative capsule used clean exact clones.

## Packaged Candidate

The package archives retained in the authoritative capsule are:

| Package | Bytes | SHA-256 |
| --- | ---: | --- |
| `openpencil_ui_schema-0.1.0-rc.1.crate` | 14,029 | `ec76e68ce27bdc136916da9dcca79900fe8e664277c7ba3fa18df87b08edf69f` |
| `bevy_openpencil-0.1.0-rc.1.crate` | 51,564 | `c898fc4c0cf822b0bb77e812dc75af87105ede1dc1cf54a3888e1758326e5373` |

Independent archive inspection confirms:

- both normalized manifests identify version `0.1.0-rc.1`;
- the runtime requires exactly `openpencil_ui_schema = "=0.1.0-rc.1"`;
- neither normalized manifest contains a path dependency;
- no packaged file contains an owner-workspace absolute path;
- both archives identify source revision
  `1a00b72b6bd333e67c1c5224a07d8d81881ca30d`;
- every official Bevy 0.19.1 package resolves from Veritasium revision
  `7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0`.

The runtime archive records Cargo's temporary packaging rewrite as dirty. This
is expected from the schema-first unpublished-package rehearsal used to remove
the workspace path dependency. The retained bytes are deterministic across the
two independent clean capsules.

## Evidence Candidate

The authoritative engineering capsule is
`release/release-1787882067758/`. It contains the completion marker, release
reports, capsule identity, package archives, case evidence, SPDX SBOM,
performance evidence, API and semver logs, artifact manifest, unsigned
provenance, and paired-clean report.

The retained `artifact-manifest.json` SHA-256 is
`552bd00dd3fa96ed2ccbac2851bfeb1a3a6d21884cf408866323c407d425f0ca`.
That value exactly matches `provenance.json`, both entries in
`paired-clean.json`, and an independent hash of the retained manifest bytes.
The paired roots are distinct:

- `release/release-1787881175630/`
- `release/release-1787882067758/`

Both report the same manifest hash. The SPDX SBOM hash is
`0d263c39a4961af31fa528ecc79f11c848d49325c92c118d08dda74043b3f850`.
All locally owned RC1 gates and all nine case/viewports pass without refreshing
or accepting any golden.

No source commit, lockfile identity, package checksum, SBOM checksum, or
provenance identity drift was found. The publishable repositories have no
configured remote and no RC1 tag. The crate names return no existing release
identity in the retained dry-run evidence.

## Operator Actions Not Performed

- crates.io publication, schema first and runtime second;
- creation of repository remotes or hosted releases;
- tags or pushes;
- provenance signing;
- post-publication crates.io and docs.rs propagation checks.

## External Validation

The following results remain honestly `Blocked`:

- `windows_native`: Windows native runner required;
- `macos_native`: macOS native runner required;
- `physical_gamepad`: supported physical gamepad required;
- `physical_touch`: physical touch display required;
- `provenance_signature`: authorized release signing identity required.

Cross-compilation is not native evidence, synthetic input is not physical
evidence, and unsigned provenance is not a signature.
