//! Routing edges around nodes that have been moved by hand.
//!
//! The layout routes every edge through the gaps of the layers it crosses. Once nodes have been
//! moved, those routes stop fitting: brought closer together, an edge keeps bends it no longer
//! needs; pulled apart, it may cut through the nodes now in between. Such edges are routed
//! afresh, in the same spirit as the layout:
//!
//! 1. The node boxes between the edge's ends are grouped into rows (bands along the layers).
//! 2. In each row the edge passes through the gap nearest to the straight line between its
//!    ends, keeping some clearance from the boxes.
//! 3. The route is pulled taut through those gaps (the "funnel" algorithm), so it only bends
//!    where it has to, and always at the side of a gap, as the layout's routes do.
//! 4. Anything still in the way (such as a node in the same row as both ends) is walked around
//!    corner by corner, and the result is pulled taut once more.

use std::collections::HashMap;

use crate::layout::Point;

/// Clearance kept between a routed edge and the node boxes it passes.
pub const CLEARANCE: f32 = 8.0;
/// How far an edge whose parent has been moved before its child runs on along the history
/// direction before it turns round, and how far before its parent it turns back. More than
/// [`CLEARANCE`], so that the turns lie outside the end boxes.
pub const TURN: f32 = 14.0;
/// For [`route`]: no node at this end.
pub const NO_END: u32 = u32::MAX;
/// At most this many detours around boxes per edge; past that the rest is left straight.
const MAX_DETOURS: usize = 48;
/// How far to each side of the straight line boxes are looked at to find the rows.
const CORRIDOR: f32 = 300.0;
/// How far along a row a gap is looked for.
const MAX_REACH: f32 = 65_536.0;
/// Gaps considered to either side of where a route would ideally cross a row.
const GAP_CHOICES: usize = 2;
/// Grid cell size (layout units).
const CELL: f32 = 96.0;
/// Segments that only touch a box's border do not count as running into it.
const TOUCH: f32 = 0.01;

/// Node boxes (grown by the clearance) in a uniform grid, for finding what a segment runs
/// into.
#[derive(Clone, Debug, Default)]
pub struct Obstacles {
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// Indexed box of every node, if placed: minimum and maximum corner.
    boxes: Vec<Option<(Point, Point)>>,
    /// The query in which each node was last looked at, so that each is tested once.
    seen: Vec<u32>,
    query: u32,
}

impl Obstacles {
    pub fn new(count: usize) -> Obstacles {
        Obstacles {
            cells: HashMap::new(),
            boxes: vec![None; count],
            seen: vec![0; count],
            query: 0,
        }
    }

    /// Places (or moves) node `i`, centred at `centre` with half extents `half`.
    pub fn place(&mut self, i: usize, centre: Point, half: Point) {
        let grow = Point::new(half.x + CLEARANCE, half.y + CLEARANCE);
        let new = (
            Point::new(centre.x - grow.x, centre.y - grow.y),
            Point::new(centre.x + grow.x, centre.y + grow.y),
        );
        if let Some((lo, hi)) = self.boxes[i] {
            for key in cells_of(lo, hi) {
                if let Some(items) = self.cells.get_mut(&key) {
                    items.retain(|&j| j as usize != i);
                    if items.is_empty() {
                        self.cells.remove(&key);
                    }
                }
            }
        }
        for key in cells_of(new.0, new.1) {
            self.cells.entry(key).or_default().push(i as u32);
        }
        self.boxes[i] = Some(new);
    }

    /// The grown box of node `i`.
    pub fn box_of(&self, i: usize) -> Option<(Point, Point)> {
        self.boxes[i]
    }

    /// Visits every node whose grown box is in a grid cell that the segment from `p` to `q`
    /// passes, once.
    pub fn near_segment(&mut self, p: Point, q: Point, mut visit: impl FnMut(u32)) {
        self.query = self.query.wrapping_add(1);
        if self.query == 0 {
            self.seen.fill(0);
            self.query = 1;
        }
        let (cells, seen, query) = (&self.cells, &mut self.seen, self.query);
        cells_along(p, q, |key| {
            for &i in cells.get(&key).map_or(&[][..], Vec::as_slice) {
                if seen[i as usize] != query {
                    seen[i as usize] = query;
                    visit(i);
                }
            }
        });
    }

