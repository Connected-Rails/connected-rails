//! Node canvas of the vehicle: the block diagram (`sim_core::blocks`) drawn with
//! egui-snarl — pan/zoom canvas in the centre, palette and inspector in the data panel.
//!
//! The canvas mirrors `spec.graph`. Every edit (wire, block, parameter) goes into the
//! spec, so the existing snapshot undo and the RON save cover the graph for free; the
//! snarl widget is rebuilt from the spec whenever `Editor::graph_sync` is set.

use bevy_egui::egui;
use editor_ui::{colors, space};
use egui_snarl::ui::{
    BackgroundPattern, PinInfo, SnarlStyle, get_selected_nodes, set_selected_nodes,
};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};
use i18n::t;
use sim_core::blocks::{
    BakeIssue, BlockCategory, BlockDef, GraphBlock, GraphWire, ParamKind, ParamValue, PortDef,
    PortDomain, Registry, Severity, VehicleGraph, bake, parse_mod_block,
};
use sim_core::drive::{Circuit, CircuitKind};

use crate::ui::row;
use crate::{Editor, Status};

/// Built-in palette plus every preset below `mods/<id>/blocks/`. Broken presets are
/// skipped — the editor must open either way; the simulator warns about them itself.
pub fn load_registry() -> Registry {
    let mut registry = Registry::builtin();
    let Ok(mods) = std::fs::read_dir(crate::mods_dir()) else {
        return registry;
    };
    for dir in mods.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let Some(mod_id) = dir.file_name().and_then(|n| n.to_str()).map(String::from) else {
            continue;
        };
        let Ok(files) = std::fs::read_dir(dir.join("blocks")) else {
            continue;
        };
        let mut files: Vec<_> = files
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "ron"))
            .collect();
        files.sort();
        for file in files {
            let _ = std::fs::read_to_string(&file)
                .map_err(|e| e.to_string())
                .and_then(|t| parse_mod_block(&t).map_err(|e| e.to_string()))
                .and_then(|def| registry.add_mod_block(&mod_id, def));
        }
    }
    registry
}

/// Display name of a block definition.
pub fn block_name(def: &BlockDef) -> String {
    if def.name.is_empty() {
        t!(&def.name_key)
    } else {
        def.name.clone()
    }
}

/// Tooltip of a block definition.
fn block_hint(def: &BlockDef) -> Option<String> {
    if def.description.is_empty() {
        i18n::maybe(&format!("{}-hint", def.name_key))
    } else {
        Some(def.description.clone())
    }
}

/// Wire and pin colour of a domain — one colour per physical medium, as in the HUD of
/// every node editor: what may be joined shares a colour.
pub fn domain_color(domain: PortDomain) -> egui::Color32 {
    match domain {
        PortDomain::Mechanical => egui::Color32::from_rgb(0xE8, 0xA3, 0x3D),
        PortDomain::Force => egui::Color32::from_rgb(0xC9, 0xCE, 0xD6),
        PortDomain::Electrical => egui::Color32::from_rgb(0x7B, 0xC9, 0x6A),
        PortDomain::Pneumatic => egui::Color32::from_rgb(0x4D, 0xC3, 0xE6),
        PortDomain::Signal => egui::Color32::from_rgb(0xE0, 0x5C, 0x5C),
        PortDomain::Fuel => egui::Color32::from_rgb(0xC9, 0xA2, 0x27),
    }
}

/// Pin drawing: colour and shape both follow the domain, so the coding survives
/// colour-blindness and screenshots in grey.
fn domain_pin(domain: PortDomain) -> PinInfo {
    let info = match domain {
        PortDomain::Mechanical | PortDomain::Pneumatic => PinInfo::circle(),
        PortDomain::Force | PortDomain::Signal => PinInfo::square(),
        PortDomain::Electrical | PortDomain::Fuel => PinInfo::triangle(),
    };
    info.with_fill(domain_color(domain))
}

/// Rebuilds the snarl widget from `spec.graph`. Pan/zoom and selection live in egui
/// memory and survive the rebuild.
pub fn rebuild(editor: &mut Editor) {
    let mut snarl: Snarl<u32> = Snarl::new();
    if let Some(graph) = &editor.spec.graph {
        let mut nodes = std::collections::BTreeMap::new();
        for block in &graph.blocks {
            let id = snarl.insert_node(egui::pos2(block.pos.0, block.pos.1), block.id);
            nodes.insert(block.id, id);
        }
        for wire in &graph.wires {
            let Some(ends) = wire_pins(graph, &editor.registry, wire, &nodes) else {
                continue;
            };
            snarl.connect(ends.0, ends.1);
        }
    }
    editor.snarl = snarl;
    editor.graph_sync = false;
}

/// Pin pair of a stored wire, `None` when a port no longer exists (unknown mod block).
fn wire_pins(
    graph: &VehicleGraph,
    registry: &Registry,
    wire: &GraphWire,
    nodes: &std::collections::BTreeMap<u32, NodeId>,
) -> Option<(OutPinId, InPinId)> {
    let from = graph.block(wire.from)?;
    let to = graph.block(wire.to)?;
    let output = registry
        .get(&from.kind)?
        .outputs
        .iter()
        .position(|p| p.id == wire.from_port)?;
    let input = registry
        .get(&to.kind)?
        .inputs
        .iter()
        .position(|p| p.id == wire.to_port)?;
    Some((
        OutPinId {
            node: *nodes.get(&from.id)?,
            output,
        },
        InPinId {
            node: *nodes.get(&to.id)?,
            input,
        },
    ))
}

