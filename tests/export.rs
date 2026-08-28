use std::path::PathBuf;

use opui_integration::{REQUIRED_RUNTIME_IDS, runtime_ids};

fn generated() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("generated/runtime-ui.opui")
}

#[test]
fn exported_package_is_checker_clean() {
    let path = generated();
    assert!(
        path.exists(),
        "run `just export` first ({})",
        path.display()
    );
    let diags = opui::check_path(&path, &opui::CheckOptions::for_path(&path));
    let errors: Vec<_> = diags.iter().filter(|d| d.severity == "error").collect();
    assert!(errors.is_empty(), "{}", opui::format_diagnostics(&diags));
}

#[test]
fn required_runtime_ids_present() {
    let src = std::fs::read_to_string(generated()).expect("run `just export` first");
    let manifest: serde_json::Value = serde_json::from_str(&src).unwrap();
    let ids = runtime_ids(&manifest);
    for required in REQUIRED_RUNTIME_IDS {
        assert!(
            ids.iter().any(|id| id == required),
            "missing {required} in {ids:?}"
        );
    }
}

#[test]
fn fixture_features_are_honest() {
    let src = std::fs::read_to_string(generated()).expect("run `just export` first");
    let m: serde_json::Value = serde_json::from_str(&src).unwrap();
    let nodes = m["nodes"].as_object().unwrap();

    assert_eq!(nodes["banner"]["layout"]["width"]["type"], "percent");
    assert_eq!(nodes["banner"]["style"]["fill"]["type"], "linear");
    assert_eq!(
        nodes["banner"]["style"]["corner_radius"]["top_right"]["value"],
        16.0
    );

    assert_eq!(nodes["main_menu"]["layout"]["display"], "flex");
    assert_eq!(nodes["inventory.grid"]["layout"]["display"], "flex");
    assert_eq!(nodes["inventory.grid"]["layout"]["flex_direction"], "row");

    assert_eq!(nodes["hud_badge"]["layout"]["position"], "absolute");
    assert_eq!(nodes["glass"]["style"]["opacity"], 0.85);
    assert_eq!(nodes["glass"]["style"]["rotation"]["degrees"], 8.0);
    assert_eq!(nodes["glass"]["style"]["clipping"], true);
    assert!(
        nodes["glass"]["style"]["outer_shadows"]
            .as_array()
            .unwrap()
            .len()
            == 1
    );

    assert_eq!(nodes["crest"]["type"], "image");
    assert!(
        !nodes["blurb"]["text"]["runs"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    assert_eq!(nodes["main_menu.play"]["component"]["role"], "instance");
    assert_eq!(
        nodes["main_menu.play"]["component"]["definition_source_id"],
        "button_primary"
    );
    assert_eq!(
        nodes["main_menu.settings"]["component"]["definition_source_id"],
        "button_secondary"
    );

    let diags = m["diagnostics"].as_array().unwrap();
    assert!(
        diags
            .iter()
            .any(|d| d["node_id"] == "badge_orb" && d["code"] == "opui.unsupported_native"),
        "ellipse should stay a raster candidate without --raster-native"
    );
}
