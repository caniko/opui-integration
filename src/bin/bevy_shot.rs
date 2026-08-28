use std::path::PathBuf;
use std::time::Duration;

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::asset::{LoadState, RenderAssetUsages};
use bevy::camera::RenderTarget;
use bevy::color::palettes::css::{BLUE, GRAY};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::render::{RenderPlugin, settings::InstanceFlags, settings::WgpuSettings};
use bevy::ui::UiSystems;
use bevy::window::{ExitCondition, PresentMode, WindowRef};
use bevy::winit::WinitPlugin;
use bevy_openpencil::{
    OpenPencilSourceId, OpenPencilUi, OpenPencilUiPlugin, OpenPencilUiReconciled, OpenPencilUiRoot,
};
use opui_integration::image_metrics::{self, POLICY_ID};

fn main() {
    let args = Args::parse();
    let mut app = App::new();
    let mut plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: args.windowed.then(|| Window {
                title: "OPUI graphical certification".into(),
                resolution: (args.w, args.h).into(),
                present_mode: PresentMode::AutoVsync,
                ..default()
            }),
            exit_condition: ExitCondition::DontExit,
            ..default()
        })
        .set(AssetPlugin {
            file_path: args.dir.to_string_lossy().into_owned(),
            watch_for_changes_override: Some(false),
            ..default()
        });
    if !args.windowed {
        plugins = plugins.disable::<WinitPlugin>();
    } else {
        plugins = plugins.set(RenderPlugin {
            render_creation: WgpuSettings {
                instance_flags: InstanceFlags::empty(),
                force_fallback_adapter: true,
                ..default()
            }
            .into(),
            ..default()
        });
    }
    app.insert_resource(ClearColor(Color::NONE))
        .insert_resource(args.clone())
        .insert_resource(Shot::new(args.scene))
        .add_plugins(plugins);
    if !args.windowed {
        app.add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(16)));
    }
    if args.scene == Scene::Opui {
        app.add_plugins(OpenPencilUiPlugin);
    } else {
        app.add_message::<OpenPencilUiReconciled>()
            .add_message::<AssetEvent<OpenPencilUi>>();
    }
    app.add_systems(Startup, setup)
        .add_systems(PostUpdate, tick.after(UiSystems::PostLayout))
        .run();
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scene {
    Control,
    Opui,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Loading,
    Spawned,
    Reconciled,
    LayoutPending,
    LayoutStable,
    StressReloading,
    StressUnmounted,
    RenderWait,
    CaptureRequested,
    Captured,
    Compared,
    Failed,
}

#[derive(Clone, Resource)]
struct Args {
    scene: Scene,
    dir: PathBuf,
    package: String,
    entrypoint: String,
    out: PathBuf,
    reference: Option<PathBuf>,
    w: u32,
    h: u32,
    debug: bool,
    windowed: bool,
    stress_cycles: u32,
}

impl Args {
    fn parse() -> Self {
        let mut scene = Scene::Opui;
        let mut debug = false;
        let mut windowed = false;
        let mut stress_cycles = 0;
        let mut package = "runtime-ui.opui".into();
        let mut entrypoint = "default".into();
        let mut reference = None;
        let mut rest = Vec::new();
        let mut it = std::env::args().skip(1);
        while let Some(a) = it.next() {
            match a.as_str() {
                "--scene" => {
                    scene = match it.next().expect("scene").as_str() {
                        "control" => Scene::Control,
                        "opui" => Scene::Opui,
                        other => panic!("unknown scene {other}"),
                    };
                }
                "--package" => package = it.next().expect("package"),
                "--entrypoint" => entrypoint = it.next().expect("entrypoint"),
                "--reference" => {
                    reference = Some(PathBuf::from(it.next().expect("reference path")));
                }
                "--debug" => debug = true,
                "--windowed" => windowed = true,
                "--stress-cycles" => {
                    stress_cycles = it
                        .next()
                        .expect("stress cycle count")
                        .parse()
                        .expect("stress cycle count must be an integer");
                }
                _ => rest.push(a),
            }
        }
        let dir = PathBuf::from(rest.first().expect("dir"))
            .canonicalize()
            .expect("dir");
        let spec = rest.get(1).expect("WxH");
        let (w, h) = spec.split_once('x').expect("WxH");
        let w: u32 = w.parse().unwrap();
        let h: u32 = h.parse().unwrap();
        let out = dir.join(match scene {
            Scene::Control => "control.png",
            Scene::Opui => "capture.png",
        });
        Self {
            scene,
            dir,
            package,
            entrypoint,
            out,
            reference,
            w,
            h,
            debug,
            windowed,
            stress_cycles,
        }
    }
}