/// The canvas in the centre of the editor.
pub fn canvas(ui: &mut egui::Ui, editor: &mut Editor) {
    if editor.graph_sync {
        rebuild(editor);
    }
    let mut style = SnarlStyle::new();
    style.bg_pattern = Some(BackgroundPattern::Grid(egui_snarl::ui::Grid::new(
        egui::vec2(48.0, 48.0),
        0.0,
    )));
    style.min_scale = Some(0.3);

    let mut changed = false;
    editor.node_rects.clear();
    {
        let selected = editor.selected_block;
        let Editor {
            spec,
            registry,
            snarl,
            status,
            canvas_transform,
            node_rects,
            ..
        } = editor;
        let Some(graph) = spec.graph.as_mut() else {
            return;
        };
        let mut viewer = Viewer {
            graph,
            registry,
            changed: &mut changed,
            status,
            selected,
            transform: canvas_transform,
            node_rects,
        };
        snarl.show(&mut viewer, &style, "vehicle-graph", ui);
    }

    // Positions live in the snarl while dragging; the spec follows every frame so undo
    // and save see them.
    if let Some(graph) = editor.spec.graph.as_mut() {
        for (_, pos, block_id) in editor.snarl.nodes_pos_ids() {
            if let Some(block) = graph.blocks.iter_mut().find(|b| b.id == *block_id) {
                block.pos = (pos.x, pos.y);
            }
        }
    }
    let snarl_id = ui.make_persistent_id("vehicle-graph");
    // The canvas selection (click, marquee, Ctrl+A — blueprint bindings from our
    // egui-snarl patch) drives the inspector; the last selected node is edited.
    let selection = get_selected_nodes(snarl_id, ui.ctx());
    if let Some(node) = selection.last() {
        editor.selected_block = editor.snarl.get_node(*node).copied();
        editor.selected_group = None;
    }
    changed |= groups_ui(ui, editor);
    changed |= shortcuts(ui, editor, snarl_id, &selection);
    add_menu_ui(ui, editor);
    if changed {
        editor.graph_sync = true;
    }
}

/// Canvas keyboard, blueprint-style: Ctrl+A select all, Ctrl+C/X/V clipboard,
/// Ctrl+D/W duplicate, Delete removes, C frames the selection, Shift+A opens
/// the add menu at the pointer.
fn shortcuts(
    ui: &egui::Ui,
    editor: &mut Editor,
    snarl_id: egui::Id,
    selection: &[egui_snarl::NodeId],
) -> bool {
    let ctx = ui.ctx().clone();
    // A focused text field owns the keyboard.
    if ctx.egui_wants_keyboard_input() {
        return false;
    }
    use egui::{Key, KeyboardShortcut, Modifiers};
    let consume = |m: Modifiers, k: Key| {
        ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)))
    };
    let selected: Vec<u32> = selection
        .iter()
        .filter_map(|n| editor.snarl.get_node(*n).copied())
        .collect();
    let mut changed = false;

    if consume(Modifiers::COMMAND, Key::A) {
        let all: Vec<egui_snarl::NodeId> = editor.snarl.node_ids().map(|(id, _)| id).collect();
        let _ = set_selected_nodes(snarl_id, &ctx, &all);
    }
    if consume(Modifiers::COMMAND, Key::C) && !selected.is_empty() {
        editor.clipboard = clip(editor, &selected);
    }
    if consume(Modifiers::COMMAND, Key::X) && !selected.is_empty() {
        editor.clipboard = clip(editor, &selected);
        changed |= remove_blocks(editor, &selected);
    }
    if consume(Modifiers::COMMAND, Key::V) {
        changed |= paste(editor, ui);
    }
    if (consume(Modifiers::COMMAND, Key::D) || consume(Modifiers::COMMAND, Key::W))
        && !selected.is_empty()
    {
        let saved = editor.clipboard.take();
        editor.clipboard = clip(editor, &selected);
        changed |= paste(editor, ui);
        editor.clipboard = saved;
    }
    if consume(Modifiers::NONE, Key::Delete) || consume(Modifiers::NONE, Key::Backspace) {
        if !selected.is_empty() {
            changed |= remove_blocks(editor, &selected);
        } else if let (Some(group), Some(graph)) =
            (editor.selected_group.take(), editor.spec.graph.as_mut())
        {
            graph.groups.retain(|g| g.id != group);
            changed = true;
        }
    }
    // Comment frame around the selection, as a blueprint's C key does.
    if consume(Modifiers::NONE, Key::C) && !selected.is_empty() {
        let mut bb = egui::Rect::NOTHING;
        for (id, rect) in &editor.node_rects {
            if selected.contains(id) {
                bb = bb.union(*rect);
            }
        }
        if bb.is_finite()
            && let Some(graph) = editor.spec.graph.as_mut()
        {
            let bb = bb.expand2(egui::vec2(24.0, 24.0)).translate(egui::vec2(0.0, -14.0));
            let id = graph.next_group_id();
            graph.groups.push(sim_core::blocks::GraphGroup {
                id,
                title: t!("graph-group-default"),
                color: [92, 156, 245],
                pos: (bb.min.x, bb.min.y),
                size: (bb.width(), bb.height() + 14.0),
            });
            editor.selected_group = Some(id);
            changed = true;
        }
    }
    if ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::A)) {
        editor.add_menu = ctx.pointer_latest_pos();
        editor.palette_filter.clear();
    }
    changed
}

