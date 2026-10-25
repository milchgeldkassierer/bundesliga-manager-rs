//! Validates the `MANA.DAT` that `bmp mod2026 build` writes (PRD R3, R4, R6): the 64 German
//! slots hold the dataset, the foreign clubs keep the original bytes, and the output is
//! deterministic. Skipped without `BMP_DATA`.

use std::fs;
use std::path::Path;

use bmp_formats::font::encode_cp437;
use bmp_formats::mana::{
    self, CLUBS_OFFSET, FILE_SIZE, GERMAN_CLUB_COUNT, GERMAN_CLUB_RECORD_LEN, ManaDat,
    POINTS_OFFSET, STRENGTH_OFFSET,
};
use serde_json::Value;

const CLUBS_JSON: &str = include_str!("../../../docs/mod2026/data/clubs.json");
const SQUADS_JSON: &str = include_str!("../../../docs/mod2026/data/squads.json");

fn array(json: &str) -> Vec<Value> {
    match serde_json::from_str(json).unwrap() {
        Value::Array(slots) => slots,
        other => panic!("not an array: {other}"),
    }
}

fn int(value: &Value, key: &str) -> u8 {
    u8::try_from(value[key].as_u64().unwrap()).unwrap()
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap()
}

/// The `MANA.DAT` bytes `build` writes from `data` into a fresh temp directory.
fn build(data: &Path, name: &str) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("bmp-mod2026-mana-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    bmp_game::mod2026::build(data, &dir).unwrap();
    let bytes = fs::read(dir.join("MANA.DAT")).unwrap();
    fs::remove_dir_all(&dir).unwrap();
    bytes
}

/// The generated `MANA.DAT`, or `None` without `BMP_DATA`.
fn generated(name: &str) -> Option<ManaDat> {
    let data = bmp_formats::data_dir()?;
    Some(ManaDat::parse(&build(&data, name)).unwrap())
}

#[test]
fn generated_mana_dat_parses_and_round_trips() {
    let Some(data) = bmp_formats::data_dir() else {
        return;
    };
    let bytes = build(&data, "roundtrip");
    assert_eq!(ManaDat::parse(&bytes).unwrap().to_bytes(), bytes);
}

#[test]
fn slots_hold_the_dataset_names_crests_and_players() {
    let Some(m) = generated("names") else {
        return;
    };
    let clubs = array(CLUBS_JSON);
    let squads = array(SQUADS_JSON);
    for slot in 0..GERMAN_CLUB_COUNT {
        let record = &m.clubs[slot];
        assert_eq!(
            record.name_bytes(),
            encode_cp437(text(&clubs[slot], "name")),
            "slot {slot}"
        );
        assert_eq!(m.crest[slot], int(&clubs[slot], "crest"), "slot {slot}");
        let players = squads[slot]["players"].as_array().unwrap();
        for k in 0..mana::PLAYERS_PER_CLUB {
            let expected = players
                .get(k)
                .map_or_else(Vec::new, |p| encode_cp437(text(p, "name")));
            assert_eq!(
                record.player_name_bytes(k),
                Some(expected.as_slice()),
                "slot {slot} player {k}"
            );
        }
    }
}

#[test]
fn block_3_holds_the_two_point_pair() {
    let Some(m) = generated("table") else {
        return;
    };
    let clubs = array(CLUBS_JSON);
    for (slot, club) in clubs.iter().enumerate() {
        let table = &club["table_2025_26"];
        let [w, d, l] = ["wins", "draws", "losses"].map(|key| int(table, key));
        let per_win = bmp_game::league_layout::ORIGINAL.points_per_win;
        assert_eq!(per_win, 2);
        assert_eq!(
            m.points[slot],
            [per_win * w + d, per_win * l + d],
            "slot {slot}"
        );
        assert_eq!(
            m.goals[slot],
            [int(table, "goals_for"), int(table, "goals_against")],
            "slot {slot}"
        );
        assert_eq!(m.rank[slot], int(table, "rank"), "slot {slot}");
    }
    // HANSA ROSTOCK: 18 wins, 13 draws, 7 losses.
    assert_eq!(m.points[40], [49, 27]);
}

#[test]
fn foreign_club_records_are_byte_identical() {
    let Some(data) = bmp_formats::data_dir() else {
        return;
    };
    let generated = build(&data, "foreign");
    let pristine = fs::read(data.join("MANA.DAT")).unwrap();
    let strengths = STRENGTH_OFFSET + GERMAN_CLUB_COUNT * 3..POINTS_OFFSET;
    let records = CLUBS_OFFSET + GERMAN_CLUB_COUNT * GERMAN_CLUB_RECORD_LEN..FILE_SIZE;
    assert_eq!(generated[strengths.clone()], pristine[strengths]);
    assert_eq!(generated[records.clone()], pristine[records]);
}

#[test]
fn two_builds_write_the_same_mana_dat() {
    let Some(data) = bmp_formats::data_dir() else {
        return;
    };
    assert_eq!(build(&data, "first"), build(&data, "second"));
}
