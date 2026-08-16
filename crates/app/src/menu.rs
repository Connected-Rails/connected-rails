//! Main menu: pick line, vehicle and scenario, manage mods, start the run.
//!
//! Every page is the same list of rows — keyboard (↑/↓, Enter, Esc) and mouse (hover
//! selects, click confirms) drive the same selection index, so neither input is a special
//! case. The world is built only on leaving the menu, so a mod toggled here takes effect
//! on start — no restart. Any run flag on the command line (`--line`, `--frames`, …)
//! skips the menu entirely, which keeps the documented CLI and CI invocations
//! non-interactive.

use bevy::prelude::*;
use i18n::t;

use crate::mods_ui::{self, ModManager};
use crate::{GameState, Mods};

/// Background of the panel — same as the HUD's.
const PANEL_BG: Color = Color::srgba(0.0, 0.0, 0.0, 0.75);
/// Row under the cursor or the ↑/↓ selection.
const ROW_SELECTED: Color = Color::srgba(0.25, 0.45, 0.70, 0.85);

/// The player's choices. `None` means the built-in default — `setup` falls back to the
/// example line, the BR 101 and no scenario for exactly that case.
#[derive(Resource, Default, Clone)]
pub struct Selection {
    pub line_ref: Option<String>,
    pub loco_id: Option<String>,
    pub scenario_id: Option<String>,
}

/// Which list the menu is showing. The start flow walks Line → Loco → Scenario.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    #[default]
    Main,
    Line,
    Loco,
    Scenario,
    Mods,
}

/// Which page the menu shows and which row is selected.
#[derive(Resource, Default)]
pub struct MenuState {
    page: Page,
    selected: usize,
    /// Set by a click observer, consumed like an Enter press.
    clicked: bool,
    /// The node the rows hang off, so they can be rebuilt per page.
    list: Option<Entity>,
    /// Page and row count the spawned rows belong to.
    spawned: Option<(Page, usize)>,
}

/// What a text node in the panel shows. One component for all three keeps the render
/// system to a single `Query<&mut Text>` — two would need `Without` filters to be
/// provably disjoint.
#[derive(Component)]
pub enum MenuLabel {
    Header,
    Row(usize),
    Footer,
}

/// One selectable row: what it says and what it selects (`None` = built-in default).
struct Entry {
    label: String,
    id: Option<String>,
}

pub fn spawn_menu(mut commands: Commands, mut menu: ResMut<MenuState>) {
    // The world with its 3D camera does not exist yet — the menu brings its own.
    commands.spawn((Camera2d, DespawnOnExit(GameState::Menu)));
    let root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            DespawnOnExit(GameState::Menu),
        ))
        .id();
    let panel = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(14.0)),
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            ChildOf(root),
        ))
        .id();
    commands.spawn((text(String::new()), MenuLabel::Header, ChildOf(panel)));
    let list = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    commands.spawn((text(String::new()), MenuLabel::Footer, ChildOf(panel)));

    *menu = MenuState {
        list: Some(list),
        ..default()
    };
}

