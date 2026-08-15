//! Own mutable DOM, built once from the lenient `tl` parse.
//!
//! `tl` borrows from the source string, so its tree cannot outlive the load
//! call; this module copies everything into an arena of plain owned nodes
//! that the script can mutate for the lifetime of the gauge. `<style>` blocks
//! are concatenated into [`Document::css`] and the first `<script>` block is
//! kept as [`Document::script`]; neither becomes a DOM node.

use crate::SimFrame;

/// Hard cap on the text of a single node — scripts building strings in a
/// loop cannot blow up layout or paint.
pub(crate) const MAX_TEXT_LEN: usize = 256;

/// Index of the synthetic root element that holds all top-level nodes.
pub(crate) const ROOT: usize = 0;

/// Attributes and identity of an element node.
pub(crate) struct ElementData {
    /// Lowercased tag name; only ever compared against selectors.
    pub tag: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
    /// Inline `style=` declarations, name lowercased, in source order.
    pub inline_style: Vec<(String, String)>,
    /// All other attributes (`data-bind`, `data-format`, …), name lowercased.
    pub attrs: Vec<(String, String)>,
    /// The `hidden` attribute / IDL property: painted invisible, keeps space.
    pub hidden: bool,
}

impl ElementData {
    fn new(tag: String) -> Self {
        Self {
            tag,
            id: None,
            classes: Vec::new(),
            inline_style: Vec::new(),
            attrs: Vec::new(),
            hidden: false,
        }
    }

