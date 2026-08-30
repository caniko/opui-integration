openpencil := "../openpencil"
opui := "../opui"
out := "generated"
item := "artboard"
designer_source := "designer/showcase.op"
designer_spec := "designer/showcase.runtime-ui.json"
designer_package := "generated/showcase.opui"
op := "../openpencil/target/debug/op"

export:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -n "${OPUI_RASTER_EXPORTER:-}" ]]; then
      "$OPUI_RASTER_EXPORTER" export --file fixtures/runtime-ui.op --format opui --output {{out}}/runtime-ui.opui --item {{item}}
    else
      cargo run --manifest-path {{openpencil}}/Cargo.toml -p op-cli -- export --file fixtures/runtime-ui.op --format opui --output {{out}}/runtime-ui.opui --item {{item}}
    fi

export-raster:
    cargo run --manifest-path {{openpencil}}/Cargo.toml -p op-cli --features opui-raster -- export --file fixtures/runtime-ui.op --format opui --output {{out}}/runtime-ui-raster.opui --item {{item}} --raster-native

check:
    cargo run --manifest-path {{opui}}/Cargo.toml --bin opui -- check {{out}}/runtime-ui.opui

determinism: export
    cp {{out}}/runtime-ui.opui {{out}}/runtime-ui.copy.opui
    cargo run --manifest-path {{openpencil}}/Cargo.toml -p op-cli -- export --file fixtures/runtime-ui.op --format opui --output {{out}}/runtime-ui.opui --item {{item}}
    cmp {{out}}/runtime-ui.opui {{out}}/runtime-ui.copy.opui

visual-op SIZE:
    #!/usr/bin/env bash
    set -euo pipefail
    dir="{{out}}/visual/{{SIZE}}"
    mkdir -p "$dir"
    case "{{SIZE}}" in
      1280x720) src=fixtures/runtime-ui.op ;;
      *) src="$dir/runtime-ui.op" ;;
    esac
    OPENPENCIL_RENDER_MARGIN=0 openpencil-desktop --render-shots "$src" "$dir" 1

visual-control SIZE:
    #!/usr/bin/env bash
    set -euo pipefail
    dir="{{out}}/visual/{{SIZE}}"
    mkdir -p "$dir"
    cargo run --features visual --bin bevy-shot -- --scene control "$dir" "{{SIZE}}"

visual-bevy SIZE:
    #!/usr/bin/env bash
    set -euo pipefail
    dir="{{out}}/visual/{{SIZE}}"
    mkdir -p "$dir"
    cp "{{out}}/runtime-ui.opui" "$dir/"
    if [ -d "{{out}}/runtime-ui.opui.assets" ]; then
      rm -rf "$dir/runtime-ui.opui.assets"
      cp -a "{{out}}/runtime-ui.opui.assets" "$dir/"
    fi
    cargo run --features visual --bin bevy-shot -- --scene opui "$dir" "{{SIZE}}"

test:
    cargo test

verify-pins:
    #!/usr/bin/env bash
    set -euo pipefail
    exp=f4b6dc6df431efae9245be51b6c08c828339b007
    got=$(git -C "{{opui}}" rev-parse feat/opui-v1-contract)
    test "$got" = "$exp"

verify-lock-dev:
    cargo test --quiet --lib lock::
    cargo run --quiet --bin opui-certify -- --verify-lock-dev

verify-lock-strict:
    cargo test --quiet --lib lock::
    cargo run --quiet --bin opui-certify -- --verify-lock-strict

verify-lock: verify-lock-dev

certify-case CASE SIZE:
    cargo run --quiet --bin opui-certify -- {{CASE}} {{SIZE}}

diagnose-visual CASE SIZE:
    cargo run --quiet --bin opui-certify -- --diagnose-visual {{CASE}} {{SIZE}}

certify-release:
    cargo run --quiet --bin opui-certify -- --release

certify-release-clean:
    cargo run --quiet --bin opui-certify -- --release-clean

