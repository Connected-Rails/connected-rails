//! Main menu: start the run, manage mods, quit.
//!
//! Keyboard-driven, in the same text-panel style as the HUD. The world is built only on
//! leaving the menu, so a mod toggled here takes effect on start — no restart. Any run
//! flag on the command line (`--line`, `--frames`, …) skips the menu entirely, which keeps
//! the documented CLI and CI invocations non-interactive.

use bevy::prelude::*;
use i18n::t;

use crate::mods_ui::{self, ModManager};
use crate::{GameState, Mods};

/// Which page the menu shows and which entry is selected.
#[derive(Resource, Default)]
pub struct MenuState {
    selected: usize,
    mods_open: bool,
}

/// Text node of the menu.
#[derive(Component)]
pub struct MenuPanel;

pub fn spawn_menu(mut commands: Commands) {
    // The world with its 3D camera does not exist yet — the menu brings its own.
    commands.spawn((Camera2d, DespawnOnExit(GameState::Menu)));
    commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        DespawnOnExit(GameState::Menu),
        children![(
            Text::new(""),
            TextFont {
                font_size: bevy::text::FontSize::Px(16.0),
                ..default()
            },
            TextColor(Color::WHITE),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
            Node {
                padding: UiRect::all(Val::Px(14.0)),
                ..default()
            },
            MenuPanel,
        )],
    ));
}

/// ↑/↓ select, Enter confirms; the mods entry opens the mod manager as its own page.
pub fn menu(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<MenuState>,
    mut manager: ResMut<ModManager>,
    mut mods: ResMut<Mods>,
    mut next: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
    mut panel: Query<&mut Text, With<MenuPanel>>,
) {
    let Ok(mut text) = panel.single_mut() else {
        return;
    };
    if menu.mods_open {
        mods_ui::navigate(&keys, &mut manager, &mut mods.0);
        if keys.just_pressed(KeyCode::Escape) {
            menu.mods_open = false;
        }
        **text = mods_ui::render(&mods.0, &manager, true);
        return;
    }

    let entries = [t!("menu-start"), t!("menu-mods"), t!("menu-quit")];
    if keys.just_pressed(KeyCode::ArrowDown) {
        menu.selected = (menu.selected + 1) % entries.len();
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        menu.selected = (menu.selected + entries.len() - 1) % entries.len();
    }
    if keys.just_pressed(KeyCode::Enter) {
        match menu.selected {
            0 => next.set(GameState::Driving),
            1 => menu.mods_open = true,
            _ => {
                exit.write(AppExit::Success);
            }
        }
    }

    let mut lines = vec![t!("window-simulator"), String::new()];
    for (i, entry) in entries.iter().enumerate() {
        let marker = if i == menu.selected { ">" } else { " " };
        lines.push(format!(" {marker} {entry}"));
    }
    lines.push(String::new());
    lines.push(t!("menu-keys"));
    **text = lines.join("\n");
}