/// ↑/↓ or hover selects, Enter or left click confirms, Esc goes one page back.
#[allow(clippy::too_many_arguments)]
pub fn menu(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut menu: ResMut<MenuState>,
    mut selection: ResMut<Selection>,
    mut manager: ResMut<ModManager>,
    mut mods: ResMut<Mods>,
    mut next: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
    mut labels: Query<(&MenuLabel, &mut Text, &mut BackgroundColor)>,
) {
    let Some(list) = menu.list else {
        return;
    };

    let items = entries(menu.page, &mods.0);
    if items.is_empty() {
        menu.selected = 0;
    } else {
        if keys.just_pressed(KeyCode::ArrowDown) {
            menu.selected = (menu.selected + 1) % items.len();
        }
        if keys.just_pressed(KeyCode::ArrowUp) {
            menu.selected = (menu.selected + items.len() - 1) % items.len();
        }
        // A shrinking list (a mod switched off) must not leave the cursor past the end.
        menu.selected = menu.selected.min(items.len() - 1);
    }

    let confirmed = keys.just_pressed(KeyCode::Enter) || std::mem::take(&mut menu.clicked);
    if confirmed && !items.is_empty() {
        let id = items[menu.selected].id.clone();
        match menu.page {
            Page::Main => match menu.selected {
                0 => go(&mut menu, Page::Line),
                1 => go(&mut menu, Page::Mods),
                _ => {
                    exit.write(AppExit::Success);
                }
            },
            Page::Line => {
                selection.line_ref = id;
                go(&mut menu, Page::Loco);
            }
            Page::Loco => {
                selection.loco_id = id;
                go(&mut menu, Page::Scenario);
            }
            Page::Scenario => {
                selection.scenario_id = id;
                next.set(GameState::Driving);
            }
            Page::Mods => {
                mods_ui::toggle(&mut mods.0, menu.selected, &mut manager);
                // Reload right away, so the selection lists show what is enabled now.
                // Every mod stays in `manifests` either way, so the row keeps its index.
                mods.0 = mod_runtime::ModRuntime::load("mods");
            }
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        match menu.page {
            Page::Main => {}
            Page::Line | Page::Mods => go(&mut menu, Page::Main),
            Page::Loco => go(&mut menu, Page::Line),
            Page::Scenario => go(&mut menu, Page::Loco),
        }
    }

    // The page or the list may have changed above — re-read before drawing.
    let page = menu.page;
    let items = entries(page, &mods.0);
    let rebuilt = menu.spawned != Some((page, items.len()));
    if rebuilt {
        build_rows(&mut commands, list, &items, menu.selected);
        menu.spawned = Some((page, items.len()));
    }

    for (label, mut text, mut background) in &mut labels {
        match label {
            MenuLabel::Header => **text = header(page),
            MenuLabel::Footer => **text = footer(page, &mods.0, &manager),
            // The rows spawned this frame are not in the query yet; the ones that are
            // still belong to the previous page.
            MenuLabel::Row(_) if rebuilt => {}
            MenuLabel::Row(i) => {
                let Some(entry) = items.get(*i) else {
                    continue;
                };
                **text = row_text(entry, *i == menu.selected);
                *background = BackgroundColor(if *i == menu.selected {
                    ROW_SELECTED
                } else {
                    Color::NONE
                });
            }
        }
    }
}

/// Hovering a row selects it — the cursor and ↑/↓ share one index.
fn on_row_over(over: On<Pointer<Over>>, rows: Query<&MenuLabel>, mut menu: ResMut<MenuState>) {
    if let Ok(MenuLabel::Row(i)) = rows.get(over.event().entity) {
        menu.selected = *i;
    }
}

fn on_row_click(click: On<Pointer<Click>>, rows: Query<&MenuLabel>, mut menu: ResMut<MenuState>) {
    if click.event().event.button != PointerButton::Primary {
        return;
    }
    if let Ok(MenuLabel::Row(i)) = rows.get(click.event().entity) {
        menu.selected = *i;
        menu.clicked = true;
    }
}

fn go(menu: &mut MenuState, page: Page) {
    menu.page = page;
    menu.selected = 0;
}

fn build_rows(commands: &mut Commands, list: Entity, items: &[Entry], selected: usize) {
    commands.entity(list).despawn_related::<Children>();
    for (i, entry) in items.iter().enumerate() {
        commands
            .spawn((
                text(row_text(entry, i == selected)),
                BackgroundColor(if i == selected {
                    ROW_SELECTED
                } else {
                    Color::NONE
                }),
                Node {
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                    ..default()
                },
                MenuLabel::Row(i),
                ChildOf(list),
            ))
            .observe(on_row_over)
            .observe(on_row_click);
    }
}

fn row_text(entry: &Entry, selected: bool) -> String {
    format!("{} {}", if selected { ">" } else { " " }, entry.label)
}

fn text(content: String) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font_size: bevy::text::FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::WHITE),
    )
}

fn header(page: Page) -> String {
    match page {
        Page::Main => t!("window-simulator"),
        Page::Line => t!("menu-select-line"),
        Page::Loco => t!("menu-select-loco"),
        Page::Scenario => t!("menu-select-scenario"),
        Page::Mods => t!("mods-title"),
    }
}

fn footer(page: Page, runtime: &mod_runtime::ModRuntime, manager: &ModManager) -> String {
    match page {
        Page::Main => t!("menu-keys"),
        Page::Mods => format!(
            "{}\n{}",
            mods_ui::details(runtime, manager, true),
            t!("mods-keys-menu")
        ),
        _ => t!("menu-keys-back"),
    }
}