certify-rc1:
    nix --option min-free 0 --option max-free 0 develop .#graphical --command cargo run --quiet --bin opui-certify -- --release-profile rc1

certify-rc1-clean:
    nix --option min-free 0 --option max-free 0 develop .#graphical --command cargo run --quiet --bin opui-certify -- --release-clean-profile rc1

certify-rc2:
    nix --option min-free 0 --option max-free 0 develop .#graphical --command cargo run --quiet --bin opui-certify -- --release-profile rc2

certify-rc2-clean:
    nix --option min-free 0 --option max-free 0 develop .#graphical --command cargo run --quiet --bin opui-certify -- --release-clean-profile rc2

assess-stable-v1:
    cargo run --quiet --bin opui-handoff -- assess stable-v1 release/release-1787882067758 handoff/stable-v1-readiness-0.1.0-rc.1.json

prepare-rc1-handoff:
    cargo run --quiet --bin opui-handoff -- prepare release/release-1787882067758 handoff/rc1-0.1.0-rc.1

rehearse-rc1-publication:
    cargo run --quiet --bin opui-handoff -- rehearse release/release-1787882067758 handoff/rc1-publication-rehearsal

rehearse-rc2-publication CAPSULE OUTPUT="handoff/rc2-publication-rehearsal":
    cargo run --quiet --bin opui-handoff -- rehearse-rc2 {{CAPSULE}} {{OUTPUT}}

import-rc1-external RESULT:
    cargo run --quiet --bin opui-handoff -- import-external rc1 {{RESULT}}

import-rc2-external RESULT:
    cargo run --quiet --bin opui-handoff -- import-external rc2 {{RESULT}}

hosted-rc2-candidate:
    #!/usr/bin/env bash
    set -euo pipefail
    test "${GITHUB_ACTIONS:-}" = true
    test "${RUNNER_ENVIRONMENT:-}" = github-hosted
    test "${GITHUB_EVENT_NAME:-}" = workflow_dispatch
    test "${GITHUB_REF:-}" = refs/heads/source/opui-v1-rc2
    test "$(git rev-parse HEAD)" = "${GITHUB_SHA:-}"
    test "$(cargo --version | cut -d' ' -f1-2)" = "cargo 1.95.0"
    test "$(rustc -vV | sed -n 's/^release: //p')" = "1.95.0"
    cargo run --quiet --bin opui-handoff -- materialize-public
    cargo run --quiet --bin opui-certify -- --verify-lock-strict
    capsule="$(cargo run --quiet --bin opui-certify -- --release-clean-profile rc2)"
    test -f "$capsule/complete"
    evidence="handoff/hosted-${GITHUB_RUN_ID}"
    mkdir -p "$evidence"
    cargo run --quiet --bin opui-handoff -- rehearse-rc2 "$capsule" "$evidence/rehearsal"
    cargo run --quiet --bin opui-handoff -- close-public-sources "$capsule" "$evidence/public-source-closure.json" "$evidence/rehearsal"
    cargo run --quiet --bin opui-handoff -- generate-rc2-external "$capsule" "$evidence/public-source-closure.json" "$evidence/external-results-rc2.toml"
    cargo run --quiet --bin opui-handoff -- import-external rc2 "$evidence/external-results-rc2.toml"
    cargo run --quiet --bin opui-handoff -- assess rc2 "$capsule" "$evidence/rc2-assessment.json"
    cargo run --quiet --bin opui-handoff -- bundle-index "$evidence/bundle-index.json" "$capsule" "$evidence"
    cargo run --quiet --bin opui-handoff -- verify-bundle-index "$evidence/bundle-index.json"

ci-rc1:
    cargo fmt --all --check
    cargo test --workspace
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    just _ci-rc1-handoff

_ci-rc1-handoff:
    #!/usr/bin/env bash
    set -euo pipefail
    temporary="$(mktemp -d)"
    trap 'rm -rf "$temporary"' EXIT
    output="$temporary/opui-rc1"
    cargo run --quiet --bin opui-handoff -- rehearse release/release-1787882067758 "$output/rehearsal"
    cargo run --quiet --bin opui-handoff -- prepare release/release-1787882067758 "$output/handoff"

