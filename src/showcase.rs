use crate::showcase_bindings::ShowcaseNode as Node;
use bevy::input_focus::{
    FocusCause, InputFocus,
    tab_navigation::{NavAction, TabIndex, TabNavigation, TabNavigationPlugin},
};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use bevy_openpencil::{
    OpenPencilRuntimeId, OpenPencilRuntimeIds, OpenPencilUiReconciled, OpenPencilUiRoot,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShowcaseScreen {
    #[default]
    MainMenu,
    Settings,
    Hud,
    Pause,
}

#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct ShowcaseState {
    pub screen: ShowcaseScreen,
    pub player_name: String,
    pub score: u32,
    pub king_health: u8,
    pub status: String,
    pub fullscreen: bool,
    pub music_volume: u8,
    pub applied_fullscreen: bool,
    pub applied_music_volume: u8,
    pub quit_requested: bool,
}

impl Default for ShowcaseState {
    fn default() -> Self {
        Self {
            screen: ShowcaseScreen::MainMenu,
            player_name: "Rowan".into(),
            score: 1200,
            king_health: 84,
            status: "Formation stable".into(),
            fullscreen: true,
            music_volume: 70,
            applied_fullscreen: true,
            applied_music_volume: 70,
            quit_requested: false,
        }
    }
}

impl ShowcaseState {
    pub fn activate(&mut self, node: Node) {
        match node {
            Node::MainMenuPlay => self.screen = ShowcaseScreen::Hud,
            Node::MainMenuSettings => self.screen = ShowcaseScreen::Settings,
            Node::MainMenuQuit => self.quit_requested = true,
            Node::SettingsToggleFullscreen => self.fullscreen = !self.fullscreen,
            Node::SettingsMusicDown => self.music_volume = self.music_volume.saturating_sub(10),
            Node::SettingsMusicUp => {
                self.music_volume = self.music_volume.saturating_add(10).min(100)
            }
            Node::SettingsBack => {
                self.fullscreen = self.applied_fullscreen;
                self.music_volume = self.applied_music_volume;
                self.screen = ShowcaseScreen::MainMenu;
            }
            Node::SettingsApply => {
                self.applied_fullscreen = self.fullscreen;
                self.applied_music_volume = self.music_volume;
                self.screen = ShowcaseScreen::MainMenu;
            }
            Node::HudPause => self.screen = ShowcaseScreen::Pause,
            Node::PauseResume => self.screen = ShowcaseScreen::Hud,
            Node::PauseMenu => self.screen = ShowcaseScreen::MainMenu,
            _ => {}
        }
    }

    fn back(&mut self) {
        match self.screen {
            ShowcaseScreen::Settings => self.activate(Node::SettingsBack),
            ShowcaseScreen::Pause => self.activate(Node::PauseResume),
            ShowcaseScreen::Hud => self.activate(Node::HudPause),
            ShowcaseScreen::MainMenu => {}
        }
    }
}

#[derive(Component)]
pub struct ShowcaseUi;

pub struct ShowcasePlugin;

impl Plugin for ShowcasePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShowcaseState>()
            .add_plugins(TabNavigationPlugin)
            .add_observer(activate_button)
            .add_systems(Update, handle_navigation)
            .add_systems(PostUpdate, sync_view);
    }
}

fn activate_button(
    event: On<Activate>,
    ids: Query<&OpenPencilRuntimeId>,
    mut state: ResMut<ShowcaseState>,
) {
    if let Ok(runtime_id) = ids.get(event.entity)
        && let Some(node) = Node::from_value(&runtime_id.0)
    {
        state.activate(node);
    }
}