/// Selected blocks plus the wires that run between them.
fn clip(
    editor: &Editor,
    selected: &[u32],
) -> Option<(Vec<sim_core::blocks::GraphBlock>, Vec<GraphWire>)> {
    let graph = editor.spec.graph.as_ref()?;
    let blocks: Vec<_> = graph
        .blocks
        .iter()
        .filter(|b| selected.contains(&b.id))
        .cloned()
        .collect();
    let wires: Vec<_> = graph
        .wires
        .iter()
        .filter(|w| selected.contains(&w.from) && selected.contains(&w.to))
        .cloned()
        .collect();
    (!blocks.is_empty()).then_some((blocks, wires))
}

fn remove_blocks(editor: &mut Editor, selected: &[u32]) -> bool {
    let Some(graph) = editor.spec.graph.as_mut() else {
        return false;
    };
    for id in selected {
        graph.remove_block(*id);
    }
    editor.selected_block = None;
    !selected.is_empty()
}

/// Pastes the clipboard with fresh ids, anchored at the pointer.
fn paste(editor: &mut Editor, ui: &egui::Ui) -> bool {
    let Some((blocks, wires)) = editor.clipboard.clone() else {
        return false;
    };
    let Some(graph) = editor.spec.graph.as_mut() else {
        return false;
    };
    let anchor = blocks
        .iter()
        .fold(egui::pos2(f32::MAX, f32::MAX), |a, b| {
            egui::pos2(a.x.min(b.pos.0), a.y.min(b.pos.1))
        });
    let target = ui
        .ctx()
        .pointer_latest_pos()
        .map(|p| editor.canvas_transform.inverse() * p)
        .unwrap_or_else(|| anchor + egui::vec2(32.0, 32.0));
    let mut next = graph.next_id();
    let mut ids = std::collections::BTreeMap::new();
    for block in &blocks {
        let mut copy = block.clone();
        copy.id = next;
        ids.insert(block.id, next);
        next += 1;
        copy.pos = (
            target.x + (block.pos.0 - anchor.x),
            target.y + (block.pos.1 - anchor.y),
        );
        graph.blocks.push(copy);
    }
    for wire in &wires {
        graph.wires.push(GraphWire {
            from: ids[&wire.from],
            from_port: wire.from_port.clone(),
            to: ids[&wire.to],
            to_port: wire.to_port.clone(),
        });
    }
    true
}

/// Comment frames: title bar drags the frame and everything inside it, the
/// corner handle resizes, a click puts the frame into the sidebar.
fn groups_ui(ui: &mut egui::Ui, editor: &mut Editor) -> bool {
    let mut changed = false;
    let Editor {
        spec,
        snarl,
        selected_group,
        selected_block,
        group_drag,
        ..
    } = editor;
    let Some(graph) = spec.graph.as_mut() else {
        return false;
    };
    // The interacts must live on the canvas layer itself — anywhere below it the
    // canvas' whole-area drag sense swallows every click. On that layer the
    // pointer arrives already mapped to graph space, so the stored rects and
    // drag deltas are used as they are.
    let snarl_id = ui.make_persistent_id("vehicle-graph");
    let layer = egui::LayerId::new(ui.layer_id().order, snarl_id);
    let overlay = egui::Ui::new(
        ui.ctx().clone(),
        ui.id().with("group-overlay"),
        egui::UiBuilder::new()
            .layer_id(layer)
            .max_rect(egui::Rect::EVERYTHING),
    );
    let node_of: std::collections::BTreeMap<u32, egui_snarl::NodeId> =
        snarl.node_ids().map(|(n, &b)| (b, n)).collect();
    for i in 0..graph.groups.len() {
        let group = &graph.groups[i];
        let rect = egui::Rect::from_min_size(
            egui::pos2(group.pos.0, group.pos.1),
            egui::vec2(group.size.0, group.size.1),
        );
        let title = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 26.0));
        let title_resp = overlay.interact(
            title,
            overlay.id().with(("group-title", group.id)),
            egui::Sense::click_and_drag(),
        );
        let handle =
            egui::Rect::from_min_size(rect.max - egui::vec2(16.0, 16.0), egui::vec2(16.0, 16.0));
        let handle_resp = overlay.interact(
            handle,
            overlay.id().with(("group-size", group.id)),
            egui::Sense::drag(),
        );
        if title_resp.clicked() {
            *selected_group = Some(group.id);
            *selected_block = None;
        }
        if title_resp.drag_started() {
            // What the frame carries is decided when the drag starts.
            *group_drag = Some(
                graph
                    .blocks
                    .iter()
                    .filter(|b| rect.contains(egui::pos2(b.pos.0, b.pos.1)))
                    .map(|b| b.id)
                    .collect(),
            );
        }
        if title_resp.dragged() {
            let delta = title_resp.drag_delta();
            let group = &mut graph.groups[i];
            group.pos.0 += delta.x;
            group.pos.1 += delta.y;
            for id in group_drag.clone().unwrap_or_default() {
                if let Some(block) = graph.blocks.iter_mut().find(|b| b.id == id) {
                    block.pos.0 += delta.x;
                    block.pos.1 += delta.y;
                }
                if let Some(info) = node_of.get(&id).and_then(|n| snarl.get_node_info_mut(*n)) {
                    info.pos += delta;
                }
            }
            changed = true;
        }
        if title_resp.drag_stopped() {
            *group_drag = None;
        }
        if handle_resp.dragged() {
            let delta = handle_resp.drag_delta();
            let group = &mut graph.groups[i];
            group.size.0 = (group.size.0 + delta.x).max(120.0);
            group.size.1 = (group.size.1 + delta.y).max(64.0);
            changed = true;
        }
    }
    changed
}

