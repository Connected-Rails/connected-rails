//! Walks the laid-out tree in document order and emits the flat command
//! list: a `Clear` from the root/body background, then per element its
//! background rect, border outline and wrapped text runs. `visibility:
//! hidden` and the `hidden` attribute suppress an element and its subtree
//! but keep the layout gap; opacity multiplies down the tree.

use crate::PaintCmd;
use crate::dom::{Document, NodeKind, ROOT};
use crate::layout::{LINE_HEIGHT, Solved, wrap_indices};
use crate::style::{ComputedStyle, TextAlign};

/// Hard cap on one picture — a runaway script cannot flood the renderer.
const MAX_CMDS: usize = 4096;

pub(crate) fn paint(
    doc: &Document,
    styles: &[ComputedStyle],
    solved: &Solved,
    out: &mut Vec<PaintCmd>,
) {
    out.clear();
    out.push(PaintCmd::Clear {
        color: clear_color(doc, styles),
    });
    let mut origin_x = 0.0;
    let mut origin_y = 0.0;
    if let Some(root) = solved.taffy_of[ROOT]
        && let Ok(layout) = solved.tree.layout(root)
    {
        origin_x = layout.location.x;
        origin_y = layout.location.y;
    }
    for &c in &doc.nodes[ROOT].children {
        walk(doc, styles, solved, c, origin_x, origin_y, 1.0, out);
    }
}

/// The screen background: the first `body` element's background if set,
/// then `html`, then the first top-level element; default black.
fn clear_color(doc: &Document, styles: &[ComputedStyle]) -> [f32; 4] {
    let by_tag = |tag: &str| {
        doc.nodes.iter().position(|n| match &n.kind {
            NodeKind::Element(el) => el.tag == tag,
            NodeKind::Text(_) => false,
        })
    };
    let candidates = [
        by_tag("body"),
        by_tag("html"),
        doc.nodes[ROOT].children.first().copied(),
    ];
    for idx in candidates.into_iter().flatten() {
        if let Some(bg) = styles[idx].background {
            return bg;
        }
    }
    [0.0, 0.0, 0.0, 1.0]
}

#[expect(clippy::too_many_arguments, reason = "plain recursive tree walk")]
fn walk(
    doc: &Document,
    styles: &[ComputedStyle],
    solved: &Solved,
    idx: usize,
    origin_x: f32,
    origin_y: f32,
    opacity: f32,
    out: &mut Vec<PaintCmd>,
) {
    if out.len() >= MAX_CMDS {
        return;
    }
    // `display: none` subtrees have no taffy node and paint nothing.
    let Some(taffy_node) = solved.taffy_of[idx] else {
        return;
    };
    let Ok(layout) = solved.tree.layout(taffy_node) else {
        return;
    };
    let x = origin_x + layout.location.x;
    let y = origin_y + layout.location.y;
    let cs = &styles[idx];
    match &doc.nodes[idx].kind {
        NodeKind::Element(el) => {
            // Suppress paint of the whole subtree, keep the layout space.
            if el.hidden || cs.visibility_hidden {
                return;
            }
            let opacity = opacity * cs.opacity;
            if let Some(bg) = cs.background {
                push(out, PaintCmd::Rect {
                    x,
                    y,
                    w: layout.size.width,
                    h: layout.size.height,
                    color: with_alpha(bg, opacity),
                    filled: true,
                });
            }
            if let Some((_, border_color)) = cs.border {
                push(out, PaintCmd::Rect {
                    x,
                    y,
                    w: layout.size.width,
                    h: layout.size.height,
                    color: with_alpha(border_color, opacity),
                    filled: false,
                });
            }
            for &c in &doc.nodes[idx].children {
                walk(doc, styles, solved, c, x, y, opacity, out);
            }
        }
        NodeKind::Text(text) => {
            let font_size = cs.font_size;
            let line_height = LINE_HEIGHT * font_size;
            let box_width = layout.size.width;
            let color = with_alpha(cs.color, opacity);
            let words: Vec<&str> = text.split_whitespace().collect();
            let mut line_no = 0usize;
            wrap_indices(text, font_size, box_width, |start, end, line_width| {
                let tx = x + match cs.text_align {
                    TextAlign::Left => 0.0,
                    TextAlign::Center => (box_width - line_width) / 2.0,
                    TextAlign::Right => box_width - line_width,
                };
                push(out, PaintCmd::Text {
                    x: tx,
                    y: y + line_no as f32 * line_height,
                    text: words[start..end].join(" "),
                    size: font_size,
                    color,
                });
                line_no += 1;
            });
        }
    }
}

fn push(out: &mut Vec<PaintCmd>, cmd: PaintCmd) {
    if out.len() < MAX_CMDS {
        out.push(cmd);
    }
}

fn with_alpha(color: [f32; 4], opacity: f32) -> [f32; 4] {
    [color[0], color[1], color[2], color[3] * opacity]
}