fn handle_navigation(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    navigation: TabNavigation,
    mut focus: ResMut<InputFocus>,
    mut state: ResMut<ShowcaseState>,
    buttons: Query<Has<InteractionDisabled>, With<bevy::ui_widgets::Button>>,
    mut commands: Commands,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        state.back();
    }
    let next = gamepads.iter().any(|gamepad| {
        gamepad.any_just_pressed([GamepadButton::DPadDown, GamepadButton::DPadRight])
    });
    let previous = gamepads
        .iter()
        .any(|gamepad| gamepad.any_just_pressed([GamepadButton::DPadUp, GamepadButton::DPadLeft]));
    if let Some(action) = match (next, previous) {
        (true, false) => Some(NavAction::Next),
        (false, true) => Some(NavAction::Previous),
        _ => None,
    } && let Ok(entity) = navigation.navigate(&focus, action)
    {
        focus.set(entity, FocusCause::Navigated);
    }
    if gamepads
        .iter()
        .any(|gamepad| gamepad.just_pressed(GamepadButton::South))
        && let Some(entity) = focus.get()
        && buttons.get(entity) == Ok(false)
    {
        commands.trigger(Activate { entity });
    }
    if gamepads
        .iter()
        .any(|gamepad| gamepad.just_pressed(GamepadButton::East))
    {
        state.back();
    }
    if gamepads
        .iter()
        .any(|gamepad| gamepad.just_pressed(GamepadButton::Start))
    {
        match state.screen {
            ShowcaseScreen::Hud => state.activate(Node::HudPause),
            ShowcaseScreen::Pause => state.activate(Node::PauseResume),
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_view(
    state: Res<ShowcaseState>,
    mut reconciled: MessageReader<OpenPencilUiReconciled>,
    roots: Query<Entity, (With<OpenPencilUiRoot>, With<ShowcaseUi>)>,
    ids: Res<OpenPencilRuntimeIds>,
    mut visibility: Query<&mut Visibility>,
    mut texts: Query<&mut Text>,
    mut focus: ResMut<InputFocus>,
    mut previous_screen: Local<Option<ShowcaseScreen>>,
    mut commands: Commands,
) {
    let reloaded = reconciled.read().next().is_some();
    if !state.is_changed() && !reloaded {
        return;
    }
    let Ok(root) = roots.single() else {
        return;
    };
    for (node, visible) in [
        (
            Node::ScreenMainMenu,
            state.screen == ShowcaseScreen::MainMenu,
        ),
        (
            Node::ScreenSettings,
            state.screen == ShowcaseScreen::Settings,
        ),
        (
            Node::ScreenHud,
            matches!(state.screen, ShowcaseScreen::Hud | ShowcaseScreen::Pause),
        ),
        (Node::ScreenPause, state.screen == ShowcaseScreen::Pause),
    ] {
        if let Some(entity) = ids.get(root, node.as_str())
            && let Ok(mut visibility) = visibility.get_mut(entity)
        {
            *visibility = if visible {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
    for (node, value) in [
        (
            Node::HudPlayerName,
            format!("Player: {}", state.player_name),
        ),
        (Node::HudScore, format!("Score: {}", state.score)),
        (
            Node::HudHealth,
            format!("King Health: {}", state.king_health),
        ),
        (Node::HudStatus, state.status.clone()),
        (
            Node::SettingsFullscreen,
            format!(
                "Fullscreen: {}",
                if state.fullscreen { "On" } else { "Off" }
            ),
        ),
        (
            Node::SettingsMusic,
            format!("Music Volume: {}%", state.music_volume),
        ),
    ] {
        if let Some(entity) = ids.get(root, node.as_str())
            && let Ok(mut text) = texts.get_mut(entity)
        {
            text.0 = value;
        }
    }
    for (node, screen, tab_index) in BUTTONS {
        if let Some(entity) = ids.get(root, node.as_str()) {
            if state.screen == screen {
                commands
                    .entity(entity)
                    .insert(TabIndex(tab_index))
                    .remove::<InteractionDisabled>();
            } else {
                commands
                    .entity(entity)
                    .remove::<TabIndex>()
                    .insert(InteractionDisabled);
            }
        }
    }
    if reloaded || *previous_screen != Some(state.screen) {
        let first = match state.screen {
            ShowcaseScreen::MainMenu => Node::MainMenuPlay,
            ShowcaseScreen::Settings => Node::SettingsToggleFullscreen,
            ShowcaseScreen::Hud => Node::HudPause,
            ShowcaseScreen::Pause => Node::PauseResume,
        };
        if let Some(entity) = ids.get(root, first.as_str()) {
            focus.set(entity, FocusCause::Navigated);
        }
        *previous_screen = Some(state.screen);
    }
}

const BUTTONS: [(Node, ShowcaseScreen, i32); 11] = [
    (Node::MainMenuPlay, ShowcaseScreen::MainMenu, 0),
    (Node::MainMenuSettings, ShowcaseScreen::MainMenu, 1),
    (Node::MainMenuQuit, ShowcaseScreen::MainMenu, 2),
    (Node::SettingsToggleFullscreen, ShowcaseScreen::Settings, 3),
    (Node::SettingsMusicDown, ShowcaseScreen::Settings, 4),
    (Node::SettingsMusicUp, ShowcaseScreen::Settings, 5),
    (Node::SettingsBack, ShowcaseScreen::Settings, 6),
    (Node::SettingsApply, ShowcaseScreen::Settings, 7),
    (Node::HudPause, ShowcaseScreen::Hud, 8),
    (Node::PauseResume, ShowcaseScreen::Pause, 9),
    (Node::PauseMenu, ShowcaseScreen::Pause, 10),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_journey_keeps_staged_settings_explicit() {
        let mut state = ShowcaseState::default();
        state.activate(Node::MainMenuSettings);
        state.activate(Node::SettingsToggleFullscreen);
        state.activate(Node::SettingsMusicDown);
        assert_eq!(state.screen, ShowcaseScreen::Settings);
        assert!(!state.fullscreen);
        assert_eq!(state.music_volume, 60);
        state.activate(Node::SettingsBack);
        assert_eq!(state.screen, ShowcaseScreen::MainMenu);
        assert!(state.fullscreen);
        assert_eq!(state.music_volume, 70);
        state.activate(Node::MainMenuSettings);
        state.activate(Node::SettingsToggleFullscreen);
        state.activate(Node::SettingsMusicUp);
        state.activate(Node::SettingsApply);
        assert!(!state.applied_fullscreen);
        assert_eq!(state.applied_music_volume, 80);
        state.activate(Node::MainMenuPlay);
        state.activate(Node::HudPause);
        state.activate(Node::PauseResume);
        assert_eq!(state.screen, ShowcaseScreen::Hud);
        state.activate(Node::HudPause);
        state.activate(Node::PauseMenu);
        state.activate(Node::MainMenuQuit);
        assert!(state.quit_requested);
    }
}
