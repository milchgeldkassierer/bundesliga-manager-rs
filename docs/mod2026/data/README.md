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
