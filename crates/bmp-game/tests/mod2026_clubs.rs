//! Validates the Mod 2026 club dataset `docs/mod2026/data/clubs.json` (PRD R1, R3): slot order,
//! name format, crest numbers, sources and the 2025/26 tables.

use std::collections::HashSet;

use bmp_formats::font::encode_cp437;
use bmp_formats::mana::{CLUB_NAME_LEN, GERMAN_CLUB_COUNT};
use serde_json::Value;

const CLUBS_JSON: &str = include_str!("../../../docs/mod2026/data/clubs.json");

/// The MANA.DAT club name field ends with a NUL.
const MAX_NAME_BYTES: usize = CLUB_NAME_LEN - 1;

/// Leagues of the 2026/27 season in slot order, with their slot ranges (PRD R1).
const LEAGUE_SLOTS: [(&str, std::ops::Range<usize>); 4] = [
    ("bundesliga", 0..18),
    ("2-bundesliga", 18..36),
    ("3-liga", 36..56),
    ("regionalliga", 56..64),
];

/// Leagues a 2025/26 table can come from: the `league` of their tier and number of clubs.
fn table_league(name: &str) -> Option<(&'static str, i64)> {
    match name {
        "bundesliga" => Some(("bundesliga", 18)),
        "2-bundesliga" => Some(("2-bundesliga", 18)),
        "3-liga" => Some(("3-liga", 20)),
        "regionalliga-nord"
        | "regionalliga-nordost"
        | "regionalliga-west"
        | "regionalliga-suedwest"
        | "regionalliga-bayern" => Some(("regionalliga", 18)),
        _ => None,
    }
}

fn tier(league: &str) -> usize {
    LEAGUE_SLOTS
        .iter()
        .position(|(name, _)| *name == league)
        .unwrap_or_else(|| panic!("unknown league `{league}`"))
}

fn clubs() -> Vec<Value> {
    match serde_json::from_str(CLUBS_JSON).expect("clubs.json must be valid JSON") {
        Value::Array(clubs) => clubs,
        other => panic!("clubs.json must be an array, got {other}"),
    }
}

fn int(value: &Value, key: &str) -> i64 {
    value[key]
        .as_i64()
        .unwrap_or_else(|| panic!("`{key}` must be an integer in {value}"))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("`{key}` must be a string in {value}"))
}

fn label(club: &Value) -> String {
    format!("slot {} ({})", int(club, "slot"), text(club, "name"))
}

#[test]
fn slots_are_in_order_and_leagues_follow_the_slot_table() {
    let clubs = clubs();
    assert_eq!(clubs.len(), GERMAN_CLUB_COUNT);
    for (slot, club) in clubs.iter().enumerate() {
        assert_eq!(int(club, "slot"), slot as i64);
        let (expected, _) = LEAGUE_SLOTS
            .iter()
            .find(|(_, slots)| slots.contains(&slot))
            .unwrap();
        assert_eq!(text(club, "league"), *expected, "{}", label(club));
    }
}

/// Sort key of the slot order rule in `docs/mod2026/data/README.md`: the tier of the 2025/26
/// league, then the 2025/26 rank, then the name of the 2025/26 league.
fn slot_order(club: &Value) -> (usize, i64, &str) {
    let from = text(club, "table_league");
    let (from_league, _) = table_league(from).unwrap();
    (tier(from_league), int(&club["table_2025_26"], "rank"), from)
}

#[test]
fn slots_inside_a_league_follow_the_2025_26_tables() {
    for pair in clubs().windows(2) {
        if text(&pair[0], "league") == text(&pair[1], "league") {
            assert!(
                slot_order(&pair[0]) < slot_order(&pair[1]),
                "{} must come after {}",
                label(&pair[0]),
                label(&pair[1])
            );
        }
    }
}

#[test]
fn names_use_the_mana_dat_text_format() {
    let mut seen = HashSet::new();
    for club in &clubs() {
        let name = text(club, "name");
        let bytes = encode_cp437(name);
        assert!(
            !name.is_empty() && bytes.len() <= MAX_NAME_BYTES,
            "{}",
            label(club)
        );
        // Lower case includes the CP437 umlauts ä ö ü; MANA.DAT names only use Ä Ö Ü ß.
        assert!(
            !bytes
                .iter()
                .any(|b| b.is_ascii_lowercase() || matches!(b, 0x84 | 0x94 | 0x81)),
            "lower-case letter in {}",
            label(club)
        );
        // encode_cp437 maps every character the game cannot store to 0x7f.
        assert!(
            !bytes.contains(&0x7f),
            "unencodable character in {}",
            label(club)
        );
        assert!(seen.insert(name.to_string()), "duplicate {}", label(club));
    }
}

#[test]
fn crests_are_the_crest_picture_of_their_slot() {
    for club in &clubs() {
        assert_eq!(
            int(club, "crest"),
            80 + int(club, "slot"),
            "{}",
            label(club)
        );
    }
}

#[test]
fn every_club_names_dated_https_sources() {
    let is_date = |s: &str| {
        let parts: Vec<&str> = s.split('-').collect();
        parts.len() == 3
            && [4, 2, 2]
                .iter()
                .zip(&parts)
                .all(|(len, part)| part.len() == *len && part.bytes().all(|b| b.is_ascii_digit()))
    };
    for club in &clubs() {
        let sources = club["sources"]
            .as_array()
            .expect("`sources` must be an array");
        assert!(!sources.is_empty(), "no source for {}", label(club));
        for source in sources {
            assert!(
                text(source, "url").starts_with("https://"),
                "{}",
                label(club)
            );
            assert!(is_date(text(source, "fetched")), "{}", label(club));
        }
    }
}

#[test]
fn tables_2025_26_are_complete_seasons_of_the_named_league() {
    let mut ranks = HashSet::new();
    for club in &clubs() {
        let league = text(club, "league");
        // The 2025/26 league is named per entry: promoted and relegated clubs played one tier
        // away, and 3. Liga clubs relegated from the 2. Bundesliga played 34 matches, not 38.
        let from = text(club, "table_league");
        let (from_league, teams) =
            table_league(from).unwrap_or_else(|| panic!("table_league `{from}` unknown"));
        assert!(
            tier(from_league).abs_diff(tier(league)) <= 1,
            "{} cannot come from {from}",
            label(club)
        );

        let table = &club["table_2025_26"];
        let [wins, draws, losses, goals_for, goals_against, rank] = [
            "wins",
            "draws",
            "losses",
            "goals_for",
            "goals_against",
            "rank",
        ]
        .map(|field| int(table, field));
        assert!(
            [wins, draws, losses, goals_for, goals_against]
                .iter()
                .all(|n| *n >= 0),
            "{}",
            label(club)
        );
        assert_eq!(wins + draws + losses, 2 * (teams - 1), "{}", label(club));
        assert!(
            (1..=teams).contains(&rank),
            "rank {rank} of {}",
            label(club)
        );
        assert!(
            ranks.insert((from.to_string(), rank)),
            "rank {rank} in {from} given twice, {}",
            label(club)
        );
    }
}