    /// Visits, once, every node whose box is in a grid cell touching the rectangle from `lo`
    /// to `hi`, with its box.
    pub fn in_rect(&mut self, lo: Point, hi: Point, mut visit: impl FnMut(u32, (Point, Point))) {
        self.query = self.query.wrapping_add(1);
        if self.query == 0 {
            self.seen.fill(0);
            self.query = 1;
        }
        let (lo, hi) = (
            Point::new(lo.x.min(hi.x), lo.y.min(hi.y)),
            Point::new(lo.x.max(hi.x), lo.y.max(hi.y)),
        );
        for key in cells_of(lo, hi) {
            for &i in self.cells.get(&key).map_or(&[][..], Vec::as_slice) {
                if self.seen[i as usize] != self.query {
                    self.seen[i as usize] = self.query;
                    if let Some(bounds) = self.boxes[i as usize] {
                        visit(i, bounds);
                    }
                }
            }
        }
    }

    /// The node box that the segment from `p` to `q` runs into first (keeping the clearance),
    /// ignoring the nodes in `skip`. Near an end inside the clearance only the box itself
    /// counts, and a box around either end is ignored (the segment cannot avoid it).
    pub fn first_hit(&mut self, p: Point, q: Point, skip: &[u32]) -> Option<u32> {
        let mut best: Option<(f32, u32)> = None;
        let boxes = std::mem::take(&mut self.boxes);
        self.near_segment(p, q, |i| {
            if skip.contains(&i) {
                return;
            }
            let Some((mut lo, mut hi)) = boxes[i as usize] else {
                return;
            };
            if inside(p, lo, hi) || inside(q, lo, hi) {
                let c = Point::new(CLEARANCE, CLEARANCE);
                (lo, hi) = (add(lo, c), sub(hi, c));
                if inside(p, lo, hi) || inside(q, lo, hi) {
                    return;
                }
            }
            if let Some(t) = entry(p, q, lo, hi)
                && best.is_none_or(|(b, _)| t < b)
            {
                best = Some((t, i));
            }
        });
        self.boxes = boxes;
        best.map(|(_, i)| i)
    }
}

/// Interior points of a route from `a` to `b` (on the borders of the nodes in `ends`) that
/// keeps clear of all other node boxes where it can. `vertical` is true if layers run
/// horizontally.
pub fn route(
    obstacles: &mut Obstacles,
    vertical: bool,
    a: Point,
    b: Point,
    ends: [u32; 2],
) -> Vec<Point> {
    let mut pts = through_rows(obstacles, vertical, a, b, &ends);
    detour_blocked(obstacles, &mut pts, &ends);
    pull_taut(obstacles, &mut pts, &ends);
    pts[1..pts.len() - 1].to_vec()
}

