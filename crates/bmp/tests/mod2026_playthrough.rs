//! The Mod 2026 playthrough (PRD R7, #194): the scripts `tests/mod2026/playthrough_*.port.txt`
//! run headless on Mod 2026 data built from `BMP_DATA`, and their key screenshots must equal the
//! port-vs-port golden frames in `tests/mod2026/golden/` pixel for pixel. Needs `BMP_DATA`.
//!
//! Regenerate the goldens after an intended change of these screens and review them before
//! committing: `BMP_PLAYTHROUGH_GOLDEN_WRITE=1 cargo test -p bmp --test mod2026_playthrough`.

use std::path::{Path, PathBuf};
use std::process::Command;

use bmp_formats::savegame::SaveGame;
use bmp_game::state::GameState;

/// The seeds of the scripts' `# seed:` and `# seed2:` lines.
const SEEDS: [&str; 2] = ["45817", "45841"];

fn repo_file(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

/// A fresh directory below the system temp directory.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "bmp-mod2026-playthrough-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A finished run of a playthrough script: the mod data directory it played in and its
/// screenshots. Both are removed on drop.
struct Run {
    data: PathBuf,
    shots: PathBuf,
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.data);
        let _ = std::fs::remove_dir_all(&self.shots);
    }
}

/// Builds the Mod 2026 data from `original` into a temp directory and runs the script
/// `tests/mod2026/<script>.port.txt` headless on it; asserts that `bmp` exits 0.
fn play(original: &Path, script: &str) -> Run {
    let run = Run {
        data: temp_dir(&format!("{script}-data")),
        shots: temp_dir(&format!("{script}-shots")),
    };
    bmp_game::mod2026::build(original, &run.data).unwrap();
    std::fs::create_dir_all(&run.shots).unwrap();
    let script_path = repo_file(&format!("tests/mod2026/{script}.port.txt"));
    let output = Command::new(env!("CARGO_BIN_EXE_bmp"))
        .arg("--data")
        .arg(&run.data)
        .arg("run")
        .arg("--script")
        .arg(&script_path)
        .arg("--shots")
        .arg(&run.shots)
        .args(["--time", SEEDS[0], "--time2", SEEDS[1]])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{script}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    run
}

/// The size and RGB pixels of a PNG written by `bmp run --shots`.
fn read_png(path: &Path) -> (u32, u32, Vec<u8>) {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).unwrap();
    pixels.truncate(info.buffer_size());
    (info.width, info.height, pixels)
}

/// Compares each screenshot `shot` of `run` with its golden `tests/mod2026/golden/<golden>.png`,
/// or writes the goldens when `BMP_PLAYTHROUGH_GOLDEN_WRITE` is set.
fn assert_goldens(run: &Run, shots: &[(&str, &str)]) {
    let write = std::env::var_os("BMP_PLAYTHROUGH_GOLDEN_WRITE").is_some();
    let mut differing = Vec::new();
    for (shot, golden) in shots {
        let actual = run.shots.join(format!("{shot}.png"));
        let golden = repo_file(&format!("tests/mod2026/golden/{golden}.png"));
        if write {
            std::fs::copy(&actual, &golden).unwrap();
            continue;
        }
        let (w, h, want) = read_png(&golden);
        let (aw, ah, got) = read_png(&actual);
        if (w, h) != (aw, ah) {
            differing.push(format!("{shot}: {aw}x{ah} instead of {w}x{h}"));
        } else if got != want {
            let pixels = got
                .chunks(3)
                .zip(want.chunks(3))
                .filter(|(a, b)| a != b)
                .count();
            differing.push(format!("{shot}: {pixels} pixels differ"));
        }
    }
    assert!(differing.is_empty(), "{differing:#?}");
}

/// The clubs of each league tier's table: the longest prefix of `league_teams[tier]` whose clubs
/// hold the table positions 0, 1, 2, ... in order.
fn tier_tables(state: &GameState) -> [Vec<u8>; 3] {
    state.league_teams.map(|teams| {
        teams
            .iter()
            .enumerate()
            .take_while(|&(i, &team)| {
                state
                    .team_tables
                    .get(usize::from(team))
                    .is_some_and(|t| usize::from(t.table_position) == i)
            })
            .map(|(_, &team)| team)
            .collect()
    })
}

/// Run A: a new endless game through match day 1 and the whole season 2026/27 to the first
/// office of 2027/28, where the script saves the game.
#[test]
fn endless_game_plays_the_season_to_its_end() {
    let Some(original) = bmp_formats::data_dir() else {
        eprintln!("skipped: BMP_DATA is not set");
        return;
    };
    let run = play(&original, "playthrough_endless");
    assert_goldens(
        &run,
        &[
            "a01-office",
            "a02-table-bundesliga",
            "a03-table-2-bundesliga",
            "a04-table-3-liga",
            "a08-play-off-1",
            "a09-play-off-2",
            "a11-season-end",
            "a12-office-2027",
        ]
        .map(|shot| (shot, shot)),
    );
    let save = SaveGame::parse(&std::fs::read(run.data.join("PLAYTHRU.MAN")).unwrap()).unwrap();
    let state = GameState::from_savegame(&save).unwrap();
    let tables = tier_tables(&state);
    assert_eq!(tables.each_ref().map(Vec::len), [18, 18, 20]);
    let mut clubs = tables.concat();
    clubs.sort_unstable();
    assert_eq!(
        clubs,
        (0..56).collect::<Vec<u8>>(),
        "every league club in one table"
    );
    // 29 July 2027 (`month` is 0-based): the first day of season 2027/28.
    assert_eq!((state.day_of_month, state.month, state.year), (29, 6, 2027));
}

/// Run B: a new one-year game, whose manager starts in the Bundesliga, through match day 1.
#[test]
fn one_year_game_plays_match_day_1_in_the_bundesliga() {
    let Some(original) = bmp_formats::data_dir() else {
        eprintln!("skipped: BMP_DATA is not set");
        return;
    };
    let run = play(&original, "playthrough_one_year");
    // The office does not show the league, so run B's office is run A's frame.
    assert_goldens(
        &run,
        &[
            ("b01-office", "a01-office"),
            ("b02-table-bundesliga", "b02-table-bundesliga"),
        ],
    );
}