/// Shift+A popup: search and place a block at the pointer.
fn add_menu_ui(ui: &mut egui::Ui, editor: &mut Editor) {
    let Some(pos) = editor.add_menu else {
        return;
    };
    let mut close = ui.ctx().input(|i| i.key_pressed(egui::Key::Escape));
    let area = egui::Area::new(ui.id().with("graph-add-menu"))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(240.0);
                ui.label(editor_ui::section_title(t!("graph-add-block")));
                ui.add(
                    egui::TextEdit::singleline(&mut editor.palette_filter)
                        .hint_text(t!("graph-search")),
                );
                let filter = editor.palette_filter.to_lowercase();
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    let defs: Vec<(String, String)> = editor
                        .registry
                        .defs
                        .iter()
                        .map(|d| (d.id.clone(), block_name(d)))
                        .filter(|(_, n)| filter.is_empty() || n.to_lowercase().contains(&filter))
                        .collect();
                    for (kind, name) in defs {
                        if ui.button(name).clicked() {
                            let at = editor.canvas_transform.inverse() * pos;
                            if let Some(graph) = editor.spec.graph.as_mut() {
                                add_block(graph, &editor.registry, &kind, (at.x, at.y));
                                editor.graph_sync = true;
                            }
                            close = true;
                        }
                    }
                });
            });
        });
    if area.response.clicked_elsewhere() {
        close = true;
    }
    if close {
        editor.add_menu = None;
    }
}

/// The canvas' viewer: every mutation goes into `spec.graph`; the snarl widget itself is
/// rebuilt from it on the next frame.
struct Viewer<'a> {
    graph: &'a mut VehicleGraph,
    registry: &'a Registry,
    changed: &'a mut bool,
    status: &'a mut Status,
    /// Block whose parameters the sidebar shows — its node wears the accent.
    selected: Option<u32>,
    /// Canvas transform, written back for paste/add-menu placement.
    transform: &'a mut egui::emath::TSTransform,
    /// Node rects in graph space — comment frames form around them.
    node_rects: &'a mut Vec<(u32, egui::Rect)>,
}

impl Viewer<'_> {
    fn def(&self, block_id: u32) -> Option<&BlockDef> {
        self.registry.get(&self.graph.block(block_id)?.kind)
    }

    fn input_port(&self, pin: InPinId, snarl: &Snarl<u32>) -> Option<&PortDef> {
        self.def(*snarl.get_node(pin.node)?)?.inputs.get(pin.input)
    }

    fn output_port(&self, pin: OutPinId, snarl: &Snarl<u32>) -> Option<&PortDef> {
        self.def(*snarl.get_node(pin.node)?)?.outputs.get(pin.output)
    }
}

