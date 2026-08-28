pub mod case;
pub mod cert;
pub mod clean;
pub mod computed_diff;
pub mod external_results;
pub mod gate;
pub mod handoff;
pub mod image_metrics;
pub mod lock;
pub mod package;
pub mod package_preflight;
pub mod rehearsal;
pub mod release_artifacts;
pub mod release_profile;
#[cfg(feature = "visual")]
pub mod showcase;
pub mod showcase_bindings;
pub mod visual_diagnostics;

pub const REQUIRED_RUNTIME_IDS: &[&str] = &[
    "main_menu",
    "main_menu.title",
    "main_menu.play",
    "main_menu.settings",
    "main_menu.quit",
    "inventory",
    "inventory.grid",
    "inventory.close",
];

pub fn runtime_ids(manifest: &serde_json::Value) -> Vec<String> {
    manifest["nodes"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, n)| n["runtime_id"].as_str().map(str::to_string))
        .collect()
}
pub mod bindings;
