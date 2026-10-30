//! The parser proper: box roles, scopes, the text-across-spaces patterns of
//! SPEC §6, then one edge per wire by the role table of SPEC §7.

use std::collections::{HashMap, HashSet};

use shawline_grid::{Code, Diagnostic, Dir, Grid, Pos, Rect};
use shawline_shape::{BoxStyle, Diagram, EndKind, Port, PortKind, Text, TextKind, Wire};

use crate::ast::{
    Arm, Definition, Edge, Endpoint, Graph, Guard, Literal, Name, Node, NodeId, NodeKind, PortDecl,
    Sheet, Switch,
};
use crate::message;

/// The empty list, which works as a literal.
const NIL: &str = "𐑯𐑦𐑤";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// Part of an `S001` overlap, or inside such a box: shape already
    /// reported it, so syntax stays quiet about everything in it.
    Skip,
    Def,
    Node,
    Switch,
    /// Inside a node box. The node gets `Y002`; its contents are ignored.
    Inner,
}

/// A definition body (`arm == None`) or one arm of a switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Scope {
    boxi: usize,
    arm: Option<usize>,
}

#[derive(Clone, Debug)]
struct ArmInfo {
    rows: (usize, usize),
    params: Vec<Pos>,
    results: Vec<Pos>,
    /// The left end of the line above the arm, where a missing guard is
    /// reported (`Y008`).
    head: Pos,
}

#[derive(Clone, Debug)]
struct SwitchInfo {
    selector: Option<Pos>,
    arms: Vec<ArmInfo>,
}

/// What a wire meeting a box port means, by the port's box and place.
#[derive(Clone, Copy, Debug)]
enum PortRole {
    DefIn(usize, u32),
    DefOut(usize, u32),
    NodeIn(usize, u32),
    NodeOut(usize, u32),
    Selector(usize),
    ThroughIn(usize, usize, u32),
    ThroughOut(usize, usize, u32),
    /// A port of a skipped box: already diagnosed by shape.
    Ignored,
    /// A port no rule gives a meaning, such as a tee on a definition.
    Unknown,
}

/// A node before ids are assigned. Ids follow row-major order of top-left
/// cells, so every scope's nodes are collected and sorted first.
#[derive(Clone, Copy, Debug)]
enum Pending {
    Box(usize),
    Inline { text: usize, arrow: Pos, out: Pos },
    Const { text: usize, out: Pos },
}

/// A source found at one wire end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Plain(Endpoint),
    /// The outer side of a switch's right through-port.
    SwitchOut {
        node: NodeId,
        arm: usize,
        ordinal: u32,
    },
}

struct DefHead {
    boxi: usize,
    name: Name,
    inputs: Vec<PortDecl>,
    outputs: Vec<PortDecl>,
}

struct Parser<'a> {
    grid: &'a Grid,
    d: &'a Diagram,
    diags: Vec<Diagnostic>,
    roles: Vec<Role>,
    switches: HashMap<usize, SwitchInfo>,
    ports: HashMap<Pos, PortRole>,
    /// The box each port is on, for messages.
    port_box: HashMap<Pos, usize>,
    /// Each definition's name without its namer dot, for definitions whose
    /// title is a `·`-word. Messages name a definition by it.
    definition_names: HashMap<usize, String>,
    /// Every cell covered by a text run, to its index.
    text_at: HashMap<Pos, usize>,
    /// Open wire ends facing W, by cell.
    open_w: HashMap<Pos, usize>,
    /// Free arrowheads pointing E, by cell.
    arrow_e: HashMap<Pos, usize>,
    /// Texts accounted for by some construct; the rest are stray.
    used: Vec<bool>,
    pending: HashMap<Scope, Vec<Pending>>,
    edges: HashMap<Scope, Vec<Edge>>,
    box_node: HashMap<usize, (Scope, NodeId)>,
    /// Free arrowheads that are inline-node inputs.
    arrow_claim: HashMap<Pos, (Scope, NodeId)>,
    /// Open ends facing W that are sources by the text-across-spaces rules.
    open_claim: HashMap<Pos, (Scope, Endpoint)>,
}

/// Builds the AST from a recognized diagram and reports every `Y` diagnostic
/// of SPEC §14, sorted by position. The sheet is best-effort when there are
/// errors: unresolvable wires are left out of their scope's edges.
pub fn parse(grid: &Grid, diagram: &Diagram) -> (Sheet, Vec<Diagnostic>) {
    let mut p = Parser {
        grid,
        d: diagram,
        diags: Vec::new(),
        roles: Vec::new(),
        switches: HashMap::new(),
        ports: HashMap::new(),
        port_box: HashMap::new(),
        definition_names: HashMap::new(),
        text_at: HashMap::new(),
        open_w: HashMap::new(),
        arrow_e: HashMap::new(),
        used: vec![false; diagram.texts.len()],
        pending: HashMap::new(),
        edges: HashMap::new(),
        box_node: HashMap::new(),
        arrow_claim: HashMap::new(),
        open_claim: HashMap::new(),
    };
    p.index();
    p.classify_boxes();
    p.collect_switches();
    p.collect_ports();
    let heads = p.declare_definitions();
    p.collect_nodes();
    p.assign_ids();
    p.trace_edges();
    p.report_unfed();
    let definitions = heads
        .into_iter()
        .map(|h| Definition {
            name: h.name,
            rect: diagram.boxes[h.boxi].rect,
            inputs: h.inputs,
            outputs: h.outputs,
            body: p.graph(Scope { boxi: h.boxi, arm: None }),
        })
        .collect();
    p.report_stray();
    let mut diags = p.diags;
    diags.sort_by(|a, b| (a.at, a.code).cmp(&(b.at, b.code)));
    (Sheet { definitions }, diags)
}

