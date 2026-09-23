//! Painting a [`Scene`]: TortoiseGit-style nodes (one coloured row per ref) and edges.

use eframe::egui::{
    Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2,
    vec2,
};
use gitgraph_core::layout::Direction;

use crate::scene::{CORNER_RADIUS, FONT_SIZE, MARGIN_X, RowKind, Scene, to_pos};
use crate::settings::{Arrows, EdgeStyle, Settings};
use crate::theme::{Palette, text_on};
use crate::view::View;

/// Nodes and edges that get special emphasis.
#[derive(Clone, Debug, Default)]
pub struct Marks {
    pub hovered: Option<usize>,
    pub hovered_edge: Option<usize>,
    /// Per node: selected.
    pub selected: Vec<bool>,
    /// Per node: would move along if the hovered node were dragged.
    pub preview: Vec<bool>,
    /// Per node: matches the current search.
    pub search_hits: Vec<bool>,
}

impl Marks {
    fn is_hit(&self, node: usize) -> bool {
        self.search_hits.get(node).copied().unwrap_or(false)
    }

    fn is_selected(&self, node: usize) -> bool {
        self.selected.get(node).copied().unwrap_or(false)
    }

    fn is_previewed(&self, node: usize) -> bool {
        self.preview.get(node).copied().unwrap_or(false)
    }

    fn emphasised(&self, node: usize) -> bool {
        self.hovered == Some(node) || self.is_selected(node)
    }
}

/// Text smaller than this many pixels is not drawn.
const MIN_TEXT_PX: f32 = 4.0;

