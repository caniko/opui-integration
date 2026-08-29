# Pins

| Repo | Branch | SHA | Role |
| --- | --- | --- | --- |
| opui | `feat/opui-v1-contract` | `f4b6dc6df431efae9245be51b6c08c828339b007` | frozen v1 |
| opui | `feat/opui-v2-contract` | `04fdda1c8a2dabd4fad3ee66dd9043f44ed8509c` | checker; must stay v1-compatible |
| openpencil | detached candidate | `a18720df451501878afcdc537026e15dfd15a14d` | public export + native Runtime UI inspector + RC2 asset provenance |
| Jian | detached | `ba334d27edf05b7e4c7a2746fc3c664d9ed24f28` | layout, native text, runtime metadata schema |
| bevy_openpencil | `feat/opui-runtime-v1` | see `repos.lock.toml` | Bevy 0.19 loader + precise mount lifecycle |
| Veritasium | `trunk` | `7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0` | exact Bevy 0.19.1 source |

The cancellation owner fix is promoted on Codeberg `trunk`, resolves over Cargo HTTPS, and is the active consumer pin.

`just verify-pins` checks the frozen v1 SHA only.
`repos.lock.toml` pins all repositories, the renderer executable hash, and
OpenPencil submodules. `just certify-release-clean` realizes those pins in
isolated clones.
