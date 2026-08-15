//! Mirrors the DOM into a taffy tree and solves it. `display: none`
//! subtrees never enter the tree; text nodes become measured leaves using
//! the app's monospaced metrics (0.6 em advance, 1.2 em line height), so
//! wrapping reacts to the width the solver actually offers.

use taffy::prelude::*;

use crate::dom::{Document, NodeKind, ROOT};
use crate::style::{self, Align, ComputedStyle, Dim, Justify};

/// Monospaced advance per character, as a fraction of the font size.
pub(crate) const CHAR_ADVANCE: f32 = 0.6;
/// Line height as a fraction of the font size.
pub(crate) const LINE_HEIGHT: f32 = 1.2;
/// Tolerance against float noise when re-deriving line breaks from a solved
/// width — a line that fitted during measure must still fit during paint.
const WRAP_EPS: f32 = 0.01;

/// The solved layout: taffy tree plus the DOM-index → taffy-node mapping
/// (`None` for nodes outside layout, i.e. `display: none` subtrees).
pub(crate) struct Solved {
    pub tree: TaffyTree<usize>,
    pub taffy_of: Vec<Option<NodeId>>,
}

pub(crate) fn solve(
    doc: &Document,
    styles: &[ComputedStyle],
    width: f32,
    height: f32,
) -> Result<Solved, String> {
    let mut tree: TaffyTree<usize> = TaffyTree::new();
    // Exact fractional coordinates: text metrics are already deterministic,
    // and rounding could make a measured line no longer fit its box.
    tree.disable_rounding();
    let mut taffy_of: Vec<Option<NodeId>> = vec![None; doc.nodes.len()];

    let mut top = Vec::new();
    for &c in &doc.nodes[ROOT].children {
        if let Some(n) = build(doc, styles, c, &mut tree, &mut taffy_of)
            .map_err(|e| format!("layout tree build failed: {e}"))?
        {
            top.push(n);
        }
    }
    let root_style = Style {
        display: Display::Block,
        size: Size {
            width: length(width),
            height: length(height),
        },
        ..Style::default()
    };
    let root = tree
        .new_with_children(root_style, &top)
        .map_err(|e| format!("layout tree build failed: {e}"))?;
    taffy_of[ROOT] = Some(root);

    tree.compute_layout_with_measure(
        root,
        Size {
            width: AvailableSpace::Definite(width),
            height: AvailableSpace::Definite(height),
        },
        |known, available, _node, ctx, _style| {
            let Some(&mut idx) = ctx else {
                return Size::ZERO;
            };
            let NodeKind::Text(text) = &doc.nodes[idx].kind else {
                return Size::ZERO;
            };
            let font_size = styles[idx].font_size;
            let max_width = known.width.unwrap_or(match available.width {
                AvailableSpace::Definite(w) => w,
                AvailableSpace::MaxContent => f32::INFINITY,
                AvailableSpace::MinContent => 0.0,
            });
            let (widest, lines) = measure_text(text, font_size, max_width);
            Size {
                width: known.width.unwrap_or(widest),
                height: known
                    .height
                    .unwrap_or(lines as f32 * LINE_HEIGHT * font_size),
            }
        },
    )
    .map_err(|e| format!("layout failed: {e}"))?;

    Ok(Solved { tree, taffy_of })
}

fn build(
    doc: &Document,
    styles: &[ComputedStyle],
    idx: usize,
    tree: &mut TaffyTree<usize>,
    taffy_of: &mut [Option<NodeId>],
) -> Result<Option<NodeId>, taffy::TaffyError> {
    match &doc.nodes[idx].kind {
        NodeKind::Text(_) => {
            let n = tree.new_leaf_with_context(Style::default(), idx)?;
            taffy_of[idx] = Some(n);
            Ok(Some(n))
        }
        NodeKind::Element(_) => {
            let cs = &styles[idx];
            if cs.display == style::Display::None {
                return Ok(None);
            }
            let mut children = Vec::new();
            for &c in &doc.nodes[idx].children {
                if let Some(n) = build(doc, styles, c, tree, taffy_of)? {
                    children.push(n);
                }
            }
            let n = tree.new_with_children(to_taffy(cs), &children)?;
            taffy_of[idx] = Some(n);
            Ok(Some(n))
        }
    }
}