impl egui_snarl::ui::SnarlViewer<u32> for Viewer<'_> {
    fn current_transform(
        &mut self,
        to_global: &mut egui::emath::TSTransform,
        _snarl: &mut Snarl<u32>,
    ) {
        *self.transform = *to_global;
    }

    fn final_node_rect(
        &mut self,
        node: NodeId,
        rect: egui::Rect,
        _ui: &mut egui::Ui,
        snarl: &mut Snarl<u32>,
    ) {
        if let Some(&block) = snarl.get_node(node) {
            self.node_rects.push((block, rect));
        }
    }

    fn draw_background(
        &mut self,
        background: Option<&BackgroundPattern>,
        viewport: &egui::Rect,
        snarl_style: &SnarlStyle,
        style: &egui::Style,
        painter: &egui::Painter,
        _snarl: &Snarl<u32>,
    ) {
        if let Some(background) = background {
            background.draw(viewport, snarl_style, style, painter);
        }
        // Comment frames: translucent body, stronger title bar, title text. The
        // painter works in graph space here, so the stored rects draw directly.
        for group in &self.graph.groups {
            let rect = egui::Rect::from_min_size(
                egui::pos2(group.pos.0, group.pos.1),
                egui::vec2(group.size.0, group.size.1),
            );
            let [r, g, b] = group.color;
            let fill = egui::Color32::from_rgb(r, g, b);
            painter.rect_filled(rect, 6.0, fill.gamma_multiply(0.16));
            let title = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 26.0));
            painter.rect_filled(title, 6.0, fill.gamma_multiply(0.45));
            painter.text(
                title.left_center() + egui::vec2(8.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &group.title,
                egui::FontId::proportional(14.0),
                colors::TEXT_STRONG,
            );
        }
    }

    fn node_frame(
        &mut self,
        default: egui::Frame,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<u32>,
    ) -> egui::Frame {
        // The block being edited in the sidebar wears the accent.
        if snarl.get_node(node).copied() == self.selected {
            default.stroke(egui::Stroke::new(2.0, colors::ACCENT))
        } else {
            default
        }
    }

    fn title(&mut self, block_id: &u32) -> String {
        self.def(*block_id).map(|d| block_name(d)).unwrap_or_else(|| {
            self.graph
                .block(*block_id)
                .map(|b| b.kind.clone())
                .unwrap_or_default()
        })
    }

    fn inputs(&mut self, block_id: &u32) -> usize {
        self.def(*block_id).map_or(0, |d| d.inputs.len())
    }

    fn outputs(&mut self, block_id: &u32) -> usize {
        self.def(*block_id).map_or(0, |d| d.outputs.len())
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<u32>,
    ) -> impl egui_snarl::ui::SnarlPin + 'static {
        match self.input_port(pin.id, snarl) {
            Some(port) => {
                let label = ui.label(t!(&port.key));
                label.on_hover_text(t!(port.domain.key()));
                domain_pin(port.domain)
            }
            None => PinInfo::circle(),
        }
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<u32>,
    ) -> impl egui_snarl::ui::SnarlPin + 'static {
        match self.output_port(pin.id, snarl) {
            Some(port) => {
                let label = ui.label(t!(&port.key));
                label.on_hover_text(t!(port.domain.key()));
                domain_pin(port.domain)
            }
            None => PinInfo::circle(),
        }
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<u32>) {
        let (Some(out_port), Some(in_port)) = (
            self.output_port(from.id, snarl).cloned(),
            self.input_port(to.id, snarl).cloned(),
        ) else {
            return;
        };
        // Only like domains join — the colour coding is the rule, not a hint.
        if out_port.domain != in_port.domain {
            *self.status = Status::Error(t!("graph-domain-mismatch"));
            return;
        }
        let (Some(&from_block), Some(&to_block)) =
            (snarl.get_node(from.id.node), snarl.get_node(to.id.node))
        else {
            return;
        };
        let wire = GraphWire {
            from: from_block,
            from_port: out_port.id,
            to: to_block,
            to_port: in_port.id,
        };
        if !self.graph.wires.contains(&wire) {
            self.graph.wires.push(wire);
            *self.changed = true;
        }
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<u32>) {
        let (Some(out_port), Some(in_port)) = (
            self.output_port(from.id, snarl).cloned(),
            self.input_port(to.id, snarl).cloned(),
        ) else {
            return;
        };
        let (Some(&from_block), Some(&to_block)) =
            (snarl.get_node(from.id.node), snarl.get_node(to.id.node))
        else {
            return;
        };
        self.graph.wires.retain(|w| {
            !(w.from == from_block
                && w.from_port == out_port.id
                && w.to == to_block
                && w.to_port == in_port.id)
        });
        *self.changed = true;
    }

    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<u32>) {
        let (Some(port), Some(&block)) = (
            self.output_port(pin.id, snarl).cloned(),
            snarl.get_node(pin.id.node),
        ) else {
            return;
        };
        self.graph
            .wires
            .retain(|w| !(w.from == block && w.from_port == port.id));
        *self.changed = true;
    }

    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<u32>) {
        let (Some(port), Some(&block)) = (
            self.input_port(pin.id, snarl).cloned(),
            snarl.get_node(pin.id.node),
        ) else {
            return;
        };
        self.graph
            .wires
            .retain(|w| !(w.to == block && w.to_port == port.id));
        *self.changed = true;
    }

    fn has_graph_menu(&mut self, _pos: egui::Pos2, _snarl: &mut Snarl<u32>) -> bool {
        true
    }

    fn show_graph_menu(&mut self, pos: egui::Pos2, ui: &mut egui::Ui, _snarl: &mut Snarl<u32>) {
        ui.label(editor_ui::section_title(t!("graph-add-block")));
        for category in BlockCategory::ALL {
            let defs: Vec<&BlockDef> = self
                .registry
                .defs
                .iter()
                .filter(|d| d.category == category)
                .collect();
            if defs.is_empty() {
                continue;
            }
            ui.menu_button(t!(category.key()), |ui| {
                for def in defs {
                    if ui.button(block_name(def)).clicked() {
                        add_block(self.graph, self.registry, &def.id.clone(), (pos.x, pos.y));
                        *self.changed = true;
                        ui.close();
                    }
                }
            });
        }
    }

    fn has_node_menu(&mut self, _block_id: &u32) -> bool {
        true
    }

    fn show_node_menu(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<u32>,
    ) {
        let Some(&block_id) = snarl.get_node(node) else {
            return;
        };
        if ui.button(t!("graph-remove-block")).clicked() {
            self.graph.remove_block(block_id);
            *self.changed = true;
            ui.close();
        }
    }
}

