//! Writes every icon asset from the drawing in `parterre_core::icon` into `packaging/icon/`:
//! the SVG, PNGs at the usual sizes, the Windows `.ico` (embedded in the `.exe` by
//! `crates/parterre/build.rs`) and the macOS `.icns`.
//!
//! Usage: `cargo run --release -p parterre-core --example icon_assets`

use std::fs;
use std::path::PathBuf;

use parterre_core::icon;

/// The `.icns` chunk types and the pixel side each holds.
const ICNS: [(&[u8; 4], u32); 11] = [
    (b"icp4", 16),
    (b"icp5", 32),
    (b"icp6", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
    (b"ic11", 32),  // 16 pt @2x
    (b"ic12", 64),  // 32 pt @2x
    (b"ic13", 256), // 128 pt @2x
    (b"ic14", 512), // 256 pt @2x
];

fn png(side: u32, rgba: Vec<u8>) -> Vec<u8> {
    let img = image::RgbaImage::from_raw(side, side, rgba).expect("pixel count");
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .expect("encode");
    out.into_inner()
}

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packaging/icon");
    fs::create_dir_all(&dir).expect("create packaging/icon");
    let write = |name: &str, data: Vec<u8>| {
        fs::write(dir.join(name), &data).expect("write");
        println!("{name}: {} bytes", data.len());
    };

    write("parterre.svg", icon::svg().into_bytes());
    for side in [16, 24, 32, 48, 64, 128, 256, 512] {
        write(
            &format!("parterre-{side}.png"),
            png(side, icon::render(side)),
        );
    }

    // Bitmaps up to 128, PNG at 256: the layout every Windows since Vista reads.
    let mut entries: Vec<(u32, Vec<u8>)> = [16, 24, 32, 48, 64, 128]
        .into_iter()
        .map(|side| (side, icon::ico_dib(side, &icon::render(side))))
        .collect();
    entries.push((256, png(256, icon::render(256))));
    write("parterre.ico", icon::ico(&entries));

    // macOS draws its rounded squares inside a transparent margin, so the tile is inset.
    let chunks: Vec<([u8; 4], Vec<u8>)> = ICNS
        .iter()
        .map(|&(kind, side)| {
            (
                *kind,
                png(side, icon::render_inset(side, icon::MACOS_INSET)),
            )
        })
        .collect();
    write("parterre.icns", icon::icns(&chunks));
}