fn text_rect(t: &Text) -> Rect {
    Rect { top: t.pos.row, left: t.pos.col, bottom: t.pos.row, right: t.pos.col + t.width - 1 }
}

fn last_cell(t: &Text) -> Pos {
    Pos::new(t.pos.row, t.pos.col + t.width - 1)
}

fn top_left(r: Rect) -> Pos {
    Pos::new(r.top, r.left)
}

fn name_of(t: &Text) -> Name {
    Name { text: t.content.clone(), at: t.pos }
}

/// True for the texts that can be constants or guards.
fn is_literal(t: &Text) -> bool {
    match t.kind {
        TextKind::Number | TextKind::Literal => true,
        TextKind::Word => t.content == NIL,
        TextKind::Comment => false,
    }
}

/// A literal text as it is written: a text literal keeps its guillemets.
fn literal_phrase(t: &Text) -> String {
    match t.kind {
        TextKind::Literal => format!("«{}»", t.content),
        _ => t.content.clone(),
    }
}

fn is_selector_port(p: &Port) -> bool {
    p.side == Dir::N && p.kind == PortKind::Arrow && p.dir == Dir::S
}

/// Sources that together form one switch output: the outer sides of right
/// through-ports of one switch, at one ordinal, one per arm (SPEC §7).
fn merged_switch_output(sources: &[Source]) -> Option<Source> {
    let Source::SwitchOut { node, ordinal, .. } = sources[0] else { return None };
    let mut arms = Vec::new();
    for s in sources {
        match *s {
            Source::SwitchOut { node: n, arm, ordinal: o }
                if n == node && o == ordinal && !arms.contains(&arm) =>
            {
                arms.push(arm);
            }
            _ => return None,
        }
    }
    Some(sources[0])
}

impl<'a> Parser<'a> {
    fn error(&mut self, at: Pos, code: Code, message: &str) {
        self.diags.push(Diagnostic::new(at, code, message));
    }

    fn index(&mut self) {
        let d = self.d;
        for (i, t) in d.texts.iter().enumerate() {
            for col in t.pos.col..t.pos.col + t.width {
                self.text_at.insert(Pos::new(t.pos.row, col), i);
            }
            self.used[i] = t.kind == TextKind::Comment;
        }
        for (wi, w) in d.wires.iter().enumerate() {
            for e in &w.ends {
                match (e.kind, e.facing) {
                    (EndKind::Open, Dir::W) => {
                        self.open_w.insert(e.pos, wi);
                    }
                    (EndKind::Arrow, Dir::E) => {
                        self.arrow_e.insert(e.pos, wi);
                    }
                    _ => {}
                }
            }
        }
    }

    /// The first non-space cell west of `p` on its row.
    fn west(&self, p: Pos) -> Option<Pos> {
        (0..p.col).rev().map(|c| Pos::new(p.row, c)).find(|&q| self.grid.get(q) != ' ')
    }

    /// The first non-space cell east of `p` on its row.
    fn east(&self, p: Pos) -> Option<Pos> {
        let len = self.grid.row(p.row).len();
        (p.col + 1..len).map(|c| Pos::new(p.row, c)).find(|&q| self.grid.get(q) != ' ')
    }

    /// The text starting at the first non-space cell east of `p`.
    fn text_east(&self, p: Pos) -> Option<usize> {
        let q = self.east(p)?;
        self.text_at.get(&q).copied().filter(|&i| self.d.texts[i].pos == q)
    }