certify-rc1-handoff-clean OUTPUT="handoff/rc1-final-0.1.0-rc.1":
    cargo fmt --all --check
    cargo test --workspace
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo run --quiet --bin opui-handoff -- finalize release/release-1787882067758 {{OUTPUT}}

package-preflight:
    cargo run --quiet --bin opui-certify -- --package-preflight

package-preflight-rc2:
    cargo run --quiet --bin opui-certify -- --package-preflight rc2

rc2-artifacts DESTINATION:
    cargo run --quiet --bin opui-certify -- --rc2-artifacts {{DESTINATION}}

accept-golden CASE SIZE:
    cargo run --quiet --bin opui-certify -- --accept-golden {{CASE}} {{SIZE}}

export-raster-nix:
    nix develop .#raster --command just export-raster

ci: certify-release-clean

repro-clean:
    #!/usr/bin/env bash
    set -euo pipefail
    dest=/tmp/opui-repro-$$
    mkdir -p "$dest"
    git -C "{{opui}}" worktree add "$dest/opui" HEAD
    git -C "{{openpencil}}" worktree add "$dest/openpencil" HEAD
    git -C ../bevy_openpencil worktree add "$dest/bevy_openpencil" HEAD
    git worktree add "$dest/opui-integration" HEAD
    (cd "$dest/opui-integration" && cargo test --lib)
    git -C "{{opui}}" worktree remove "$dest/opui"
    git -C "{{openpencil}}" worktree remove "$dest/openpencil"
    git -C ../bevy_openpencil worktree remove "$dest/bevy_openpencil"
    git worktree remove "$dest/opui-integration"

conformance: determinism check test

designer-export:
    cargo build --manifest-path {{openpencil}}/Cargo.toml -p op-cli
    {{op}} runtime-ui:metadata --file {{designer_source}} --spec {{designer_spec}}
    {{op}} export --file {{designer_source}} --format opui --output {{designer_package}}

designer-check: designer-export
    cargo run --manifest-path {{opui}}/Cargo.toml --quiet --bin opui -- check {{designer_package}}

designer-bindings: designer-export
    cargo run --quiet --bin opui-bindings -- {{designer_package}} src/showcase_bindings.rs

designer-bindings-check:
    cargo run --quiet --bin opui-bindings -- --check {{designer_package}} src/showcase_bindings.rs

designer-ui-dev: designer-check
    #!/usr/bin/env bash
    set -euo pipefail
    {{op}} export --file {{designer_source}} --format opui --output {{designer_package}} --watch &
    watcher=$!
    trap 'kill "$watcher" 2>/dev/null || true' EXIT
    cargo run --features visual --bin designer-showcase -- --watch

designer-workflow-test:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --manifest-path {{openpencil}}/Cargo.toml -p op-cli
    {{op}} runtime-ui:metadata --file {{designer_source}} --spec {{designer_spec}}
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    {{op}} export --file {{designer_source}} --format opui --output "$tmp/showcase.opui"
    cargo run --manifest-path {{opui}}/Cargo.toml --quiet --bin opui -- check "$tmp/showcase.opui"
    diff -r "$tmp/showcase.opui" {{designer_package}}
    diff -r "$tmp/showcase.opui.assets" {{designer_package}}.assets
    cargo run --quiet --bin opui-bindings -- --check "$tmp/showcase.opui" src/showcase_bindings.rs
    cargo test --features visual --lib showcase::tests::application_journey_keeps_staged_settings_explicit
    cargo test --features visual --test showcase
    cargo check --features visual --bin designer-showcase
    cargo run --quiet --features visual --bin runtime-probe -- generated showcase.opui app 1280x720 app.root screen.main_menu main_menu.play
    cargo run --quiet --features visual --bin runtime-probe -- generated showcase.opui app 1920x1080 app.root screen.main_menu main_menu.play
    cargo run --quiet --features visual --bin runtime-probe -- generated showcase.opui app 800x1280 app.root screen.main_menu main_menu.play
    test -s generated/accessibility.json
    grep -q '"runtime_id": "main_menu.play"' generated/accessibility.json
    cargo test --manifest-path {{openpencil}}/Cargo.toml -p op-runtime-ui package_promotion_keeps_last_good_output_on_validation_failure
    cargo test --manifest-path {{openpencil}}/Cargo.toml -p op-cli export_opui_watch_

