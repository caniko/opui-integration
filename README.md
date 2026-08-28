# opui-integration

E2E for frozen OPUI v1: OpenPencil public CLI → checker → `bevy_openpencil`.

```
just conformance          # determinism + check + tests
just verify-pins          # frozen opui v1 SHA
just verify-lock-dev      # pins; permits declared development dirt
just verify-lock-strict   # pins + clean release worktrees
just certify-case C S     # one case/viewport with provenance
just certify-release      # aggregate release report
just certify-release-clean # exact clean clones, strict lock, aggregate, atomic evidence
just certify-rc1-clean     # paired RC1 clean capsules + matching artifact hashes
just package-preflight    # schema dry-run + packaged runtime/schema validation
just ci                   # provider-neutral clean release lane
just visual-control SIZE  # official UI-to-texture (no OPUI)
just visual-bevy SIZE     # OPUI screenshot vs OP artboard
just export-raster-nix    # reproducible freetype + Skia environment
just designer-bindings    # regenerate typed Rust ids from showcase.opui
just designer-bindings-check # fail when generated bindings are stale
```

Cases: `conformance/cases/`. Pins: `repos.lock.toml`. Verdict: `RELEASE.md`.

The integration harness remains independently versioned at `0.1.0` and is not
published. The two publishable crates are versioned together at
`0.1.0-rc.1`.