    /// Top-level boxes are definitions; boxes in a definition or switch are
    /// switches (with a selector or dividers) or nodes.
    fn classify_boxes(&mut self) {
        let boxes = &self.d.boxes;
        let overlapping = |i: usize| {
            let a = boxes[i].rect;
            boxes.iter().enumerate().any(|(j, b)| {
                j != i && a.intersects(&b.rect) && !a.encloses(&b.rect) && !b.rect.encloses(&a)
            })
        };
        // Parents enclose their children strictly, so they come first in
        // row-major order and already have a role.
        for (i, b) in boxes.iter().enumerate() {
            let role = if overlapping(i) {
                Role::Skip
            } else {
                match b.parent.map(|p| self.roles[p]) {
                    None => Role::Def,
                    Some(Role::Skip) => Role::Skip,
                    Some(Role::Node | Role::Inner) => Role::Inner,
                    Some(Role::Def | Role::Switch) => {
                        if b.ports.iter().any(is_selector_port) || !b.dividers.is_empty() {
                            Role::Switch
                        } else {
                            Role::Node
                        }
                    }
                }
            };
            self.roles.push(role);
            if role == Role::Skip {
                continue;
            }
            let at = top_left(b.rect);
            if b.style == BoxStyle::Mixed {
                self.error(at, "Y015", "box mixes rounded and square corners");
            }
            if b.style == BoxStyle::Rounded && b.parent.is_some() {
                self.error(at, "Y014", "rounded box inside a definition; definitions don't nest");
            }
            // Still parsed as a definition, so the rest of the sheet checks.
            if b.style == BoxStyle::Square && b.parent.is_none() {
                self.error(at, "Y018", "square box outside any definition; definitions are rounded");
            }
        }
    }

    fn collect_switches(&mut self) {
        for (i, b) in self.d.boxes.iter().enumerate() {
            if self.roles[i] != Role::Switch {
                continue;
            }
            let r = b.rect;
            let selector = b.ports.iter().find(|p| is_selector_port(p)).map(|p| p.pos);
            let mut bounds = vec![r.top];
            bounds.extend(b.dividers.iter().map(|d| d.row));
            bounds.push(r.bottom);
            let arms = bounds
                .windows(2)
                .map(|w| {
                    let rows = (w[0] + 1, w[1]);
                    let through = |side: Dir| -> Vec<Pos> {
                        b.ports
                            .iter()
                            .filter(|p| p.side == side && p.kind == PortKind::Arrow)
                            .filter(|p| p.dir == Dir::E && (rows.0..rows.1).contains(&p.pos.row))
                            .map(|p| p.pos)
                            .collect()
                    };
                    let head = Pos::new(w[0], r.left);
                    ArmInfo { rows, params: through(Dir::W), results: through(Dir::E), head }
                })
                .collect();
            self.switches.insert(i, SwitchInfo { selector, arms });
        }
    }

    fn collect_ports(&mut self) {
        for (i, b) in self.d.boxes.iter().enumerate() {
            let role = self.roles[i];
            let (mut ins, mut outs) = (0u32, 0u32);
            for p in &b.ports {
                let arrow_e = p.kind == PortKind::Arrow && p.dir == Dir::E;
                let tee_e = p.kind == PortKind::Tee && p.dir == Dir::E;
                let pr = match role {
                    Role::Skip => PortRole::Ignored,
                    Role::Def if arrow_e && p.side == Dir::W => {
                        ins += 1;
                        PortRole::DefIn(i, ins - 1)
                    }
                    Role::Def if arrow_e && p.side == Dir::E => {
                        outs += 1;
                        PortRole::DefOut(i, outs - 1)
                    }
                    Role::Node if arrow_e && p.side == Dir::W => {
                        ins += 1;
                        PortRole::NodeIn(i, ins - 1)
                    }
                    Role::Node if tee_e && p.side == Dir::E => {
                        outs += 1;
                        PortRole::NodeOut(i, outs - 1)
                    }
                    Role::Switch => self.switch_port(i, p.pos),
                    _ => PortRole::Unknown,
                };
                self.ports.insert(p.pos, pr);
                self.port_box.insert(p.pos, i);
            }
        }
    }

    fn switch_port(&self, sw: usize, pos: Pos) -> PortRole {
        let info = &self.switches[&sw];
        if info.selector == Some(pos) {
            return PortRole::Selector(sw);
        }
        for (k, arm) in info.arms.iter().enumerate() {
            if let Some(j) = arm.params.iter().position(|&p| p == pos) {
                return PortRole::ThroughIn(sw, k, j as u32);
            }
            if let Some(j) = arm.results.iter().position(|&p| p == pos) {
                return PortRole::ThroughOut(sw, k, j as u32);
            }
        }
        PortRole::Unknown
    }

    /// The scope that a cell inside box `boxi` belongs to.
    fn scope_in(&self, boxi: usize, pos: Pos) -> Option<Scope> {
        match self.roles[boxi] {
            Role::Def => Some(Scope { boxi, arm: None }),
            Role::Switch => {
                let arms = &self.switches[&boxi].arms;
                let arm = arms.iter().position(|a| (a.rows.0..a.rows.1).contains(&pos.row))?;
                Some(Scope { boxi, arm: Some(arm) })
            }
            Role::Node => self.node_scope(boxi),
            Role::Skip | Role::Inner => None,
        }
    }

    /// The scope a node or switch box sits in.
    fn node_scope(&self, boxi: usize) -> Option<Scope> {
        let b = &self.d.boxes[boxi];
        self.scope_in(b.parent?, top_left(b.rect))
    }