fn to_taffy(cs: &ComputedStyle) -> Style {
    let dim = |d: Option<Dim>| match d {
        Some(Dim::Px(v)) => length(v),
        Some(Dim::Percent(v)) => percent(v),
        None => Dimension::AUTO,
    };
    // CSS order top/right/bottom/left → taffy rect.
    let sides = |s: [f32; 4]| Rect {
        top: length(s[0]),
        right: length(s[1]),
        bottom: length(s[2]),
        left: length(s[3]),
    };
    let border = cs.border.map(|(w, _)| w).unwrap_or(0.0);
    let inset = |v: Option<f32>| v.map(length).unwrap_or(LengthPercentageAuto::AUTO);
    Style {
        display: match cs.display {
            style::Display::Flex => Display::Flex,
            style::Display::Block => Display::Block,
            style::Display::None => Display::None,
        },
        flex_direction: match cs.direction {
            style::Direction::Row => FlexDirection::Row,
            style::Direction::Column => FlexDirection::Column,
        },
        justify_content: Some(match cs.justify {
            Justify::Start => JustifyContent::FlexStart,
            Justify::Center => JustifyContent::Center,
            Justify::End => JustifyContent::FlexEnd,
            Justify::SpaceBetween => JustifyContent::SpaceBetween,
        }),
        align_items: Some(match cs.align_items {
            Align::Start => AlignItems::FlexStart,
            Align::Center => AlignItems::Center,
            Align::End => AlignItems::FlexEnd,
            Align::Stretch => AlignItems::Stretch,
        }),
        flex_grow: cs.flex_grow,
        gap: Size {
            width: length(cs.gap),
            height: length(cs.gap),
        },
        size: Size {
            width: dim(cs.width),
            height: dim(cs.height),
        },
        padding: sides(cs.padding),
        margin: sides(cs.margin).map(LengthPercentageAuto::from),
        border: Rect {
            top: length(border),
            right: length(border),
            bottom: length(border),
            left: length(border),
        },
        position: if cs.absolute {
            Position::Absolute
        } else {
            Position::Relative
        },
        inset: Rect {
            left: inset(cs.inset[0]),
            top: inset(cs.inset[1]),
            right: inset(cs.inset[2]),
            bottom: inset(cs.inset[3]),
        },
        ..Style::default()
    }
}

/// Greedy word wrap over `split_whitespace`, identical for measure and
/// paint: `emit` is called per line with the half-open word index range and
/// the line width in pixels. Returns (widest line, line count).
pub(crate) fn wrap_indices(
    text: &str,
    font_size: f32,
    max_width: f32,
    mut emit: impl FnMut(usize, usize, f32),
) -> (f32, usize) {
    let advance = CHAR_ADVANCE * font_size;
    let mut widest = 0.0f32;
    let mut lines = 0usize;
    let mut start = 0usize;
    let mut line_width = 0.0f32;
    let mut i = 0usize;
    for word in text.split_whitespace() {
        let word_width = word.chars().count() as f32 * advance;
        if i == start {
            line_width = word_width;
        } else if line_width + advance + word_width <= max_width + WRAP_EPS {
            line_width += advance + word_width;
        } else {
            emit(start, i, line_width);
            widest = widest.max(line_width);
            lines += 1;
            start = i;
            line_width = word_width;
        }
        i += 1;
    }
    if i > start {
        emit(start, i, line_width);
        widest = widest.max(line_width);
        lines += 1;
    }
    (widest, lines)
}

pub(crate) fn measure_text(text: &str, font_size: f32, max_width: f32) -> (f32, usize) {
    wrap_indices(text, font_size, max_width, |_, _, _| {})
}
