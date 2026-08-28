use std::path::PathBuf;

use opui_integration::REQUIRED_RUNTIME_IDS;

fn raster() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("generated/runtime-ui-raster.opui")
}

#[test]
#[ignore = "needs `just export-raster` (op --features opui-raster); local Linux environment cannot link skia"]
fn raster_native_rewrites_ellipse() {
    let path = raster();
    assert!(
        path.exists(),
        "run `just export-raster` first ({})",
        path.display()
    );
    let m: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(m["nodes"]["badge_orb"]["type"], "fallback");
    assert_eq!(m["nodes"]["badge_orb"]["runtime_id"], "badge_orb");
    let asset = m["nodes"]["badge_orb"]["fallback"]["asset"]
        .as_str()
        .unwrap();
    let uri = m["assets"][asset]["uri"].as_str().unwrap();
    assert!(uri.starts_with("fallback/"), "{uri}");
    let png = path.with_file_name(format!(
        "{}.assets/{uri}",
        path.file_name().unwrap().to_string_lossy()
    ));
    assert!(png.is_file(), "missing {}", png.display());
    assert!(
        m["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["node_id"] == "badge_orb" && d["code"] == "opui.rasterization"),
        "{:?}",
        m["diagnostics"]
    );
    for id in REQUIRED_RUNTIME_IDS {
        assert!(
            m["nodes"]
                .as_object()
                .unwrap()
                .values()
                .any(|n| n["runtime_id"] == *id),
            "missing {id}"
        );
    }
    let diags = opui::check_path(&path, &opui::CheckOptions::for_path(&path));
    let errors: Vec<_> = diags.iter().filter(|d| d.severity == "error").collect();
    assert!(errors.is_empty(), "{}", opui::format_diagnostics(&diags));
}
