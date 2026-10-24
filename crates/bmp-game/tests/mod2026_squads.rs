//! Validates the Mod 2026 squad dataset `docs/mod2026/data/squads.json` (PRD R4) and the squad
//! cut rule that chose its players.

use std::collections::HashSet;

use bmp_formats::font::encode_cp437;
use bmp_formats::mana::{GERMAN_CLUB_COUNT, PLAYER_NAME_LEN, PLAYERS_PER_CLUB};
use serde_json::Value;

const SQUADS_JSON: &str = include_str!("../../../docs/mod2026/data/squads.json");
const CLUBS_JSON: &str = include_str!("../../../docs/mod2026/data/clubs.json");
const UNCUT_JSON: &str = include_str!("../../../docs/mod2026/research/squads-uncut.json");

/// The MANA.DAT player name field ends with a NUL.
const MAX_NAME_BYTES: usize = PLAYER_NAME_LEN - 1;

/// Goalkeeper, defender, midfielder, forward.
const POSITIONS: [&str; 4] = ["TW", "AB", "MF", "ST"];

/// A player of the full Transfermarkt squad, as far as the cut needs him.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Candidate<'a> {
    name: &'a str,
    position: &'a str,
    /// `None` when Transfermarkt shows no market value; it ranks like 0.
    market_value_eur: Option<u64>,
    /// ISO date `YYYY-MM-DD`, so the string order is the date order.
    birth_date: &'a str,
}

/// The squad cut of PRD R4: keeps the `PLAYERS_PER_CLUB` players with the highest market value,
/// on a tie the younger one. If that leaves no goalkeeper, the best-ranked goalkeeper takes the
/// place of the last kept player. The kept players stay in squad order.
fn cut<'a>(squad: &[Candidate<'a>]) -> Vec<Candidate<'a>> {
    let mut ranked: Vec<usize> = (0..squad.len()).collect();
    ranked.sort_by(|&a, &b| {
        let (a, b) = (&squad[a], &squad[b]);
        (b.market_value_eur.unwrap_or(0), b.birth_date)
            .cmp(&(a.market_value_eur.unwrap_or(0), a.birth_date))
    });
    let mut kept: Vec<usize> = ranked.iter().copied().take(PLAYERS_PER_CLUB).collect();
    let is_goalkeeper = |i: &usize| squad[*i].position == "TW";
    if !kept.iter().any(is_goalkeeper)
        && let Some(goalkeeper) = ranked.iter().copied().find(is_goalkeeper)
    {
        *kept.last_mut().unwrap() = goalkeeper;
    }
    kept.sort_unstable();
    kept.into_iter().map(|i| squad[i]).collect()
}

fn array(json: &str, file: &str) -> Vec<Value> {
    match serde_json::from_str(json).unwrap_or_else(|e| panic!("{file} must be valid JSON: {e}")) {
        Value::Array(entries) => entries,
        other => panic!("{file} must be an array, got {other}"),
    }
}

fn squads() -> Vec<Value> {
    array(SQUADS_JSON, "squads.json")
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("`{key}` must be a string in {value}"))
}

fn players(club: &Value) -> &Vec<Value> {
    club["players"]
        .as_array()
        .unwrap_or_else(|| panic!("`players` must be an array in slot {}", club["slot"]))
}

fn label(club: &Value) -> String {
    format!("slot {} ({})", club["slot"], text(club, "club"))
}

#[test]
fn every_club_slot_has_a_squad_of_11_to_20_players() {
    let squads = squads();
    let clubs = array(CLUBS_JSON, "clubs.json");
    assert_eq!(squads.len(), GERMAN_CLUB_COUNT);
    for (slot, (squad, club)) in squads.iter().zip(&clubs).enumerate() {
        assert_eq!(squad["slot"].as_u64(), Some(slot as u64));
        assert_eq!(text(squad, "club"), text(club, "name"), "slot {slot}");
        let count = players(squad).len();
        assert!(
            (11..=PLAYERS_PER_CLUB).contains(&count),
            "{} has {count} players",
            label(squad)
        );
    }
}