/// Adds a block of `kind`, placed at `pos` on the canvas.
fn add_block(graph: &mut VehicleGraph, registry: &Registry, kind: &str, pos: (f32, f32)) {
    if let Some(block) = registry.instantiate(kind, graph.next_id(), pos) {
        graph.blocks.push(block);
    }
}

// ---------------------------------------------------------------------------
// Data panel content: palette, inspector, findings
// ---------------------------------------------------------------------------

/// The data panel while the canvas is shown: block palette, the selected block's
/// parameters, and what baking has to say about the graph.
pub fn side_panel(ui: &mut egui::Ui, editor: &mut Editor) {
    // Palette: search plus one collapsible per category. A click drops the block near
    // the last one, the canvas menu (right click) places it exactly.
    editor_ui::section(ui, "palette", t!("graph-palette"), |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut editor.palette_filter)
                .hint_text(t!("graph-search"))
                .desired_width(f32::INFINITY),
        );
        ui.add_space(space::XS);
        let filter = editor.palette_filter.to_lowercase();
        let mut added = None;
        for category in BlockCategory::ALL {
            let defs: Vec<&BlockDef> = editor
                .registry
                .defs
                .iter()
                .filter(|d| d.category == category)
                .filter(|d| filter.is_empty() || block_name(d).to_lowercase().contains(&filter))
                .collect();
            if defs.is_empty() {
                continue;
            }
            ui.label(editor_ui::section_title(t!(category.key())));
            ui.horizontal_wrapped(|ui| {
                for def in defs {
                    let button = ui.add(egui::Button::new(block_name(def)).small());
                    let button = match block_hint(def) {
                        Some(hint) => button.on_hover_text(hint),
                        None => button,
                    };
                    if button.clicked() {
                        added = Some(def.id.clone());
                    }
                }
            });
            ui.add_space(space::XS);
        }
        if let (Some(kind), Some(graph)) = (added, editor.spec.graph.as_mut()) {
            // Below the lowest block, so new blocks never land on top of the diagram.
            let y = graph
                .blocks
                .iter()
                .map(|b| b.pos.1 as i32)
                .max()
                .unwrap_or(0) as f32;
            add_block(graph, &editor.registry, &kind, (0.0, y + 170.0));
            editor.graph_sync = true;
        }
    });

    // Inspector of the selected block.
    editor_ui::section(ui, "inspector", t!("graph-inspector"), |ui| {
        inspector(ui, editor);
    });

    // Baking findings — refreshed every frame; the graph is small and the editor should
    // complain while the user is still wiring, not at save time.
    let issues = match &editor.spec.graph {
        Some(graph) => {
            let mut scratch = editor.spec.clone();
            bake(graph, &editor.registry, &mut scratch)
        }
        None => Vec::new(),
    };
    editor.bake_issues = issues;
    if !editor.bake_issues.is_empty() {
        editor_ui::section(ui, "issues", t!("graph-issues"), |ui| {
            let issues = editor.bake_issues.clone();
            for issue in &issues {
                issue_row(ui, editor, issue);
            }
        });
    }
}

fn issue_row(ui: &mut egui::Ui, editor: &mut Editor, issue: &BakeIssue) {
    let color = match issue.severity {
        Severity::Error => colors::ERROR,
        Severity::Warning => colors::WARN,
    };
    let block_name = issue
        .block
        .and_then(|id| editor.spec.graph.as_ref()?.block(id))
        .and_then(|b| editor.registry.get(&b.kind))
        .map(|d| block_name(d));
    let text = match block_name {
        Some(name) => format!("{name}: {}", t!(issue.key)),
        None => t!(issue.key),
    };
    let label = ui.add(
        egui::Label::new(egui::RichText::new(text).small().color(color))
            .sense(egui::Sense::click()),
    );
    if label.clicked() {
        editor.selected_block = issue.block;
    }
}

