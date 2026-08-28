#![cfg(feature = "visual")]

use sha2::{Digest, Sha256};
use std::path::PathBuf;

use bevy::camera::NormalizedRenderTarget;
use bevy::input::ButtonState;
use bevy::input::gamepad::{
    GamepadButton, GamepadConnection, GamepadConnectionEvent, RawGamepadButtonChangedEvent,
    RawGamepadEvent,
};
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{InputDispatchPlugin, InputFocus, InputFocusPlugin};
use bevy::picking::backend::{HitData, PointerHits};
use bevy::picking::pointer::{
    Location, PointerAction, PointerButton, PointerId, PointerInput, PointerLocation,
};
use bevy::picking::{InteractionPlugin, PickingPlugin};
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::{Activate, ButtonPlugin};
use bevy::window::{PrimaryWindow, WindowRef};
use bevy_openpencil::openpencil_ui_schema::parse_and_validate;
use bevy_openpencil::{
    OpenPencilRuntimeId, OpenPencilRuntimeIds, OpenPencilUi, OpenPencilUiPlugin, OpenPencilUiRoot,
};
use opui_integration::showcase::{ShowcasePlugin, ShowcaseScreen, ShowcaseState, ShowcaseUi};
use opui_integration::showcase_bindings::ShowcaseEntrypoint;

#[derive(Resource, Default)]
struct Activations(Vec<String>);

fn record_activation(
    event: On<Activate>,
    ids: Query<&OpenPencilRuntimeId>,
    mut activations: ResMut<Activations>,
) {
    if let Ok(id) = ids.get(event.entity) {
        activations.0.push(id.0.clone());
    }
}

struct Harness {
    app: App,
    root: Entity,
    window: Entity,
    camera: Entity,
    gamepad: Option<Entity>,
}

impl Harness {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            AssetPlugin::default(),
            InputFocusPlugin,
            InputDispatchPlugin,
            PickingPlugin,
            InteractionPlugin,
            ButtonPlugin,
            OpenPencilUiPlugin,
            ShowcasePlugin,
        ))
        .init_resource::<Activations>()
        .add_observer(record_activation);

        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let camera = app.world_mut().spawn_empty().id();
        let location = Location {
            target: NormalizedRenderTarget::Window(
                WindowRef::Entity(window).normalize(Some(window)).unwrap(),
            ),
            position: Vec2::new(20.0, 20.0),
        };
        app.world_mut()
            .spawn((PointerId::Mouse, PointerLocation::new(location)));

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
                OpenPencilUiRoot::new(handle, ShowcaseEntrypoint::App.as_str()),
                ShowcaseUi,
            ))
            .id();
        app.update();
        app.update();
        Self {
            app,
            root,
            window,
            camera,
            gamepad: None,
        }
    }

    fn entity(&self, runtime_id: &str) -> Entity {
        self.app
            .world()
            .resource::<OpenPencilRuntimeIds>()
            .get(self.root, runtime_id)
            .unwrap_or_else(|| panic!("missing {runtime_id}"))
    }

    fn focus(&mut self, runtime_id: &str) {
        let entity = self.entity(runtime_id);
        self.app
            .world_mut()
            .resource_mut::<InputFocus>()
            .set(entity, bevy::input_focus::FocusCause::Navigated);
        self.app.update();
    }

    fn clear_focus(&mut self) {
        self.app.world_mut().resource_mut::<InputFocus>().clear();
        self.app.update();
    }

    fn screen(&mut self, screen: ShowcaseScreen) {
        self.app.world_mut().resource_mut::<ShowcaseState>().screen = screen;
        self.app.update();
    }

    fn pointer(
        &mut self,
        pointer: PointerId,
        target: Option<Entity>,
        position: Vec2,
        action: PointerAction,
    ) {
        if self
            .app
            .world()
            .resource::<bevy::picking::pointer::PointerMap>()
            .get_entity(pointer)
            .is_none()
        {
            self.app.world_mut().spawn((
                pointer,
                PointerLocation::new(Location {
                    target: NormalizedRenderTarget::Window(
                        WindowRef::Entity(self.window)
                            .normalize(Some(self.window))
                            .unwrap(),
                    ),
                    position,
                }),
            ));
        }
        let location = Location {
            target: NormalizedRenderTarget::Window(
                WindowRef::Entity(self.window)
                    .normalize(Some(self.window))
                    .unwrap(),
            ),
            position,
        };
        self.app.world_mut().write_message(PointerHits::new(
            pointer,
            target
                .map(|entity| {
                    vec![(
                        entity,
                        HitData::new(self.camera, 0.0, Some(position.extend(0.0)), None),
                    )]
                })
                .unwrap_or_default(),
            0.0,
        ));
        self.app
            .world_mut()
            .write_message(PointerInput::new(pointer, location, action));
        self.app.update();
    }

    fn key(&mut self, key_code: KeyCode, state: ButtonState, repeat: bool) {
        self.app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat,
            window: self.window,
        });
        self.app.update();
    }

    fn connect_gamepad(&mut self) -> Entity {
        let gamepad = self
            .gamepad
            .unwrap_or_else(|| self.app.world_mut().spawn_empty().id());
        self.app
            .world_mut()
            .write_message(GamepadConnectionEvent::new(
                gamepad,
                GamepadConnection::Connected {
                    name: "synthetic test gamepad".into(),
                    vendor_id: None,
                    product_id: None,
                },
            ));
        self.app.update();
        self.gamepad = Some(gamepad);
        gamepad
    }

    fn disconnect_gamepad(&mut self) {
        let gamepad = self.gamepad.unwrap();
        self.app
            .world_mut()
            .write_message(GamepadConnectionEvent::new(
                gamepad,
                GamepadConnection::Disconnected,
            ));
        self.app.update();
    }

    fn gamepad_buttons(&mut self, values: &[(GamepadButton, f32)]) {
        let gamepad = self.gamepad.unwrap();
        for (button, value) in values {
            self.app.world_mut().write_message(RawGamepadEvent::Button(
                RawGamepadButtonChangedEvent::new(gamepad, *button, *value),
            ));
        }
        self.app.update();
    }

    fn tap_gamepad(&mut self, button: GamepadButton) {
        self.gamepad_buttons(&[(button, 1.0)]);
        self.gamepad_buttons(&[(button, 0.0)]);
    }

    fn activations(&self) -> &[String] {
        &self.app.world().resource::<Activations>().0
    }

    fn visible(&self, runtime_id: &str) -> bool {
        self.app.world().get::<Visibility>(self.entity(runtime_id)) == Some(&Visibility::Visible)
    }

    fn visible_button_states(&self, runtime_id: &str) -> usize {
        ["default", "hover", "pressed", "disabled", "focused"]
            .into_iter()
            .filter(|state| self.visible(&format!("{runtime_id}.{state}")))
            .count()
    }
}

