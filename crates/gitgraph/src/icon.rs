//! The window icon, drawn in code: a tiny revision graph in TortoiseGit's colours.

use eframe::egui::IconData;

const SIZE: usize = 64;

pub fn icon() -> IconData {
    let mut px = vec![[0u8; 4]; SIZE * SIZE];
    let edge = [40, 40, 40, 255];
    // Edges from the two tips down to the root.
    line(&mut px, (18.0, 18.0), (32.0, 48.0), 3.5, edge);
    line(&mut px, (46.0, 18.0), (32.0, 48.0), 3.5, edge);
    // Nodes: local branch (green), remote branch (orange), tag (yellow).
    rounded_rect(&mut px, (3.0, 10.0, 30.0, 26.0), 5.0, [0, 195, 0, 255]);
    rounded_rect(&mut px, (34.0, 10.0, 61.0, 26.0), 5.0, [255, 221, 170, 255]);
    rounded_rect(&mut px, (16.0, 40.0, 48.0, 56.0), 5.0, [255, 230, 0, 255]);
    IconData {
        rgba: px.into_iter().flatten().collect(),
        width: SIZE as u32,
        height: SIZE as u32,
    }
}

/// Coverage-based anti-aliasing: sample each pixel 4x4 times.
fn paint(px: &mut [[u8; 4]], color: [u8; 4], inside: impl Fn(f32, f32) -> bool) {
    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut hits = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let (fx, fy) = (
                        x as f32 + (sx as f32 + 0.5) / 4.0,
                        y as f32 + (sy as f32 + 0.5) / 4.0,
                    );
                    hits += inside(fx, fy) as u32;
                }
            }
            if hits == 0 {
                continue;
            }
            let a = hits as f32 / 16.0 * color[3] as f32 / 255.0;
            let dst = &mut px[y * SIZE + x];
            for c in 0..3 {
                dst[c] = (color[c] as f32 * a + dst[c] as f32 * (1.0 - a)).round() as u8;
            }
            dst[3] = ((a + dst[3] as f32 / 255.0 * (1.0 - a)) * 255.0).round() as u8;
        }
    }
}

fn line(px: &mut [[u8; 4]], a: (f32, f32), b: (f32, f32), width: f32, color: [u8; 4]) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    paint(px, color, |x, y| {
        let t = (((x - a.0) * dx + (y - a.1) * dy) / len2).clamp(0.0, 1.0);
        let (cx, cy) = (a.0 + t * dx - x, a.1 + t * dy - y);
        cx * cx + cy * cy <= width * width / 4.0
    });
}

fn rounded_rect(
    px: &mut [[u8; 4]],
    (x0, y0, x1, y1): (f32, f32, f32, f32),
    r: f32,
    color: [u8; 4],
) {
    paint(px, color, |x, y| {
        let cx = x.clamp(x0 + r, x1 - r);
        let cy = y.clamp(y0 + r, y1 - r);
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r && x >= x0 && x <= x1 && y >= y0 && y <= y1
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_has_opaque_and_transparent_pixels() {
        let icon = super::icon();
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);
        let alphas: Vec<u8> = icon.rgba.chunks(4).map(|p| p[3]).collect();
        assert!(alphas.contains(&0) && alphas.contains(&255));
    }
}