    /// Titles and ports of every definition. Port names are marked used, and
    /// the wire after an input's name becomes a `Param` source.
    fn declare_definitions(&mut self) -> Vec<DefHead> {
        let d = self.d;
        let mut heads = Vec::new();
        for (boxi, b) in d.boxes.iter().enumerate() {
            if self.roles[boxi] != Role::Def {
                continue;
            }
            let name = self.definition_name(boxi);
            let scope = Scope { boxi, arm: None };
            let (mut inputs, mut outputs) = (Vec::new(), Vec::new());
            for p in &b.ports {
                let (is_input, n) = match self.ports[&p.pos] {
                    PortRole::DefIn(_, n) => (true, n),
                    PortRole::DefOut(_, n) => (false, n),
                    _ => continue,
                };
                let named = self.text_east(p.pos).filter(|&t| d.texts[t].kind == TextKind::Word);
                let name = match named {
                    Some(t) => {
                        self.used[t] = true;
                        name_of(&d.texts[t])
                    }
                    None => {
                        let port = self.port_of(self.ports[&p.pos]).expect("a definition port");
                        let message = format!("{port} has no name");
                        self.error(p.pos, "Y009", &message);
                        Name { text: String::new(), at: p.pos }
                    }
                };
                if is_input {
                    // Without a name the wire may start right after the
                    // arrowhead; it is still the input's wire.
                    let after = named.map_or(p.pos, |t| last_cell(&d.texts[t]));
                    if let Some(q) = self.east(after).filter(|q| self.open_w.contains_key(q)) {
                        self.open_claim.insert(q, (scope, Endpoint::Param(n)));
                    }
                    inputs.push(PortDecl { arrow: p.pos, name });
                } else {
                    outputs.push(PortDecl { arrow: p.pos, name });
                }
            }
            heads.push(DefHead { boxi, name, inputs, outputs });
        }
        heads
    }

    fn definition_name(&mut self, boxi: usize) -> Name {
        let d = self.d;
        let titles: Vec<usize> =
            (0..d.texts.len()).filter(|&t| d.texts[t].title_of == Some(boxi)).collect();
        for &t in &titles {
            self.used[t] = true;
        }
        let [t] = titles[..] else {
            let at = titles.first().map_or(top_left(d.boxes[boxi].rect), |&t| d.texts[t].pos);
            self.error(at, "Y001", "a definition's title must be one ·-word");
            return Name { text: String::new(), at };
        };
        let text = &d.texts[t];
        match text.content.strip_prefix('·') {
            Some(rest) if text.kind == TextKind::Word && !rest.is_empty() => {
                if rest.chars().count() == 1 || rest == NIL {
                    self.error(text.pos, "Y013", "reserved for primitives");
                }
                self.definition_names.insert(boxi, rest.to_owned());
                Name { text: rest.to_owned(), at: text.pos }
            }
            _ => {
                self.error(text.pos, "Y001", "a definition's title must be a ·-word");
                Name { text: text.content.trim_start_matches('·').to_owned(), at: text.pos }
            }
        }
    }

    /// Box nodes, constants and inline nodes, per scope.
    fn collect_nodes(&mut self) {
        let d = self.d;
        for i in 0..d.boxes.len() {
            if !matches!(self.roles[i], Role::Node | Role::Switch) {
                continue;
            }
            if let Some(s) = self.node_scope(i) {
                self.pending.entry(s).or_default().push(Pending::Box(i));
            }
        }
        for (i, t) in d.texts.iter().enumerate() {
            if self.used[i] || t.title_of.is_some() || t.divider_of.is_some() {
                continue;
            }
            let Some(owner) = t.owner else { continue };
            if matches!(self.roles[owner], Role::Node | Role::Skip | Role::Inner) {
                // Node words belong to their box; the rest is ignored.
                self.used[i] = true;
                continue;
            }
            let Some(scope) = self.scope_in(owner, t.pos) else { continue };
            let out = self.east(last_cell(t)).filter(|q| self.open_w.contains_key(q));
            let arrow = self.west(t.pos).filter(|q| self.arrow_e.contains_key(q));
            let node = match (arrow, out) {
                (Some(arrow), Some(out)) if t.kind == TextKind::Word => {
                    Pending::Inline { text: i, arrow, out }
                }
                (None, Some(out)) if is_literal(t) => Pending::Const { text: i, out },
                _ => continue,
            };
            self.used[i] = true;
            self.pending.entry(scope).or_default().push(node);
        }
    }

    fn corner_of(&self, p: Pending) -> Pos {
        match p {
            Pending::Box(b) => top_left(self.d.boxes[b].rect),
            Pending::Inline { text, .. } | Pending::Const { text, .. } => self.d.texts[text].pos,
        }
    }

