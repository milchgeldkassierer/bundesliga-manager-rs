//! The team editor's save on Mod 2026 data (#224): "MACH MAL !" recomputes the strengths with
//! the generator's grouping, "OH NEIN !" and a click outside the question write nothing.
//! Needs `BMP_DATA`.

use std::path::{Path, PathBuf};
use std::process::Command;

use bmp_formats::mana::{self, ManaDat};
use bmp_game::rng::Rng;
use bmp_game::strength;

/// A 3. Liga club (HANSA ROSTOCK) and a pool club (1.FC LOK LEIPZIG).
const CLUBS: [usize; 2] = [40, 57];

/// Mod 2026 data built from `BMP_DATA` in a temp directory, removed on drop.
struct ModData(PathBuf);

impl Drop for ModData {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn mod_data(name: &str) -> Option<ModData> {
    let original = bmp_formats::data_dir()?;
    let dir = std::env::temp_dir().join(format!(
        "bmp-mod2026-teamed-save-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    bmp_game::mod2026::build(&original, &dir).unwrap();
    Some(ModData(dir))
}

/// Runs `bmp teamed` headless on `data` with the script `lines`; asserts that it exits 0.
fn teamed(data: &Path, lines: &str) {
    let script = data.join("save.port.txt");
    let shots = data.join("shots");
    std::fs::write(&script, lines).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bmp"))
        .arg("--data")
        .arg(data)
        .arg("teamed")
        .arg("--script")
        .arg(&script)
        .arg("--shots")
        .arg(&shots)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn read_mana(data: &Path) -> Vec<u8> {
    std::fs::read(data.join("MANA.DAT")).unwrap()
}

/// Overwrites the strengths of [`CLUBS`] with a value no recomputation yields, keeping their
/// tables; returns the file's new bytes.
fn spoil_strengths(data: &Path) -> Vec<u8> {
    let mut m = ManaDat::parse(&read_mana(data)).unwrap();
    for i in CLUBS {
        m.strength[i] = [1, 1, 1];
    }
    let bytes = m.to_bytes();
    std::fs::write(data.join("MANA.DAT"), &bytes).unwrap();
    bytes
}

/// PROGRAMM-ENDE, then the click `answer` on the save question.
fn save_script(answer: &str) -> String {
    format!("wait 2000\nclick 77 78\nwait 500\nclick {answer}\nwait 1000\n")
}

#[test]
fn mach_mal_keeps_the_generator_grouping() {
    let Some(data) = mod_data("save") else {
        return;
    };
    let before = ManaDat::parse(&spoil_strengths(&data.0)).unwrap();
    let recompute = |grouping| {
        let mut m = before.clone();
        strength::recompute_strengths_grouped(&mut m, &mut Rng::default(), grouping);
        m.strength
    };
    let generator = recompute(&strength::MOD2026);
    let original = recompute(&strength::ORIGINAL);
    assert!(
        CLUBS.iter().any(|&i| generator[i] != original[i]),
        "the original grouping must differ for at least one club"
    );

    teamed(&data.0, &save_script("115 190"));

    let after = ManaDat::parse(&read_mana(&data.0)).unwrap();
    for i in CLUBS {
        assert_ne!(after.strength[i], [1, 1, 1], "club {i} not recomputed");
        assert_eq!(after.strength[i], generator[i], "club {i}");
    }
    // The whole 3. Liga and pool keep the strengths the generator wrote.
    assert_eq!(
        after.strength[36..mana::GERMAN_CLUB_COUNT],
        generator[36..mana::GERMAN_CLUB_COUNT]
    );
}

#[test]
fn oh_nein_and_outside_write_nothing() {
    let Some(data) = mod_data("cancel") else {
        return;
    };
    let before = spoil_strengths(&data.0);
    // "OH NEIN !".
    teamed(&data.0, &save_script("196 190"));
    assert!(read_mana(&data.0) == before, "OH NEIN ! wrote MANA.DAT");
    // A click outside the question: back to the editor, the script ends there.
    teamed(&data.0, &save_script("10 10"));
    assert!(
        read_mana(&data.0) == before,
        "the cancelled save wrote MANA.DAT"
    );
}
