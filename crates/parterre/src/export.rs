//! SVG export of the whole graph (TortoiseGit: "Save graph as...").

use std::fmt::Write as _;

use eframe::egui::{Color32, CornerRadius, Pos2, Rect};

use crate::render::{ARROW_LEN, arrowhead_points, edge_path, node_rows, row_colors};
use crate::scene::{CORNER_RADIUS, FONT_SIZE, MARGIN_X, Scene};
use crate::settings::Settings;
use crate::theme::Palette;

const MARGIN: f32 = 20.0;
const EDGE_WIDTH: f32 = 2.0;

/// Renders the scene at 100% as a standalone SVG document.
pub fn to_svg(scene: &Scene, settings: &Settings, palette: &Palette) -> String {
    let bounds = scene.bounds().expand(MARGIN);
    let origin = bounds.min.to_vec2();
    let map = |p: Pos2| p - origin;
    let mut svg = String::new();
    let _ = writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}">"#,
        w = bounds.width(),
        h = bounds.height()
    );
    let _ = writeln!(
        svg,
        r#"<rect width="100%" height="100%" fill="{}"/>"#,
        hex(palette.background)
    );

    let _ = writeln!(
        svg,
        r#"<g fill="none" stroke="{}" stroke-width="{EDGE_WIDTH}" stroke-linejoin="round" stroke-linecap="round">"#,
        hex(palette.edge)
    );
    let mut heads = Vec::new();
    for e in 0..scene.graph.edges.len() {
        let Some(path) = edge_path(scene, e, settings.edge_style, map, None) else {
            continue;
        };
        let points: Vec<String> = path
            .iter()
            .map(|p| format!("{:.1},{:.1}", p.x, p.y))
            .collect();
        let _ = writeln!(svg, r#"<polyline points="{}"/>"#, points.join(" "));
        if let Some(head) = arrowhead_points(&path, settings.arrows, ARROW_LEN) {
            heads.extend(head);
        }
    }
    svg.push_str("</g>\n");
    let _ = writeln!(svg, r#"<g fill="{}">"#, hex(palette.edge));
    for tri in heads {
        let pts: Vec<String> = tri
            .iter()
            .map(|p| format!("{:.1},{:.1}", p.x, p.y))
            .collect();
        let _ = writeln!(svg, r#"<polygon points="{}"/>"#, pts.join(" "));
    }
    svg.push_str("</g>\n");

    let _ = writeln!(
        svg,
        r#"<g font-family="Consolas, 'DejaVu Sans Mono', Menlo, monospace" font-size="{FONT_SIZE}">"#
    );
    let radius = CORNER_RADIUS as u8;
    for (i, visual) in scene.visuals.iter().enumerate() {
        let r = scene.node_rect(i);
        let rect = Rect::from_min_max(map(r.min), map(r.max));
        for ((row_rect, corners), row) in
            node_rows(rect, visual.rows.len(), scene.row_height, radius).zip(&visual.rows)
        {
            let (fill, border, text) = row_colors(row, palette);
            let _ = writeln!(
                svg,
                r#"<path d="{}" fill="{}" stroke="{}"/>"#,
                rounded_rect(row_rect, corners),
                hex(fill),
                hex(border)
            );
            let _ = writeln!(
                svg,
                r#"<text x="{:.1}" y="{:.1}" dominant-baseline="central" fill="{}">{}</text>"#,
                row_rect.min.x + MARGIN_X,
                row_rect.center().y,
                hex(text),
                escape(&row.label)
            );
        }
    }
    svg.push_str("</g>\n</svg>\n");
    svg
}

/// SVG path of a rectangle with individually rounded corners (inset by half a pixel so the
/// 1 px border stays inside, as on screen).
fn rounded_rect(r: Rect, c: CornerRadius) -> String {
    let r = r.shrink(0.5);
    let (x0, y0, x1, y1) = (r.min.x, r.min.y, r.max.x, r.max.y);
    let (nw, ne, se, sw) = (c.nw as f32, c.ne as f32, c.se as f32, c.sw as f32);
    format!(
        "M{:.1},{y0:.1} H{:.1} A{ne},{ne} 0 0 1 {x1:.1},{:.1} V{:.1} A{se},{se} 0 0 1 {:.1},{y1:.1} H{:.1} A{sw},{sw} 0 0 1 {x0:.1},{:.1} V{:.1} A{nw},{nw} 0 0 1 {:.1},{y0:.1} Z",
        x0 + nw,
        x1 - ne,
        y0 + ne,
        y1 - se,
        x1 - se,
        x0 + sw,
        y1 - sw,
        y0 + nw,
        x0 + nw,
    )
}

fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup() {
        assert_eq!(escape("a<b>&\"c\""), "a&lt;b&gt;&amp;&quot;c&quot;");
    }

    #[test]
    fn colours_are_hex() {
        assert_eq!(hex(Color32::from_rgb(255, 221, 170)), "#ffddaa");
    }
}
