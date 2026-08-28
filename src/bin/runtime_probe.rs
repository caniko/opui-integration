use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::asset::{AssetPlugin, AssetServer, Assets, LoadState};
use bevy::camera::{Camera, ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::core_pipeline::CorePipelinePlugin;
use bevy::image::{CompressedImageFormats, ImageLoader};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::sprite_render::SpriteRenderPlugin;
use bevy::ui::ComputedStackIndex;
use bevy::ui_render::UiRenderPlugin;
use bevy::window::{ExitCondition, WindowPlugin};
use bevy::winit::WinitPlugin;
use bevy_openpencil::{
    OpenPencilKind, OpenPencilOwned, OpenPencilRuntimeId, OpenPencilRuntimeIds, OpenPencilSourceId,
    OpenPencilUi, OpenPencilUiPlugin, OpenPencilUiRoot,
};

#[derive(serde::Serialize)]
struct DependencyState {
    id: String,
    kind: &'static str,
    load: String,
    direct_dependencies: String,
    recursive_dependencies: String,
}

#[derive(serde::Serialize)]
struct LoaderReport {
    package: String,
    manifest: String,
    direct_dependencies: String,
    recursive_dependencies: String,
    dependencies: Vec<DependencyState>,
    loaded_with_dependencies: bool,
    error: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("runtime-probe: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(
        args.next()
            .ok_or("usage: runtime-probe DIR PACKAGE ENTRYPOINT WxH [ID ...]")?,
    );
    let package = args.next().ok_or("missing package")?;
    let entrypoint = args.next().ok_or("missing entrypoint")?;
    let size = args.next().ok_or("missing WxH")?;
    let expected_ids: Vec<String> = args.collect();
    let (width, height) = parse_size(&size)?;

    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .build()
            .disable::<bevy::log::LogPlugin>()
            .disable::<bevy::app::TerminalCtrlCHandlerPlugin>()
            .disable::<WinitPlugin>()
            .disable::<RenderPlugin>()
            .disable::<PipelinedRenderingPlugin>()
            .disable::<CorePipelinePlugin>()
            .disable::<SpriteRenderPlugin>()
            .disable::<UiRenderPlugin>()
            .set(AssetPlugin {
                file_path: dir.to_string_lossy().into_owned(),
                watch_for_changes_override: Some(false),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            }),
        OpenPencilUiPlugin,
    ));
    app.register_asset_loader(ImageLoader::new(CompressedImageFormats::empty()));
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(width, height),
                        scale_factor: 1.0,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size: UVec2::new(width, height),
                    ..default()
                }),
                ..default()
            },
            IsDefaultUiCamera,
        ))
        .id();
    let handle: Handle<OpenPencilUi> = app.world().resource::<AssetServer>().load(package.clone());
    let deadline = Instant::now() + Duration::from_secs(30);
    let load_error = loop {
        app.update();
        std::thread::sleep(Duration::from_millis(1));
        let server = app.world().resource::<AssetServer>();
        if server.is_loaded_with_dependencies(&handle) {
            break None;
        }
        if let Some(LoadState::Failed(error)) = server.get_load_state(&handle) {
            break Some(error.to_string());
        }
        if Instant::now() >= deadline {
            break Some("AssetServer deadline exceeded before LoadedWithDependencies".into());
        }
    };
    let loader = loader_report(&app, &handle, &package, load_error.clone());
    write_json(&dir.join("loader-probe.json"), &loader)?;
    if let Some(error) = load_error {
        return Err(error);
    }

    let root = app
        .world_mut()
        .spawn((
            Node {
                width: px(width as f32),
                height: px(height as f32),
                ..default()
            },
            UiTargetCamera(camera),
            OpenPencilUiRoot::new(handle.clone(), entrypoint),
        ))
        .id();
    let mut last = None;
    let computed = loop {
        app.update();
        std::thread::sleep(Duration::from_millis(1));
        let ids_ready = {
            let ids = app.world().resource::<OpenPencilRuntimeIds>();
            expected_ids.iter().all(|id| ids.get(root, id).is_some())
        };
        let current = computed_snapshot(&mut app, root);
        if ids_ready && current == last {
            break current.unwrap_or_default();
        }
        last = current;
        if Instant::now() >= deadline {
            return Err(
                "reconciliation/layout deadline exceeded before stable expected IDs".into(),
            );
        }
    };
    let mapping = mapping_snapshot(&mut app, &handle)?;
    let computed_context = computed_context_snapshot(&mut app, camera);
    let registry = runtime_registry(&app, root, &expected_ids);
    let accessibility = accessibility_snapshot(&mut app);
    write_json(&dir.join("mapping.json"), &mapping)?;
    write_json(&dir.join("computed.json"), &computed)?;
    write_json(&dir.join("computed-context.json"), &computed_context)?;
    write_json(&dir.join("runtime-registry.json"), &registry)?;
    write_json(&dir.join("accessibility.json"), &accessibility)?;
    if registry["missing"]
        .as_array()
        .is_some_and(|v| !v.is_empty())
    {
        return Err("runtime registry is missing expected IDs".into());
    }
    Ok(())
}