    fn assign_ids(&mut self) {
        let mut pending = std::mem::take(&mut self.pending);
        for (&scope, list) in &mut pending {
            list.sort_by_key(|&p| self.corner_of(p));
            for (id, &p) in list.iter().enumerate() {
                match p {
                    Pending::Box(b) => {
                        self.box_node.insert(b, (scope, id));
                    }
                    Pending::Inline { arrow, out, .. } => {
                        self.arrow_claim.insert(arrow, (scope, id));
                        self.open_claim.insert(out, (scope, Endpoint::Output(id, 0)));
                    }
                    Pending::Const { out, .. } => {
                        self.open_claim.insert(out, (scope, Endpoint::Output(id, 0)));
                    }
                }
            }
        }
        self.pending = pending;
    }

    /// The scope a wire lies in: `Some(None)` outside every definition, and
    /// `None` inside a skipped box, where nothing is reported.
    fn wire_scope(&self, w: &Wire) -> Option<Option<Scope>> {
        let first = w.cells[0];
        let boxes = &self.d.boxes;
        let inner = (0..boxes.len())
            .filter(|&i| boxes[i].rect.interior_contains(first))
            .min_by_key(|&i| boxes[i].rect.area());
        match inner {
            None => Some(None),
            Some(b) if matches!(self.roles[b], Role::Skip | Role::Inner) => None,
            Some(b) => Some(self.scope_in(b, first)),
        }
    }

    fn trace_edges(&mut self) {
        let d = self.d;
        for (wi, w) in d.wires.iter().enumerate() {
            let Some(scope) = self.wire_scope(w) else { continue };
            let mut sources = Vec::new();
            let mut sinks = Vec::new();
            // Set once an end is diagnosed here or by shape: then the wire
            // gets no `Y005` as well.
            let mut diagnosed = false;
            for e in &w.ends {
                match e.kind {
                    EndKind::Blocked => diagnosed = true,
                    EndKind::Arrow => match self.arrow_claim.get(&e.pos) {
                        Some(&(s, id)) if Some(s) == scope => sinks.push(Endpoint::Input(id, 0)),
                        _ => {
                            self.error(e.pos, "Y004", "dangling arrowhead");
                            diagnosed = true;
                        }
                    },
                    EndKind::Open => match self.open_claim.get(&e.pos) {
                        Some(&(s, ep)) if e.facing == Dir::W && Some(s) == scope => {
                            sources.push(Source::Plain(ep));
                        }
                        _ => {
                            self.error(e.pos, "Y003", "dangling wire end");
                            diagnosed = true;
                        }
                    },
                    EndKind::Port(q) => match self.port_end(scope, q, e.facing) {
                        Some(Ok(src)) => sources.push(src),
                        Some(Err(sink)) => sinks.push(sink),
                        None if matches!(self.ports.get(&q), Some(PortRole::Ignored)) => {
                            diagnosed = true;
                        }
                        None => {
                            let message = self.unusable_port(q, e.facing);
                            self.error(e.pos, "Y003", &message);
                            diagnosed = true;
                        }
                    },
                }
            }
            let first = w.cells[0];
            let merged = match sources[..] {
                [] => None,
                [s] => Some(s),
                _ => merged_switch_output(&sources),
            };
            // Sources and sinks are only ever found in a scope.
            if !diagnosed && sinks.is_empty() {
                let message = match (scope, merged) {
                    (Some(s), Some(from)) => {
                        format!("the wire from {} reaches no arrowhead", self.source_phrase(s, from, true))
                    }
                    _ => "wire reaches no arrowhead".to_owned(),
                };
                self.error(first, "Y005", &message);
            } else if !diagnosed
                && sources.is_empty()
                && let Some(s) = scope
            {
                let each: Vec<String> = sinks.iter().map(|&to| self.endpoint_phrase(s, to)).collect();
                let message = format!("wire with no source reaches {}", message::series(&each));
                self.error(first, "Y005", &message);
            }
            if merged.is_none()
                && sources.len() > 1
                && let Some(s) = scope
            {
                let each: Vec<String> = sources.iter().map(|&from| self.source_phrase(s, from, false)).collect();
                let count = message::quantity(sources.len(), "source");
                let message = format!("{count} on one wire: {}", message::series(&each));
                self.error(first, "Y006", &message);
            }
            let from = merged.or(sources.first().copied());
            let (Some(scope), Some(from)) = (scope, from) else { continue };
            let from = match from {
                Source::Plain(ep) => ep,
                Source::SwitchOut { node, ordinal, .. } => Endpoint::Output(node, ordinal),
            };
            self.edges.entry(scope).or_default().push(Edge { wire: wi, from, to: sinks });
        }
    }

