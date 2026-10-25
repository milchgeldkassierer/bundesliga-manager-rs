//! MOD2026: writes the 2x fonts `docs/mod2026/fonts/*.txt` from the original fonts in a
//! data directory (read only): `cargo run -p bmp-formats --example hires_fonts -- <data dir>`.

use std::path::{Path, PathBuf};

use bmp_formats::font::{FontId, FontSet};
use bmp_formats::hires_font;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [data] = args.as_slice() else {
        eprintln!("usage: hires_fonts <data dir>");
        std::process::exit(2);
    };
    let set = FontSet::load(Path::new(data)).unwrap_or_else(|e| {
        eprintln!("{data}: {e}");
        std::process::exit(1);
    });
    let dir = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/mod2026/fonts"
    ));
    for id in FontId::ALL {
        let path = dir.join(hires_font::file_name(id));
        std::fs::write(&path, hires_font::font_file(id, set.get(id))).unwrap_or_else(|e| {
            eprintln!("{}: {e}", path.display());
            std::process::exit(1);
        });
        println!("{}", path.display());
    }
}
