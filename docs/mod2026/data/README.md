# Mod 2026 data

## clubs.json

The 64 German club slots for the 2026/27 season (PRD R1, R3), checked by
`crates/bmp-game/tests/mod2026_clubs.rs`.

- **Fields:** `slot`, `name`, `league` (2026/27), `table_league` (the league the 2025/26 table
  comes from), `table_2025_26` (`wins`, `draws`, `losses`, `goals_for`, `goals_against`, `rank`),
  `crest`, `sources`.
- **Slot order:** inside each 2026/27 league, the clubs are ordered by the tier of their 2025/26
  league, then by their 2025/26 rank, as the original orders its slots by the previous season.
  Regionalliga clubs with the same rank follow the alphabetical order of their league.
- **No reserve teams** (PRD R3): VfB Stuttgart II and TSG Hoffenheim II of the real 3. Liga
  2026/27 are replaced by FC Erzgebirge Aue and SSV Ulm 1846, the clubs relegated from the
  3. Liga 2025/26 with the most points. They keep their 3. Liga rows (ranks 17 and 18), so the
  slot order puts them after TSV Havelse (16th): 1. FC Saarbrücken, Havelse, Aue and Ulm hold
  slots 48..51, and their crests and squads moved with them.
- **Crests:** `crest` = 80 + `slot`. The converted crest pictures are `docs/mod2026/crests/<crest>.png`.
- **Regionalliga pool (56..63):** the 8 first teams of the Regionalligen 2026/27 with the best
  average rank in fans, followers and relevance (PRD R1), from the ranked candidate list
  `docs/mod2026/research/pool-candidates.json`; BFC Dynamo is ranked there but excluded from the
  pool. Pool clubs keep their real 2025/26 rows (1860 München its 3. Liga row).
- **Ranks:** the official final tables. TSV 1860 München finished 8th on points but was placed
  20th after it was refused a 3. Liga licence for 2026/27; TSV Havelse therefore stayed up as
  16th. Würzburger Kickers were promoted as Regionalliga Bayern runners-up, because the champion
  1. FC Nürnberg II did not apply for a licence.
- **Names:** upper case in the style of `MANA.DAT` (for example `1.FC KÖLN`,
  `B. MÖNCHENGLADBACH`, `EINTR. BRAUNSCHWEIG`), at most 19 characters and in every game font no
  wider than the widest original name (`EINTR. BRAUNSCHWEIG` 79 px in micro4, `EINTRACHT
  FRANKFURT` 130 px in flow and 247 px in newbold). Clubs that the original already has keep the
  original's name (`CARL ZEISS JENA`, `FC RW ERFURT`, `HALLESCHER FC`, `CHEMNITZER FC`), except
  where the owner chose the fuller name (owner decision 2026-10-25: `SG DYNAMO DRESDEN`,
  `FC HANSA ROSTOCK`, `FC ENERGIE COTTBUS`, `B. MÖNCHENGLADBACH`).
- **Sources:** the German Wikipedia pages of the 2025/26 league (table) and of the club's 2026/27
  league (membership), fetched 2026-10-24 (2026-10-25 for the pool clubs added then). Aue and Ulm
  name their real 2026/27 Regionalliga as membership source.

## research/pool-candidates.json

The ranked candidates for the Regionalliga pool, checked by
`crates/bmp-game/tests/mod2026_clubs.rs`.

- **Candidates:** all first teams of the five Regionalligen 2026/27 except Aue and Ulm (they
  play the 3. Liga in the mod), so it holds the required top 50 and every rank can be
  recomputed. `club` names the slot of the 8 pool clubs; BFC Dynamo has
  `excluded_from_pool: true`.
- **Measures and ranking:** the `measures` and `ranking` fields of the file state the three
  measures (fans, followers, relevance; fetched 2026-10-25), the relevance formula and the
  tie-break; the test recomputes every total, score, rank and the order from the values and
  checks that every candidate is a first team with dated https sources. The pool is the first 8
  candidates that are not excluded.

## squads.json

The first-team squads of the 64 club slots as of October 2026 (PRD R4), checked by
`crates/bmp-game/tests/mod2026_squads.rs`.

- **Fields:** `slot`, `club` (the `name` of the slot in `clubs.json`), `source` (the club's
  Transfermarkt squad page of 2026/27), `fetched`, and `players`: `name`, `position`
  (`TW` goalkeeper, `AB` defender, `MF` midfielder, `ST` forward, as Transfermarkt groups them),
  `market_value_eur` (`null` where Transfermarkt shows no value) and `birth_date`, in
  Transfermarkt's squad order. The squads were fetched 2026-10-24, those of the pool clubs added
  then on 2026-10-25.
- **Research data:** `docs/mod2026/research/squads-uncut.json` holds the full squads with
  full name, Transfermarkt id, position, market value, birth date and a `kept` flag. The test
  checks that the cut of these squads gives exactly `squads.json`. The public copy leaves the
  research directory out (MISSION.md, Publication).
- **Squad cut:** every squad was larger than 20, so each club keeps the 20 players with the highest
  market value; a player without a value counts as 0, and on a tie the younger player is kept.
  If that leaves no goalkeeper, the best-ranked goalkeeper takes the place of the 20th player
  (TSG Hoffenheim, TSV Havelse).
- **Names:** upper case surname in the style of `MANA.DAT`, at most 25 CP437 bytes. Accents are
  dropped except Ä, Ö, Ü and ß (for example `DIAZ`, `MITTELSTÄDT`, `KRAUß`). The surname is
  the last word of the Transfermarkt name, or everything from a particle such as `von`, `van`
  or `de` on (`VAN DEN BOSCH`). When two kept players of a club share a surname, both get their
  first initial (`C.CONTEH`, `S.CONTEH`), or more letters of the first name if the initials are
  the same too (`LE.MAIER`, `LO.MAIER`); of two players with the same full name, the younger
  one gets `JUN.` (`M.SCHULZ`, `M.SCHULZ JUN.`).
- **Width** (owner report 2026-10-25): a name is at most 52 pixels wide in micro4, the limit of
  TEAMED's name input, for which every screen of the game is laid out (the original stores
  `SCHLOTTERBEC`). A wider name keeps the first part of a double name (`AMAIMOUNI`, `ROSSING`),
  else the last word (`BOSCH`, `KIALA`), else it is cut at the end like the original's
  (`SCHLOTTERBEC`, `KÖNIGSDÖRFF`).