/// The shortest route from `a` to `b` through a gap in every row of boxes between them.
fn through_rows(
    obstacles: &mut Obstacles,
    vertical: bool,
    a: Point,
    b: Point,
    ends: &[u32],
) -> Vec<Point> {
    // Work in (u, v): u along the layers, v across them.
    let uv = |p: Point| if vertical { p } else { Point::new(p.y, p.x) };
    let (a, b) = (uv(a), uv(b));
    let span = b.y - a.y;
    if span.abs() < 1.0 {
        return vec![uv(a), uv(b)];
    }
    let forward = span > 0.0;
    let (v0, v1) = (a.y.min(b.y), a.y.max(b.y));

    // Boxes near the straight line, grouped into rows by their extent across the layers.
    let lo = Point::new(a.x.min(b.x) - CORRIDOR, v0);
    let hi = Point::new(a.x.max(b.x) + CORRIDOR, v1);
    let mut extents: Vec<(f32, f32)> = Vec::new();
    obstacles.in_rect(uv(lo), uv(hi), |i, (blo, bhi)| {
        let (blo, bhi) = (uv(blo), uv(bhi));
        if !ends.contains(&i) && blo.y < v1 && bhi.y > v0 {
            extents.push((blo.y, bhi.y));
        }
    });
    extents.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut rows: Vec<(f32, f32)> = Vec::new();
    for (top, bottom) in extents {
        match rows.last_mut() {
            Some(row) if top < row.1 => row.1 = row.1.max(bottom),
            _ => rows.push((top, bottom)),
        }
    }
    if !forward {
        rows.reverse();
    }

    // The gaps each row offers, and where the route would enter and leave it.
    struct Row {
        gaps: Vec<(f32, f32)>,
        enter: f32,
        leave: f32,
    }
    let rows: Vec<Row> = rows
        .into_iter()
        .filter_map(|(top, bottom)| {
            let centre = ((top + bottom) / 2.0).clamp(v0, v1);
            let ideal = a.x + (b.x - a.x) * (centre - a.y) / span;
            let gaps = gaps_in_row(obstacles, vertical, (top, bottom), ideal, ends);
            let (enter, leave) = if forward {
                (top.max(v0), bottom.min(v1))
            } else {
                (bottom.min(v1), top.max(v0))
            };
            (!gaps.is_empty()).then_some(Row { gaps, enter, leave })
        })
        .collect();

    // Choose a gap in every row. Moving sideways between rows costs more the less room there
    // is for it (shift² / room), so the route changes sides where there is space to do so
    // instead of running flat along the rows.
    let cost = |g: (f32, f32), h: (f32, f32), room: f32| {
        let d = (h.0 - g.1).max(g.0 - h.1).max(0.0);
        d * d / room.abs().max(1.0) + d * 0.01
    };
    // best[k][j]: the cheapest way to reach gap j of row k, and the gap of row k - 1 it
    // comes from.
    let mut best: Vec<Vec<(f32, usize)>> = Vec::with_capacity(rows.len());
    for (k, row) in rows.iter().enumerate() {
        let options = row
            .gaps
            .iter()
            .map(|&g| match k {
                0 => (cost((a.x, a.x), g, row.enter - a.y), 0),
                _ => {
                    let prev = &rows[k - 1];
                    let room = row.enter - prev.leave;
                    (0..prev.gaps.len())
                        .map(|i| (best[k - 1][i].0 + cost(prev.gaps[i], g, room), i))
                        .min_by(|x, y| x.0.total_cmp(&y.0))
                        .expect("rows have gaps")
                }
            })
            .collect();
        best.push(options);
    }
    let mut chosen = vec![0; rows.len()];
    if let Some(last) = rows.last() {
        let k = rows.len() - 1;
        let total = |i: usize| best[k][i].0 + cost(last.gaps[i], (b.x, b.x), b.y - last.leave);
        chosen[k] = (0..last.gaps.len())
            .min_by(|&i, &j| total(i).total_cmp(&total(j)))
            .expect("rows have gaps");
        for k in (1..rows.len()).rev() {
            chosen[k - 1] = best[k][chosen[k]].1;
        }
    }

    // A portal (the chosen gap, as a line across the row) where the route enters each row and
    // one where it leaves; left and right as seen travelling along the route.
    let mut portals: Vec<(Point, Point)> = vec![(a, a)];
    for (row, &j) in rows.iter().zip(&chosen) {
        let (left, right) = row.gaps[j];
        for v in [row.enter, row.leave] {
            let (l, r) = (Point::new(left, v), Point::new(right, v));
            portals.push(if forward { (l, r) } else { (r, l) });
        }
    }
    portals.push((b, b));
    funnel(&portals).into_iter().map(uv).collect()
}

