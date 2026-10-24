//! Lists the differences between two `.MAN` saves block by block.
//! Usage: cargo run -p bmp-formats --example savediff -- A.MAN B.MAN
use bmp_formats::savegame::{BLOCKS, SaveGame};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let load = |p: &str| SaveGame::parse(&std::fs::read(p).expect("read")).expect("parse");
    let (a, b) = (load(&args[0]), load(&args[1]));
    println!(
        "header: id {:08x}/{:08x} pad {}/{} key {}/{}",
        a.save_id, b.save_id, a.pad, b.pad, a.initial_key, b.initial_key
    );
    for (i, d) in BLOCKS.iter().enumerate() {
        let (x, y) = (&a.blocks[i], &b.blocks[i]);
        let diffs: Vec<usize> = (0..x.len()).filter(|&k| x[k] != y[k]).collect();
        if diffs.is_empty() {
            continue;
        }
        println!(
            "block {i} {:?}:{:04x} len {:#x}: {} bytes differ",
            d.segment,
            d.offset,
            d.len,
            diffs.len()
        );
        for &k in diffs.iter().take(400) {
            println!("  +{k:#06x} ({}): {:3} {:3}", k, x[k], y[k]);
        }
    }
    if a.strings != b.strings {
        println!("strings differ");
    }
    if a.tail != b.tail {
        println!("tail {:02x?} {:02x?}", a.tail, b.tail);
    }
}
