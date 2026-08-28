#![cfg(feature = "visual")]

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use bevy_openpencil::openpencil_ui_schema::parse_and_validate;
use bevy_openpencil::{
    OpenPencilRuntimeIds, OpenPencilSourceId, OpenPencilUi, OpenPencilUiPlugin, OpenPencilUiRoot,
};
use opui_integration::showcase::{ShowcasePlugin, ShowcaseScreen, ShowcaseState, ShowcaseUi};
use opui_integration::showcase_bindings::{ShowcaseEntrypoint, ShowcaseNode};
use sha2::{Digest, Sha256};

#[derive(Component)]
struct ApplicationOwned;

fn setup() -> (App, Handle<OpenPencilUi>, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::input::InputPlugin,
        AssetPlugin::default(),
        OpenPencilUiPlugin,
        ShowcasePlugin,
    ));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("generated/showcase.opui");
    let bytes = std::fs::read(path).unwrap();
    let validated = parse_and_validate(&bytes).unwrap();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<OpenPencilUi>>()
        .add(OpenPencilUi {
            document: validated.document,
            package_sha256: format!("{:x}", Sha256::digest(&bytes)),
            images: default(),
            fonts: default(),
            warnings: validated.warnings,
        });
    let root = app
        .world_mut()
        .spawn((
            Node::default(),
            OpenPencilUiRoot::new(handle.clone(), ShowcaseEntrypoint::App.as_str()),
            ShowcaseUi,
        ))
        .id();
    app.update();
    app.update();
    (app, handle, root)
}

fn runtime_entity(app: &App, root: Entity, node: ShowcaseNode) -> Entity {
    app.world()
        .resource::<OpenPencilRuntimeIds>()
        .get(root, node.as_str())
        .unwrap_or_else(|| panic!("missing {}", node.as_str()))
}

#[test]
fn buttons_drive_application_state_and_dynamic_text() {
    let (mut app, _, root) = setup();
    let play = runtime_entity(&app, root, ShowcaseNode::MainMenuPlay);
    app.world_mut().trigger(Activate { entity: play });
    app.update();
    assert_eq!(
        app.world().resource::<ShowcaseState>().screen,
        ShowcaseScreen::Hud
    );

    {
        let mut state = app.world_mut().resource_mut::<ShowcaseState>();
        state.score = 1440;
        state.king_health = 63;
    }
    app.update();
    let score = runtime_entity(&app, root, ShowcaseNode::HudScore);
    let health = runtime_entity(&app, root, ShowcaseNode::HudHealth);
    assert_eq!(app.world().get::<Text>(score).unwrap().0, "Score: 1440");
    assert_eq!(
        app.world().get::<Text>(health).unwrap().0,
        "King Health: 63"
    );
}

#[test]
fn reload_and_remove_reinsert_preserve_application_state() {
    let (mut app, handle, root) = setup();
    let play = runtime_entity(&app, root, ShowcaseNode::MainMenuPlay);
    app.world_mut().entity_mut(play).insert(ApplicationOwned);
    app.world_mut().resource_mut::<ShowcaseState>().score = 1550;
    let play_source = app
        .world()
        .get::<OpenPencilSourceId>(play)
        .unwrap()
        .0
        .clone();
    let parent_source = {
        let assets = app.world().resource::<Assets<OpenPencilUi>>();
        assets
            .get(&handle)
            .unwrap()
            .document
            .nodes
            .values()
            .find(|node| node.children.contains(&play_source))
            .unwrap()
            .source_id
            .clone()
    };

    {
        let mut assets = app.world_mut().resource_mut::<Assets<OpenPencilUi>>();
        assets
            .get_mut(&handle)
            .unwrap()
            .document
            .nodes
            .get_mut(&parent_source)
            .unwrap()
            .children
            .reverse();
        assets.get_mut(&handle).unwrap().package_sha256 = "reordered".into();
    }
    app.world_mut()
        .write_message(AssetEvent::<OpenPencilUi>::Modified { id: handle.id() });
    app.update();
    app.update();
    let reordered = runtime_entity(&app, root, ShowcaseNode::MainMenuPlay);
    assert_eq!(play, reordered);
    assert!(app.world().get::<ApplicationOwned>(reordered).is_some());
    assert_eq!(app.world().resource::<ShowcaseState>().score, 1550);

    {
        let mut assets = app.world_mut().resource_mut::<Assets<OpenPencilUi>>();
        assets
            .get_mut(&handle)
            .unwrap()
            .document
            .nodes
            .get_mut(&parent_source)
            .unwrap()
            .children
            .retain(|child| child != &play_source);
        assets.get_mut(&handle).unwrap().package_sha256 = "removed".into();
    }
    app.world_mut()
        .write_message(AssetEvent::<OpenPencilUi>::Modified { id: handle.id() });
    app.update();
    assert!(
        app.world()
            .resource::<OpenPencilRuntimeIds>()
            .get(root, ShowcaseNode::MainMenuPlay.as_str())
            .is_none()
    );

    {
        let mut assets = app.world_mut().resource_mut::<Assets<OpenPencilUi>>();
        assets
            .get_mut(&handle)
            .unwrap()
            .document
            .nodes
            .get_mut(&parent_source)
            .unwrap()
            .children
            .push(play_source);
        assets.get_mut(&handle).unwrap().package_sha256 = "reinserted".into();
    }
    app.world_mut()
        .write_message(AssetEvent::<OpenPencilUi>::Modified { id: handle.id() });
    app.update();
    app.update();
    assert!(
        app.world()
            .resource::<OpenPencilRuntimeIds>()
            .get(root, ShowcaseNode::MainMenuPlay.as_str())
            .is_some()
    );
    assert_eq!(app.world().resource::<ShowcaseState>().score, 1550);
}