/// The gaps (between the boxes of other nodes, along the layers) through which a route could
/// cross the row spanning `(top, bottom)` across the layers, when it would ideally cross at
/// `ideal`: the one there, if any, and the nearest ones to either side.
fn gaps_in_row(
    obstacles: &mut Obstacles,
    vertical: bool,
    (top, bottom): (f32, f32),
    ideal: f32,
    ends: &[u32],
) -> Vec<(f32, f32)> {
    let uv = |p: Point| if vertical { p } else { Point::new(p.y, p.x) };
    let mut reach = 256.0f32;
    loop {
        let (from, to) = (ideal - reach, ideal + reach);
        let mut blocked: Vec<(f32, f32)> = Vec::new();
        obstacles.in_rect(
            uv(Point::new(from, top)),
            uv(Point::new(to, bottom)),
            |i, (blo, bhi)| {
                let (blo, bhi) = (uv(blo), uv(bhi));
                if !ends.contains(&i) && blo.y < bottom && bhi.y > top {
                    blocked.push((blo.x, bhi.x));
                }
            },
        );
        blocked.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut merged: Vec<(f32, f32)> = Vec::new();
        for (l, r) in blocked {
            match merged.last_mut() {
                Some(m) if l <= m.1 => m.1 = m.1.max(r),
                _ => merged.push((l, r)),
            }
        }
        // Gaps between the blocked stretches, within the window looked at.
        let mut gaps = Vec::with_capacity(merged.len() + 1);
        let mut at = from;
        for &(l, r) in &merged {
            if l > at {
                gaps.push((at, l));
            }
            at = at.max(r);
        }
        if at < to {
            gaps.push((at, to));
        }
        if gaps.is_empty() && reach < MAX_REACH {
            // Boxes all the way through the window: look further.
            reach *= 4.0;
            continue;
        }
        let at = gaps.partition_point(|g| g.1 < ideal);
        let first = at.saturating_sub(GAP_CHOICES);
        let last = (at + GAP_CHOICES).min(gaps.len());
        return gaps[first..last].to_vec();
    }
}

/// The shortest path through a sequence of portals (left and right end as seen travelling
/// along it), the first and last being the start and the end point: the "simple stupid funnel
/// algorithm". It bends only at portal ends.
fn funnel(portals: &[(Point, Point)]) -> Vec<Point> {
    let n = portals.len();
    let start = portals[0].0;
    let mut path = vec![start];
    let (mut apex, mut left, mut right) = (start, start, start);
    let (mut left_at, mut right_at) = (0, 0);
    let mut i = 1;
    while i < n {
        let (l, r) = portals[i];
        // Narrow the funnel from the right.
        if cross(sub(right, apex), sub(r, apex)) >= 0.0 {
            if same(apex, right) || cross(sub(left, apex), sub(r, apex)) < 0.0 {
                right = r;
                right_at = i;
            } else {
                // The right side crosses the left: the left corner is a bend.
                path.push(left);
                apex = left;
                (right, right_at) = (apex, left_at);
                i = left_at + 1;
                continue;
            }
        }
        // Narrow it from the left.
        if cross(sub(left, apex), sub(l, apex)) <= 0.0 {
            if same(apex, left) || cross(sub(right, apex), sub(l, apex)) > 0.0 {
                left = l;
                left_at = i;
            } else {
                path.push(right);
                apex = right;
                (left, left_at) = (apex, right_at);
                i = right_at + 1;
                continue;
            }
        }
        i += 1;
    }
    path.push(portals[n - 1].0);
    path.dedup_by(|x, y| same(*x, *y));
    if path.len() == 1 {
        path.push(path[0]);
    }
    path
}

fn same(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
}

/// Walks around every box that a segment of `pts` still runs into, corner by corner.
fn detour_blocked(obstacles: &mut Obstacles, pts: &mut Vec<Point>, ends: &[u32]) {
    let mut detours = 0;
    let mut i = 0;
    while i + 1 < pts.len() {
        let hit = if detours < MAX_DETOURS {
            obstacles.first_hit(pts[i], pts[i + 1], ends)
        } else {
            None
        };
        match hit.and_then(|o| obstacles.box_of(o as usize)) {
            Some(bounds) => {
                detours += 1;
                let around = detour(pts[i], pts[i + 1], bounds);
                pts.splice(i + 1..i + 1, around);
            }
            None => i += 1,
        }
    }
}