#[test]
fn pointer_and_touch_follow_native_button_semantics() {
    let mut h = Harness::new();
    h.screen(ShowcaseScreen::Settings);
    h.clear_focus();
    let button = h.entity("settings.music_up");

    h.pointer(
        PointerId::Mouse,
        Some(button),
        Vec2::new(20.0, 20.0),
        PointerAction::Move {
            delta: Vec2::new(1.0, 0.0),
        },
    );
    assert!(
        h.app
            .world()
            .resource::<bevy::picking::hover::HoverMap>()
            .get(&PointerId::Mouse)
            .is_some_and(|hits| hits.contains_key(&button)),
        "synthetic backend hit was not retained"
    );
    assert!(
        h.app
            .world()
            .get::<bevy::picking::hover::Hovered>(button)
            .is_some_and(|hovered| hovered.0)
    );
    assert!(h.visible("settings.music_up.hover"));
    assert_eq!(h.visible_button_states("settings.music_up"), 1);
    h.pointer(
        PointerId::Mouse,
        None,
        Vec2::new(30.0, 20.0),
        PointerAction::Move {
            delta: Vec2::new(10.0, 0.0),
        },
    );
    assert_eq!(
        h.app.world().get::<bevy::picking::hover::Hovered>(button),
        Some(&bevy::picking::hover::Hovered(false))
    );
    assert!(h.visible("settings.music_up.default"));
    h.pointer(
        PointerId::Mouse,
        Some(button),
        Vec2::new(20.0, 20.0),
        PointerAction::Move {
            delta: Vec2::new(-10.0, 0.0),
        },
    );
    h.pointer(
        PointerId::Mouse,
        Some(button),
        Vec2::new(20.0, 20.0),
        PointerAction::Press(PointerButton::Primary),
    );
    assert!(h.app.world().get::<Pressed>(button).is_some());
    assert!(h.visible("settings.music_up.pressed"));
    assert_eq!(h.visible_button_states("settings.music_up"), 1);
    assert!(h.activations().is_empty());
    h.pointer(
        PointerId::Mouse,
        Some(button),
        Vec2::new(20.0, 20.0),
        PointerAction::Release(PointerButton::Primary),
    );
    assert!(h.app.world().get::<Pressed>(button).is_none());
    assert_eq!(h.activations(), &["settings.music_up"]);
    assert!(h.visible("settings.music_up.focused"));
    assert_eq!(h.visible_button_states("settings.music_up"), 1);

    h.pointer(
        PointerId::Mouse,
        Some(button),
        Vec2::new(20.0, 20.0),
        PointerAction::Press(PointerButton::Primary),
    );
    h.pointer(
        PointerId::Mouse,
        None,
        Vec2::new(40.0, 20.0),
        PointerAction::Move {
            delta: Vec2::new(20.0, 0.0),
        },
    );
    h.pointer(
        PointerId::Mouse,
        None,
        Vec2::new(40.0, 20.0),
        PointerAction::Release(PointerButton::Primary),
    );
    assert!(h.app.world().get::<Pressed>(button).is_none());
    assert_eq!(h.activations().len(), 1);

    let touch = PointerId::Touch(7);
    h.pointer(
        touch,
        Some(button),
        Vec2::new(20.0, 20.0),
        PointerAction::Press(PointerButton::Primary),
    );
    h.pointer(
        touch,
        Some(button),
        Vec2::new(20.0, 20.0),
        PointerAction::Release(PointerButton::Primary),
    );
    assert_eq!(h.activations().len(), 2);

    h.screen(ShowcaseScreen::MainMenu);
    let hidden = h.entity("settings.music_up");
    h.pointer(
        PointerId::Mouse,
        Some(hidden),
        Vec2::new(20.0, 20.0),
        PointerAction::Press(PointerButton::Primary),
    );
    h.pointer(
        PointerId::Mouse,
        Some(hidden),
        Vec2::new(20.0, 20.0),
        PointerAction::Release(PointerButton::Primary),
    );
    assert_eq!(
        h.activations().len(),
        2,
        "disabled controls must not activate"
    );
    assert!(h.app.world().get::<Pressed>(hidden).is_none());
    assert!(h.visible("settings.music_up.disabled"));
    assert_eq!(h.visible_button_states("settings.music_up"), 1);
}

