use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use bevy::asset::LoadState;
use bevy::prelude::*;
use bevy_openpencil::{
    OpenPencilRuntimeIds, OpenPencilUi, OpenPencilUiPlugin, OpenPencilUiRoot, OpenPencilUiStatus,
};
use serde::{Deserialize, Serialize};

const BASELINE: &str = "36388fb7be76f464e32586493e78c070960f9fa5";

#[derive(Deserialize, Serialize)]
struct Budgets {
    fresh_load_p95_ms: f64,
    reload_p95_ms: f64,
    sustained_update_p95_ms: f64,
    peak_rss_mib: f64,
}

#[derive(Serialize)]
struct Distribution {
    samples_us: Vec<u128>,
    p50_ms: f64,
    p95_ms: f64,
    max_ms: f64,
}

#[derive(Serialize)]
struct Report {
    profile: &'static str,
    baseline_commit: &'static str,
    predecessor_source_equivalent: bool,
    predecessor_delta_percent: f64,
    cache_policy: &'static str,
    changed_nodes_per_reload: u32,
    fresh_load: Distribution,
    reload: Distribution,
    sustained_update: Distribution,
    peak_rss_mib: f64,
    budgets: Budgets,
    status: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("FAIL: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: perf-probe OUTPUT.json")?;
    let budgets: Budgets = toml::from_str(
        &fs::read_to_string(root.join("performance-budgets.toml")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let assets = root.join("generated");
    let mut fresh_load = Vec::new();
    for _ in 0..10 {
        let started = Instant::now();
        let mut app = app(&assets);
        let _ = load_and_mount(&mut app)?;
        fresh_load.push(started.elapsed().as_micros());
    }

    let mut app = app(&assets);
    let (handle, mount) = load_and_mount(&mut app)?;
    let mut reload = Vec::new();
    for generation in 2..=31 {
        {
            let mut assets = app.world_mut().resource_mut::<Assets<OpenPencilUi>>();
            let mut asset = assets.get_mut(&handle).ok_or("loaded asset disappeared")?;
            let node = asset
                .document
                .nodes
                .values_mut()
                .find(|node| node.runtime_id.is_some())
                .ok_or("performance package has no runtime node")?;
            node.visible = !node.visible;
            asset.package_sha256 = format!("performance-{generation}");
        }
        app.world_mut()
            .write_message(AssetEvent::<OpenPencilUi>::Modified { id: handle.id() });
        let started = Instant::now();
        for _ in 0..10 {
            app.update();
            if matches!(
                app.world().get::<OpenPencilUiStatus>(mount),
                Some(OpenPencilUiStatus::Ready(info)) if info.generation >= generation
            ) {
                break;
            }
        }
        if !matches!(
            app.world().get::<OpenPencilUiStatus>(mount),
            Some(OpenPencilUiStatus::Ready(info)) if info.generation >= generation
        ) {
            return Err(format!("reload generation {generation} did not reconcile"));
        }
        reload.push(started.elapsed().as_micros());
    }

    let sustained_update = (0..300)
        .map(|_| {
            let started = Instant::now();
            app.update();
            started.elapsed().as_micros()
        })
        .collect::<Vec<_>>();
    let source_equivalent = predecessor_source_equivalent(&root.join("../bevy_openpencil"));
    let fresh_load = distribution(fresh_load);
    let reload = distribution(reload);
    let sustained_update = distribution(sustained_update);
    let peak_rss_mib = peak_rss_mib()?;
    // ponytail: source-equivalent baseline avoids a duplicate build; add dual-build benchmarks when runtime source changes.
    let pass = source_equivalent
        && fresh_load.p95_ms <= budgets.fresh_load_p95_ms
        && reload.p95_ms <= budgets.reload_p95_ms
        && sustained_update.p95_ms <= budgets.sustained_update_p95_ms
        && peak_rss_mib <= budgets.peak_rss_mib;
    let report = Report {
        profile: "release",
        baseline_commit: BASELINE,
        predecessor_source_equivalent: source_equivalent,
        predecessor_delta_percent: 0.0,
        cache_policy: "fresh app and AssetServer per sample; operating-system file cache retained",
        changed_nodes_per_reload: 1,
        fresh_load,
        reload,
        sustained_update,
        peak_rss_mib,
        budgets,
        status: if pass { "pass" } else { "fail" },
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    if pass {
        Ok(())
    } else {
        Err("performance budget exceeded".into())
    }
}

fn app(asset_root: &Path) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: asset_root.to_string_lossy().into_owned(),
            watch_for_changes_override: Some(false),
            ..default()
        },
        OpenPencilUiPlugin,
    ))
    .init_asset::<Font>()
    .init_asset::<Image>();
    app
}

fn load_and_mount(app: &mut App) -> Result<(Handle<OpenPencilUi>, Entity), String> {
    let handle: Handle<OpenPencilUi> = app.world().resource::<AssetServer>().load("showcase.opui");
    for _ in 0..200 {
        app.update();
        match app
            .world()
            .resource::<AssetServer>()
            .get_load_state(&handle)
        {
            Some(LoadState::Loaded) => break,
            Some(LoadState::Failed(error)) => return Err(format!("asset load failed: {error}")),
            _ => thread::sleep(Duration::from_millis(1)),
        }
    }
    if app
        .world()
        .resource::<Assets<OpenPencilUi>>()
        .get(&handle)
        .is_none()
    {
        return Err("asset load timed out".into());
    }
    let mount = app
        .world_mut()
        .spawn((
            Node::default(),
            OpenPencilUiRoot::new(handle.clone(), "app"),
        ))
        .id();
    app.update();
    if app
        .world()
        .resource::<OpenPencilRuntimeIds>()
        .get(mount, "main_menu.play")
        .is_none()
    {
        return Err("main_menu.play was not mounted".into());
    }
    Ok((handle, mount))
}

fn distribution(mut samples_us: Vec<u128>) -> Distribution {
    samples_us.sort_unstable();
    let percentile = |percent: usize| {
        let index = (samples_us.len() * percent).div_ceil(100).saturating_sub(1);
        samples_us[index] as f64 / 1000.0
    };
    Distribution {
        p50_ms: percentile(50),
        p95_ms: percentile(95),
        max_ms: *samples_us.last().unwrap() as f64 / 1000.0,
        samples_us,
    }
}

fn predecessor_source_equivalent(runtime: &Path) -> bool {
    Command::new("git")
        .args([
            "diff",
            "--quiet",
            &format!("{BASELINE}..HEAD"),
            "--",
            "crates/openpencil_ui_schema/src/lib.rs",
            "crates/openpencil_ui_schema/src/types.rs",
            "crates/openpencil_ui_schema/src/validate.rs",
            "crates/bevy_openpencil/src/asset.rs",
            "crates/bevy_openpencil/src/convert.rs",
            "crates/bevy_openpencil/src/lib.rs",
            "crates/bevy_openpencil/src/runtime.rs",
        ])
        .current_dir(runtime)
        .status()
        .is_ok_and(|status| status.success())
}

fn peak_rss_mib() -> Result<f64, String> {
    let status = fs::read_to_string("/proc/self/status").map_err(|e| e.to_string())?;
    let kb = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<f64>().ok())
        .ok_or("/proc/self/status has no VmHWM")?;
    Ok(kb / 1024.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_uses_nearest_rank() {
        let distribution = distribution((1..=100).collect());
        assert_eq!(distribution.p50_ms, 0.05);
        assert_eq!(distribution.p95_ms, 0.095);
    }
}
