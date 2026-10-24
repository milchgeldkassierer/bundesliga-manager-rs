//! Mod 2026 (`docs/mod2026/PRD.md`): the generator of the mod data directory and the switch
//! that tells mod data from the original data.
//!
//! Not part of the original program: everything here is mod-only (`docs/mod2026/CHANGES.md`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bmp_formats::font::encode_cp437;
use bmp_formats::mana::{GERMAN_CLUB_COUNT, ManaDat, PLAYER_NAME_LEN, PLAYERS_PER_CLUB};
use serde_json::Value;

use crate::rng::Rng;
use crate::strength;

/// The marker file whose presence in the data directory switches mod behaviour on.
pub const MARKER: &str = "MOD2026.DAT";

/// Fixed content of [`MARKER`], so that two builds of the same input are byte-identical.
const MARKER_CONTENT: &[u8] = b"Mod 2026 data directory, written by `bmp mod2026 build`.\n";

/// The clubs of the 64 German slots (`docs/mod2026/data/README.md`).
const CLUBS_JSON: &str = include_str!("../../../docs/mod2026/data/clubs.json");
/// The squads of the 64 German slots.
const SQUADS_JSON: &str = include_str!("../../../docs/mod2026/data/squads.json");

/// Whether `data_dir` holds mod data, i.e. contains [`MARKER`].
pub fn is_active(data_dir: &Path) -> bool {
    data_dir.join(MARKER).is_file()
}

/// Writes the mod data directory `out`: a copy of every file below `from` with the mod clubs
/// written into `MANA.DAT`, then [`MARKER`].
///
/// Refuses, before anything is created or written, an `out` that is `from` or lies inside
/// it, one that is or lies inside the owner's play directory `<repo>/BMP`, one that
/// already holds files, a `from` without a valid `MANA.DAT` and a dataset that does not fit it. The
/// copies are written as new files, so they do not inherit the read-only permissions of the
/// pristine copy.
pub fn build(from: &Path, out: &Path) -> Result<(), String> {
    let source = fs::canonicalize(from).map_err(|e| format!("{}: {e}", from.display()))?;
    if !source.is_dir() {
        return Err(format!("{}: not a directory", from.display()));
    }
    let target = resolve(out).map_err(|e| format!("{}: {e}", out.display()))?;
    if target.starts_with(&source) {
        return Err(format!(
            "--out {} is the --from directory or lies inside it",
            out.display()
        ));
    }
    let play_dir = resolve(&repo_play_dir()).map_err(|e| e.to_string())?;
    if target.starts_with(&play_dir) {
        return Err(format!(
            "--out {} is the play directory {}, which is never written",
            out.display(),
            play_dir.display()
        ));
    }
    if target.exists() {
        let mut entries = fs::read_dir(&target).map_err(|e| format!("{}: {e}", out.display()))?;
        if entries.next().is_some() {
            return Err(format!("--out {} is not empty", out.display()));
        }
    }
    let mana = mod_mana(&source)?;
    copy_tree(&source, &target).map_err(|e| format!("{}: {e}", out.display()))?;
    fs::write(target.join("MANA.DAT"), mana).map_err(|e| format!("{}: {e}", out.display()))?;
    fs::write(target.join(MARKER), MARKER_CONTENT).map_err(|e| format!("{}: {e}", out.display()))
}

/// The `MANA.DAT` of `source` with [`write_mod_clubs`] applied, as file bytes.
fn mod_mana(source: &Path) -> Result<Vec<u8>, String> {
    let path = source.join("MANA.DAT");
    let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut mana = ManaDat::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    write_mod_clubs(&mut mana)?;
    Ok(mana.to_bytes())
}