    /// Raw attribute lookup (`data-bind` etc.), excluding the special-cased
    /// `id`/`class`/`style`/`hidden`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

pub(crate) enum NodeKind {
    Element(ElementData),
    Text(String),
}

pub(crate) struct Node {
    pub kind: NodeKind,
    pub children: Vec<usize>,
}

pub(crate) struct Document {
    pub nodes: Vec<Node>,
    /// Concatenated `<style>` block contents.
    pub css: String,
    /// Source of the first `<script>` block, if any.
    pub script: Option<String>,
    /// Set by every DOM mutation; cleared after a repaint. Starts `true` so
    /// the first tick paints the initial picture.
    pub dirty: bool,
}

/// Cuts every `<script>…</script>` block out of the source before the HTML
/// parse and returns the first block's contents.
///
/// `tl` does not treat script contents as raw text: a comparison like
/// `dist < 0` starts what it takes for a tag and the code behind it is
/// mangled. Scripts are opaque to the DOM anyway, so a plain string scan is
/// the robust extraction.
fn extract_scripts(source: &str) -> (String, Option<String>) {
    let lower = source.to_ascii_lowercase();
    let (mut html, mut script) = (String::with_capacity(source.len()), None);
    let mut at = 0;
    while let Some(open) = lower[at..].find("<script") {
        let open = at + open;
        html.push_str(&source[at..open]);
        // End of the opening tag; a file that never closes it ends the scan.
        let Some(body) = lower[open..].find('>').map(|i| open + i + 1) else {
            at = source.len();
            break;
        };
        let end = lower[body..]
            .find("</script")
            .map(|i| body + i)
            .unwrap_or(source.len());
        if script.is_none() {
            script = Some(source[body..end].to_string());
        }
        at = lower[end..]
            .find('>')
            .map(|i| end + i + 1)
            .unwrap_or(source.len());
    }
    html.push_str(&source[at..]);
    (html, script)
}

/// Parses the source into an owned document. Lenient: unknown tags become
/// plain elements, malformed constructs are dropped silently.
pub(crate) fn parse(source: &str) -> Result<Document, String> {
    let (source, script) = extract_scripts(source);
    let vdom = tl::parse(&source, tl::ParserOptions::default())
        .map_err(|e| format!("HTML parse failed: {e}"))?;
    let parser = vdom.parser();
    let mut doc = Document {
        nodes: vec![Node {
            kind: NodeKind::Element(ElementData::new("#root".to_string())),
            children: Vec::new(),
        }],
        css: String::new(),
        script,
        dirty: true,
    };
    let mut top = Vec::new();
    for handle in vdom.children() {
        if let Some(node) = handle.get(parser)
            && let Some(idx) = build_node(&mut doc, parser, node)
        {
            top.push(idx);
        }
    }
    doc.nodes[ROOT].children = top;
    Ok(doc)
}

fn build_node(doc: &mut Document, parser: &tl::Parser, node: &tl::Node) -> Option<usize> {
    match node {
        tl::Node::Tag(tag) => {
            let name = tag.name().as_utf8_str().to_ascii_lowercase();
            if name == "style" {
                doc.css.push_str(&tag.inner_text(parser));
                doc.css.push('\n');
                return None;
            }
            // Scripts were cut out before the parse; a stray tag that
            // survived (e.g. inside malformed markup) is dropped here.
            if name == "script" {
                return None;
            }
            let mut data = ElementData::new(name);
            for (key, value) in tag.attributes().iter() {
                let key = key.to_ascii_lowercase();
                let value = value.map(|v| decode_entities(&v)).unwrap_or_default();
                match key.as_str() {
                    "id" => data.id = Some(value),
                    "class" => {
                        data.classes = value.split_whitespace().map(str::to_owned).collect();
                    }
                    "style" => data.inline_style = parse_inline_style(&value),
                    "hidden" => data.hidden = true,
                    _ => data.attrs.push((key, value)),
                }
            }
            let idx = doc.nodes.len();
            doc.nodes.push(Node {
                kind: NodeKind::Element(data),
                children: Vec::new(),
            });
            let mut children = Vec::new();
            for handle in tag.children().top().iter() {
                if let Some(child) = handle.get(parser)
                    && let Some(c) = build_node(doc, parser, child)
                {
                    children.push(c);
                }
            }
            doc.nodes[idx].children = children;
            Some(idx)
        }
        tl::Node::Raw(bytes) => {
            let text = normalize_whitespace(&decode_entities(&bytes.as_utf8_str()));
            if text.is_empty() {
                return None;
            }
            let idx = doc.nodes.len();
            doc.nodes.push(Node {
                kind: NodeKind::Text(truncate_text(&text)),
                children: Vec::new(),
            });
            Some(idx)
        }
        tl::Node::Comment(_) => None,
    }
}

/// Splits a `style=` string (or a rule body) into `(name, value)` pairs:
/// declarations on `;`, name/value on the first `:`. Names are lowercased,
/// later declarations of the same name win.
pub(crate) fn parse_inline_style(s: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for decl in s.split(';') {
        if let Some((name, value)) = decl.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_owned();
            if name.is_empty() || value.is_empty() {
                continue;
            }
            if let Some(slot) = out.iter_mut().find(|(n, _)| *n == name) {
                slot.1 = value;
            } else {
                out.push((name, value));
            }
        }
    }
    out
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let (rep, len) = if rest.starts_with("&amp;") {
            ("&", 5)
        } else if rest.starts_with("&lt;") {
            ("<", 4)
        } else if rest.starts_with("&gt;") {
            (">", 4)
        } else if rest.starts_with("&quot;") {
            ("\"", 6)
        } else if rest.starts_with("&#39;") {
            ("'", 5)
        } else if rest.starts_with("&apos;") {
            ("'", 6)
        } else if rest.starts_with("&nbsp;") {
            (" ", 6)
        } else {
            ("&", 1)
        };
        out.push_str(rep);
        rest = &rest[len..];
    }
    out.push_str(rest);
    out
}

fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

pub(crate) fn truncate_text(s: &str) -> String {
    if s.len() <= MAX_TEXT_LEN || s.chars().count() <= MAX_TEXT_LEN {
        s.to_owned()
    } else {
        s.chars().take(MAX_TEXT_LEN).collect()
    }
}

impl Document {
    pub fn element(&self, idx: usize) -> Option<&ElementData> {
        match &self.nodes.get(idx)?.kind {
            NodeKind::Element(el) => Some(el),
            NodeKind::Text(_) => None,
        }
    }

    fn element_mut(&mut self, idx: usize) -> Option<&mut ElementData> {
        match &mut self.nodes.get_mut(idx)?.kind {
            NodeKind::Element(el) => Some(el),
            NodeKind::Text(_) => None,
        }
    }

    /// First attached element with the given `id`, depth-first in document
    /// order. Orphaned nodes (replaced by `textContent`) are not found.
    pub fn find_by_id(&self, id: &str) -> Option<usize> {
        self.find_by_id_from(ROOT, id)
    }

    fn find_by_id_from(&self, idx: usize, id: &str) -> Option<usize> {
        if let NodeKind::Element(el) = &self.nodes[idx].kind
            && el.id.as_deref() == Some(id)
        {
            return Some(idx);
        }
        for &c in &self.nodes[idx].children {
            if let Some(found) = self.find_by_id_from(c, id) {
                return Some(found);
            }
        }
        None
    }

