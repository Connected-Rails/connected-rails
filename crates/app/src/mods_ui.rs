//! Mod manager (plan 19.7): list the installed mods, switch them on and off, show what a
//! mod is missing.
//!
//! The list lives on the main menu (`menu.rs`), where a toggle takes effect when the run
//! starts. F9 opens the same list in the simulator; there a toggle only takes effect after
//! a restart — reloading mid-run would mean rebuilding the line, the trains and the
//! interlocking from scratch.

use crate::Mods;
use bevy::prelude::*;
use i18n::t;

/// Root node of the panel.
#[derive(Component)]
pub struct ModPanel;

/// Which entry is selected, and whether the panel is open at all.
#[derive(Resource, Default)]
pub struct ModManager {
    pub open: bool,
    pub selected: usize,
    /// Set once something was toggled — the loaded set no longer matches the disk.
    pub(crate) restart_needed: bool,
}

pub fn spawn_panel(commands: &mut Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: bevy::text::FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::WHITE),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(60.0),
            right: Val::Px(20.0),
            padding: UiRect::all(Val::Px(14.0)),
            ..default()
        },
        Visibility::Hidden,
        ModPanel,
    ));
}

/// F9 opens and closes, ↑/↓ select, Enter toggles.
pub fn mod_manager(
    keys: Res<ButtonInput<KeyCode>>,
    mut manager: ResMut<ModManager>,
    mut mods: ResMut<Mods>,
    mut panel: Query<(&mut Text, &mut Visibility), With<ModPanel>>,
) {
    if keys.just_pressed(KeyCode::F9) {
        manager.open = !manager.open;
    }
    let Ok((mut text, mut visibility)) = panel.single_mut() else {
        return;
    };
    if !manager.open {
        *visibility = Visibility::Hidden;
        return;
    }
    *visibility = Visibility::Inherited;

    navigate(&keys, &mut manager, &mut mods.0);
    **text = render(&mods.0, &manager, false);
}

/// ↑/↓ select, Enter toggles `enabled` on disk — shared by the F9 panel and the menu.
pub(crate) fn navigate(
    keys: &ButtonInput<KeyCode>,
    manager: &mut ModManager,
    runtime: &mut mod_runtime::ModRuntime,
) {
    let count = runtime.mods.manifests.len();
    if count == 0 {
        return;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        manager.selected = (manager.selected + 1) % count;
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        manager.selected = (manager.selected + count - 1) % count;
    }
    if keys.just_pressed(KeyCode::Enter) {
        let manifest = &mut runtime.mods.manifests[manager.selected];
        let wanted = !manifest.enabled;
        match manifest.set_enabled(wanted) {
            Ok(()) => manager.restart_needed = true,
            Err(e) => warn!("mod {}: {e}", manifest.id),
        }
    }
}

pub(crate) fn render(
    runtime: &mod_runtime::ModRuntime,
    manager: &ModManager,
    in_menu: bool,
) -> String {
    let mods = &runtime.mods;
    let mut lines = vec![t!("mods-title"), String::new()];
    if mods.manifests.is_empty() {
        lines.push(t!("mods-none"));
    }
    for (i, man) in mods.manifests.iter().enumerate() {
        let marker = if i == manager.selected { ">" } else { " " };
        lines.push(format!(
            "{marker} [{}] {:<24} {:<8} {}",
            if man.enabled { "x" } else { " " },
            man.id,
            man.version,
            man.name
        ));
        let missing = mods.missing_depends(&man.id);
        if !missing.is_empty() {
            lines.push(format!(
                "      {}",
                t!("mods-missing-depends", depends = missing.join(", "))
            ));
        }
    }
    lines.push(String::new());
    lines.push(t!(
        "mods-content",
        vehicles = mods.vehicles.len(),
        lines = mods.lines.len(),
        compositions = mods.compositions.len(),
        scenarios = mods.scenarios.len(),
        timetables = mods.timetables.len(),
        signals = mods.signal_types.len(),
        scripts = mods.scripts.len(),
    ));
    // Loading warnings and script errors — the modder's first place to look.
    let log = runtime.log();
    if !log.is_empty() {
        lines.push(String::new());
        lines.push(t!("mods-log"));
        for entry in log.iter().take(6) {
            lines.push(format!("  {entry}"));
        }
    }
    // On the menu a toggle simply applies when the run starts; in the simulator it
    // has to wait for a restart.
    if manager.restart_needed && !in_menu {
        lines.push(String::new());
        lines.push(t!("mods-restart"));
    }
    lines.push(String::new());
    lines.push(if in_menu {
        t!("mods-keys-menu")
    } else {
        t!("mods-keys")
    });
    lines.join("\n")
}