#[derive(Resource)]
struct Shot {
    phase: Phase,
    frames: u32,
    last_layout: String,
    stable: u32,
    render_wait: u32,
    captured: Option<Image>,
    mount: Option<Entity>,
    stress_completed: u32,
}

impl Shot {
    fn new(scene: Scene) -> Self {
        Self {
            phase: match scene {
                Scene::Control => Phase::Spawned,
                Scene::Opui => Phase::Loading,
            },
            frames: 0,
            last_layout: String::new(),
            stable: 0,
            render_wait: 0,
            captured: None,
            mount: None,
            stress_completed: 0,
        }
    }
}

#[derive(Resource)]
struct UiHandle(Handle<OpenPencilUi>);
#[derive(Resource)]
enum Target {
    Image(Handle<Image>),
    Window,
}
#[derive(Resource)]
struct Cam(Entity);

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    args: Res<Args>,
) {
    let render_target = if args.windowed {
        commands.insert_resource(Target::Window);
        RenderTarget::Window(WindowRef::Primary)
    } else {
        let mut image = Image::new_fill(
            Extent3d {
                width: args.w,
                height: args.h,
                ..default()
            },
            TextureDimension::D2,
            &[0, 0, 0, 0],
            TextureFormat::Bgra8UnormSrgb,
            RenderAssetUsages::default(),
        );
        image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_DST
            | TextureUsages::COPY_SRC
            | TextureUsages::RENDER_ATTACHMENT;
        let target = images.add(image);
        commands.insert_resource(Target::Image(target.clone()));
        RenderTarget::Image(target.into())
    };
    let cam = commands
        .spawn((
            Camera2d,
            Camera {
                order: -1,
                ..default()
            },
            IsDefaultUiCamera,
            Visibility::Visible,
            render_target,
        ))
        .id();
    commands.insert_resource(Cam(cam));
    if args.scene == Scene::Opui {
        commands.insert_resource(UiHandle(assets.load(args.package.clone())));
    }
}