/// The rows of a page. Every selection page opens with the built-in default, so the list
/// is never empty and the run starts even with no mod installed.
fn entries(page: Page, runtime: &mod_runtime::ModRuntime) -> Vec<Entry> {
    let mods = &runtime.mods;
    let default = |key: &str| Entry {
        label: t!(key),
        id: None,
    };
    match page {
        Page::Main => [t!("menu-start"), t!("menu-mods"), t!("menu-quit")]
            .map(|label| Entry { label, id: None })
            .into(),
        Page::Line => std::iter::once(default("menu-line-builtin"))
            // Lines and compositions share one list: `resolve_line` takes either name,
            // and the player is picking a route, not a file format.
            .chain(named(mods.lines.iter().map(|(id, l)| (id, &l.name))))
            .chain(named(mods.compositions.iter().map(|(id, c)| (id, &c.name))))
            .collect(),
        Page::Loco => std::iter::once(default("menu-loco-builtin"))
            .chain(named(mods.vehicles.iter().map(|(id, v)| (id, &v.name))))
            .collect(),
        Page::Scenario => std::iter::once(default("menu-scenario-none"))
            .chain(named(mods.scenarios.iter().map(|(id, s)| (id, &s.name))))
            .collect(),
        // A row for the hint keeps the page from being blank; toggling it is a no-op.
        Page::Mods if mods.manifests.is_empty() => vec![default("mods-none")],
        Page::Mods => (0..mods.manifests.len())
            .map(|i| Entry {
                label: mods_ui::row(mods, i),
                id: None,
            })
            .collect(),
    }
}

fn named<'a>(items: impl Iterator<Item = (&'a String, &'a String)>) -> impl Iterator<Item = Entry> {
    items.map(|(id, name)| Entry {
        label: format!("{name}  ({id})"),
        id: Some(id.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every selection page offers the built-in default, so an empty mods directory
    /// still yields a startable run — the `% len()` navigation would panic otherwise.
    #[test]
    fn selection_pages_are_never_empty() {
        let runtime = mod_runtime::ModRuntime::load("does-not-exist");
        for page in [Page::Main, Page::Line, Page::Loco, Page::Scenario] {
            let items = entries(page, &runtime);
            assert!(!items.is_empty(), "{page:?} is empty");
        }
        // The defaults carry no id — `setup` reads that as "use the built-in".
        for page in [Page::Line, Page::Loco, Page::Scenario] {
            assert!(entries(page, &runtime)[0].id.is_none(), "{page:?}");
        }
    }

    /// The whole flow without a window: three confirmations pick line, vehicle and
    /// scenario and hand over to `Driving`; Esc walks back the same way.
    #[test]
    fn the_start_flow_reaches_driving_and_esc_walks_back() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<MenuState>()
            .init_resource::<Selection>()
            .init_resource::<ModManager>()
            // The example mod is the run's content; a missing directory is not an error,
            // so this also covers the "no mods installed" case on CI.
            .insert_resource(Mods(mod_runtime::ModRuntime::load("../../mods")))
            .init_state::<GameState>()
            .add_systems(Startup, spawn_menu)
            .add_systems(Update, menu);
        app.update();

        // `press` only counts as `just_pressed` while the key was up — the release has
        // to be simulated too, or the second Enter is silently swallowed.
        let key = |app: &mut App, code| {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(code);
            app.update();
            let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            input.reset(code);
            input.clear();
            app.update();
        };
        let page = |app: &App| app.world().resource::<MenuState>().page;

        key(&mut app, KeyCode::Enter);
        assert_eq!(page(&app), Page::Line);
        key(&mut app, KeyCode::Escape);
        assert_eq!(page(&app), Page::Main);

        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter);
        assert_eq!(page(&app), Page::Loco);
        key(&mut app, KeyCode::Enter);
        assert_eq!(page(&app), Page::Scenario);
        key(&mut app, KeyCode::Escape);
        assert_eq!(page(&app), Page::Loco);

        // What the row observers set: a click confirms exactly like Enter, and once.
        app.world_mut().resource_mut::<MenuState>().clicked = true;
        app.update();
        assert_eq!(page(&app), Page::Scenario);
        app.update();
        assert_eq!(page(&app), Page::Scenario, "the click was consumed");
        key(&mut app, KeyCode::Escape);

        // Second row where there is one, so the choice is not just the default.
        key(&mut app, KeyCode::ArrowDown);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter);
        assert_eq!(
            *app.world().resource::<State<GameState>>().get(),
            GameState::Driving
        );
        let selection = app.world().resource::<Selection>();
        assert!(selection.line_ref.is_none(), "the built-in line was picked");
        assert!(selection.scenario_id.is_none(), "no scenario was picked");
    }
}
