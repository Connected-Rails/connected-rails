//! Hand-rolled CSS for the documented subset: rule parsing, selector
//! matching with specificity, the cascade, and the computed style struct
//! that layout and paint consume. Anything the subset does not know is
//! ignored without error.

use crate::dom::{Document, ElementData, NodeKind, ROOT, parse_inline_style};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Display {
    Flex,
    Block,
    None,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Direction {
    Row,
    Column,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Justify {
    Start,
    Center,
    End,
    SpaceBetween,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Align {
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TextAlign {
    Left,
    Center,
    Right,
}

/// A width/height value: absolute pixels or a fraction of the parent (a CSS
/// percentage stored as 0..=1).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Dim {
    Px(f32),
    Percent(f32),
}

/// The resolved style of one node. Sides are in CSS order top/right/bottom/
/// left; insets in the property order left/top/right/bottom.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct ComputedStyle {
    pub display: Display,
    pub direction: Direction,
    pub justify: Justify,
    pub align_items: Align,
    pub flex_grow: f32,
    pub gap: f32,
    pub width: Option<Dim>,
    pub height: Option<Dim>,
    pub padding: [f32; 4],
    pub margin: [f32; 4],
    pub absolute: bool,
    pub inset: [Option<f32>; 4],
    pub background: Option<[f32; 4]>,
    /// Inherited.
    pub color: [f32; 4],
    /// Inherited.
    pub font_size: f32,
    /// Inherited.
    pub text_align: TextAlign,
    pub border: Option<(f32, [f32; 4])>,
    pub visibility_hidden: bool,
    pub opacity: f32,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            display: Display::Block,
            direction: Direction::Row,
            justify: Justify::Start,
            align_items: Align::Stretch,
            flex_grow: 0.0,
            gap: 0.0,
            width: None,
            height: None,
            padding: [0.0; 4],
            margin: [0.0; 4],
            absolute: false,
            inset: [None; 4],
            background: None,
            color: [1.0, 1.0, 1.0, 1.0],
            font_size: 14.0,
            text_align: TextAlign::Left,
            border: None,
            visibility_hidden: false,
            opacity: 1.0,
        }
    }
}

/// A compound selector: every part must match one and the same element.
struct Selector {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    specificity: u32,
}

impl Selector {
    fn matches(&self, el: &ElementData) -> bool {
        if let Some(tag) = &self.tag
            && *tag != el.tag
        {
            return false;
        }
        if let Some(id) = &self.id
            && el.id.as_deref() != Some(id.as_str())
        {
            return false;
        }
        self.classes
            .iter()
            .all(|c| el.classes.iter().any(|e| e == c))
    }
}

struct Rule {
    selectors: Vec<Selector>,
    decls: Vec<(String, String)>,
}

#[derive(Default)]
pub(crate) struct Stylesheet {
    rules: Vec<Rule>,
}

