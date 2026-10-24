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
- **Crests:** `crest` = 80 + `slot`.
- **Regionalliga pool (56..63):** the three clubs relegated from the 3. Liga with the most 2025/26
  points (Aue, Ulm, 1860 München; Schweinfurt is left out), and the five Regionalliga leaders as
  of 18–20 October 2026.
- **Ranks:** the official final tables. TSV 1860 München finished 8th on points but was placed
  20th after it was refused a 3. Liga licence for 2026/27; TSV Havelse therefore stayed up as
  16th. Würzburger Kickers were promoted as Regionalliga Bayern runners-up, because the champion
  1. FC Nürnberg II did not apply for a licence.
- **Names:** upper case in the style of `MANA.DAT` (for example `1.FC KÖLN`, `MÖNCHENGLADBACH`,
  `EINTR. BRAUNSCHWEIG`), at most 19 characters. SGV Freiberg plays as SGV Heilbronn-Freiberg
  since 2026/27.
- **Sources:** the German Wikipedia pages of the 2025/26 league (table) and of the club's 2026/27
  league (membership), fetched 2026-10-24.

## squads.json

The first-team squads of the 64 club slots as of October 2026 (PRD R4), checked by
`crates/bmp-game/tests/mod2026_squads.rs`.

- **Fields:** `slot`, `club` (the `name` of the slot in `clubs.json`), `source` (the club's
  Transfermarkt squad page of 2026/27), `fetched`, and `players`: `name`, `position`
  (`TW` goalkeeper, `AB` defender, `MF` midfielder, `ST` forward, as Transfermarkt groups them),
  `market_value_eur` (`null` where Transfermarkt shows no value) and `birth_date`, in
  Transfermarkt's squad order. All squads were fetched 2026-10-24.
- **Research data:** `docs/mod2026/research/squads-uncut.json` holds the full squads (1808
  players) with full name, Transfermarkt id, position, market value, birth date and a `kept` flag
  (`kept_by_first_team` where a reserve-team player is kept by the first team). The test checks
  that the cut of these squads gives exactly `squads.json`. The public copy leaves the research
  directory out (MISSION.md, Publication).
- **Squad cut:** every squad was larger than 20, so each club keeps the 20 players with the highest
  market value; a player without a value counts as 0, and on a tie the younger player is kept.
  If that leaves no goalkeeper, the best-ranked goalkeeper takes the place of the 20th player
  (TSG Hoffenheim, VfB Stuttgart II, TSV Havelse).
- **Reserve teams:** Transfermarkt lists some players in both a first team and its reserve team.
  The first team is cut first; a player it keeps is not counted for its reserve team (Spalt for
  VfB Stuttgart II).
- **Names:** upper case surname in the style of `MANA.DAT`, at most 25 CP437 bytes. Accents are
  dropped except Ä, Ö, Ü and ß (for example `DIAZ`, `MITTELSTÄDT`, `KRAUß`). The surname is
  the last word of the Transfermarkt name, or everything from a particle such as `von`, `van`
  or `de` on (`VAN DEN BOSCH`). When two kept players of a club share a surname, both get their
  first initial (`C.CONTEH`, `S.CONTEH`), or more letters of the first name if the initials are
  the same too (`LE.MAIER`, `LO.MAIER`); of two players with the same full name, the younger
  one gets `JUN.` (`M.SCHULZ`, `M.SCHULZ JUN.`).