/// The corners by which the segment from `p` to `q` goes around the box `(lo, hi)` it runs
/// into: those on the side of the segment that makes the shorter way round, in order.
fn detour(p: Point, q: Point, (lo, hi): (Point, Point)) -> Vec<Point> {
    let d = sub(q, p);
    let corners = [lo, Point::new(hi.x, lo.y), hi, Point::new(lo.x, hi.y)];
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for c in corners {
        let side = cross(d, sub(c, p));
        if side >= 0.0 {
            left.push(c);
        }
        if side <= 0.0 {
            right.push(c);
        }
    }
    let along = |c: &Point| dot(sub(*c, p), d);
    for side in [&mut left, &mut right] {
        side.sort_by(|a, b| along(a).total_cmp(&along(b)));
    }
    let length = |side: &[Point]| {
        let mut at = p;
        let mut total = 0.0;
        for &c in side.iter().chain([&q]) {
            total += len(sub(c, at));
            at = c;
        }
        total
    };
    match (left.is_empty(), right.is_empty()) {
        (true, _) => right,
        (_, true) => left,
        _ if length(&left) <= length(&right) => left,
        _ => right,
    }
}

/// Drops every bend that the route can do without: from each point, goes straight to the
/// farthest later point it can reach without running into a box.
fn pull_taut(obstacles: &mut Obstacles, pts: &mut Vec<Point>, ends: &[u32]) {
    let last = pts.len() - 1;
    let mut out = vec![pts[0]];
    let mut i = 0;
    while i < last {
        let mut j = last;
        while j > i + 1 && obstacles.first_hit(pts[i], pts[j], ends).is_some() {
            j -= 1;
        }
        out.push(pts[j]);
        i = j;
    }
    *pts = out;
}

/// True if `p` lies inside the box (not on its border).
fn inside(p: Point, lo: Point, hi: Point) -> bool {
    p.x > lo.x + TOUCH && p.x < hi.x - TOUCH && p.y > lo.y + TOUCH && p.y < hi.y - TOUCH
}

/// Where (as a fraction from `p`) the segment from `p` to `q` enters the inside of the box, if
/// it does.
pub fn entry(p: Point, q: Point, lo: Point, hi: Point) -> Option<f32> {
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (a, d, l, h) in [
        (p.x, q.x - p.x, lo.x + TOUCH, hi.x - TOUCH),
        (p.y, q.y - p.y, lo.y + TOUCH, hi.y - TOUCH),
    ] {
        if l >= h {
            return None;
        }
        if d == 0.0 {
            if a <= l || a >= h {
                return None;
            }
        } else {
            let (mut ta, mut tb) = ((l - a) / d, (h - a) / d);
            if ta > tb {
                std::mem::swap(&mut ta, &mut tb);
            }
            t0 = t0.max(ta);
            t1 = t1.min(tb);
            if t0 >= t1 {
                return None;
            }
        }
    }
    Some(t0)
}

fn key(p: Point) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

