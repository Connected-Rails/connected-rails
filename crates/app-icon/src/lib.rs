//! The window icon of the simulator and of the three editors.
//!
//! It takes two paths, because neither one covers a desktop on its own. The `build.rs`
//! of every binary compiles `icon.ico` into the executable's resource, which is what
//! Windows Explorer, the task bar and Alt+Tab read; the startup system below hands the
//! same drawing to winit, which is what the title bar and X11 want. macOS reads
//! neither — an icon there belongs to an `.app` bundle, and the release ships plain
//! binaries. Wayland has no window-icon protocol: winit takes the call without
//! complaint but shows nothing, and a desktop entry would be the place to point at
//! `icon.png` should the programs ever be packaged.
//!
//! `icon.png` is the master and the only file to edit — a square RGBA drawing.
//! `tools/gen_icon.py` bakes `icon.ico` out of it.

use bevy::prelude::*;
use bevy::winit::WINIT_WINDOWS;

/// Compiled in, so the executable needs no file beside it.
const ICON: &[u8] = include_bytes!("../icon.png");

/// Gives every window of the app its icon.
pub fn plugin(app: &mut App) {
    app.add_systems(Startup, set_icon);
}

/// Winit has created the primary window by the time `Startup` runs — it happens on
/// `resumed`, which comes before the first update.
fn set_icon() {
    let image = image::load_from_memory(ICON)
        .expect("the compiled-in icon is a PNG")
        .into_rgba8();
    let (width, height) = image.dimensions();
    let icon = winit::window::Icon::from_rgba(image.into_raw(), width, height)
        .expect("the compiled-in icon is RGBA");
    WINIT_WINDOWS.with_borrow(|windows| {
        for window in windows.windows.values() {
            window.set_window_icon(Some(icon.clone()));
        }
    });
}
