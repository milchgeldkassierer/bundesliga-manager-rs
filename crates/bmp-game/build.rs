//! Embeds the Mod 2026 crest pictures `docs/mod2026/crests/<n>.png` into the program: writes
//! `$OUT_DIR/mod2026_crests.rs`, a sorted array of `(n, include_bytes!(<n>.png))`.

use std::fmt::Write;
use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let dir = Path::new(&manifest).join("../../docs/mod2026/crests");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut crests: Vec<(u8, String)> = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let Some(stem) = path
            .extension()
            .filter(|ext| *ext == "png")
            .and_then(|_| path.file_stem())
            .and_then(|stem| stem.to_str())
        else {
            continue;
        };
        let n: u8 = stem
            .parse()
            .unwrap_or_else(|_| panic!("{}: not a picture number", path.display()));
        println!("cargo:rerun-if-changed={}", path.display());
        let absolute = std::fs::canonicalize(&path).unwrap();
        crests.push((n, absolute.to_str().unwrap().to_owned()));
    }
    crests.sort();
    let mut code = String::from("&[\n");
    for (n, path) in &crests {
        writeln!(code, "    ({n}, include_bytes!({path:?})),").unwrap();
    }
    code.push(']');
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("mod2026_crests.rs");
    std::fs::write(out, code).unwrap();
}