/// Writes the German slots 0..63 from the dataset (name, crest, the 2025/26 table as the
/// 2-point pair, goals and rank, players) and recomputes their strengths with
/// [`strength::MOD2026`]. The foreign clubs keep their bytes.
fn write_mod_clubs(mana: &mut ManaDat) -> Result<(), String> {
    let clubs = dataset(CLUBS_JSON, "clubs.json")?;
    let squads = dataset(SQUADS_JSON, "squads.json")?;
    for (slot, (club, squad)) in clubs.iter().zip(&squads).enumerate() {
        write_club(mana, slot, club, squad).map_err(|e| format!("dataset slot {slot}: {e}"))?;
    }
    // Fixed seed: the MSC start state (srand(1)) the team editor starts with, so two builds
    // write the same bytes.
    strength::recompute_strengths_grouped(mana, &mut Rng::default(), &strength::MOD2026);
    Ok(())
}

/// The array of 64 slots in `json`.
fn dataset(json: &str, file: &str) -> Result<Vec<Value>, String> {
    match serde_json::from_str(json).map_err(|e| format!("{file}: {e}"))? {
        Value::Array(slots) if slots.len() == GERMAN_CLUB_COUNT => Ok(slots),
        _ => Err(format!("{file}: not an array of {GERMAN_CLUB_COUNT} slots")),
    }
}

fn write_club(mana: &mut ManaDat, slot: usize, club: &Value, squad: &Value) -> Result<(), String> {
    let players = squad["players"]
        .as_array()
        .ok_or("`players` is not an array")?;
    if players.len() > PLAYERS_PER_CLUB {
        return Err(format!(
            "{} players, at most {PLAYERS_PER_CLUB}",
            players.len()
        ));
    }
    let table = &club["table_2025_26"];
    let [wins, draws, losses] = [
        int(table, "wins")?,
        int(table, "draws")?,
        int(table, "losses")?,
    ];
    mana.crest[slot] = byte(int(club, "crest")?, "crest")?;
    mana.points[slot] = [
        byte(2 * wins + draws, "plus points")?,
        byte(2 * losses + draws, "minus points")?,
    ];
    mana.goals[slot] = [
        byte(int(table, "goals_for")?, "goals_for")?,
        byte(int(table, "goals_against")?, "goals_against")?,
    ];
    mana.rank[slot] = byte(int(table, "rank")?, "rank")?;
    let record = &mut mana.clubs[slot];
    record.name = field(text(club, "name")?)?;
    for (k, player) in record.players.iter_mut().enumerate() {
        *player = match players.get(k) {
            Some(p) => field(text(p, "name")?)?,
            None => [0; PLAYER_NAME_LEN],
        };
    }
    Ok(())
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("`{key}` is not a string"))
}

fn int(value: &Value, key: &str) -> Result<i64, String> {
    value[key]
        .as_i64()
        .ok_or_else(|| format!("`{key}` is not an integer"))
}

fn byte(n: i64, what: &str) -> Result<u8, String> {
    u8::try_from(n).map_err(|_| format!("{what} {n} does not fit a byte"))
}

/// `name` in CP437, NUL-padded to a name field of `N` bytes that keeps its NUL terminator.
fn field<const N: usize>(name: &str) -> Result<[u8; N], String> {
    let bytes = encode_cp437(name);
    if bytes.len() >= N {
        return Err(format!("`{name}` is longer than {} bytes", N - 1));
    }
    let mut field = [0; N];
    field[..bytes.len()].copy_from_slice(&bytes);
    Ok(field)
}

/// `<repo>/BMP` of the checkout this crate was built from.
fn repo_play_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../BMP")
}

/// `path` made absolute with every symbolic link resolved, also when it does not exist
/// (yet): the longest existing ancestor is canonicalised and the rest appended.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut existing = absolute.as_path();
    let mut rest = Vec::new();
    loop {
        match fs::canonicalize(existing) {
            Ok(base) => return Ok(rest.iter().rev().fold(base, |p, c| p.join(c))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                rest.push(existing.file_name().ok_or(e)?.to_owned());
                existing = existing
                    .parent()
                    .expect("a path with a file name has a parent");
            }
            Err(e) => return Err(e),
        }
    }
}

