use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::window::{MonitorSelection, WindowMode};
use bevy_openpencil::{OpenPencilUiPlugin, OpenPencilUiRoot};
use opui_integration::showcase::{ShowcasePlugin, ShowcaseState, ShowcaseUi};
use opui_integration::showcase_bindings::ShowcaseEntrypoint;

fn main() {
    let watch = std::env::args().any(|argument| argument == "--watch");
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/generated").into(),
                    watch_for_changes_override: Some(watch),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "OpenPencil to Bevy Showcase".into(),
                        resolution: (1280, 720).into(),
                        ..default()
                    }),
                    ..default()
                }),
            OpenPencilUiPlugin,
            ShowcasePlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, apply_window_settings)
        .run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn(Camera2d);
    commands.spawn((
        Node {
            width: percent(100),
            height: percent(100),
            ..default()
        },
        OpenPencilUiRoot::new(
            assets.load("showcase.opui"),
            ShowcaseEntrypoint::App.as_str(),
        ),
        ShowcaseUi,
    ));
}

fn apply_window_settings(state: Res<ShowcaseState>, mut windows: Query<&mut Window>) {
    if !state.is_changed() {
        return;
    }
    if let Ok(mut window) = windows.single_mut() {
        window.mode = if state.applied_fullscreen {
            WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
        } else {
            WindowMode::Windowed
        };
    }
}