fn keys(value: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap_or_else(|| panic!("{value} must be an object"))
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

/// Per club and per player the dataset holds the fields of PRD R4 and nothing else.
#[test]
fn squads_hold_names_positions_values_and_birth_dates() {
    for squad in &squads() {
        assert_eq!(
            keys(squad),
            ["club", "fetched", "players", "slot", "source"],
            "{}",
            label(squad)
        );
        for player in players(squad) {
            assert_eq!(
                keys(player),
                ["birth_date", "market_value_eur", "name", "position"],
                "{player} in {}",
                label(squad)
            );
            assert!(
                player["market_value_eur"].is_null() || player["market_value_eur"].is_u64(),
                "{player} in {}",
                label(squad)
            );
            assert!(
                is_date(text(player, "birth_date")),
                "{player} in {}",
                label(squad)
            );
        }
    }
}

/// Runs the cut on the uncut research squads and checks that it keeps exactly the committed
/// players, in squad order, with their positions, market values and birth dates.
#[test]
fn squads_are_the_cut_of_the_uncut_research_data() {
    let research: Value = serde_json::from_str(UNCUT_JSON)
        .unwrap_or_else(|e| panic!("squads-uncut.json must be valid JSON: {e}"));
    let uncut = research["clubs"]
        .as_array()
        .expect("squads-uncut.json must hold a `clubs` array");
    let squads = squads();
    assert_eq!(uncut.len(), squads.len());
    for (research, squad) in uncut.iter().zip(&squads) {
        assert_eq!(research["slot"], squad["slot"]);
        assert_eq!(
            text(research, "source"),
            text(squad, "source"),
            "{}",
            label(squad)
        );
        // A player the first team keeps is not counted for its reserve team.
        let candidates: Vec<Candidate> = players(research)
            .iter()
            .filter(|p| !p["kept_by_first_team"].as_bool().unwrap_or(false))
            .map(|p| Candidate {
                name: text(p, "full_name"),
                position: text(p, "position"),
                market_value_eur: p["market_value_eur"].as_u64(),
                birth_date: text(p, "birth_date"),
            })
            .collect();
        let kept: Vec<&str> = players(research)
            .iter()
            .filter(|p| p["kept"].as_bool() == Some(true))
            .map(|p| text(p, "full_name"))
            .collect();
        let cut = cut(&candidates);
        assert_eq!(names(&cut), kept, "{}", label(squad));
        let committed: Vec<(&str, Option<u64>, &str)> = players(squad)
            .iter()
            .map(|p| {
                (
                    text(p, "position"),
                    p["market_value_eur"].as_u64(),
                    text(p, "birth_date"),
                )
            })
            .collect();
        let expected: Vec<(&str, Option<u64>, &str)> = cut
            .iter()
            .map(|p| (p.position, p.market_value_eur, p.birth_date))
            .collect();
        assert_eq!(committed, expected, "{}", label(squad));
    }
}

/// `YYYY-MM-DD`.
fn is_date(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 3
        && [4, 2, 2]
            .iter()
            .zip(&parts)
            .all(|(len, part)| part.len() == *len && part.bytes().all(|b| b.is_ascii_digit()))
}

#[test]
fn every_squad_names_its_transfermarkt_page_and_fetch_date() {
    for squad in &squads() {
        assert!(
            text(squad, "source").starts_with("https://www.transfermarkt.de/"),
            "{}",
            label(squad)
        );
        assert!(is_date(text(squad, "fetched")), "{}", label(squad));
    }
}

#[test]
fn every_player_has_a_position_and_every_squad_a_goalkeeper() {
    for squad in &squads() {
        let positions: Vec<&str> = players(squad).iter().map(|p| text(p, "position")).collect();
        for position in &positions {
            assert!(
                POSITIONS.contains(position),
                "position `{position}` in {}",
                label(squad)
            );
        }
        assert!(
            positions.contains(&"TW"),
            "no goalkeeper in {}",
            label(squad)
        );
    }
}

#[test]
fn player_names_use_the_mana_dat_text_format() {
    for squad in &squads() {
        let mut seen = HashSet::new();
        for player in players(squad) {
            let name = text(player, "name");
            let bytes = encode_cp437(name);
            assert!(
                !name.is_empty() && bytes.len() <= MAX_NAME_BYTES,
                "`{name}` in {}",
                label(squad)
            );
            // CP437 has no upper-case ß, so upper-case names keep it (`KRAUß`).
            assert!(
                !name.chars().any(|c| c.is_lowercase() && c != 'ß'),
                "lower-case letter in `{name}`, {}",
                label(squad)
            );
            // encode_cp437 maps every character the game cannot store to 0x7f.
            assert!(
                !bytes.contains(&0x7f),
                "unencodable character in `{name}`, {}",
                label(squad)
            );
            assert!(seen.insert(name), "`{name}` twice in {}", label(squad));
        }
    }
}

/// 23 players; A07 and A14 share 300 000 at ranks 20 and 21, so the boundary between kept and
/// dropped falls inside the tie. A07, the older, comes first in squad order.
fn fixture() -> Vec<Candidate<'static>> {
    let player = |name, position, value, birth_date| Candidate {
        name,
        position,
        market_value_eur: value,
        birth_date,
    };
    vec![
        player("A01", "TW", Some(5_000_000), "1995-01-01"),
        player("A02", "TW", Some(350_000), "1990-01-01"),
        player("A03", "AB", Some(4_000_000), "1996-01-01"),
        player("A04", "AB", Some(3_500_000), "1997-01-01"),
        player("A05", "AB", Some(3_000_000), "1998-01-01"),
        player("A06", "AB", Some(2_500_000), "1999-01-01"),
        player("A07", "AB", Some(300_000), "1993-04-01"),
        player("A08", "AB", None, "2005-01-01"),
        player("A09", "MF", Some(2_000_000), "2000-01-01"),
        player("A10", "MF", Some(1_800_000), "2001-01-01"),
        player("A11", "MF", Some(1_600_000), "2002-01-01"),
        player("A12", "MF", Some(1_400_000), "1994-01-01"),
        player("A13", "MF", Some(1_200_000), "1993-01-01"),
        player("A14", "MF", Some(300_000), "2003-04-01"),
        player("A15", "MF", Some(320_000), "2004-01-01"),
        player("A16", "ST", Some(1_000_000), "1992-01-01"),
        player("A17", "ST", Some(900_000), "1991-01-01"),
        player("A18", "ST", Some(800_000), "1990-06-01"),
        player("A19", "ST", Some(700_000), "1989-01-01"),
        player("A20", "ST", Some(600_000), "1988-01-01"),
        player("A21", "ST", Some(500_000), "2006-01-01"),
        player("A22", "ST", Some(400_000), "2007-01-01"),
        player("A23", "ST", Some(200_000), "2001-06-01"),
    ]
}