    /// Concatenated descendant text, DOM `textContent` semantics.
    pub fn text_content(&self, idx: usize) -> String {
        let mut out = String::new();
        self.append_text(idx, &mut out);
        out
    }

    fn append_text(&self, idx: usize, out: &mut String) {
        match &self.nodes[idx].kind {
            NodeKind::Text(t) => out.push_str(t),
            NodeKind::Element(_) => {
                for &c in &self.nodes[idx].children {
                    self.append_text(c, out);
                }
            }
        }
    }

    /// Replaces the children with a single text node. Returns whether the
    /// document changed; the text is truncated to [`MAX_TEXT_LEN`].
    pub fn set_text_content(&mut self, idx: usize, text: &str) -> bool {
        let text = truncate_text(text);
        if self.text_content(idx) == text {
            return false;
        }
        // Fast path: an existing single text child is rewritten in place, so
        // per-tick script updates do not grow the arena.
        if let [only] = self.nodes[idx].children[..]
            && let NodeKind::Text(t) = &mut self.nodes[only].kind
        {
            *t = text;
            self.dirty = true;
            return true;
        }
        if text.is_empty() {
            self.nodes[idx].children.clear();
        } else {
            let t = self.nodes.len();
            self.nodes.push(Node {
                kind: NodeKind::Text(text),
                children: Vec::new(),
            });
            self.nodes[idx].children = vec![t];
        }
        self.dirty = true;
        true
    }

    pub fn get_attribute(&self, idx: usize, name: &str) -> Option<String> {
        let el = self.element(idx)?;
        match name {
            "id" => el.id.clone(),
            "class" => {
                if el.classes.is_empty() {
                    None
                } else {
                    Some(el.classes.join(" "))
                }
            }
            "style" => {
                if el.inline_style.is_empty() {
                    None
                } else {
                    let mut out = String::new();
                    for (n, v) in &el.inline_style {
                        if !out.is_empty() {
                            out.push_str("; ");
                        }
                        out.push_str(n);
                        out.push_str(": ");
                        out.push_str(v);
                    }
                    Some(out)
                }
            }
            "hidden" => el.hidden.then(String::new),
            _ => el.attr(name).map(str::to_owned),
        }
    }

    /// Returns whether the document changed.
    pub fn set_attribute(&mut self, idx: usize, name: &str, value: &str) -> bool {
        let Some(el) = self.element_mut(idx) else {
            return false;
        };
        let changed = match name {
            "id" => {
                let new = Some(value.to_owned());
                if el.id == new {
                    false
                } else {
                    el.id = new;
                    true
                }
            }
            "class" => {
                let new: Vec<String> = value.split_whitespace().map(str::to_owned).collect();
                if el.classes == new {
                    false
                } else {
                    el.classes = new;
                    true
                }
            }
            "style" => {
                let new = parse_inline_style(value);
                if el.inline_style == new {
                    false
                } else {
                    el.inline_style = new;
                    true
                }
            }
            // Presence attribute: setting it to anything means hidden.
            "hidden" => {
                if el.hidden {
                    false
                } else {
                    el.hidden = true;
                    true
                }
            }
            _ => {
                if let Some(slot) = el.attrs.iter_mut().find(|(n, _)| n == name) {
                    if slot.1 == value {
                        false
                    } else {
                        slot.1 = value.to_owned();
                        true
                    }
                } else {
                    el.attrs.push((name.to_owned(), value.to_owned()));
                    true
                }
            }
        };
        if changed {
            self.dirty = true;
        }
        changed
    }

    pub fn set_hidden(&mut self, idx: usize, hidden: bool) -> bool {
        let Some(el) = self.element_mut(idx) else {
            return false;
        };
        if el.hidden == hidden {
            return false;
        }
        el.hidden = hidden;
        self.dirty = true;
        true
    }

    pub fn class_contains(&self, idx: usize, class: &str) -> bool {
        self.element(idx)
            .is_some_and(|el| el.classes.iter().any(|c| c == class))
    }

    pub fn class_add(&mut self, idx: usize, class: &str) -> bool {
        if class.is_empty() || self.class_contains(idx, class) {
            return false;
        }
        let Some(el) = self.element_mut(idx) else {
            return false;
        };
        el.classes.push(class.to_owned());
        self.dirty = true;
        true
    }

