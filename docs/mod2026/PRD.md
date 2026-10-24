# Mod 2026: Bundesliga Manager Professional with the 2026/27 season

Branch: `mod_2026`, started from tag `v1.0.0` (the complete 1:1 port). All work for this mod
targets `mod_2026`. Nothing is merged into `main`.

## Goal

The player can start a new game in the 2026/27 season with today's German clubs:
- the 18 clubs of the Bundesliga, the 18 of the 2. Bundesliga and the 20 of the 3. Liga, all as of
  October 2026;
- their current crests, redrawn in the style and size of the original crest pictures;
- their current squads.

Everything else plays like version 1.0.0.

## Ground rules

- **Fidelity:** the 1:1 comparison with the DOS original no longer applies on this branch.
  - The port must still run unchanged with the original, unmodded data. Every existing test and
    visual scenario keeps passing against `~/Apps/bmp-pristine`.
  - Mod behaviour switches on only when the data directory contains the mod data. The detection
    rule is decided in ticket 1, for example a marker file `MOD2026.DAT`.
- **Data safety:**
  - Never write into `<repo>/BMP`. It is the owner's play directory.
  - The mod data directory is generated into `~/Apps/bmp-mod2026`, a full copy of
    `~/Apps/bmp-pristine` with the mod files on top.
  - Original game files are never committed.
- **Committed mod assets:** the club and player dataset (JSON) and the converted low-resolution
  crest images (PNG, the original crest size) may be committed. The repository is private.
- **Text format:** names follow the original's format:
  - upper case, CP437 umlauts (`Ä Ö Ü ß` as in `MANA.DAT`);
  - club names at most 22 characters;
  - player names short, in the style of the shipped file: surname only, or an initial plus
    surname when two players of the club share a surname;
  - every name must fit the widths of the screens that show it.
- **Code conventions:** code, comments and docs are in English. Changes to original behaviour are
  marked `// MOD2026:` and listed in `docs/mod2026/CHANGES.md`.

## Facts from the reverse engineering

- **MANA.DAT** (`docs/re/mana.md`) has 64 German club slots and 136 foreign clubs.
  - Each German club has 20 player-name slots.
  - The game uses league groups of 18 / 20 / 20 clubs: indices 0..17, 18..37 and 38..57.
    Slots 58..63 are unused.
  - Blocks 3–5 hold the previous season's table (points, goals, rank).
  - The team editor recomputes the strengths from that table.
- **Crests:** `PIC/<n>.VGA` with n = 80..143, selected by block 1 of MANA.DAT. Format:
  `docs/re/vga.md`.
- **3. Liga:** it has 20 clubs, which fits the third group (20 slots) exactly.
- **2. Bundesliga:** it has 18 clubs today, but the game's second group has 20. This is the only
  league-size mismatch.

## Requirements

### R1 League structure 18 / 18 / 20 (mod data only)

- The 2. Bundesliga has 18 clubs.
- The fixtures, the number of match days, the tables and the season-end rules match the size of
  each league.
- **Promotion and relegation as in 2026:**
  - Bundesliga: 17th and 18th go down; the 16th plays the relegation play-off against the 3rd of
    the 2. Bundesliga.
  - 2. Bundesliga: 17th and 18th go down; the 16th plays the play-off against the 3rd of the
    3. Liga. The 1st and 2nd go up.
  - 3. Liga: 1st and 2nd go up; the 17th to 20th go down. They are replaced the way the original
    replaces relegated amateur clubs. Ticket 1 finds out how that works.
- Ticket 1 checks every place that hard-codes 18/20/20, 38 or 58: league order lists, fixture
  generator, tables, newspaper, cup qualification, statistics and save games. It lists them in
  `docs/mod2026/league-structure.md` before any code changes.
- With the original data the game keeps 18 / 20 / 20.

### R2 Season 2026/27

- A new game with mod data starts in season 2026/27.
- **Calendar and dates:** whatever the start menu, calendar, newspaper, statistics and history
  derive from the start year shows 2026/27 and the following years, and stays correct across the
  year 2027+ (no two-digit or 19xx overflow).
- **DFB-Pokal:** the cup draw uses the clubs that exist in the mod data.

### R3 Club data

- `docs/mod2026/data/clubs.json` lists every club with:
  - name, league, previous-season table (2025/26 final table: points, goals, rank; the rank
    inside its league as of 2025/26, or 0);
  - crest picture number;
  - source URLs.
- Clubs promoted into the 3. Liga for 2026/27 take their Regionalliga rank.
- Strengths are recomputed from the table as the team editor does. 3. Liga clubs get table-based
  strengths too instead of the purely random ones.
- **Points:** the table holds the 2-point values the format expects. Converting 3-point data to
  that form is documented.

### R4 Squads

- `docs/mod2026/data/squads.json` lists, for each club, the first-team squad as of October 2026:
  player name, position and market value in euros, with the source and the date fetched.
- **Squad cut:** a club has at most 20 player slots. If the squad is larger, the players with the
  lowest market value are left out. On a tie, the younger player is kept.

### R5 Crests

- Each of the 56 clubs gets a crest picture in its current design (several clubs changed their
  logo since 1992).
- Each crest is converted to the size and palette of the original crest pictures and committed as
  PNG under `docs/mod2026/crests/`. It is written as `PIC/<n>.VGA` by the generator.
- A contact sheet of all 56 crests rendered by the port goes to `progress/`.

### R6 Generator

- The command `bmp mod2026 build --from <pristine dir> --out <dir>` copies the pristine data and
  writes MANA.DAT, the crest pictures and the marker. The output is deterministic.
- `tools/check.sh` gets a test that runs the generator on the pristine copy and checks:
  - MANA.DAT parses and round-trips;
  - every name fits;
  - every crest decodes.

### R7 Verification

- **Playthrough:** a scripted headless run of the mod covers new game 2026/27 → office →
  first match day → table, in all three leagues, and a whole season to season end with promotion
  and relegation. It produces screenshots in `progress/` and port-vs-port golden frames, which
  replace the DOSBox references for mod screens.
- **Original data:** all existing visual scenarios against the original data stay unchanged.

## Out of scope for now

- Foreign UEFA clubs (they keep the 1992 data).
- The 3-point rule and other modern rule changes, unless the owner asks for them.
- The English edition (#118).