#[test]
fn touch_cancel_clears_native_pressed_state() {
    let mut h = Harness::new();
    h.screen(ShowcaseScreen::Settings);
    let button = h.entity("settings.music_up");
    let touch = PointerId::Touch(7);
    h.pointer(
        touch,
        Some(button),
        Vec2::new(20.0, 20.0),
        PointerAction::Press(PointerButton::Primary),
    );
    assert!(h.app.world().get::<Pressed>(button).is_some());
    h.pointer(
        touch,
        Some(button),
        Vec2::new(20.0, 20.0),
        PointerAction::Cancel,
    );
    assert!(h.app.world().get::<Pressed>(button).is_none());
    assert!(h.activations().is_empty());
}

#[test]
fn focused_keyboard_dispatches_navigation_activation_and_back() {
    let mut h = Harness::new();
    let play = h.entity("main_menu.play");
    assert_eq!(h.app.world().resource::<InputFocus>().get(), Some(play));

    h.key(KeyCode::Tab, ButtonState::Pressed, false);
    assert_eq!(
        h.app.world().resource::<InputFocus>().get(),
        Some(h.entity("main_menu.settings"))
    );
    h.key(KeyCode::Tab, ButtonState::Released, false);
    h.key(KeyCode::ShiftLeft, ButtonState::Pressed, false);
    h.key(KeyCode::Tab, ButtonState::Pressed, false);
    assert_eq!(h.app.world().resource::<InputFocus>().get(), Some(play));
    h.key(KeyCode::Tab, ButtonState::Released, false);
    h.key(KeyCode::ShiftLeft, ButtonState::Released, false);

    let settings = h.entity("main_menu.settings");
    let quit = h.entity("main_menu.quit");
    h.app.world_mut().entity_mut(play).insert(TabIndex(0));
    h.app.world_mut().entity_mut(settings).insert(TabIndex(0));
    h.app.world_mut().entity_mut(quit).insert(TabIndex(0));
    h.focus("main_menu.play");
    h.key(KeyCode::Tab, ButtonState::Pressed, false);
    assert_eq!(
        h.app.world().resource::<InputFocus>().get(),
        Some(settings),
        "equal tab indexes use hierarchy order"
    );
    h.key(KeyCode::Tab, ButtonState::Released, false);

    h.screen(ShowcaseScreen::Settings);
    h.focus("settings.music_up");
    h.key(KeyCode::Enter, ButtonState::Pressed, false);
    assert_eq!(h.activations(), &["settings.music_up"]);
    h.key(KeyCode::Enter, ButtonState::Pressed, true);
    h.key(KeyCode::Enter, ButtonState::Released, false);
    assert_eq!(h.activations().len(), 1);
    h.key(KeyCode::Space, ButtonState::Pressed, false);
    h.key(KeyCode::Space, ButtonState::Released, false);
    assert_eq!(h.activations().len(), 2);
    h.key(KeyCode::Escape, ButtonState::Pressed, false);
    assert_eq!(
        h.app.world().resource::<ShowcaseState>().screen,
        ShowcaseScreen::MainMenu
    );

    h.focus("settings.music_up");
    h.key(KeyCode::Enter, ButtonState::Pressed, false);
    assert_eq!(
        h.activations().len(),
        2,
        "hidden controls must not activate"
    );
}

