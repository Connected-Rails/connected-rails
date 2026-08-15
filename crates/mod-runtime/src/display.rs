//! Draw lists for cab displays (plan ch. 12).
//!
//! A vehicle script's `display(ctx)` hook answers with an array of draw
//! commands; the app renders them into the display's texture. The same
//! primitives are what the declarative [`sim_core::cab::Widget`] list compiles
//! to, so both content paths share one renderer.
//!
//! Trust boundary as everywhere in this crate: a script may return anything.
//! Non-finite numbers drop the command, text is truncated, and the list is
//! capped — a runaway script cannot balloon the frame.

use mlua::Table;

/// A script may draw this many commands per display and frame.
pub const MAX_COMMANDS: usize = 512;
/// Longest text one command may carry.
pub const MAX_TEXT: usize = 128;

/// One draw command, in pixels from the top left of the display texture.
/// Colors are linear RGBA 0 … 1.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawCmd {
    /// Background fill — usually the first command.
    Clear { color: [f32; 4] },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
        filled: bool,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        width: f32,
        color: [f32; 4],
    },
    Text {
        x: f32,
        y: f32,
        text: String,
        size: f32,
        color: [f32; 4],
        align: TextAlign,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// `{r, g, b}` or `{r, g, b, a}`, clamped; white when absent or malformed.
fn color(table: &Table, key: &str) -> [f32; 4] {
    let Ok(entry) = table.get::<Table>(key) else {
        return [1.0, 1.0, 1.0, 1.0];
    };
    let channel = |i: i64, default: f32| {
        entry
            .get::<f64>(i)
            .ok()
            .filter(|v| v.is_finite())
            .map(|v| v.clamp(0.0, 1.0) as f32)
            .unwrap_or(default)
    };
    [
        channel(1, 1.0),
        channel(2, 1.0),
        channel(3, 1.0),
        channel(4, 1.0),
    ]
}

/// A finite number, or `None` — one bad coordinate drops one command, not the list.
fn number(table: &Table, key: &str) -> Option<f32> {
    table
        .get::<f64>(key)
        .ok()
        .filter(|v| v.is_finite())
        .map(|v| v as f32)
}

/// Parses what `display(ctx)` returned. Malformed entries are skipped and the
/// first complaint per script is reported through the returned message.
pub fn parse_draw_list(out: &Table) -> (Vec<DrawCmd>, Option<String>) {
    let mut commands = Vec::new();
    let mut complaint = None;
    for entry in out.sequence_values::<Table>() {
        if commands.len() >= MAX_COMMANDS {
            complaint.get_or_insert_with(|| format!("draw list capped at {MAX_COMMANDS}"));
            break;
        }
        let Ok(entry) = entry else {
            complaint.get_or_insert_with(|| "draw list entry is not a table".to_string());
            continue;
        };
        let kind = entry.get::<String>("kind").unwrap_or_default();
        let cmd = match kind.as_str() {
            "clear" => Some(DrawCmd::Clear {
                color: color(&entry, "color"),
            }),
            "rect" => (|| {
                Some(DrawCmd::Rect {
                    x: number(&entry, "x")?,
                    y: number(&entry, "y")?,
                    w: number(&entry, "w")?,
                    h: number(&entry, "h")?,
                    color: color(&entry, "color"),
                    filled: entry.get::<bool>("filled").unwrap_or(true),
                })
            })(),
            "line" => (|| {
                Some(DrawCmd::Line {
                    x1: number(&entry, "x1")?,
                    y1: number(&entry, "y1")?,
                    x2: number(&entry, "x2")?,
                    y2: number(&entry, "y2")?,
                    width: number(&entry, "width").unwrap_or(1.0).max(0.5),
                    color: color(&entry, "color"),
                })
            })(),
            "text" => (|| {
                let mut text = entry.get::<String>("text").ok()?;
                text.truncate(MAX_TEXT);
                Some(DrawCmd::Text {
                    x: number(&entry, "x")?,
                    y: number(&entry, "y")?,
                    text,
                    size: number(&entry, "size").unwrap_or(16.0).clamp(4.0, 128.0),
                    color: color(&entry, "color"),
                    align: match entry.get::<String>("align").as_deref() {
                        Ok("center") => TextAlign::Center,
                        Ok("right") => TextAlign::Right,
                        _ => TextAlign::Left,
                    },
                })
            })(),
            other => {
                complaint.get_or_insert_with(|| format!("unknown draw kind {other:?}"));
                None
            }
        };
        if let Some(cmd) = cmd {
            commands.push(cmd);
        }
    }
    (commands, complaint)
}