    pub fn class_remove(&mut self, idx: usize, class: &str) -> bool {
        let Some(el) = self.element_mut(idx) else {
            return false;
        };
        let before = el.classes.len();
        el.classes.retain(|c| c != class);
        if el.classes.len() == before {
            return false;
        }
        self.dirty = true;
        true
    }

    /// Returns the new presence state, DOM `classList.toggle` semantics.
    pub fn class_toggle(&mut self, idx: usize, class: &str) -> bool {
        if self.class_contains(idx, class) {
            self.class_remove(idx, class);
            false
        } else {
            self.class_add(idx, class);
            true
        }
    }

    /// `style.setProperty`: upserts an inline declaration; an empty value
    /// removes it. Returns whether the document changed.
    pub fn style_set_property(&mut self, idx: usize, name: &str, value: &str) -> bool {
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        if name.is_empty() {
            return false;
        }
        let Some(el) = self.element_mut(idx) else {
            return false;
        };
        let changed = if value.is_empty() {
            let before = el.inline_style.len();
            el.inline_style.retain(|(n, _)| *n != name);
            el.inline_style.len() != before
        } else if let Some(slot) = el.inline_style.iter_mut().find(|(n, _)| *n == name) {
            if slot.1 == value {
                false
            } else {
                slot.1 = value.to_owned();
                true
            }
        } else {
            el.inline_style.push((name, value.to_owned()));
            true
        };
        if changed {
            self.dirty = true;
        }
        changed
    }
}

/// Value of a flat sim field name: `time`, any entry of `numbers`, or
/// `lamp.<name>` mapped to 1/0.
pub(crate) fn lookup_field(frame: &SimFrame, name: &str) -> Option<f64> {
    if name == "time" {
        return Some(frame.time);
    }
    if let Some((_, v)) = frame.numbers.iter().find(|(n, _)| n == name) {
        return Some(*v);
    }
    if let Some(lamp) = name.strip_prefix("lamp.")
        && let Some((_, lit)) = frame.lamps.iter().find(|(n, _)| n == lamp)
    {
        return Some(if *lit { 1.0 } else { 0.0 });
    }
    None
}

/// Applies `data-bind`/`data-show` for one tick. Sets the dirty flag only
/// through the mutators, which compare before writing — an unchanged value
/// causes no relayout.
pub(crate) fn apply_bindings(doc: &mut Document, frame: &SimFrame) {
    for idx in 0..doc.nodes.len() {
        let Some(el) = doc.element(idx) else { continue };
        let bind = el.attr("data-bind").map(str::to_owned);
        let format = el.attr("data-format").map(str::to_owned);
        let show = el.attr("data-show").map(str::to_owned);
        if let Some(field) = bind
            && let Some(value) = lookup_field(frame, &field)
        {
            let text = format_value(format.as_deref(), value);
            doc.set_text_content(idx, &text);
        }
        if let Some(field) = show
            && let Some(value) = lookup_field(frame, &field)
        {
            doc.set_hidden(idx, value == 0.0);
        }
    }
}

/// printf subset for `data-format`: `%d`, `%s`, `%.Nf` and `%%`; literal
/// text around the specifier is kept. No format → shortest number form.
pub(crate) fn format_value(fmt: Option<&str>, v: f64) -> String {
    let Some(fmt) = fmt else {
        return default_number(v);
    };
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('%') => {
                chars.next();
                out.push('%');
            }
            Some('d') => {
                chars.next();
                out.push_str(&format!("{}", v.trunc() as i64));
            }
            Some('s') => {
                chars.next();
                out.push_str(&default_number(v));
            }
            Some('.') => {
                // Try `%.Nf`; on anything else the '%' stays literal.
                let mut probe = chars.clone();
                probe.next();
                let mut digits = String::new();
                while let Some(d) = probe.peek().filter(|d| d.is_ascii_digit()) {
                    digits.push(*d);
                    probe.next();
                }
                if !digits.is_empty() && probe.peek() == Some(&'f') {
                    probe.next();
                    chars = probe;
                    let precision: usize = digits.parse().unwrap_or(0);
                    out.push_str(&format!("{v:.precision$}"));
                } else {
                    out.push('%');
                }
            }
            _ => out.push('%'),
        }
    }
    out
}

fn default_number(v: f64) -> String {
    if v.is_finite() && v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}
