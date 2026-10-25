//! Embeds the Mod 2026 crest pictures into the program: `docs/mod2026/crests/<n>.png` as
//! `$OUT_DIR/mod2026_crests.rs` and their 2x versions `docs/mod2026/crests/hires/<n>.png` as
//! `$OUT_DIR/mod2026_hires_crests.rs`, each a sorted array of `(n, include_bytes!(<n>.png))`.

use std::fmt::Write;
use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let crests = Path::new(&manifest).join("../../docs/mod2026/crests");
    embed(&crests, "mod2026_crests.rs");
    embed(&crests.join("hires"), "mod2026_hires_crests.rs");
}

/// Writes `$OUT_DIR/<out_file>` with every `<n>.png` directly in `dir`.
fn embed(dir: &Path, out_file: &str) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut crests: Vec<(u8, String)> = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
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
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join(out_file);
    std::fs::write(out, code).unwrap();
}