    /// `Y019` for every arrowhead that must be fed but that no wire reaches
    /// from the feeding side. Every such port points E (fed from the west) or,
    /// for a selector, S (fed from the north), so the end meeting it faces the
    /// way the arrowhead points.
    fn report_unfed(&mut self) {
        let d = self.d;
        let fed: HashSet<(Pos, Dir)> = d
            .wires
            .iter()
            .flat_map(|w| &w.ends)
            .filter_map(|e| match e.kind {
                EndKind::Port(q) => Some((q, e.facing)),
                _ => None,
            })
            .collect();
        for b in &d.boxes {
            for p in &b.ports {
                let sink = matches!(
                    self.ports.get(&p.pos),
                    Some(
                        PortRole::DefOut(..)
                            | PortRole::NodeIn(..)
                            | PortRole::Selector(_)
                            | PortRole::ThroughIn(..)
                            | PortRole::ThroughOut(..)
                    )
                );
                if sink && p.kind == PortKind::Arrow && !fed.contains(&(p.pos, p.dir)) {
                    let port = self.port_of(self.ports[&p.pos]).expect("a sink port");
                    let message = message::unfed(port);
                    self.error(p.pos, "Y019", &message);
                }
            }
        }
    }

    /// The role of a wire end in `scope` meeting the port at `q`, facing
    /// `facing`: `Ok` for a source, `Err` for a sink, `None` if no rule
    /// applies.
    fn port_end(
        &self,
        scope: Option<Scope>,
        q: Pos,
        facing: Dir,
    ) -> Option<Result<Source, Endpoint>> {
        let scope = scope?;
        let here = |boxi: usize| Some(scope) == self.node_scope(boxi);
        let id = |boxi: usize| self.box_node.get(&boxi).map(|&(_, id)| id);
        let inside = |boxi: usize, arm: Option<usize>| scope == Scope { boxi, arm };
        Some(match *self.ports.get(&q)? {
            PortRole::DefIn(d, n) if facing == Dir::W && inside(d, None) => {
                Ok(Source::Plain(Endpoint::Param(n)))
            }
            PortRole::DefOut(d, n) if facing == Dir::E && inside(d, None) => {
                Err(Endpoint::Result(n))
            }
            PortRole::NodeIn(b, n) if facing == Dir::E && here(b) => {
                Err(Endpoint::Input(id(b)?, n))
            }
            PortRole::NodeOut(b, n) if facing == Dir::W && here(b) => {
                Ok(Source::Plain(Endpoint::Output(id(b)?, n)))
            }
            PortRole::Selector(b) if facing == Dir::S && here(b) => Err(Endpoint::Input(id(b)?, 0)),
            PortRole::ThroughIn(b, k, j) if facing == Dir::E && here(b) => {
                let arms = &self.switches[&b].arms;
                let before: usize = arms[..k].iter().map(|a| a.params.len()).sum();
                Err(Endpoint::Input(id(b)?, 1 + before as u32 + j))
            }
            PortRole::ThroughIn(b, k, j) if facing == Dir::W && inside(b, Some(k)) => {
                Ok(Source::Plain(Endpoint::Param(j)))
            }
            PortRole::ThroughOut(b, k, j) if facing == Dir::E && inside(b, Some(k)) => {
                Err(Endpoint::Result(j))
            }
            PortRole::ThroughOut(b, k, j) if facing == Dir::W && here(b) => {
                Ok(Source::SwitchOut { node: id(b)?, arm: k, ordinal: j })
            }
            _ => return None,
        })
    }

    fn graph(&mut self, scope: Scope) -> Graph {
        let pending = self.pending.remove(&scope).unwrap_or_default();
        let nodes = pending.into_iter().map(|p| self.node(p)).collect();
        let edges = self.edges.remove(&scope).unwrap_or_default();
        Graph { nodes, edges }
    }

    fn node(&mut self, p: Pending) -> Node {
        let d = self.d;
        match p {
            Pending::Inline { text, arrow, out } => Node {
                kind: NodeKind::Inline(name_of(&d.texts[text])),
                rect: text_rect(&d.texts[text]),
                inputs: vec![arrow],
                outputs: vec![out],
            },
            Pending::Const { text, out } => Node {
                kind: NodeKind::Const(self.literal(text)),
                rect: text_rect(&d.texts[text]),
                inputs: Vec::new(),
                outputs: vec![out],
            },
            Pending::Box(b) if self.roles[b] == Role::Switch => self.switch(b),
            Pending::Box(b) => self.call(b),
        }
    }

    /// Decodes a literal text, reporting `Y017` for an oversized number.
    fn literal(&mut self, text: usize) -> Literal {
        let d = self.d;
        let t = &d.texts[text];
        match t.kind {
            TextKind::Number => t.content.parse().map(Literal::Int).unwrap_or_else(|_| {
                self.error(t.pos, "Y017", "number literal too large for an Int");
                Literal::Int(i64::MAX)
            }),
            TextKind::Literal => Literal::Text(t.content.clone()),
            _ => Literal::Nil,
        }
    }