fn cells_of(lo: Point, hi: Point) -> impl Iterator<Item = (i32, i32)> {
    let ((x0, y0), (x1, y1)) = (key(lo), key(hi));
    (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
}

/// Visits the grid cells that the segment from `p` to `q` passes through, in order.
fn cells_along(p: Point, q: Point, mut visit: impl FnMut((i32, i32))) {
    let (mut cx, mut cy) = key(p);
    let (ex, ey) = key(q);
    let d = sub(q, p);
    let step = |v: f32| if v > 0.0 { 1 } else { -1 };
    let first = |a: f32, v: f32, c: i32| {
        if v > 0.0 {
            ((c + 1) as f32 * CELL - a) / v
        } else if v < 0.0 {
            (c as f32 * CELL - a) / v
        } else {
            f32::INFINITY
        }
    };
    let (mut tx, mut ty) = (first(p.x, d.x, cx), first(p.y, d.y, cy));
    let delta = |v: f32| {
        if v == 0.0 {
            f32::INFINITY
        } else {
            CELL / v.abs()
        }
    };
    let (dx, dy) = (delta(d.x), delta(d.y));
    // Rounding can make the walk miss the last cell by one; it never needs more steps.
    let steps = (ex - cx).unsigned_abs() + (ey - cy).unsigned_abs();
    visit((cx, cy));
    for _ in 0..steps {
        if tx < ty {
            cx += step(d.x);
            tx += dx;
        } else {
            cy += step(d.y);
            ty += dy;
        }
        visit((cx, cy));
    }
    if (cx, cy) != (ex, ey) {
        visit((ex, ey));
    }
}

fn add(a: Point, b: Point) -> Point {
    Point::new(a.x + b.x, a.y + b.y)
}

fn sub(a: Point, b: Point) -> Point {
    Point::new(a.x - b.x, a.y - b.y)
}

fn dot(a: Point, b: Point) -> f32 {
    a.x * b.x + a.y * b.y
}

fn cross(a: Point, b: Point) -> f32 {
    a.x * b.y - a.y * b.x
}

fn len(a: Point) -> f32 {
    dot(a, a).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(boxes: &[(Point, Point)]) -> Obstacles {
        let mut o = Obstacles::new(boxes.len());
        for (i, &(c, h)) in boxes.iter().enumerate() {
            o.place(i, c, h);
        }
        o
    }

    /// True if no segment of the path enters a box other than the ends'.
    fn clear(o: &mut Obstacles, path: &[Point], ends: [u32; 2]) -> bool {
        path.windows(2)
            .all(|w| o.first_hit(w[0], w[1], &ends).is_none())
    }

    #[test]
    fn straight_when_nothing_is_in_the_way() {
        let mut o = world(&[
            (Point::new(0.0, 0.0), Point::new(30.0, 10.0)),
            (Point::new(0.0, 200.0), Point::new(30.0, 10.0)),
            (Point::new(300.0, 100.0), Point::new(30.0, 10.0)),
        ]);
        let r = route(
            &mut o,
            true,
            Point::new(0.0, 0.0),
            Point::new(0.0, 200.0),
            [0, 1],
        );
        assert!(r.is_empty(), "{r:?}");
    }

    #[test]
    fn goes_around_a_box_in_the_way_with_few_bends() {
        let mut o = world(&[
            (Point::new(0.0, 0.0), Point::new(30.0, 10.0)),
            (Point::new(20.0, 300.0), Point::new(30.0, 10.0)),
            (Point::new(10.0, 150.0), Point::new(60.0, 20.0)),
        ]);
        let (a, b) = (Point::new(0.0, 0.0), Point::new(20.0, 300.0));
        let r = route(&mut o, true, a, b, [0, 1]);
        assert!(!r.is_empty() && r.len() <= 2, "{r:?}");
        let path: Vec<Point> = [a].into_iter().chain(r).chain([b]).collect();
        assert!(clear(&mut o, &path, [0, 1]), "{path:?}");
    }

    #[test]
    fn threads_between_boxes_in_a_row() {
        // A row of boxes with gaps, between the two ends.
        let mut boxes = vec![
            (Point::new(0.0, 0.0), Point::new(30.0, 10.0)),
            (Point::new(0.0, 400.0), Point::new(30.0, 10.0)),
        ];
        for k in -3..=3 {
            boxes.push((Point::new(k as f32 * 100.0, 200.0), Point::new(40.0, 10.0)));
        }
        let mut o = world(&boxes);
        let (a, b) = (Point::new(0.0, 0.0), Point::new(0.0, 400.0));
        let r = route(&mut o, true, a, b, [0, 1]);
        let path: Vec<Point> = [a].into_iter().chain(r.clone()).chain([b]).collect();
        assert!(clear(&mut o, &path, [0, 1]), "{path:?}");
        assert!(r.len() <= 2, "{r:?}");
    }

    #[test]
    fn funnel_bends_only_where_it_must() {
        let p = Point::new;
        // A straight corridor: no bends.
        let straight = funnel(&[
            (p(0.0, 0.0), p(0.0, 0.0)),
            (p(-10.0, 10.0), p(10.0, 10.0)),
            (p(-10.0, 20.0), p(10.0, 20.0)),
            (p(0.0, 30.0), p(0.0, 30.0)),
        ]);
        assert_eq!(straight, [p(0.0, 0.0), p(0.0, 30.0)]);
        // A gap off to the right (larger x) of the line: one bend at its near side, entering
        // and leaving.
        let around = funnel(&[
            (p(0.0, 0.0), p(0.0, 0.0)),
            (p(20.0, 10.0), p(40.0, 10.0)),
            (p(20.0, 20.0), p(40.0, 20.0)),
            (p(0.0, 30.0), p(0.0, 30.0)),
        ]);
        assert_eq!(
            around,
            [p(0.0, 0.0), p(20.0, 10.0), p(20.0, 20.0), p(0.0, 30.0)]
        );
        // Travelling the other way, left and right swap.
        let back = funnel(&[
            (p(0.0, 30.0), p(0.0, 30.0)),
            (p(40.0, 20.0), p(20.0, 20.0)),
            (p(40.0, 10.0), p(20.0, 10.0)),
            (p(0.0, 0.0), p(0.0, 0.0)),
        ]);
        assert_eq!(
            back,
            [p(0.0, 30.0), p(20.0, 20.0), p(20.0, 10.0), p(0.0, 0.0)]
        );
    }

    #[test]
    fn crosses_rows_at_the_side_of_a_gap() {
        // Rows of boxes at y = 100 and y = 200, each blocking the straight line.
        let mut boxes = vec![
            (Point::new(0.0, 0.0), Point::new(30.0, 10.0)),
            (Point::new(0.0, 300.0), Point::new(30.0, 10.0)),
        ];
        for y in [100.0, 200.0] {
            for k in -2..=2 {
                boxes.push((
                    Point::new(k as f32 * 120.0 + 10.0, y),
                    Point::new(50.0, 10.0),
                ));
            }
        }
        for vertical in [true, false] {
            let flip = |p: Point| if vertical { p } else { Point::new(p.y, p.x) };
            let mut o = world(
                &boxes
                    .iter()
                    .map(|&(c, h)| (flip(c), flip(h)))
                    .collect::<Vec<_>>(),
            );
            let (a, b) = (flip(Point::new(0.0, 10.0)), flip(Point::new(0.0, 290.0)));
            let r = route(&mut o, vertical, a, b, [0, 1]);
            let path: Vec<Point> = [a].into_iter().chain(r.clone()).chain([b]).collect();
            assert!(clear(&mut o, &path, [0, 1]), "{path:?}");
            // Bends sit at the edges of rows (grown by the clearance), in the gap.
            for q in r {
                let v = flip(q).y;
                let at_row_edge = [100.0f32, 200.0]
                    .iter()
                    .any(|y| ((v - y).abs() - (10.0 + CLEARANCE)).abs() < 0.01);
                assert!(at_row_edge, "{:?} in {path:?}", flip(q));
            }
        }
    }

    #[test]
    fn walks_every_cell_a_segment_passes() {
        let (p, q) = (Point::new(-250.0, 10.0), Point::new(420.0, -333.0));
        let mut walked = Vec::new();
        cells_along(p, q, |c| walked.push(c));
        for k in 0..=1000 {
            let t = k as f32 / 1000.0;
            let at = Point::new(p.x + (q.x - p.x) * t, p.y + (q.y - p.y) * t);
            assert!(walked.contains(&key(at)), "missed {:?}", key(at));
        }
    }
}