/// Copies every file below `from` to the same relative path below `to`, in sorted order.
fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    let mut entries = fs::read_dir(from)?.collect::<io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let target = to.join(entry.file_name());
        if entry.metadata()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::write(target, fs::read(entry.path())?)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmp_formats::mana;

    /// A fresh, empty directory below the system temp directory.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bmp-mod2026-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Every file below `dir` as (relative path, bytes), sorted by path.
    fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(root: &Path, dir: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(root, &path, files);
                } else {
                    let bytes = fs::read(&path).unwrap();
                    files.push((path.strip_prefix(root).unwrap().to_path_buf(), bytes));
                }
            }
        }
        let mut files = Vec::new();
        walk(dir, dir, &mut files);
        files.sort();
        files
    }

    /// A `MANA.DAT` that parses: every byte zero.
    fn zeroed_mana() -> Vec<u8> {
        vec![0; mana::FILE_SIZE]
    }

    #[test]
    fn marker_decides_whether_mod_data_is_active() {
        let dir = temp("active");
        assert!(!is_active(&dir));
        fs::write(dir.join(MARKER), MARKER_CONTENT).unwrap();
        assert!(is_active(&dir));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_copies_the_data_and_writes_the_marker() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let dir = temp("copy");
        let out = dir.join("out");
        build(&data, &out).unwrap();
        let copied = tree(&out);
        let original = tree(&data);
        for file in &original {
            if file.0 == Path::new("MANA.DAT") {
                continue;
            }
            assert!(copied.contains(file), "{} not copied", file.0.display());
        }
        assert!(ManaDat::parse(&fs::read(out.join("MANA.DAT")).unwrap()).is_ok());
        assert!(is_active(&out));
        assert_eq!(
            copied.len(),
            original.len() + usize::from(!is_active(&data))
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_is_deterministic() {
        let dir = temp("twice");
        let from = dir.join("from");
        fs::create_dir_all(from.join("PIC")).unwrap();
        fs::write(from.join("MANA.DAT"), zeroed_mana()).unwrap();
        fs::write(from.join("PIC").join("80.VGA"), [4, 5]).unwrap();
        build(&from, &dir.join("a")).unwrap();
        build(&from, &dir.join("b")).unwrap();
        let a = tree(&dir.join("a"));
        assert_eq!(a.len(), 3);
        assert_eq!(a, tree(&dir.join("b")));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_refuses_unsafe_targets_and_writes_nothing() {
        let dir = temp("refuse");
        let from = dir.join("from");
        fs::create_dir_all(&from).unwrap();
        fs::write(from.join("MANA.DAT"), [1]).unwrap();
        let before = tree(&dir);
        let play_dir_existed = repo_play_dir().exists();

        assert!(build(&from, &from).is_err());
        assert!(build(&from, &from.join("sub")).is_err());
        assert!(build(&from, &repo_play_dir()).is_err());
        assert!(build(&from, &repo_play_dir().join("sub")).is_err());
        let full = dir.join("full");
        fs::create_dir_all(&full).unwrap();
        fs::write(full.join("keep"), [9]).unwrap();
        assert!(build(&from, &full).is_err());

        assert_eq!(tree(&full), [(PathBuf::from("keep"), vec![9])]);
        fs::remove_dir_all(&full).unwrap();
        assert_eq!(tree(&dir), before);
        assert_eq!(repo_play_dir().exists(), play_dir_existed);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn build_resolves_symbolic_links_before_refusing() {
        let dir = temp("link");
        let from = dir.join("from");
        fs::create_dir_all(&from).unwrap();
        fs::write(from.join("MANA.DAT"), [1]).unwrap();
        std::os::unix::fs::symlink(&from, dir.join("link")).unwrap();
        let before = tree(&dir);

        assert!(build(&from, &dir.join("link").join("sub")).is_err());

        assert_eq!(tree(&dir), before);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copies_of_read_only_files_are_writable() {
        let dir = temp("readonly");
        let from = dir.join("from");
        fs::create_dir_all(&from).unwrap();
        let source = from.join("MANA.DAT");
        fs::write(&source, zeroed_mana()).unwrap();
        let mut permissions = fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&source, permissions.clone()).unwrap();

        let out = dir.join("out");
        let result = build(&from, &out);
        #[expect(
            clippy::permissions_set_readonly_false,
            reason = "restores the test file"
        )]
        permissions.set_readonly(false);
        fs::set_permissions(&source, permissions).unwrap();

        result.unwrap();
        fs::write(out.join("MANA.DAT"), [2]).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_refuses_an_unparsable_mana_dat_and_writes_nothing() {
        let dir = temp("unparsable");
        let from = dir.join("from");
        fs::create_dir_all(&from).unwrap();
        fs::write(from.join("MANA.DAT"), [1, 2, 3]).unwrap();
        let out = dir.join("out");

        assert!(build(&from, &out).is_err());

        assert!(!out.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Slot 40 (HANSA ROSTOCK, 3. Liga, rank 5, W/D/L 18/13/7, goals 74:49) gets its strength
    /// from its table with the 3. Liga group's base 20 and size 20, not from random numbers;
    /// one club of each other group pins that group's base and size.
    #[test]
    fn mod_clubs_get_table_based_strengths() {
        let mut m = ManaDat::parse(&zeroed_mana()).unwrap();
        write_mod_clubs(&mut m).unwrap();
        assert_eq!(m.clubs[40].name_bytes(), b"HANSA ROSTOCK");
        // Group averages: points 40, scored 64, conceded 54; the rank term is 20 - 5.
        assert_eq!(m.strength[40], [37, 37, 37]);
        // Slot 9 (FSV MAINZ 05, Bundesliga): base 70, size 18, rank 10, averages 38/58/50.
        assert_eq!(m.strength[9], [80, 80, 79]);
        // Slot 25 (1.FC NÜRNBERG, 2. Bundesliga): base 34, size 18, rank 8, averages
        // 33/50/53; [46, 46, 46] scaled by 125/100.
        assert_eq!(m.strength[25], [57, 57, 57]);
        // Slot 60 (ROT-WEIß OBERHAUSEN, pool): base 20, size 20, rank 2, averages 35/56/55.
        assert_eq!(m.strength[60], [42, 40, 40]);

        m.rank[40] = 15;
        let mut rng = Rng::default();
        strength::recompute_strengths_grouped(&mut m, &mut rng, &strength::MOD2026);
        assert_eq!(m.strength[40], [27, 27, 27]);
        assert_eq!(rng, Rng::default());
    }

    /// A squad of fewer than 20 players clears the remaining player fields, one of more than
    /// 20 is refused.
    #[test]
    fn short_squads_leave_empty_player_fields() {
        let mut m = ManaDat::parse(&zeroed_mana()).unwrap();
        m.clubs[0].players = vec![[b'X'; PLAYER_NAME_LEN]; PLAYERS_PER_CLUB];
        let clubs = dataset(CLUBS_JSON, "clubs.json").unwrap();
        let squad = |n: usize| {
            let players = (0..n).map(|k| serde_json::json!({ "name": format!("P{k}") }));
            serde_json::json!({ "players": players.collect::<Vec<_>>() })
        };

        write_club(&mut m, 0, &clubs[0], &squad(3)).unwrap();
        assert_eq!(&m.clubs[0].players[2][..3], b"P2\0");
        assert!(
            m.clubs[0].players[3..]
                .iter()
                .all(|p| *p == [0; PLAYER_NAME_LEN])
        );

        assert!(write_club(&mut m, 0, &clubs[0], &squad(PLAYERS_PER_CLUB + 1)).is_err());
    }
}
