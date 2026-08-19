//! The look the interface shares: surfaces, type and the two faces it is set in.
//!
//! Menu and HUD are the same product and have to read as one. The rules they both keep:
//!
//! **Surfaces are opaque tiers, neutral rather than blue** — a blue-black dark theme with
//! one warm accent is the look every generated dashboard wears. The HUD lies over a
//! moving picture instead of a wallpaper, so it takes the same tiers with an alpha (see
//! `hud::GLASS`); the hue never changes with it.
//!
//! **Prose is Fira Sans, machine output is Fira Mono.** Names, sentences and labels in
//! the proportional face; speeds, pressures, times, key caps and ids in the fixed one, so
//! figures stay in their columns while a value is running.
//!
//! **The interface is monochrome.** A state is a brighter surface, not a hue. The one
//! saturated colour is traffic red, and it means danger — the wordmark, the button that
//! starts something, an emergency brake. Amber is the single warning tone. Nothing else
//! carries a colour of its own, which is what leaves the cab's own signal lamps — 1000 Hz
//! amber, 500 Hz red — legible as signals rather than as decoration.

use bevy::prelude::*;
use bevy::text::FontSource;

/// The page behind everything.
pub const BASE: Color = Color::srgb(0.047, 0.047, 0.055);
/// Footer bar and detail pane.
pub const PANE: Color = Color::srgb(0.078, 0.078, 0.094);
/// A row at rest …
pub const ROW: Color = Color::srgb(0.102, 0.102, 0.122);
/// … under the cursor …
pub const ROW_HOVER: Color = Color::srgb(0.137, 0.137, 0.161);
/// … and the one the selection sits on.
pub const ROW_ACTIVE: Color = Color::srgb(0.173, 0.173, 0.204);
/// The leading slot of a row, which will hold artwork once there is any.
pub const SLOT: Color = Color::srgb(0.149, 0.149, 0.173);
/// Slider track, the off state of a toggle, and the empty part of a HUD gauge.
pub const TRACK: Color = Color::srgb(0.200, 0.200, 0.231);
/// Key cap in the footer.
pub const CHIP: Color = Color::srgb(0.118, 0.118, 0.137);
/// Rules and panel edges — opaque, so they do not change with what is behind them.
pub const HAIRLINE: Color = Color::srgb(0.165, 0.165, 0.192);

/// Selection and focus: warm bone white, no hue of its own. Everything the cursor is on
/// simply becomes brighter, which is what keeps the one saturated colour below meaningful.
pub const ACCENT: Color = Color::srgb(0.949, 0.941, 0.918);
/// Traffic red (RAL 3020), the colour German railways are painted and signed in. It marks
/// the wordmark, fills the button that starts something, and in the cab it means danger —
/// nothing else.
pub const BRAND: Color = Color::srgb(0.882, 0.000, 0.059);
/// Amber, for everything that wants attention without being danger: a mod missing a
/// dependency, a speed that is about to be exceeded, the 1000 Hz magnet.
pub const WARN: Color = Color::srgb(0.878, 0.663, 0.231);

pub const TEXT_BRIGHT: Color = Color::srgb(0.976, 0.973, 0.965);
pub const TEXT: Color = Color::srgb(0.902, 0.902, 0.910);
pub const TEXT_MID: Color = Color::srgb(0.627, 0.627, 0.659);
pub const TEXT_DIM: Color = Color::srgb(0.459, 0.459, 0.494);
pub const TEXT_FAINT: Color = Color::srgb(0.333, 0.333, 0.361);

/// The three faces the interface uses. Mono is the app's default font handle, so it needs
/// no handle of its own.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Sans,
    Semibold,
    Mono,
}

/// Handles of the two proportional faces, put into `Assets<Font>` while the app is built.
#[derive(Resource, Default, Clone)]
pub struct Fonts {
    pub sans: Handle<Font>,
    pub semibold: Handle<Font>,
}

/// The picture behind the menu. Compiled in like the fonts, so the binary stays
/// self-contained and there is no asset directory to ship beside it.
#[derive(Resource, Default, Clone)]
pub struct Wallpaper(pub Handle<Image>);

impl Fonts {
    pub fn source(&self, face: Face) -> FontSource {
        match face {
            Face::Sans => FontSource::Handle(self.sans.clone()),
            Face::Semibold => FontSource::Handle(self.semibold.clone()),
            Face::Mono => FontSource::default(),
        }
    }
}

/// A one pixel hairline. Opaque, so it does not change with what lies behind it.
pub fn rule(width: Val) -> impl Bundle {
    (
        Node {
            width,
            height: Val::Px(1.0),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(HAIRLINE),
    )
}

pub fn text(fonts: &Fonts, content: String, face: Face, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font: fonts.source(face),
            font_size: bevy::text::FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}