    fn call(&mut self, b: usize) -> Node {
        let d = self.d;
        let shape = &d.boxes[b];
        let name = match self.node_word(b) {
            Some(t) => name_of(t),
            None => {
                let first = d.texts.iter().find(|t| t.owner == Some(b) && t.kind != TextKind::Comment);
                let at = first.map_or(top_left(shape.rect), |t| t.pos);
                self.error(at, "Y002", "a node box must hold exactly one word");
                Name { text: first.map(|t| t.content.clone()).unwrap_or_default(), at }
            }
        };
        for (i, t) in d.texts.iter().enumerate() {
            if t.title_of == Some(b) {
                self.used[i] = true;
                self.error(t.pos, "Y016", "a guard on a box that is not a switch");
            }
        }
        let ports_where = |want: fn(&PortRole) -> bool| -> Vec<Pos> {
            shape.ports.iter().map(|p| p.pos).filter(|q| want(&self.ports[q])).collect()
        };
        Node {
            kind: NodeKind::Call(name),
            rect: shape.rect,
            inputs: ports_where(|r| matches!(r, PortRole::NodeIn(..))),
            outputs: ports_where(|r| matches!(r, PortRole::NodeOut(..))),
        }
    }

    fn switch(&mut self, b: usize) -> Node {
        let rect = self.d.boxes[b].rect;
        let info = self.switches[&b].clone();
        // Without a selector, the corner stands in for it, so input slot 0
        // stays the selector's.
        let selector = info.selector.unwrap_or(top_left(rect));
        if info.selector.is_none() {
            self.error(selector, "Y010", "switch without a selector");
        }
        if info.arms.windows(2).any(|w| w[0].results.len() != w[1].results.len()) {
            let results: Vec<usize> = info.arms.iter().map(|a| a.results.len()).collect();
            self.error(selector, "Y011", &message::unequal_arms(&results));
        }
        let mut inputs = vec![selector];
        let mut outputs: Vec<Pos> = Vec::new();
        for a in &info.arms {
            inputs.extend(&a.params);
            outputs.extend(a.results.iter().skip(outputs.len()));
        }

        let mut arms = Vec::new();
        let mut seen_default = false;
        for (k, a) in info.arms.into_iter().enumerate() {
            let guard = self.guard(b, k);
            match &guard {
                Some(g) if seen_default => self.error(g.at, "Y007", "guard after the default arm"),
                Some(_) => {}
                None if seen_default => self.error(a.head, "Y008", "more than one default arm"),
                None => seen_default = true,
            }
            let body = self.graph(Scope { boxi: b, arm: Some(k) });
            arms.push(Arm { guard, rows: a.rows, params: a.params, results: a.results, body });
        }
        Node { kind: NodeKind::Switch(Switch { selector, arms }), rect, inputs, outputs }
    }

    /// The guard of arm `k`: the literal in the top border's gap for the
    /// first arm, in the divider's gap for later arms. Any other text there
    /// stays unused and is reported as stray.
    fn guard(&mut self, b: usize, k: usize) -> Option<Guard> {
        let d = self.d;
        let first = d.texts.iter().position(|t| {
            if k == 0 {
                t.title_of == Some(b)
            } else {
                t.divider_of == Some(b) && t.pos.row == d.boxes[b].dividers[k - 1].row
            }
        })?;
        if !is_literal(&d.texts[first]) {
            return None;
        }
        self.used[first] = true;
        Some(Guard { value: self.literal(first), at: d.texts[first].pos })
    }