fn spawn_control(commands: &mut Commands, cam: Entity) {
    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(GRAY.into()),
            UiTargetCamera(cam),
            Visibility::Visible,
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    width: px(160),
                    height: px(80),
                    ..default()
                },
                BackgroundColor(BLUE.into()),
            ));
        });
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn tick(
    mut commands: Commands,
    assets: Res<AssetServer>,
    handle: Option<Res<UiHandle>>,
    target: Res<Target>,
    cam: Res<Cam>,
    args: Res<Args>,
    mut shot: ResMut<Shot>,
    mut exit: MessageWriter<AppExit>,
    mut reconciled: MessageReader<OpenPencilUiReconciled>,
    mut asset_events: MessageWriter<AssetEvent<OpenPencilUi>>,
    mut ui_assets: Option<ResMut<Assets<OpenPencilUi>>>,
    mut windows: Query<&mut Window>,
    computed: Query<(
        Entity,
        &ComputedNode,
        &Node,
        Option<&OpenPencilSourceId>,
        Option<&UiGlobalTransform>,
        Option<&InheritedVisibility>,
        Option<&UiTargetCamera>,
    )>,
    children: Query<&Children>,
    roots: Query<Entity, With<OpenPencilUiRoot>>,
) {
    shot.frames += 1;
    if shot.frames > 600 {
        let _ = std::fs::write(args.dir.join("computed.json"), &shot.last_layout);
        fail(&mut shot, &mut exit, "timeout");
        return;
    }
    match shot.phase {
        Phase::Loading => match handle.as_ref().and_then(|h| assets.get_load_state(&h.0)) {
            Some(LoadState::Loaded) => {
                let mount = commands
                    .spawn((
                        Node {
                            width: Val::Px(args.w as f32),
                            height: Val::Px(args.h as f32),
                            ..default()
                        },
                        UiTargetCamera(cam.0),
                        Visibility::Visible,
                        OpenPencilUiRoot::new(handle.unwrap().0.clone(), args.entrypoint.clone()),
                    ))
                    .id();
                shot.mount = Some(mount);
                shot.phase = Phase::Spawned;
            }
            Some(LoadState::Failed(_)) => fail(&mut shot, &mut exit, "load failed"),
            _ => {}
        },
        Phase::Spawned => {
            if args.scene == Scene::Control {
                spawn_control(&mut commands, cam.0);
                shot.phase = Phase::LayoutPending;
            } else if reconciled.read().len() > 0 {
                if let (Some(h), Some(assets)) = (handle.as_ref(), ui_assets.as_ref())
                    && let Some(ui) = assets.get(&h.0)
                {
                    let _ = std::fs::write(
                        args.dir.join("mapping.json"),
                        serde_json::to_string_pretty(&ui.document).unwrap(),
                    );
                }
                shot.phase = Phase::Reconciled;
            }
        }
        Phase::Reconciled => shot.phase = Phase::LayoutPending,
        Phase::LayoutPending => {
            let snap = computed_snapshot(&computed, &children, &roots);
            if layout_ready(&snap, args.scene == Scene::Opui) && snap == shot.last_layout {
                shot.stable += 1;
            } else {
                shot.stable = 0;
                shot.last_layout = snap;
            }
            if shot.stable >= 2 {
                let _ = std::fs::write(args.dir.join("computed.json"), &shot.last_layout);
                shot.phase = Phase::LayoutStable;
            }
        }
        Phase::LayoutStable => {
            if shot.stress_completed < args.stress_cycles {
                let Some(handle) = handle.as_ref() else {
                    fail(&mut shot, &mut exit, "stress run lost asset handle");
                    return;
                };
                let Some(mut asset) = ui_assets
                    .as_deref_mut()
                    .and_then(|assets| assets.get_mut(&handle.0))
                else {
                    fail(&mut shot, &mut exit, "stress run lost loaded asset");
                    return;
                };
                let Some(node) = asset
                    .document
                    .nodes
                    .values_mut()
                    .find(|node| node.runtime_id.is_some())
                else {
                    fail(&mut shot, &mut exit, "stress package has no runtime node");
                    return;
                };
                node.visible = !node.visible;
                asset.package_sha256 = format!("stress-{}", shot.stress_completed + 1);
                asset_events.write(AssetEvent::Modified { id: handle.0.id() });
                shot.phase = Phase::StressReloading;
                return;
            }
            shot.render_wait = 0;
            shot.phase = Phase::RenderWait;
        }
        Phase::StressReloading => {
            let Some(mount) = shot.mount else {
                fail(&mut shot, &mut exit, "stress run lost mount");
                return;
            };
            if reconciled.read().any(|event| event.root == mount) {
                commands.entity(mount).remove::<OpenPencilUiRoot>();
                if let Ok(mut window) = windows.single_mut() {
                    let offset = if shot.stress_completed.is_multiple_of(2) {
                        64
                    } else {
                        0
                    };
                    window
                        .resolution
                        .set((args.w + offset) as f32, (args.h + offset / 2) as f32);
                }
                shot.phase = Phase::StressUnmounted;
            }
        }
        Phase::StressUnmounted => {
            let Some(mount) = shot.mount else {
                fail(&mut shot, &mut exit, "stress run lost mount");
                return;
            };
            let Some(handle) = handle.as_ref() else {
                fail(&mut shot, &mut exit, "stress run lost asset handle");
                return;
            };
            commands.entity(mount).insert(OpenPencilUiRoot::new(
                handle.0.clone(),
                args.entrypoint.clone(),
            ));
            shot.stress_completed += 1;
            shot.stable = 0;
            shot.phase = Phase::Spawned;
        }
        Phase::RenderWait => {
            shot.render_wait += 1;
            if shot.render_wait >= 16 {
                let mut screenshot = match &*target {
                    Target::Image(target) => commands.spawn(Screenshot::image(target.clone())),
                    Target::Window => commands.spawn(Screenshot::primary_window()),
                };
                screenshot.observe(on_captured);
                shot.phase = Phase::CaptureRequested;
            }
        }
        Phase::CaptureRequested => {}
        Phase::Captured => finish(&args, &mut shot, &mut exit),
        Phase::Compared | Phase::Failed => {}
    }
    if args.debug {
        eprintln!("phase={:?} frames={}", shot.phase, shot.frames);
    }
}

fn on_captured(cap: On<ScreenshotCaptured>, mut shot: ResMut<Shot>) {
    shot.captured = Some(cap.image.clone());
    shot.phase = Phase::Captured;
}

fn layout_ready(snap: &str, require_source: bool) -> bool {
    let Ok(rows) = serde_json::from_str::<Vec<serde_json::Value>>(snap) else {
        return false;
    };
    let mut any = false;
    for r in &rows {
        let w = r["w"].as_f64().unwrap_or(0.0);
        let h = r["h"].as_f64().unwrap_or(0.0);
        let has_source = r["source_id"].as_str().is_some();
        if (!require_source || has_source) && w > 1.0 && h > 1.0 {
            any = true;
        }
        if require_source && has_source {
            let style_h = r["style_h"].as_str().unwrap_or("");
            if style_h.contains("Auto") && h < 1.0 {
                return false;
            }
        }
    }
    any
}

