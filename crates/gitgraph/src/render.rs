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
    pub selected: Option<usize>,
    /// Per node: matches the current search.
    pub search_hits: Vec<bool>,
}

impl Marks {
    fn is_hit(&self, node: usize) -> bool {
        self.search_hits.get(node).copied().unwrap_or(false)
    }

    fn emphasised(&self, node: usize) -> bool {
        self.hovered == Some(node) || self.selected == Some(node)
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
        if settings.highlight_edges && (marks.emphasised(c) || marks.emphasised(p)) {
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
        let last = visual.rows.len() - 1;
        for (r, row) in visual.rows.iter().enumerate() {
            let row_rect = Rect::from_min_size(
                rect.min + vec2(0.0, r as f32 * row_h),
                vec2(rect.width(), row_h),
            );
            let corners = CornerRadius {
                nw: if r == 0 { radius } else { 0 },
                ne: if r == 0 { radius } else { 0 },
                sw: if r == last { radius } else { 0 },
                se: if r == last { radius } else { 0 },
            };
            let (fill, border, text) = match &row.kind {
                RowKind::Hash => (palette.plain_fill, palette.plain_border, palette.plain_text),
                RowKind::Ref { kind, head } => {
                    let fill = palette.ref_fill(*kind, *head);
                    (fill, fill, text_on(fill))
                }
            };
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
        if marks.selected == Some(i) {
            outline((4.0 * zoom).max(2.0), palette.selection);
        } else if marks.is_hit(i) {
            outline((3.0 * zoom).max(2.0), palette.search_hit);
        } else if marks.hovered == Some(i) {
            outline((2.0 * zoom).max(1.0), palette.selection);
        }
        if scene.net.is_pinned(i) {
            painter.circle_filled(
                rect.right_top() + vec2(-3.0, 3.0),
                (3.0 * zoom).max(2.0),
                palette.pinned_marker,
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
    let edge = scene.graph.edges[e];
    let (c, p) = (edge.child as usize, edge.parent as usize);
    let pts: Vec<Pos2> = scene
        .net
        .edge_points(e)
        .map(|pt| view.to_screen(canvas, to_pos(pt)))
        .collect();
    let bbox = Rect::from_points(&pts);
    if !visible.intersects(bbox) {
        return;
    }
    let child = view.rect_to_screen(canvas, scene.node_rect(c));
    let parent = view.rect_to_screen(canvas, scene.node_rect(p));
    let n = pts.len();

    let path = match settings.edge_style {
        EdgeStyle::Straight => {
            let mut path = pts.clone();
            path[0] = clip_to_rect(child, pts[1]);
            path[n - 1] = clip_to_rect(parent, pts[n - 2]);
            path
        }
        EdgeStyle::Curved => curved_path(&pts, child, parent, scene.layout.direction),
    };
    if path.len() < 2 {
        return;
    }
    painter.add(Shape::line(path.clone(), stroke));

    let head_len = 8.0 * view.zoom.max(0.25);
    match settings.arrows {
        Arrows::ToParent => arrowhead(
            painter,
            path[path.len() - 2],
            path[path.len() - 1],
            head_len,
            stroke.color,
        ),
        Arrows::ToChild => arrowhead(painter, path[1], path[0], head_len, stroke.color),
        Arrows::None => {}
    }
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

/// Filled arrowhead with its tip at `tip`, pointing away from `from` (TortoiseGit: wings at
/// ±22.5°, notch 0.6 of the wing length back).
fn arrowhead(painter: &Painter, from: Pos2, tip: Pos2, len: f32, color: Color32) {
    let d = tip - from;
    if d.length_sq() < 1e-6 {
        return;
    }
    let dir = d.normalized();
    let angle = std::f32::consts::PI / 8.0;
    let rot = |v: Vec2, a: f32| vec2(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos());
    let wing1 = tip - rot(dir, angle) * len;
    let wing2 = tip - rot(dir, -angle) * len;
    let notch = tip - dir * (0.6 * len);
    painter.add(Shape::convex_polygon(
        vec![tip, wing1, notch],
        color,
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(
        vec![tip, notch, wing2],
        color,
        Stroke::NONE,
    ));
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