graphical-showcase-test EVIDENCE:
    #!/usr/bin/env bash
    set -euo pipefail
    evidence="{{EVIDENCE}}"
    runtime="$(mktemp -d)"
    weston_pid=""
    cleanup() {
      if [[ -n "$weston_pid" ]]; then
        kill "$weston_pid" 2>/dev/null || true
        wait "$weston_pid" 2>/dev/null || true
      fi
      rm -rf "$runtime"
    }
    trap cleanup EXIT
    chmod 700 "$runtime"
    export XDG_RUNTIME_DIR="$runtime"
    mkdir -p "$evidence"
    libinput list-devices > "$evidence/libinput.log"
    cargo build --quiet --features visual --bin bevy-shot
    binary="${CARGO_TARGET_DIR:-target}/debug/bevy-shot"
    run_shot() {
      size="$1"
      width="${size%x*}"
      height="${size#*x}"
      dir="$evidence/$size"
      mkdir -p "$dir"
      cp generated/showcase.opui "$dir/"
      cp -a generated/showcase.opui.assets "$dir/"
      weston -B headless --renderer=pixman --shell=kiosk-shell.so --width="$width" --height="$height" --fake-seat -S "wayland-$size" --no-config --log="$dir/weston.log" &
      weston_pid="$!"
      for _ in {1..200}; do
        [[ -S "$runtime/wayland-$size" ]] && break
        kill -0 "$weston_pid"
        sleep 0.05
      done
      test -S "$runtime/wayland-$size"
      WAYLAND_DISPLAY="wayland-$size" timeout 120s "$binary" --windowed --scene opui --package showcase.opui --entrypoint app "$dir" "$size"
      kill "$weston_pid" 2>/dev/null || true
      wait "$weston_pid" || true
      weston_pid=""
    }
    run_shot 1280x720
    run_shot 1920x1080
    run_shot 800x1280

graphical-rc1-stress-test EVIDENCE:
    #!/usr/bin/env bash
    set -euo pipefail
    evidence="{{EVIDENCE}}"
    runtime="$(mktemp -d)"
    weston_pid=""
    cleanup() {
      if [[ -n "$weston_pid" ]]; then
        kill "$weston_pid" 2>/dev/null || true
        wait "$weston_pid" 2>/dev/null || true
      fi
      rm -rf "$runtime"
    }
    trap cleanup EXIT
    chmod 700 "$runtime"
    export XDG_RUNTIME_DIR="$runtime"
    mkdir -p "$evidence"
    cp generated/showcase.opui "$evidence/"
    cp -a generated/showcase.opui.assets "$evidence/"
    cargo build --quiet --release --features visual --bin bevy-shot
    binary="${CARGO_TARGET_DIR:-target}/release/bevy-shot"
    weston -B headless --renderer=pixman --shell=kiosk-shell.so --width=1280 --height=720 --fake-seat -S wayland-rc1-stress --no-config --log="$evidence/weston.log" &
    weston_pid="$!"
    for _ in {1..200}; do
      [[ -S "$runtime/wayland-rc1-stress" ]] && break
      kill -0 "$weston_pid"
      sleep 0.05
    done
    test -S "$runtime/wayland-rc1-stress"
    ulimit -v 16777216
    WAYLAND_DISPLAY=wayland-rc1-stress timeout 180s "$binary" --windowed --scene opui --package showcase.opui --entrypoint app --stress-cycles 20 "$evidence" 1280x720
    test "$(jq -r .completed_cycles "$evidence/stress.json")" = 20