#[allow(clippy::type_complexity)]
fn computed_snapshot(
    computed: &Query<(
        Entity,
        &ComputedNode,
        &Node,
        Option<&OpenPencilSourceId>,
        Option<&UiGlobalTransform>,
        Option<&InheritedVisibility>,
        Option<&UiTargetCamera>,
    )>,
    children: &Query<&Children>,
    roots: &Query<Entity, With<OpenPencilUiRoot>>,
) -> String {
    let mut rows: Vec<serde_json::Value> = computed
        .iter()
        .map(|(e, n, style, id, xf, vis, cam)| {
            let kids = children
                .get(e)
                .map(|c| c.iter().map(|k| k.to_bits()).collect::<Vec<_>>())
                .unwrap_or_default();
            serde_json::json!({
                "entity": e.to_bits(),
                "source_id": id.map(|s| s.0.clone()),
                "style_w": format!("{:?}", style.width),
                "style_h": format!("{:?}", style.height),
                "w": n.size.x,
                "h": n.size.y,
                "content_w": n.content_size.x,
                "content_h": n.content_size.y,
                "tx": xf.map(|t| t.translation.x),
                "ty": xf.map(|t| t.translation.y),
                "visible": vis.map(|v| v.get()),
                "camera": cam.map(|c| c.0.to_bits()),
                "children": kids,
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        let sa = a["source_id"].as_str().unwrap_or("");
        let sb = b["source_id"].as_str().unwrap_or("");
        sa.cmp(sb)
            .then(a["entity"].as_u64().cmp(&b["entity"].as_u64()))
    });
    let _ = roots;
    serde_json::to_string_pretty(&rows).unwrap()
}

fn finish(args: &Args, shot: &mut Shot, exit: &mut MessageWriter<AppExit>) {
    let Some(image) = shot.captured.take() else {
        fail(shot, exit, "captured without image");
        return;
    };
    let rgba = match image.try_into_dynamic() {
        Ok(d) => d.to_rgba8(),
        Err(e) => {
            fail(shot, exit, &format!("image convert: {e}"));
            return;
        }
    };
    if let Err(e) = rgba.save(&args.out) {
        fail(shot, exit, &format!("save: {e}"));
        return;
    }
    if args.stress_cycles > 0 {
        let _ = std::fs::write(
            args.dir.join("stress.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "requested_cycles": args.stress_cycles,
                "completed_cycles": shot.stress_completed,
                "reload": true,
                "resize": true,
                "unmount_remount": true,
                "status": if shot.stress_completed == args.stress_cycles { "pass" } else { "fail" },
            }))
            .unwrap(),
        );
    }
    if args.scene == Scene::Control {
        match image_metrics::assert_control(&rgba) {
            Ok(()) => {
                println!("CONTROL ok {}x{} policy={POLICY_ID}", args.w, args.h);
                shot.phase = Phase::Compared;
                exit.write(AppExit::Success);
            }
            Err(e) => fail(shot, exit, &e),
        }
        return;
    }
    match image_metrics::reject_corrupt(&rgba, args.w, args.h) {
        Ok(s) => println!(
            "OPUI capture {}x{} buckets={} clear={:.3} windowed={}",
            s.w, s.h, s.unique_buckets, s.clear_ratio, args.windowed
        ),
        Err(e) => {
            fail(shot, exit, &e);
            return;
        }
    }
    let Some(reference) = args.reference.as_ref() else {
        shot.phase = Phase::Compared;
        exit.write(AppExit::Success);
        return;
    };
    if !reference.is_file() {
        fail(
            shot,
            exit,
            &format!("reference missing: {}", reference.display()),
        );
        return;
    }
    let a = match image_metrics::load_rgba(reference) {
        Ok(a) => a,
        Err(e) => {
            fail(shot, exit, &e);
            return;
        }
    };
    match image_metrics::diff(&a, &rgba) {
        Ok(d) => {
            let heat = image_metrics::heatmap(&a, &rgba);
            let _ = heat.save(args.dir.join("heatmap.png"));
            let tiles = image_metrics::tile_mae(&a, &rgba, 64);
            let _ = std::fs::write(
                args.dir.join("regions.json"),
                serde_json::to_string_pretty(&tiles).unwrap(),
            );
            let msg = format!(
                "mae={:.2} rmse={:.2} max={} exact={:.4} thresh={:.4} policy={POLICY_ID}",
                d.mae, d.rmse, d.max, d.exact_diff_ratio, d.thresh_diff_ratio
            );
            println!("{msg}");
            if !image_metrics::passes_visual(&d) {
                fail(shot, exit, &msg);
                return;
            }
        }
        Err(e) => {
            fail(shot, exit, &e);
            return;
        }
    }
    shot.phase = Phase::Compared;
    exit.write(AppExit::Success);
}

fn fail(shot: &mut Shot, exit: &mut MessageWriter<AppExit>, msg: &str) {
    eprintln!("FAIL {:?}: {msg}", shot.phase);
    shot.phase = Phase::Failed;
    exit.write(AppExit::error());
    std::process::exit(1);
}