pub fn paint_scene(
    painter: &Painter,
    canvas: Rect,
    view: &View,
    scene: &Scene,
    palette: &Palette,
    settings: &Settings,
    marks: &Marks,
) {
    painter.rect_filled(canvas, 0.0, palette.background);
    let visible = canvas.expand(40.0);
    let zoom = view.zoom;
    let width = (2.0 * zoom).max(1.0);

    // Edges first (TortoiseGit draws them on top; boxes on top reads better with dragging).
    let mut emphasised = Vec::new();
    for (e, edge) in scene.graph.edges.iter().enumerate() {
        let (c, p) = (edge.child as usize, edge.parent as usize);
        if marks.hovered_edge == Some(e)
            || settings.highlight_edges && (marks.emphasised(c) || marks.emphasised(p))
        {
            emphasised.push(e);
            continue;
        }
        paint_edge(
            painter,
            canvas,
            view,
            scene,
            settings,
            e,
            visible,
            Stroke::new(width, palette.edge),
        );
    }
    for e in emphasised {
        let stroke = Stroke::new(width * 1.6, palette.selection);
        paint_edge(painter, canvas, view, scene, settings, e, visible, stroke);
    }

    let font = FontId::monospace(FONT_SIZE * zoom);
    let draw_text = FONT_SIZE * zoom >= MIN_TEXT_PX;
    let radius = (CORNER_RADIUS * zoom).round().clamp(0.0, 255.0) as u8;
    let row_h = scene.row_height * zoom;
    for (i, visual) in scene.visuals.iter().enumerate() {
        let rect = view.rect_to_screen(canvas, scene.node_rect(i));
        if !visible.intersects(rect) {
            continue;
        }
        for ((row_rect, corners), row) in
            node_rows(rect, visual.rows.len(), row_h, radius).zip(&visual.rows)
        {
            let (fill, border, text) = row_colors(&row.kind, palette);
            painter.rect(
                row_rect,
                corners,
                fill,
                Stroke::new(1.0, border),
                StrokeKind::Inside,
            );
            if draw_text {
                painter.text(
                    Pos2::new(row_rect.min.x + MARGIN_X * zoom, row_rect.center().y),
                    Align2::LEFT_CENTER,
                    &row.label,
                    font.clone(),
                    text,
                );
            }
        }

        let outline = |w: f32, color: Color32| {
            painter.rect_stroke(
                rect.expand(w / 2.0 + 1.0),
                radius,
                Stroke::new(w, color),
                StrokeKind::Middle,
            );
        };
        if marks.is_selected(i) {
            outline((4.0 * zoom).max(2.0), palette.selection);
        } else if marks.is_hit(i) {
            outline((3.0 * zoom).max(2.0), palette.search_hit);
        } else if marks.hovered == Some(i) {
            outline((2.0 * zoom).max(1.0), palette.selection);
        } else if marks.is_previewed(i) {
            outline((2.0 * zoom).max(1.0), palette.selection.gamma_multiply(0.5));
        }
        if scene.net.is_moved(i) {
            painter.circle_filled(
                rect.right_top() + vec2(-3.0, 3.0),
                (3.0 * zoom).max(2.0),
                palette.moved_marker,
            );
        }
    }

    if settings.show_hidden_counts && FONT_SIZE * zoom * 0.85 >= MIN_TEXT_PX {
        paint_hidden_counts(painter, canvas, view, scene, palette, visible);
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_edge(
    painter: &Painter,
    canvas: Rect,
    view: &View,
    scene: &Scene,
    settings: &Settings,
    e: usize,
    visible: Rect,
    stroke: Stroke,
) {
    let Some(path) = edge_path(
        scene,
        e,
        settings.edge_style,
        |p| view.to_screen(canvas, p),
        Some(visible),
    ) else {
        return;
    };
    painter.add(Shape::line(path.clone(), stroke));
    if let Some(head) = arrowhead_points(&path, settings.arrows, 8.0 * view.zoom.max(0.25)) {
        for tri in head {
            painter.add(Shape::convex_polygon(
                tri.to_vec(),
                stroke.color,
                Stroke::NONE,
            ));
        }
    }
}

/// The drawn path of edge `e`, mapped through `to_screen`: straight segments clipped to the
/// node boxes, or a smooth curve. `None` if it lies entirely outside `visible`.
pub fn edge_path(
    scene: &Scene,
    e: usize,
    style: EdgeStyle,
    to_screen: impl Fn(Pos2) -> Pos2,
    visible: Option<Rect>,
) -> Option<Vec<Pos2>> {
    let edge = scene.graph.edges[e];
    let (c, p) = (edge.child as usize, edge.parent as usize);
    let pts: Vec<Pos2> = scene
        .net
        .edge_points(e)
        .map(|pt| to_screen(to_pos(pt)))
        .collect();
    if let Some(visible) = visible
        && !visible.intersects(Rect::from_points(&pts))
    {
        return None;
    }
    let map_rect = |r: Rect| Rect::from_two_pos(to_screen(r.min), to_screen(r.max));
    let child = map_rect(scene.node_rect(c));
    let parent = map_rect(scene.node_rect(p));
    let n = pts.len();
    let path = match style {
        EdgeStyle::Straight => {
            let mut path = pts.clone();
            path[0] = clip_to_rect(child, pts[1]);
            path[n - 1] = clip_to_rect(parent, pts[n - 2]);
            path
        }
        EdgeStyle::Curved => curved_path(&pts, child, parent, scene.layout.direction),
    };
    (path.len() >= 2).then_some(path)
}

/// The two triangles of an arrowhead for `path`, or `None` without arrows.
pub fn arrowhead_points(path: &[Pos2], arrows: Arrows, len: f32) -> Option<[[Pos2; 3]; 2]> {
    let n = path.len();
    match arrows {
        Arrows::ToParent => arrowhead(path[n - 2], path[n - 1], len),
        Arrows::ToChild => arrowhead(path[1], path[0], len),
        Arrows::None => None,
    }
}

/// The edge whose drawn path passes within `tolerance` of `pointer` (screen space), if any.
pub fn edge_at(
    scene: &Scene,
    style: EdgeStyle,
    to_screen: impl Fn(Pos2) -> Pos2 + Copy,
    pointer: Pos2,
    tolerance: f32,
) -> Option<usize> {
    let probe = Rect::from_center_size(pointer, Vec2::splat(2.0 * tolerance));
    let mut best = None;
    let mut best_d = tolerance;
    for e in 0..scene.graph.edges.len() {
        let Some(path) = edge_path(scene, e, style, to_screen, Some(probe)) else {
            continue;
        };
        for w in path.windows(2) {
            let d = distance_to_segment(pointer, w[0], w[1]);
            if d < best_d {
                best_d = d;
                best = Some(e);
            }
        }
    }
    best
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 {
        ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * t).distance(p)
}

/// Where the ray from `rect`'s centre towards `toward` leaves the rectangle.
fn clip_to_rect(rect: Rect, toward: Pos2) -> Pos2 {
    let c = rect.center();
    let d = toward - c;
    let h = rect.size() / 2.0 + Vec2::splat(0.5);
    let tx = if d.x.abs() > 1e-6 {
        h.x / d.x.abs()
    } else {
        f32::INFINITY
    };
    let ty = if d.y.abs() > 1e-6 {
        h.y / d.y.abs()
    } else {
        f32::INFINITY
    };
    let t = tx.min(ty);
    if t >= 1.0 || !t.is_finite() {
        c
    } else {
        c + d * t
    }
}

/// A smooth path that leaves the child along the history direction, passes through every bend
/// point with a tangent along that direction, and enters the parent the same way.
fn curved_path(pts: &[Pos2], child: Rect, parent: Rect, direction: Direction) -> Vec<Pos2> {
    let f = direction.flow();
    let flow = vec2(f.x, f.y);
    let n = pts.len();
    let side = |r: Rect, sign: f32| {
        r.center() + flow * (sign * (flow.x.abs() * r.width() + flow.y.abs() * r.height()) / 2.0)
    };
    let mut knots = Vec::with_capacity(n);
    knots.push(side(child, 1.0));
    knots.extend_from_slice(&pts[1..n - 1]);
    knots.push(side(parent, -1.0));

    let mut out = vec![knots[0]];
    for w in knots.windows(2) {
        let (a, b) = (w[0], w[1]);
        let reach = ((b - a).dot(flow).abs() / 2.0).max(4.0);
        let (c1, c2) = (a + flow * reach, b - flow * reach);
        let steps = (((b - a).length() / 6.0) as usize).clamp(4, 32);
        for s in 1..=steps {
            let t = s as f32 / steps as f32;
            let u = 1.0 - t;
            let p = a.to_vec2() * (u * u * u)
                + c1.to_vec2() * (3.0 * u * u * t)
                + c2.to_vec2() * (3.0 * u * t * t)
                + b.to_vec2() * (t * t * t);
            out.push(p.to_pos2());
        }
    }
    out
}

/// Arrowhead with its tip at `tip`, pointing away from `from` (TortoiseGit: wings at ±22.5°,
/// notch 0.6 of the wing length back), as two triangles.
fn arrowhead(from: Pos2, tip: Pos2, len: f32) -> Option<[[Pos2; 3]; 2]> {
    let d = tip - from;
    if d.length_sq() < 1e-6 {
        return None;
    }
    let dir = d.normalized();
    let angle = std::f32::consts::PI / 8.0;
    let rot = |v: Vec2, a: f32| vec2(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos());
    let wing1 = tip - rot(dir, angle) * len;
    let wing2 = tip - rot(dir, -angle) * len;
    let notch = tip - dir * (0.6 * len);
    Some([[tip, wing1, notch], [tip, notch, wing2]])
}

/// Screen rectangles and corner radii of a node's rows (only the outer corners are rounded).
pub fn node_rows(
    rect: Rect,
    rows: usize,
    row_height: f32,
    radius: u8,
) -> impl Iterator<Item = (Rect, CornerRadius)> {
    (0..rows).map(move |r| {
        let row_rect = Rect::from_min_size(
            rect.min + vec2(0.0, r as f32 * row_height),
            vec2(rect.width(), row_height),
        );
        let corners = CornerRadius {
            nw: if r == 0 { radius } else { 0 },
            ne: if r == 0 { radius } else { 0 },
            sw: if r + 1 == rows { radius } else { 0 },
            se: if r + 1 == rows { radius } else { 0 },
        };
        (row_rect, corners)
    })
}

/// Fill, border and text colour of a row.
pub fn row_colors(kind: &RowKind, palette: &Palette) -> (Color32, Color32, Color32) {
    match kind {
        RowKind::Hash => (palette.plain_fill, palette.plain_border, palette.plain_text),
        RowKind::Ref { kind, head } => {
            let fill = palette.ref_fill(*kind, *head);
            (fill, fill, text_on(fill))
        }
    }
}

fn paint_hidden_counts(
    painter: &Painter,
    canvas: Rect,
    view: &View,
    scene: &Scene,
    palette: &Palette,
    visible: Rect,
) {
    let font = FontId::proportional(FONT_SIZE * 0.85 * view.zoom);
    let color = palette.edge.gamma_multiply(0.7);
    for (e, edge) in scene.graph.edges.iter().enumerate() {
        if edge.hidden == 0 {
            continue;
        }
        let pts: Vec<Pos2> = scene
            .net
            .edge_points(e)
            .collect::<Vec<_>>()
            .into_iter()
            .map(to_pos)
            .collect();
        let mid = if pts.len() % 2 == 1 {
            pts[pts.len() / 2]
        } else {
            pts[pts.len() / 2 - 1].lerp(pts[pts.len() / 2], 0.5)
        };
        let at = view.to_screen(canvas, mid);
        if visible.contains(at) {
            painter.text(
                at + vec2(4.0, 0.0),
                Align2::LEFT_CENTER,
                format!("+{}", edge.hidden),
                font.clone(),
                color,
            );
        }
    }
}

/// Paints a miniature of the whole graph into `rect` with the visible area outlined.
/// Returns the world rectangle the miniature represents and the scale used.
pub fn paint_overview(
    painter: &Painter,
    rect: Rect,
    canvas: Rect,
    view: &View,
    scene: &Scene,
    palette: &Palette,
) -> (Rect, f32) {
    let world = scene.bounds().expand(20.0);
    let scale = (rect.width() / world.width()).min(rect.height() / world.height());
    let to_mini = |p: Pos2| rect.center() + (p - world.center()) * scale;
    painter.rect(
        rect.expand(1.0),
        4.0,
        palette.background,
        Stroke::new(1.0, palette.edge.gamma_multiply(0.5)),
        StrokeKind::Outside,
    );
    let edge_stroke = Stroke::new(1.0, palette.edge.gamma_multiply(0.35));
    for e in 0..scene.graph.edges.len() {
        let pts: Vec<Pos2> = scene
            .net
            .edge_points(e)
            .map(|p| to_mini(to_pos(p)))
            .collect();
        painter.add(Shape::line(pts, edge_stroke));
    }
    for (i, v) in scene.visuals.iter().enumerate() {
        let r = Rect::from_center_size(
            to_mini(scene.node_center(i)),
            (v.size * scale).max(Vec2::splat(2.0)),
        );
        let fill = match &v.rows[0].kind {
            RowKind::Hash => palette.plain_border,
            RowKind::Ref { kind, head } => palette.ref_fill(*kind, *head),
        };
        painter.rect_filled(r, 0.0, fill);
    }
    let seen = view.visible_world(canvas);
    let seen_mini = Rect::from_min_max(to_mini(seen.min), to_mini(seen.max)).intersect(rect);
    painter.rect(
        seen_mini,
        0.0,
        Color32::from_black_alpha(40),
        Stroke::new(1.0, palette.selection),
        StrokeKind::Inside,
    );
    (world, scale)
}
