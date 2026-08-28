# RC1 Publication Runbook

This runbook is an operator handoff. It does not authorize publication,
tagging, signing, pushing, or a hosted release.

## Preconditions

1. Use the completed handoff bundle generated from
   `release/release-1787882067758/`.
2. Verify `rc1-handoff.json` and every file hash it records. Verify
   `signing-payload.sha256` before an authorized external signer signs
   `signing-payload.json`; do not sign regenerated or substituted package bytes.
3. Verify the paired manifest SHA-256 is
   `552bd00dd3fa96ed2ccbac2851bfeb1a3a6d21884cf408866323c407d425f0ca`.
4. Require an explicitly authorized crates.io operator and short-lived token in
   `CARGO_REGISTRY_TOKEN`; never write the token into a file or command log.
5. Confirm `openpencil_ui_schema` and `bevy_openpencil` version
   `0.1.0-rc.1` do not already exist.
6. Import external results with `just import-rc1-external RESULT`. A passing
   gate requires repository-relative evidence files whose SHA-256 values match;
   never convert unavailable hardware, runners, or signing authority into a
   pass.
7. Run the provider-neutral `just ci-rc1` gate. No hosted-provider workflow is
   claimed until these repositories have a configured writable remote.

## Dependency Order

The schema must become resolvable before the runtime is uploaded:

```console
cargo search openpencil_ui_schema --limit 1
cargo publish --locked -p openpencil_ui_schema
cargo info openpencil_ui_schema@0.1.0-rc.1
cargo publish --locked -p bevy_openpencil
cargo info bevy_openpencil@0.1.0-rc.1
```

Before each real `cargo publish`, compare the generated archive with the
handoff checksum. Abort on a mismatch. The retained runtime archive records the
temporary unpublished-schema packaging rewrite in `.cargo_vcs_info.json`; do
not conceal that fact or silently substitute different bytes. If project policy
requires upload of the exact retained archive rather than Cargo regeneration,
use only a separately reviewed crates.io API uploader and its byte-exact upload
request. This repository intentionally does not auto-upload.

## Post-Publication Verification

After crates.io propagation:

```console
cargo info openpencil_ui_schema@0.1.0-rc.1
cargo info bevy_openpencil@0.1.0-rc.1
cargo new --bin /tmp/opui-rc1-smoke
cargo add --manifest-path /tmp/opui-rc1-smoke/Cargo.toml openpencil_ui_schema@=0.1.0-rc.1
cargo add --manifest-path /tmp/opui-rc1-smoke/Cargo.toml bevy_openpencil@=0.1.0-rc.1
cargo check --manifest-path /tmp/opui-rc1-smoke/Cargo.toml --locked
```

Verify docs.rs metadata, repository, license, readme, features, Rust 1.95 MSRV,
and the exact schema dependency. Verify every Bevy 0.19.1 package resolves from
the accepted Veritasium revision before declaring the adopter supported.

## Abort Conditions

Abort immediately for:

- an existing crate/version identity;
- package, manifest, SBOM, or provenance checksum mismatch;
- unresolved exact schema dependency;
- any owner-workspace path or path patch in a downstream lock;
- unexpected API or semver drift;
- failed documentation build or metadata mismatch;
- an unexplained panic, error log, leak, or external-result identity mismatch;
- absent publication authority.

## Rollback

Crates.io releases cannot be overwritten. If the schema publishes but runtime
publication fails, do not yank the schema automatically. Preserve logs, stop
the release, diagnose the runtime package, and make an explicit operator
decision. If an incorrect crate is published, follow crates.io yank policy,
record the incident, and prepare a new pre-release version; never reuse
`0.1.0-rc.1` bytes or tag names.