fn loader_report(
    app: &App,
    handle: &Handle<OpenPencilUi>,
    package: &str,
    error: Option<String>,
) -> LoaderReport {
    let server = app.world().resource::<AssetServer>();
    let states = server
        .get_load_states(handle)
        .map(|(a, b, c)| (format!("{a:?}"), format!("{b:?}"), format!("{c:?}")));
    let mut dependencies = Vec::new();
    if let Some(ui) = app.world().resource::<Assets<OpenPencilUi>>().get(handle) {
        for (id, child) in &ui.images {
            dependencies.push(dependency_state(server, id, "image", child));
        }
        for (id, child) in &ui.fonts {
            dependencies.push(dependency_state(server, id, "font", child));
        }
    }
    dependencies.sort_by(|a, b| a.id.cmp(&b.id));
    LoaderReport {
        package: package.into(),
        manifest: states
            .as_ref()
            .map(|v| v.0.clone())
            .unwrap_or_else(|| "NotLoaded".into()),
        direct_dependencies: states
            .as_ref()
            .map(|v| v.1.clone())
            .unwrap_or_else(|| "NotLoaded".into()),
        recursive_dependencies: states
            .as_ref()
            .map(|v| v.2.clone())
            .unwrap_or_else(|| "NotLoaded".into()),
        dependencies,
        loaded_with_dependencies: server.is_loaded_with_dependencies(handle),
        error,
    }
}

fn dependency_state<A: Asset>(
    server: &AssetServer,
    id: &str,
    kind: &'static str,
    handle: &Handle<A>,
) -> DependencyState {
    let states = server
        .get_load_states(handle)
        .map(|(a, b, c)| (format!("{a:?}"), format!("{b:?}"), format!("{c:?}")));
    DependencyState {
        id: id.into(),
        kind,
        load: states
            .as_ref()
            .map(|v| v.0.clone())
            .unwrap_or_else(|| "NotLoaded".into()),
        direct_dependencies: states
            .as_ref()
            .map(|v| v.1.clone())
            .unwrap_or_else(|| "NotLoaded".into()),
        recursive_dependencies: states
            .as_ref()
            .map(|v| v.2.clone())
            .unwrap_or_else(|| "NotLoaded".into()),
    }
}