fn names<'a>(players: &[Candidate<'a>]) -> Vec<&'a str> {
    players.iter().map(|p| p.name).collect()
}

#[test]
fn cut_drops_the_lowest_market_values_and_the_older_player_on_a_tie() {
    let kept = cut(&fixture());
    let expected: Vec<&str> = names(&fixture())
        .into_iter()
        .filter(|name| !["A07", "A08", "A23"].contains(name))
        .collect();
    assert_eq!(expected.len(), PLAYERS_PER_CLUB);
    assert_eq!(names(&kept), expected);
}

#[test]
fn cut_keeps_a_goalkeeper_when_the_market_values_would_drop_them_all() {
    let mut squad = fixture();
    squad[0].market_value_eur = Some(100_000);
    squad[1].market_value_eur = Some(150_000);
    let kept = cut(&squad);
    assert_eq!(kept.len(), PLAYERS_PER_CLUB);
    // A02 is the better goalkeeper and replaces A23, the last of the 20 by market value.
    let expected: Vec<&str> = names(&squad)
        .into_iter()
        .filter(|name| !["A01", "A08", "A23"].contains(name))
        .collect();
    assert_eq!(names(&kept), expected);
}

#[test]
fn cut_keeps_a_small_squad_whole() {
    let squad = &fixture()[..15];
    assert_eq!(cut(squad), squad);
}