    /// The word of node box `b`, if it holds exactly one word and no box.
    /// Otherwise the node is `Y002`.
    fn node_word(&self, b: usize) -> Option<&'a Text> {
        let d = self.d;
        let mut owned = d.texts.iter().filter(|t| t.owner == Some(b) && t.kind != TextKind::Comment);
        let (Some(t), None) = (owned.next(), owned.next()) else { return None };
        let nested = d.boxes.iter().any(|c| c.parent == Some(b));
        (!nested && t.kind == TextKind::Word).then_some(t)
    }

    /// The name of definition box `boxi`, if its title is a `·`-word.
    fn name_of_definition(&self, boxi: usize) -> Option<&str> {
        self.definition_names.get(&boxi).map(String::as_str)
    }

    /// A port that has a role, as messages name it.
    fn port_of(&self, role: PortRole) -> Option<message::Port<'_>> {
        let word = |b: usize| self.node_word(b).map(|t| t.content.as_str());
        let index = |n: u32| n as usize;
        Some(match role {
            PortRole::DefIn(d, n) => {
                message::Port::DefinitionInput { definition: self.name_of_definition(d), index: index(n) }
            }
            PortRole::DefOut(d, n) => {
                message::Port::DefinitionOutput { definition: self.name_of_definition(d), index: index(n) }
            }
            PortRole::NodeIn(b, n) => message::Port::NodeInput { word: word(b), index: index(n) },
            PortRole::NodeOut(b, n) => message::Port::NodeOutput { word: word(b), index: index(n) },
            PortRole::Selector(_) => message::Port::Selector,
            PortRole::ThroughIn(_, arm, j) => message::Port::LeftThroughPort { arm, index: index(j) },
            PortRole::ThroughOut(_, arm, j) => message::Port::RightThroughPort { arm, index: index(j) },
            PortRole::Ignored | PortRole::Unknown => return None,
        })
    }

    /// Box `b` as messages name it.
    fn box_phrase(&self, b: usize) -> String {
        match self.roles[b] {
            Role::Def => message::definition(self.name_of_definition(b)),
            Role::Node => message::node(self.node_word(b).map(|t| t.content.as_str())),
            Role::Switch => "the switch".to_owned(),
            Role::Skip | Role::Inner => "the box".to_owned(),
        }
    }

    /// The `Y003` message for a wire end that meets the port at `q`, facing
    /// `facing`, where no rule of SPEC §7 gives it a role: a port with a
    /// role met from the wrong side, or a stroke on a border that no rule
    /// makes a port.
    fn unusable_port(&self, q: Pos, facing: Dir) -> String {
        let Some(&b) = self.port_box.get(&q) else { return "a wire can't meet this port".to_owned() };
        let port = self.d.boxes[b].ports.iter().find(|p| p.pos == q).expect("a port of its box");
        let Some(named) = self.port_of(self.ports[&q]) else {
            let stroke = match port.kind {
                PortKind::Arrow => "arrowhead",
                PortKind::Tee => "tee",
            };
            return format!("a wire can't meet this {stroke} on the border of {}", self.box_phrase(b));
        };
        // The end faces the port, so it is outside when it faces inward.
        let side = if facing == port.side.opposite() { "outside" } else { "inside" };
        let kind = match self.roles[b] {
            Role::Def => "definition",
            Role::Switch => "switch",
            _ => "node",
        };
        format!("a wire can't meet {named} from {side} the {kind}")
    }

    /// The left through-port behind input slot `slot` of switch box `b`:
    /// slot 0 is the selector, then every arm's through-ports, arm by arm.
    fn left_through_port(&self, b: usize, slot: usize) -> message::Port<'static> {
        let mut rest = slot - 1;
        for (arm, a) in self.switches[&b].arms.iter().enumerate() {
            if rest < a.params.len() {
                return message::Port::LeftThroughPort { arm, index: rest };
            }
            rest -= a.params.len();
        }
        unreachable!("a switch's input slots are its selector and its arms' through-ports")
    }

    /// A wire's source or arrowhead in `scope`, as messages name it.
    fn endpoint_phrase(&self, scope: Scope, ep: Endpoint) -> String {
        let definition = self.name_of_definition(scope.boxi);
        let port = match (ep, scope.arm) {
            (Endpoint::Param(n), None) => message::Port::DefinitionInput { definition, index: n as usize },
            (Endpoint::Param(n), Some(arm)) => message::Port::LeftThroughPort { arm, index: n as usize },
            (Endpoint::Result(n), None) => message::Port::DefinitionOutput { definition, index: n as usize },
            (Endpoint::Result(n), Some(arm)) => message::Port::RightThroughPort { arm, index: n as usize },
            (Endpoint::Input(id, slot) | Endpoint::Output(id, slot), _) => {
                let output = matches!(ep, Endpoint::Output(..));
                let index = slot as usize;
                let at_node = |word: Option<&'a str>| match output {
                    true => message::Port::NodeOutput { word, index },
                    false => message::Port::NodeInput { word, index },
                };
                match self.pending.get(&scope).and_then(|list| list.get(id)) {
                    Some(&Pending::Const { text, .. }) => {
                        return format!("the constant {}", literal_phrase(&self.d.texts[text]));
                    }
                    Some(&Pending::Inline { text, .. }) => at_node(Some(&self.d.texts[text].content)),
                    Some(&Pending::Box(b)) if self.roles[b] == Role::Switch => match (output, index) {
                        (true, _) => message::Port::MergedOutput { index },
                        (false, 0) => message::Port::Selector,
                        (false, _) => self.left_through_port(b, index),
                    },
                    Some(&Pending::Box(b)) => at_node(self.node_word(b).map(|t| t.content.as_str())),
                    None => return "a node".to_owned(),
                }
            }
        };
        port.to_string()
    }

    /// A wire's source in `scope`, as messages name it. Switch outputs that
    /// merge are named as the merged output when `merged`, and otherwise as
    /// the right through-ports they leave from.
    fn source_phrase(&self, scope: Scope, source: Source, merged: bool) -> String {
        match source {
            Source::Plain(ep) => self.endpoint_phrase(scope, ep),
            Source::SwitchOut { ordinal, .. } if merged => {
                message::Port::MergedOutput { index: ordinal as usize }.to_string()
            }
            Source::SwitchOut { arm, ordinal, .. } => {
                message::Port::RightThroughPort { arm, index: ordinal as usize }.to_string()
            }
        }
    }

    /// `Y012` for every text no construct accounted for.
    fn report_stray(&mut self) {
        let d = self.d;
        for (i, t) in d.texts.iter().enumerate() {
            let host = t.title_of.or(t.divider_of).or(t.owner);
            let skipped = host.is_some_and(|h| matches!(self.roles[h], Role::Skip | Role::Inner));
            if !self.used[i] && !skipped {
                self.error(t.pos, "Y012", "stray text");
            }
        }
    }
}
