//! The window icon: the app icon from `parterre_core::icon`, rasterised at startup.

use eframe::egui::IconData;

const SIZE: u32 = 64;

pub fn icon() -> IconData {
    IconData {
        rgba: parterre_core::icon::render(SIZE),
        width: SIZE,
        height: SIZE,
    }
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