#[test]
fn gamepad_uses_processed_state_and_rejects_invalid_focus() {
    let mut h = Harness::new();
    let gamepad = h.connect_gamepad();
    h.tap_gamepad(GamepadButton::DPadDown);
    assert_eq!(
        h.app.world().resource::<InputFocus>().get(),
        Some(h.entity("main_menu.settings"))
    );
    h.tap_gamepad(GamepadButton::DPadRight);
    assert_eq!(
        h.app.world().resource::<InputFocus>().get(),
        Some(h.entity("main_menu.quit"))
    );
    h.tap_gamepad(GamepadButton::DPadUp);
    assert_eq!(
        h.app.world().resource::<InputFocus>().get(),
        Some(h.entity("main_menu.settings"))
    );
    h.tap_gamepad(GamepadButton::DPadLeft);
    assert_eq!(
        h.app.world().resource::<InputFocus>().get(),
        Some(h.entity("main_menu.play"))
    );
    h.tap_gamepad(GamepadButton::DPadDown);
    h.gamepad_buttons(&[(GamepadButton::DPadDown, 1.0), (GamepadButton::DPadUp, 1.0)]);
    assert_eq!(
        h.app.world().resource::<InputFocus>().get(),
        Some(h.entity("main_menu.settings")),
        "opposite navigation inputs cancel"
    );
    h.gamepad_buttons(&[(GamepadButton::DPadDown, 0.0), (GamepadButton::DPadUp, 0.0)]);

    h.focus("main_menu.play");
    h.tap_gamepad(GamepadButton::South);
    assert_eq!(h.activations(), &["main_menu.play"]);
    assert_eq!(
        h.app.world().resource::<ShowcaseState>().screen,
        ShowcaseScreen::Hud
    );
    h.gamepad_buttons(&[(GamepadButton::Start, 1.0)]);
    assert_eq!(
        h.app.world().resource::<ShowcaseState>().screen,
        ShowcaseScreen::Pause
    );
    h.app.update();
    h.gamepad_buttons(&[(GamepadButton::Start, 1.0)]);
    assert_eq!(
        h.app.world().resource::<ShowcaseState>().screen,
        ShowcaseScreen::Pause,
        "held buttons must not repeat"
    );
    h.gamepad_buttons(&[(GamepadButton::Start, 0.0)]);
    h.tap_gamepad(GamepadButton::East);
    assert_eq!(
        h.app.world().resource::<ShowcaseState>().screen,
        ShowcaseScreen::Hud
    );

    h.clear_focus();
    h.tap_gamepad(GamepadButton::South);
    assert_eq!(h.activations().len(), 1);
    h.disconnect_gamepad();
    h.gamepad_buttons(&[(GamepadButton::South, 1.0)]);
    assert_eq!(h.activations().len(), 1);
    assert!(
        h.app
            .world()
            .get::<bevy::input::gamepad::Gamepad>(gamepad)
            .is_none()
    );

    h.connect_gamepad();
    h.screen(ShowcaseScreen::MainMenu);
    h.focus("main_menu.play");
    h.tap_gamepad(GamepadButton::South);
    assert_eq!(h.activations().len(), 2);

    h.screen(ShowcaseScreen::MainMenu);
    h.focus("main_menu.play");
    let play = h.entity("main_menu.play");
    assert!(h.app.world_mut().despawn(play));
    h.tap_gamepad(GamepadButton::South);
    assert_eq!(
        h.activations().len(),
        2,
        "removed focused controls must not activate"
    );
}