fn mapping_snapshot(
    app: &mut App,
    handle: &Handle<OpenPencilUi>,
) -> Result<Vec<serde_json::Value>, String> {
    let document = app
        .world()
        .resource::<Assets<OpenPencilUi>>()
        .get(handle)
        .ok_or("loaded manifest disappeared")?
        .document
        .clone();
    let world = app.world_mut();
    let mut query = world.query::<(
        &OpenPencilSourceId,
        &OpenPencilKind,
        Option<&OpenPencilOwned>,
    )>();
    let entities = query
        .iter(world)
        .map(|(source, kind, owned)| (source.0.clone(), (format!("{:?}", kind.0), owned.is_some())))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut rows = document
        .nodes
        .values()
        .map(|node| {
            let entity = entities.get(&node.source_id);
            serde_json::json!({
                "source_id": node.source_id,
                "runtime_id": node.runtime_id,
                "kind": format!("{:?}", node.kind),
                "entity_kind": entity.map(|v| v.0.clone()),
                "owned": entity.is_some_and(|v| v.1),
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a["source_id"].as_str().cmp(&b["source_id"].as_str()));
    Ok(rows)
}

fn computed_snapshot(app: &mut App, root: Entity) -> Option<Vec<serde_json::Value>> {
    let world = app.world_mut();
    let mut query = world.query::<(
        &OpenPencilSourceId,
        &ComputedNode,
        &Node,
        Option<&UiGlobalTransform>,
        Option<&InheritedVisibility>,
    )>();
    let mut rows = query
        .iter(world)
        .map(|(id, computed, style, transform, visibility)| {
            serde_json::json!({
                "source_id": id.0,
                "style_width": format!("{:?}", style.width),
                "style_height": format!("{:?}", style.height),
                "width": rounded(computed.size.x),
                "height": rounded(computed.size.y),
                "content_width": rounded(computed.content_size.x),
                "content_height": rounded(computed.content_size.y),
                "x": transform.map(|v| rounded(v.translation.x)),
                "y": transform.map(|v| rounded(v.translation.y)),
                "visible": visibility.map(|v| v.get()),
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a["source_id"].as_str().cmp(&b["source_id"].as_str()));
    (!rows.is_empty() && app.world().get::<OpenPencilUiRoot>(root).is_some()).then_some(rows)
}

fn computed_context_snapshot(app: &mut App, camera: Entity) -> Vec<serde_json::Value> {
    let world = app.world_mut();
    let identities = {
        let mut query = world.query::<(Entity, &OpenPencilSourceId)>();
        query
            .iter(world)
            .map(|(entity, source)| (entity, source.0.clone()))
            .collect::<std::collections::HashMap<_, _>>()
    };
    let mut query = world.query::<(
        Entity,
        &OpenPencilSourceId,
        Option<&OpenPencilRuntimeId>,
        &ComputedNode,
        Option<&UiGlobalTransform>,
        Option<&InheritedVisibility>,
        Option<&ChildOf>,
        Option<&Children>,
        Option<&ComputedStackIndex>,
        Option<&CalculatedClip>,
        Option<&ComputedUiTargetCamera>,
    )>();
    let mut rows = query
        .iter(world)
        .map(
            |(
                _entity,
                source,
                runtime,
                computed,
                transform,
                visibility,
                parent,
                children,
                stack,
                clip,
                target_camera,
            )| {
                let (scale, angle, translation) = transform
                    .map(UiGlobalTransform::to_scale_angle_translation)
                    .unwrap_or((Vec2::ONE, 0.0, Vec2::ZERO));
                let child_order = children
                    .into_iter()
                    .flat_map(|children| children.iter())
                    .filter_map(|child| identities.get(&child).cloned())
                    .collect::<Vec<_>>();
                serde_json::json!({
                    "source_id": source.0,
                    "runtime_id": runtime.map(|runtime| runtime.0.as_str()),
                    "parent": parent.and_then(|parent| identities.get(&parent.parent())).cloned(),
                    "child_order": child_order,
                    "computed_rectangle": {
                        "center_x": rounded(translation.x),
                        "center_y": rounded(translation.y),
                        "width": rounded(computed.size.x),
                        "height": rounded(computed.size.y),
                    },
                    "content_rectangle": {
                        "width": rounded(computed.content_size.x),
                        "height": rounded(computed.content_size.y),
                    },
                    "transform": {
                        "translation": [rounded(translation.x), rounded(translation.y)],
                        "scale": [rounded(scale.x), rounded(scale.y)],
                        "angle_radians": rounded(angle),
                    },
                    "target_camera": target_camera
                        .and_then(ComputedUiTargetCamera::get)
                        .map(|target| if target == camera { "default_ui_camera" } else { "other_camera" }),
                    "stack_index": stack.map(|stack| stack.0),
                    "clipping_rectangle": clip.map(|clip| serde_json::json!({
                        "min": [rounded(clip.clip.min.x), rounded(clip.clip.min.y)],
                        "max": [rounded(clip.clip.max.x), rounded(clip.clip.max.y)],
                    })),
                    "visible": visibility.map(|visibility| visibility.get()),
                    "resolved_border": {
                        "left": rounded(computed.border.min_inset.x),
                        "top": rounded(computed.border.min_inset.y),
                        "right": rounded(computed.border.max_inset.x),
                        "bottom": rounded(computed.border.max_inset.y),
                    },
                    "resolved_radius": {
                        "top_left": rounded(computed.border_radius.top_left),
                        "top_right": rounded(computed.border_radius.top_right),
                        "bottom_right": rounded(computed.border_radius.bottom_right),
                        "bottom_left": rounded(computed.border_radius.bottom_left),
                    },
                })
            },
        )
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left["source_id"].as_str().cmp(&right["source_id"].as_str()));
    rows
}

fn runtime_registry(app: &App, root: Entity, expected: &[String]) -> serde_json::Value {
    let ids = app.world().resource::<OpenPencilRuntimeIds>();
    let found = expected
        .iter()
        .filter(|id| ids.get(root, id).is_some())
        .cloned()
        .collect::<Vec<_>>();
    let missing = expected
        .iter()
        .filter(|id| ids.get(root, id).is_none())
        .cloned()
        .collect::<Vec<_>>();
    serde_json::json!({"expected": expected, "found": found, "missing": missing})
}

fn accessibility_snapshot(app: &mut App) -> Vec<serde_json::Value> {
    let world = app.world_mut();
    let accessible = {
        let mut query = world.query_filtered::<Entity, With<bevy::a11y::AccessibilityNode>>();
        query.iter(world).collect::<std::collections::HashSet<_>>()
    };
    let identities = {
        let mut query = world.query::<(
            Entity,
            Option<&OpenPencilRuntimeId>,
            Option<&OpenPencilSourceId>,
        )>();
        query
            .iter(world)
            .map(|(entity, runtime, source)| {
                (
                    entity,
                    runtime
                        .map(|id| id.0.clone())
                        .or_else(|| source.map(|id| id.0.clone())),
                )
            })
            .collect::<std::collections::HashMap<_, _>>()
    };
    let mut query = world.query::<(
        &bevy::a11y::AccessibilityNode,
        Option<&OpenPencilRuntimeId>,
        Option<&OpenPencilSourceId>,
        Option<&ChildOf>,
        Option<&AccessibleLabel>,
        Option<&TabIndex>,
        Has<bevy::ui::InteractionDisabled>,
    )>();
    let mut rows = query
        .iter(world)
        .map(
            |(node, runtime, source, parent, label, tab_index, disabled)| {
                let parent = parent
                    .map(ChildOf::parent)
                    .filter(|parent| accessible.contains(parent))
                    .and_then(|parent| identities.get(&parent).cloned().flatten())
                    .unwrap_or_else(|| "primary_window".into());
                serde_json::json!({
                    "runtime_id": runtime.map(|id| id.0.as_str()),
                    "source_id": source.map(|id| id.0.as_str()),
                    "parent": parent,
                    "role": format!("{:?}", node.role()),
                    "label": node.label().or_else(|| label.map(|label| label.0.as_str())),
                    "value": node.value(),
                    "disabled": disabled || node.is_disabled(),
                    "tab_index": tab_index.map(|index| index.0),
                    "bounds": node.bounds().map(|bounds| format!("{bounds:?}")),
                })
            },
        )
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        left["runtime_id"]
            .as_str()
            .cmp(&right["runtime_id"].as_str())
            .then(left["source_id"].as_str().cmp(&right["source_id"].as_str()))
    });
    rows
}

fn rounded(value: f32) -> f32 {
    let value = (value * 1000.0).round() / 1000.0;
    if value == 0.0 { 0.0 } else { value }
}

fn parse_size(size: &str) -> Result<(u32, u32), String> {
    let (width, height) = size.split_once('x').ok_or("size must be WxH")?;
    Ok((
        width.parse().map_err(|_| "invalid width")?,
        height.parse().map_err(|_| "invalid height")?,
    ))
}

fn write_json(path: &PathBuf, value: &impl serde::Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}
