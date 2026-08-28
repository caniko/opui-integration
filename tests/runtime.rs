use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy_openpencil::openpencil_ui_schema::{
    Color, ColorSpace, Fill, FlexDirection as OpFlex, Length, parse_and_validate,
};
use bevy_openpencil::{OpenPencilRuntimeIds, OpenPencilUi, OpenPencilUiPlugin, OpenPencilUiRoot};
use opui_integration::REQUIRED_RUNTIME_IDS;
use sha2::{Digest, Sha256};

fn generated() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("generated/runtime-ui.opui")
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), OpenPencilUiPlugin));
    app
}

fn insert_exported(app: &mut App) -> Handle<OpenPencilUi> {
    let bytes = std::fs::read(generated()).expect("run `just export` first");
    let validated = parse_and_validate(&bytes).unwrap();
    let asset = OpenPencilUi {
        document: validated.document,
        package_sha256: format!("{:x}", Sha256::digest(&bytes)),
        images: default(),
        fonts: default(),
        warnings: validated.warnings,
    };
    app.world_mut()
        .resource_mut::<Assets<OpenPencilUi>>()
        .add(asset)
}

fn spawn_root(app: &mut App, handle: Handle<OpenPencilUi>) -> Entity {
    app.world_mut()
        .spawn((Node::default(), OpenPencilUiRoot::new(handle, "default")))
        .id()
}

fn live_entities(app: &mut App) -> usize {
    let world = app.world_mut();
    let mut query = world.query::<Entity>();
    query.iter(world).count()
}

#[test]
fn required_ids_spawn() {
    let mut app = app();
    let handle = insert_exported(&mut app);
    let root = spawn_root(&mut app, handle);
    app.update();
    let registry = app.world().resource::<OpenPencilRuntimeIds>();
    for id in REQUIRED_RUNTIME_IDS {
        assert!(registry.get(root, id).is_some(), "missing {id}");
    }
}

#[test]
fn maps_exported_layout() {
    let mut app = app();
    let handle = insert_exported(&mut app);
    spawn_root(&mut app, handle);
    app.update();
    let world = app.world_mut();
    let mut q = world.query::<&Node>();
    assert!(
        q.iter(world)
            .any(|n| n.flex_direction == FlexDirection::Column)
    );
    assert!(q.iter(world).any(|n| n.width == Val::Percent(50.)));
    assert!(
        q.iter(world)
            .any(|n| n.position_type == PositionType::Absolute)
    );
}

#[test]
fn nine_mutations_keep_root_and_user_component() {
    let mut app = app();
    let handle = insert_exported(&mut app);
    let root = spawn_root(&mut app, handle.clone());
    app.update();
    let play = app
        .world()
        .resource::<OpenPencilRuntimeIds>()
        .get(root, "main_menu.play")
        .unwrap();
    app.world_mut().entity_mut(play).insert(Name::new("user"));

    let mutates: [fn(&mut OpenPencilUi); 9] = [
        |ui| {
            ui.document
                .nodes
                .get_mut("main_menu.title")
                .unwrap()
                .text
                .as_mut()
                .unwrap()
                .content = "MUTATED".into();
        },
        |ui| {
            ui.document.nodes.get_mut("hud_badge").unwrap().style.fill = Some(Fill::Solid {
                color: Color {
                    space: ColorSpace::Srgb,
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            });
        },
        |ui| {
            ui.document.nodes.get_mut("glass").unwrap().layout.width = Some(Length::px(320.0));
        },
        |ui| {
            ui.document
                .nodes
                .get_mut("inventory.grid")
                .unwrap()
                .children
                .push("slot_a".into());
        },
        |ui| {
            ui.document
                .nodes
                .get_mut("inventory.grid")
                .unwrap()
                .children
                .retain(|c| c != "slot_d");
        },
        |ui| {
            let kids = &mut ui.document.nodes.get_mut("actions").unwrap().children;
            kids.reverse();
        },
        |ui| {
            ui.document.nodes.get_mut("banner").unwrap().visible = false;
        },
        |ui| {
            ui.document.nodes.get_mut("glass").unwrap().style.opacity = Some(0.4);
        },
        |ui| {
            ui.document
                .nodes
                .get_mut("inventory.grid")
                .unwrap()
                .layout
                .flex_direction = Some(OpFlex::Column);
        },
    ];

    for (generation, mutate) in mutates.into_iter().enumerate() {
        {
            let mut assets = app.world_mut().resource_mut::<Assets<OpenPencilUi>>();
            let mut asset = assets.get_mut(&handle).unwrap();
            mutate(&mut asset);
            asset.package_sha256 = format!("mutation-{generation}");
        }
        let id = handle.id();
        app.world_mut()
            .write_message(AssetEvent::<OpenPencilUi>::Modified { id });
        app.update();
        assert!(app.world().get_entity(root).is_ok(), "root despawned");
        let play2 = app
            .world()
            .resource::<OpenPencilRuntimeIds>()
            .get(root, "main_menu.play")
            .expect("play missing after mutation");
        assert_eq!(play, play2);
        assert_eq!(
            app.world().entity(play2).get::<Name>().map(|n| n.as_str()),
            Some("user")
        );
    }
}

#[test]
fn reconciliation_stress_keeps_identity_and_entity_count() {
    let mut app = app();
    let handle = insert_exported(&mut app);
    let root = spawn_root(&mut app, handle.clone());
    app.update();
    app.update();
    let play = app
        .world()
        .resource::<OpenPencilRuntimeIds>()
        .get(root, "main_menu.play")
        .unwrap();
    let initial_entities = live_entities(&mut app);
    let started = Instant::now();

    for generation in 0..1_000 {
        {
            let mut assets = app.world_mut().resource_mut::<Assets<OpenPencilUi>>();
            let mut asset = assets.get_mut(&handle).unwrap();
            asset
                .document
                .nodes
                .get_mut("main_menu.title")
                .unwrap()
                .text
                .as_mut()
                .unwrap()
                .content = format!("Generation {generation}");
            asset.package_sha256 = format!("stress-{generation}");
        }
        app.world_mut()
            .write_message(AssetEvent::<OpenPencilUi>::Modified { id: handle.id() });
        app.update();
        assert_eq!(
            app.world()
                .resource::<OpenPencilRuntimeIds>()
                .get(root, "main_menu.play"),
            Some(play)
        );
        assert_eq!(live_entities(&mut app), initial_entities);
    }

    let elapsed = started.elapsed();
    eprintln!(
        "reconciliation_stress iterations=1000 elapsed_ms={} entities={initial_entities}",
        elapsed.as_millis()
    );
    assert!(
        elapsed < Duration::from_secs(30),
        "stress run took {elapsed:?}"
    );
}

#[test]
fn invalid_opui_is_refused() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../opui/fixtures/invalid");
    for name in [
        "cyclic-hierarchy.opui",
        "duplicate-runtime-ids.opui",
        "missing-references.opui",
        "unsupported-schema-version.opui",
    ] {
        let bytes = std::fs::read(dir.join(name)).unwrap();
        assert!(
            parse_and_validate(&bytes).is_err(),
            "{name} should be refused"
        );
    }
}