/// Parameters of the selected block, one labelled row each — the same form language as
/// the rest of the editor.
fn inspector(ui: &mut egui::Ui, editor: &mut Editor) {
    // A selected comment frame: title, colour, removal.
    if let Some(group_id) = editor.selected_group {
        let Some(graph) = editor.spec.graph.as_mut() else {
            return;
        };
        let Some(group) = graph.groups.iter_mut().find(|g| g.id == group_id) else {
            editor.selected_group = None;
            return;
        };
        ui.label(editor_ui::section_title(t!("graph-group")));
        editor_ui::form_grid("group").show(ui, |ui| {
            row(ui, "graph-group-name", |ui| {
                ui.add(egui::TextEdit::singleline(&mut group.title).desired_width(space::FIELD));
            });
            row(ui, "graph-group-color", |ui| {
                ui.color_edit_button_srgb(&mut group.color);
            });
        });
        if ui.button(t!("graph-group-remove")).clicked() {
            graph.groups.retain(|g| g.id != group_id);
            editor.selected_group = None;
        }
        return;
    }
    let Some(block_id) = editor.selected_block else {
        ui.label(
            egui::RichText::new(t!("graph-no-selection"))
                .small()
                .color(colors::TEXT_SECONDARY),
        );
        return;
    };
    let Editor { spec, registry, .. } = editor;
    let Some(graph) = spec.graph.as_mut() else {
        return;
    };
    let Some(block) = graph.blocks.iter_mut().find(|b| b.id == block_id) else {
        return;
    };
    let Some(def) = registry.get(&block.kind) else {
        return;
    };
    let base_kind = registry.base_kind(&block.kind).unwrap_or("").to_string();

    ui.label(editor_ui::section_title(block_name(def)));
    if let Some(hint) = block_hint(def) {
        ui.label(
            egui::RichText::new(hint)
                .small()
                .color(colors::TEXT_SECONDARY),
        );
    }
    ui.add_space(space::XS);
    if def.params.is_empty() {
        ui.label(
            egui::RichText::new(t!("graph-no-params"))
                .small()
                .color(colors::TEXT_SECONDARY),
        );
        return;
    }

    let params = def.params.clone();
    editor_ui::form_grid("block-params").show(ui, |ui| {
        for param in &params {
            if !param_visible(&base_kind, block, &param.id) {
                continue;
            }
            let value = block
                .params
                .entry(param.id.clone())
                .or_insert_with(|| param.default.clone());
            param_row(ui, block_id, param, value);
        }
    });
    // The curve and circuit editors want the full panel width, not a grid cell.
    for param in &params {
        if !param_visible(&base_kind, block, &param.id) {
            continue;
        }
        let value = block
            .params
            .entry(param.id.clone())
            .or_insert_with(|| param.default.clone());
        match (&param.kind, value) {
            (ParamKind::Curve { x_unit, y_unit }, ParamValue::Curve(points)) => {
                editor_ui::subheading(ui, t!(&param.key));
                let spec = editor_ui::CurveSpec {
                    id: egui::Id::new(("block-curve", block_id, param.id.as_str())),
                    title: t!(&param.key),
                    x_unit: static_unit(x_unit),
                    y_unit: static_unit(y_unit),
                    x_speed: 1.0,
                    y_speed: 100.0,
                    x_range: 0.0..=3000.0,
                    y_range: 0.0..=2_000_000.0,
                };
                editor_ui::curve_editor(ui, &spec, points);
            }
            (ParamKind::Circuits, ParamValue::Circuits(circuits)) => {
                editor_ui::subheading(ui, t!(&param.key));
                circuits_form(ui, block_id, circuits);
            }
            _ => {}
        }
    }
}

/// Rules for parameters that only make sense in a given setting — the diesel map behind
/// its switch, the changeover figures behind the changeover, the µ table behind "custom".
fn param_visible(base_kind: &str, block: &GraphBlock, param: &str) -> bool {
    let choice_is = |id: &str, value: &str| {
        block
            .params
            .get(id)
            .is_some_and(|v| v.choice() == value)
    };
    match (base_kind, param) {
        ("diesel-engine", "governor_steps" | "governor_droop") => {
            block.params.get("engine_map").is_some_and(|v| v.flag())
                && choice_is("governor", "speed")
        }
        (
            "diesel-engine",
            "idle_rpm" | "rated_rpm" | "max_rpm" | "torque_curve" | "governor" | "inertia"
            | "response_time",
        ) => block.params.get("engine_map").is_some_and(|v| v.flag()),
        ("control-valve", "empty_share" | "changeover_mass") => {
            choice_is("load_braking", "changeover")
        }
        ("brake-rigging", "friction_curve") => choice_is("kind", "custom"),
        _ => true,
    }
}