/// Parses all rules; malformed selectors or blocks are skipped silently.
pub(crate) fn parse_stylesheet(css: &str) -> Stylesheet {
    let css = strip_comments(css);
    let mut rules = Vec::new();
    let mut rest = css.as_str();
    while let Some(open) = rest.find('{') {
        let selector_text = &rest[..open];
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        let body = &after[..close];
        rest = &after[close + 1..];
        let selectors: Vec<Selector> =
            selector_text.split(',').filter_map(parse_selector).collect();
        if selectors.is_empty() {
            continue;
        }
        let decls = parse_inline_style(body);
        if decls.is_empty() {
            continue;
        }
        rules.push(Rule { selectors, decls });
    }
    Stylesheet { rules }
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(open) = rest.find("/*") {
        out.push_str(&rest[..open]);
        match rest[open + 2..].find("*/") {
            Some(close) => rest = &rest[open + 2 + close + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// `tag`, `.class`, `#id` and compounds thereof; anything else (combinators,
/// pseudo-classes, `*`) is out of the subset and rejected.
fn parse_selector(s: &str) -> Option<Selector> {
    let s = s.trim();
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        return None;
    }
    let mut sel = Selector {
        tag: None,
        id: None,
        classes: Vec::new(),
        specificity: 0,
    };
    let mut rest = s;
    let tag_end = rest.find(['.', '#']).unwrap_or(rest.len());
    if tag_end > 0 {
        let tag = &rest[..tag_end];
        if !tag.chars().all(ident_char) {
            return None;
        }
        sel.tag = Some(tag.to_ascii_lowercase());
        sel.specificity += 1;
        rest = &rest[tag_end..];
    }
    while let Some(kind) = rest.chars().next() {
        let body = &rest[1..];
        let end = body.find(['.', '#']).unwrap_or(body.len());
        let name = &body[..end];
        if name.is_empty() || !name.chars().all(ident_char) {
            return None;
        }
        match kind {
            '.' => {
                sel.classes.push(name.to_owned());
                sel.specificity += 10;
            }
            '#' => {
                if sel.id.is_some() {
                    return None;
                }
                sel.id = Some(name.to_owned());
                sel.specificity += 100;
            }
            _ => return None,
        }
        rest = &body[end..];
    }
    Some(sel)
}

/// Computes the style of every node into `styles` (indexed like the arena):
/// cascade by specificity then source order, inline style last, inheritance
/// for `color`, `font-size` and `text-align`.
pub(crate) fn compute_into(doc: &Document, sheet: &Stylesheet, styles: &mut Vec<ComputedStyle>) {
    styles.clear();
    styles.resize(doc.nodes.len(), ComputedStyle::default());
    let mut matched: Vec<(u32, usize)> = Vec::new();
    compute_node(doc, sheet, ROOT, ComputedStyle::default(), styles, &mut matched);
}

fn compute_node(
    doc: &Document,
    sheet: &Stylesheet,
    idx: usize,
    parent: ComputedStyle,
    styles: &mut [ComputedStyle],
    matched: &mut Vec<(u32, usize)>,
) {
    let mut cs = ComputedStyle {
        color: parent.color,
        font_size: parent.font_size,
        text_align: parent.text_align,
        ..ComputedStyle::default()
    };
    if idx != ROOT
        && let NodeKind::Element(el) = &doc.nodes[idx].kind
    {
        matched.clear();
        for (ri, rule) in sheet.rules.iter().enumerate() {
            let best = rule
                .selectors
                .iter()
                .filter(|s| s.matches(el))
                .map(|s| s.specificity)
                .max();
            if let Some(specificity) = best {
                matched.push((specificity, ri));
            }
        }
        matched.sort_unstable();
        for &(_, ri) in matched.iter() {
            for (name, value) in &sheet.rules[ri].decls {
                apply_declaration(&mut cs, name, value);
            }
        }
        for (name, value) in &el.inline_style {
            apply_declaration(&mut cs, name, value);
        }
    }
    styles[idx] = cs;
    for &c in &doc.nodes[idx].children {
        compute_node(doc, sheet, c, cs, styles, matched);
    }
}

/// One declaration onto the computed struct; unknown names and unparsable
/// values are ignored without error.
fn apply_declaration(cs: &mut ComputedStyle, name: &str, value: &str) {
    let v = value.trim();
    match name {
        "display" => match v {
            "flex" => cs.display = Display::Flex,
            "block" => cs.display = Display::Block,
            "none" => cs.display = Display::None,
            _ => {}
        },
        "flex-direction" => match v {
            "row" => cs.direction = Direction::Row,
            "column" => cs.direction = Direction::Column,
            _ => {}
        },
        "justify-content" => match v {
            "flex-start" => cs.justify = Justify::Start,
            "center" => cs.justify = Justify::Center,
            "flex-end" => cs.justify = Justify::End,
            "space-between" => cs.justify = Justify::SpaceBetween,
            _ => {}
        },
        "align-items" => match v {
            "flex-start" => cs.align_items = Align::Start,
            "center" => cs.align_items = Align::Center,
            "flex-end" => cs.align_items = Align::End,
            "stretch" => cs.align_items = Align::Stretch,
            _ => {}
        },
        "flex-grow" => {
            if let Ok(g) = v.parse::<f32>()
                && g.is_finite()
                && g >= 0.0
            {
                cs.flex_grow = g;
            }
        }
        "gap" => {
            if let Some(g) = parse_px(v) {
                cs.gap = g;
            }
        }
        "width" => {
            if let Some(d) = parse_dim(v) {
                cs.width = Some(d);
            }
        }
        "height" => {
            if let Some(d) = parse_dim(v) {
                cs.height = Some(d);
            }
        }
        "padding" => {
            if let Some(sides) = parse_sides(v) {
                cs.padding = sides;
            }
        }
        "margin" => {
            if let Some(sides) = parse_sides(v) {
                cs.margin = sides;
            }
        }
        "position" => match v {
            "absolute" => cs.absolute = true,
            "static" | "relative" => cs.absolute = false,
            _ => {}
        },
        "left" => cs.inset[0] = parse_px(v).or(cs.inset[0]),
        "top" => cs.inset[1] = parse_px(v).or(cs.inset[1]),
        "right" => cs.inset[2] = parse_px(v).or(cs.inset[2]),
        "bottom" => cs.inset[3] = parse_px(v).or(cs.inset[3]),
        "background-color" => {
            if let Some(c) = parse_color(v) {
                cs.background = Some(c);
            }
        }
        "color" => {
            if let Some(c) = parse_color(v) {
                cs.color = c;
            }
        }
        "font-size" => {
            if let Some(s) = parse_px(v)
                && s > 0.0
            {
                cs.font_size = s;
            }
        }
        "text-align" => match v {
            "left" => cs.text_align = TextAlign::Left,
            "center" => cs.text_align = TextAlign::Center,
            "right" => cs.text_align = TextAlign::Right,
            _ => {}
        },
        "border" => {
            if v == "none" {
                cs.border = None;
            } else if let Some(b) = parse_border(v) {
                cs.border = Some(b);
            }
        }
        "visibility" => match v {
            "hidden" => cs.visibility_hidden = true,
            "visible" => cs.visibility_hidden = false,
            _ => {}
        },
        "opacity" => {
            if let Ok(o) = v.parse::<f32>()
                && o.is_finite()
            {
                cs.opacity = o.clamp(0.0, 1.0);
            }
        }
        _ => {}
    }
}

/// `<N>px` (or a bare number, leniently). Finite values only.
fn parse_px(v: &str) -> Option<f32> {
    let v = v.trim();
    let num = v.strip_suffix("px").unwrap_or(v).trim();
    let parsed = num.parse::<f32>().ok()?;
    parsed.is_finite().then_some(parsed)
}

fn parse_dim(v: &str) -> Option<Dim> {
    let v = v.trim();
    if let Some(p) = v.strip_suffix('%') {
        let parsed = p.trim().parse::<f32>().ok()?;
        return parsed.is_finite().then_some(Dim::Percent(parsed / 100.0));
    }
    parse_px(v).map(Dim::Px)
}

/// CSS shorthand with 1, 2 or 4 px values → top/right/bottom/left.
fn parse_sides(v: &str) -> Option<[f32; 4]> {
    let mut parts = [0.0f32; 4];
    let mut count = 0usize;
    for part in v.split_whitespace() {
        if count == 4 {
            return None;
        }
        parts[count] = parse_px(part)?;
        count += 1;
    }
    match count {
        1 => Some([parts[0]; 4]),
        2 => Some([parts[0], parts[1], parts[0], parts[1]]),
        4 => Some(parts),
        _ => None,
    }
}

/// `<N>px solid <color>`.
fn parse_border(v: &str) -> Option<(f32, [f32; 4])> {
    let mut it = v.split_whitespace();
    let width = parse_px(it.next()?)?;
    if it.next()? != "solid" {
        return None;
    }
    let color = parse_color(it.next()?)?;
    if it.next().is_some() {
        return None;
    }
    (width > 0.0).then_some((width, color))
}

/// `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()` and the documented
/// named handful.
pub(crate) fn parse_color(v: &str) -> Option<[f32; 4]> {
    let v = v.trim();
    if let Some(hex) = v.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    if let Some(inner) = v
        .strip_prefix("rgba(")
        .or_else(|| v.strip_prefix("rgb("))
        .and_then(|r| r.strip_suffix(')'))
    {
        let mut ch = [0.0f32; 4];
        ch[3] = 1.0;
        let mut count = 0usize;
        for part in inner.split(',') {
            if count == 4 {
                return None;
            }
            let n = part.trim().parse::<f32>().ok()?;
            if !n.is_finite() {
                return None;
            }
            // r/g/b are 0..=255, the rgba() alpha is 0..=1.
            ch[count] = if count < 3 { n / 255.0 } else { n };
            count += 1;
        }
        if count < 3 {
            return None;
        }
        return Some([
            ch[0].clamp(0.0, 1.0),
            ch[1].clamp(0.0, 1.0),
            ch[2].clamp(0.0, 1.0),
            ch[3].clamp(0.0, 1.0),
        ]);
    }
    named_color(v)
}

fn parse_hex_color(hex: &str) -> Option<[f32; 4]> {
    let nibble = |c: u8| char::from(c).to_digit(16);
    let bytes = hex.as_bytes();
    match bytes.len() {
        3 => {
            let mut out = [0.0f32; 4];
            out[3] = 1.0;
            for (i, &b) in bytes.iter().enumerate() {
                let d = nibble(b)?;
                out[i] = (d * 17) as f32 / 255.0;
            }
            Some(out)
        }
        6 | 8 => {
            let mut out = [0.0f32, 0.0, 0.0, 1.0];
            for i in 0..bytes.len() / 2 {
                let hi = nibble(bytes[2 * i])?;
                let lo = nibble(bytes[2 * i + 1])?;
                out[i] = (hi * 16 + lo) as f32 / 255.0;
            }
            Some(out)
        }
        _ => None,
    }
}

fn named_color(name: &str) -> Option<[f32; 4]> {
    let rgb = |r: u32, g: u32, b: u32| {
        Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
    };
    match name.to_ascii_lowercase().as_str() {
        "black" => rgb(0, 0, 0),
        "white" => rgb(255, 255, 255),
        "red" => rgb(255, 0, 0),
        "green" => rgb(0, 128, 0),
        "blue" => rgb(0, 0, 255),
        "yellow" => rgb(255, 255, 0),
        "orange" => rgb(255, 165, 0),
        "cyan" => rgb(0, 255, 255),
        "magenta" => rgb(255, 0, 255),
        "gray" | "grey" => rgb(128, 128, 128),
        "darkgray" | "darkgrey" => rgb(169, 169, 169),
        "lightgray" | "lightgrey" => rgb(211, 211, 211),
        _ => None,
    }
}
