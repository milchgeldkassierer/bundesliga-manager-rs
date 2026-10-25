//! Validates the Mod 2026 club dataset `docs/mod2026/data/clubs.json` (PRD R1, R3): slot order,
//! name format, no reserve teams, crest numbers, sources, the 2025/26 tables and the Regionalliga
//! pool chosen from `docs/mod2026/research/pool-candidates.json`.

use std::collections::HashSet;

use bmp_formats::font::encode_cp437;
use bmp_formats::mana::{CLUB_NAME_LEN, GERMAN_CLUB_COUNT};
use bmp_game::strength::MOD2026;
use serde_json::Value;

const CLUBS_JSON: &str = include_str!("../../../docs/mod2026/data/clubs.json");
const SQUADS_JSON: &str = include_str!("../../../docs/mod2026/data/squads.json");
const POOL_CANDIDATES_JSON: &str =
    include_str!("../../../docs/mod2026/research/pool-candidates.json");

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

fn array(json: &str, file: &str) -> Vec<Value> {
    match serde_json::from_str(json).unwrap_or_else(|e| panic!("{file} must be valid JSON: {e}")) {
        Value::Array(entries) => entries,
        other => panic!("{file} must be an array, got {other}"),
    }
}

fn clubs() -> Vec<Value> {
    array(CLUBS_JSON, "clubs.json")
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

/// The generator's strength grouping has its own copy of the slot table: each club is in the
/// group of its league, and its rank stays inside the group's rank term `size - rank`.
#[test]
fn mod_strength_grouping_follows_the_slot_table() {
    for (slot, club) in clubs().iter().enumerate() {
        let group = MOD2026.group(slot);
        assert_eq!(group, tier(text(club, "league")), "{}", label(club));
        let rank = int(&club["table_2025_26"], "rank");
        assert!(rank <= i64::from(MOD2026.0[group].size), "{}", label(club));
    }
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

/// Marks of a club's second team; the game has no such concept (PRD R3).
const RESERVE_MARKS: [&str; 4] = [" II", "U23", "U21", "AMAT"];

fn is_reserve_team(name: &str) -> bool {
    RESERVE_MARKS.iter().any(|mark| name.contains(mark))
}

#[test]
fn no_slot_holds_a_reserve_team() {
    let clubs = clubs();
    let squads = array(SQUADS_JSON, "squads.json");
    let names = clubs
        .iter()
        .map(|club| text(club, "name"))
        .chain(squads.iter().map(|squad| text(squad, "club")))
        .collect::<Vec<_>>();
    for name in names {
        assert!(!is_reserve_team(name), "reserve team `{name}`");
    }
}

#[test]
fn reserve_marks_are_recognised() {
    for name in [
        "VFB STUTTGART II",
        "TSG HOFFENHEIM U23",
        "MAINZ U21",
        "BAYERN AMAT.",
    ] {
        assert!(is_reserve_team(name), "{name}");
    }
    for name in ["FC INGOLSTADT 04", "SSV ULM 1846", "VFL OSNABRÜCK"] {
        assert!(!is_reserve_team(name), "{name}");
    }
}

/// The three pool measures of a candidate and the field that holds each value.
const MEASURES: [(&str, &str); 3] = [
    ("fans", "average_home_attendance"),
    ("followers", "total"),
    ("relevance", "score"),
];

fn follower_sum(followers: &Value) -> i64 {
    followers["accounts"]
        .as_array()
        .expect("`accounts` must be an array")
        .iter()
        .map(|account| int(account, "followers"))
        .sum()
}

/// The relevance formula of `pool-candidates.json`: 2 per top-flight season, 1 per 2. Bundesliga
/// season, 10 per major title.
fn relevance_score(relevance: &Value) -> i64 {
    let top_flight = int(relevance, "bundesliga_seasons")
        + int(relevance, "oberliga_seasons")
        + int(relevance, "ddr_oberliga_seasons");
    let titles: usize = relevance["titles"]
        .as_array()
        .expect("`titles` must be an array")
        .iter()
        .map(|title| {
            title["years"]
                .as_array()
                .expect("`years` must be an array")
                .len()
        })
        .sum();
    2 * top_flight + int(relevance, "zweite_bundesliga_seasons") + 10 * titles as i64
}

fn pool_candidates() -> Vec<Value> {
    let research: Value =
        serde_json::from_str(POOL_CANDIDATES_JSON).expect("pool-candidates.json must parse");
    research["candidates"]
        .as_array()
        .expect("pool-candidates.json must hold a `candidates` array")
        .clone()
}

/// Candidate names are in mixed case; the reserve marks are those of the upper-case slot names.
#[test]
fn no_pool_candidate_is_a_reserve_team() {
    for candidate in &pool_candidates() {
        let name = text(candidate, "name");
        assert!(
            !is_reserve_team(&name.to_uppercase()),
            "reserve team `{name}`"
        );
    }
}

#[test]
fn every_pool_candidate_names_dated_https_sources() {
    for candidate in &pool_candidates() {
        let name = text(candidate, "name");
        let fans = &candidate["fans"];
        let accounts = candidate["followers"]["accounts"]
            .as_array()
            .expect("`accounts` must be an array");
        let relevance = candidate["relevance"]["sources"]
            .as_array()
            .expect("`sources` must be an array");
        assert!(!relevance.is_empty(), "no relevance source for {name}");
        let sources = std::iter::once((text(fans, "source"), text(fans, "fetched"))).chain(
            accounts
                .iter()
                .chain(relevance)
                .map(|source| (text(source, "url"), text(source, "fetched"))),
        );
        for (url, fetched) in sources {
            assert!(url.starts_with("https://"), "{name}: {url}");
            assert!(is_date(fetched), "{name}: {url} fetched `{fetched}`");
        }
    }
}

/// `pool-candidates.json` ranks every candidate per measure (equal values share the best rank),
/// orders them by average rank, then fans rank, then name, and the pool slots hold the first 8
/// that are not excluded.
#[test]
fn pool_is_the_top_8_of_the_ranked_candidates() {
    let candidates = &pool_candidates();
    assert!(
        candidates.len() >= 50,
        "the candidate list keeps at least the top 50"
    );

    let value = |candidate: &Value, (measure, field): (&str, &str)| int(&candidate[measure], field);
    for candidate in candidates {
        assert_eq!(
            int(&candidate["followers"], "total"),
            follower_sum(&candidate["followers"]),
            "{} followers",
            text(candidate, "name")
        );
        assert_eq!(
            int(&candidate["relevance"], "score"),
            relevance_score(&candidate["relevance"]),
            "{} relevance",
            text(candidate, "name")
        );
        let mut rank_sum = 0;
        for measure in MEASURES {
            let own = value(candidate, measure);
            let better = candidates
                .iter()
                .filter(|other| value(other, measure) > own)
                .count();
            let rank = int(&candidate["ranks"], measure.0);
            assert_eq!(
                rank,
                better as i64 + 1,
                "{} {}",
                text(candidate, "name"),
                measure.0
            );
            rank_sum += rank;
        }
        let average = candidate["average_rank"].as_f64().unwrap();
        assert!(
            (average - rank_sum as f64 / 3.0).abs() < 0.01,
            "{}",
            text(candidate, "name")
        );
    }
    let order_key = |candidate: &Value| {
        let ranks = &candidate["ranks"];
        let sum: i64 = MEASURES.iter().map(|(name, _)| int(ranks, name)).sum();
        (sum, int(ranks, "fans"), text(candidate, "name").to_owned())
    };
    for (index, pair) in candidates.windows(2).enumerate() {
        assert_eq!(int(&pair[0], "rank"), index as i64 + 1);
        assert!(
            order_key(&pair[0]) < order_key(&pair[1]),
            "{}",
            text(&pair[1], "name")
        );
    }

    let excluded: Vec<&str> = candidates
        .iter()
        .filter(|c| c["excluded_from_pool"].as_bool() == Some(true))
        .map(|c| text(c, "name"))
        .collect();
    assert_eq!(excluded, ["BFC Dynamo"]);
    let pool_slots = LEAGUE_SLOTS[tier("regionalliga")].1.clone();
    let top: Vec<&Value> = candidates
        .iter()
        .filter(|c| c["excluded_from_pool"].as_bool() != Some(true))
        .take(pool_slots.len())
        .collect();
    assert!(
        candidates
            .iter()
            .filter(|c| c.get("club").is_some())
            .eq(top.iter().copied()),
        "exactly the top 8 candidates name their pool slot club"
    );
    let mut chosen: Vec<&str> = top.iter().map(|c| text(c, "club")).collect();
    let clubs = clubs();
    let mut pool: Vec<&str> = clubs[pool_slots].iter().map(|c| text(c, "name")).collect();
    chosen.sort_unstable();
    pool.sort_unstable();
    assert_eq!(chosen, pool);
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

fn is_date(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 3
        && [4, 2, 2]
            .iter()
            .zip(&parts)
            .all(|(len, part)| part.len() == *len && part.bytes().all(|b| b.is_ascii_digit()))
}

#[test]
fn every_club_names_dated_https_sources() {
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
