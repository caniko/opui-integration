use std::path::PathBuf;
use std::process::Command;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn op_bin() -> PathBuf {
    std::env::var_os("OPENPENCIL_OP")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo().join("../openpencil/target/debug/op"))
}

fn export(file: &str, extra: &[&str]) -> (bool, String) {
    let op = op_bin();
    assert!(
        op.exists(),
        "build op first: cargo build -p op-cli ({})",
        op.display()
    );
    let out = repo().join("generated/negative.opui");
    let mut cmd = Command::new(op);
    cmd.args(["export", "--file", file, "--format", "opui", "--output"])
        .arg(&out)
        .args(extra);
    let output = cmd.output().expect("spawn op");
    let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    (output.status.success(), text)
}

#[test]
fn missing_size_fails_cleanly() {
    let file = repo().join("fixtures/invalid/no-size.op");
    let (ok, text) = export(file.to_str().unwrap(), &[]);
    assert!(!ok, "{text}");
    assert!(
        text.contains("width/height") || text.contains("authored numeric"),
        "{text}"
    );
}

#[test]
fn garbage_json_fails_cleanly() {
    let file = repo().join("fixtures/invalid/not-json.op");
    let (ok, text) = export(file.to_str().unwrap(), &[]);
    assert!(!ok, "{text}");
}

#[test]
fn cyclic_ref_fails_cleanly() {
    let file = repo().join("fixtures/invalid/cyclic.op");
    let (ok, text) = export(file.to_str().unwrap(), &["--item", "root"]);
    assert!(!ok, "{text}");
    assert!(text.contains("cyclic"), "{text}");
}

#[test]
fn strict_and_raster_native_rejected() {
    let file = repo().join("fixtures/runtime-ui.op");
    let (ok, text) = export(
        file.to_str().unwrap(),
        &["--item", "artboard", "--strict", "--raster-native"],
    );
    assert!(!ok, "{text}");
    assert!(
        text.contains("--strict") && text.contains("--raster-native"),
        "{text}"
    );
}