/// One grid row for a scalar parameter. Curves and circuits draw below the grid.
fn param_row(ui: &mut egui::Ui, block_id: u32, param: &sim_core::blocks::ParamDef, value: &mut ParamValue) {
    match (&param.kind, value) {
        (
            ParamKind::Number {
                min,
                max,
                speed,
                unit,
            },
            ParamValue::Number(v),
        ) => {
            row(ui, &param.key, |ui| {
                let mut drag = egui::DragValue::new(v).speed(*speed).range(*min..=*max);
                if !unit.is_empty() {
                    drag = drag.suffix(format!("\u{a0}{unit}"));
                }
                ui.spacing_mut().interact_size.x = space::FIELD;
                ui.add(drag);
            });
        }
        (ParamKind::Bool, ParamValue::Bool(v)) => {
            row(ui, &param.key, |ui| {
                ui.checkbox(v, "");
            });
        }
        (ParamKind::Choice(options), ParamValue::Choice(v)) => {
            row(ui, &param.key, |ui| {
                egui::ComboBox::from_id_salt(("block-choice", block_id, param.id.as_str()))
                    .width(space::FIELD)
                    .selected_text(option_label(&param.key, v))
                    .show_ui(ui, |ui| {
                        for option in options {
                            ui.selectable_value(v, option.clone(), option_label(&param.key, option));
                        }
                    });
            });
        }
        (ParamKind::Text, ParamValue::Text(v)) => {
            row(ui, &param.key, |ui| {
                ui.add(
                    egui::TextEdit::singleline(v)
                        .hint_text(t!("field-script-hint"))
                        .desired_width(space::FIELD),
                );
            });
        }
        (ParamKind::List, ParamValue::List(values)) => {
            row(ui, &param.key, |ui| {
                let mut text = values
                    .iter()
                    .map(|v| format!("{v}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                if ui
                    .add(egui::TextEdit::singleline(&mut text).desired_width(space::FIELD))
                    .changed()
                {
                    *values = text
                        .split(',')
                        .filter_map(|p| p.trim().parse::<f64>().ok())
                        .collect();
                }
            });
        }
        // Drawn below the grid, full width.
        (ParamKind::Curve { .. } | ParamKind::Circuits, _) => {}
        // Type mismatch (edited file): show nothing rather than lie.
        _ => {}
    }
}

/// The hydraulic circuits of a transmission — the one structured parameter.
fn circuits_form(ui: &mut egui::Ui, block_id: u32, circuits: &mut Vec<Circuit>) {
    let mut remove = None;
    for (i, circuit) in circuits.iter_mut().enumerate() {
        editor_ui::form_grid(&format!("circuit-{i}")).show(ui, |ui| {
            row(ui, "cir-kind", |ui| {
                egui::ComboBox::from_id_salt(("circuit-kind", block_id, i))
                    .width(space::FIELD)
                    .selected_text(match circuit.kind {
                        CircuitKind::Converter => t!("cir-kind-converter"),
                        CircuitKind::Coupling => t!("cir-kind-coupling"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut circuit.kind,
                            CircuitKind::Converter,
                            t!("cir-kind-converter"),
                        );
                        ui.selectable_value(
                            &mut circuit.kind,
                            CircuitKind::Coupling,
                            t!("cir-kind-coupling"),
                        );
                    });
            });
            row(ui, "cir-ratio", |ui| {
                editor_ui::field(ui, &mut circuit.ratio, 0.01, 0.1..=20.0, "");
            });
            if circuit.kind == CircuitKind::Converter {
                row(ui, "cir-stall", |ui| {
                    editor_ui::field(ui, &mut circuit.stall_ratio, 0.01, 1.0..=6.0, "");
                });
                row(ui, "cir-coupling-point", |ui| {
                    editor_ui::field(ui, &mut circuit.coupling_nu, 0.01, 0.05..=1.5, "");
                });
            }
            row(ui, "cir-absorption", |ui| {
                editor_ui::field(ui, &mut circuit.absorption, 0.01, 0.0..=10.0, "");
            });
            row(ui, "cir-absorption-slope", |ui| {
                editor_ui::field(ui, &mut circuit.absorption_slope, 0.01, -2.0..=2.0, "");
            });
            row(ui, "cir-shift-up", |ui| {
                editor_ui::field(ui, &mut circuit.shift_up_kmh, 0.5, 0.0..=300.0, "km/h");
            });
            row(ui, "cir-shift-primary", |ui| {
                editor_ui::field(ui, &mut circuit.shift_primary_kmh, 0.5, 0.0..=100.0, "km/h");
            });
        });
        if ui.button(t!("graph-circuit-remove")).clicked() {
            remove = Some(i);
        }
        ui.add_space(space::S);
    }
    if let Some(i) = remove {
        circuits.remove(i);
    }
    if circuits.len() < 4 && ui.button(t!("graph-circuit-add")).clicked() {
        circuits.push(Circuit {
            kind: CircuitKind::Converter,
            ratio: 2.0,
            stall_ratio: 2.5,
            coupling_nu: 0.85,
            absorption: 0.5,
            absorption_slope: 0.0,
            shift_up_kmh: 60.0,
            shift_primary_kmh: 0.0,
        });
    }
}

/// Label of a choice option: type designations (`KE-GPR`, `PZB 90 V2.0`) are names and
/// stay literal; prose options translate via `<param key>-<option>`.
fn option_label(param_key: &str, option: &str) -> String {
    let literal = match option {
        "k-gp" => "K-GP",
        "ke-gp" => "KE-GP",
        "ke-gpr" => "KE-GPR",
        "ke-tm" => "KE-Tm",
        "ke-l2a" => "KE-L2a",
        "ke-l2d" => "KE-L2d",
        "g" => "G",
        "p" => "P",
        "r" => "R",
        "i54" => "I 54",
        "i60" => "I 60",
        "i60m" => "I 60M",
        "i60r" => "I 60R",
        "pzb60" => "PZB 60",
        "pzb90-v15" => "PZB 90 V1.5",
        "pzb90-v20" => "PZB 90 V2.0",
        "o" => "O",
        "m" => "M",
        "u" => "U",
        "tb0" => "TB0",
        "tav" => "TAV",
        "uic-wtb" => "UIC WTB",
        _ => "",
    };
    if !literal.is_empty() {
        return literal.to_string();
    }
    i18n::maybe(&format!("{param_key}-{option}")).unwrap_or_else(|| option.to_string())
}

/// The curve editor keys its axes with static strings; the parameter model stores them
/// as text. The set is closed, so a lookup covers it.
fn static_unit(unit: &str) -> &'static str {
    match unit {
        "km/h" => "km/h",
        "N" => "N",
        "1/min" => "1/min",
        "N·m" => "N·m",
        "µ" => "µ",
        _ => "",
    }
}
