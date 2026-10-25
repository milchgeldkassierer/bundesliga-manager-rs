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
    rule: the marker file `MOD2026.DAT` in the data directory (MISSION.md, branch section).
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
- **3. Liga:** it has 20 clubs, which fits the third group, the original "Amateur-Oberliga" (20 slots, 38 match days), exactly.
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
  - 3. Liga: 1st and 2nd go up; the 17th to 20th go down into the Regionalliga pool.
- **Club slots** (owner decision 2026-10-24):

  | tier | club slots | clubs | match days | relegated directly | relegation play-off |
  |---|---|---|---|---|---|
  | Bundesliga | 0..17 | 18 | 34 | 2 | 16th vs 3rd of the 2. Bundesliga |
  | 2. Bundesliga | 18..35 | 18 | 34 | 2 | 16th vs 3rd of the 3. Liga |
  | 3. Liga | 36..55 | 20 | 38 | 4 | none |
  | Regionalliga pool | 56..63 | 8 | none | none | none |

  The original keeps these values in tables: team count `DS:225e`, rounds `DS:2262`, relegated
  `DS:2266`, play-off flag `DS:226a`, first rank `DS:226e`, id ranges `DS:2272`. The mod sets them
  to the values above. The original's tier 3, "everything above 58", becomes the pool 56..63.
- **Regionalliga pool:**
  - 8 real Regionalliga clubs of 2026/27, with squads and crests: the **clubs with the most fans,
    the most followers and the most relevance** (owner decision 2026-10-24). Candidates are the
    first teams that play in one of the five Regionalligen in 2026/27. **BFC Dynamo is excluded**
    (owner decision); it still appears in the ranked candidate list. Each candidate is ranked in three measures:
    - fans: average home attendance 2025/26;
    - followers: combined followers on Instagram, Facebook, X and TikTok as of October 2026;
    - relevance: league history (seasons in the Bundesliga, 2. Bundesliga, old Oberliga before
      1963 or DDR-Oberliga) and major titles.
    The 8 clubs with the best average rank over the three measures form the pool. Each value
    records its source and fetch date.
  - The 4 clubs relegated from the 3. Liga swap places with 4 pool clubs, so no club leaves the
    game.
  - The pool clubs that go up are chosen the way the original picks replacement clubs (#177):
    the same rule, applied to the pool.
- **Relegation play-off 2. Bundesliga vs 3. Liga:** the original only has the play-off between
  tiers 0 and 1. The mod plays the second pairing the same way: the same logic, screens and
  newspaper, with the tier numbers changed.
- **Texts:** the third tier is called "3. LIGA" in every text: tables, newspaper, statistics,
  history, manager offers and the start menu. In the original it is "AMATEUR-OBERLIGA" or
  "AMATEURLIGA".
- **Inventory:** #176 lists every place that hard-codes 18/20/20, 38 or 58: league order lists,
  fixture generator, tables, newspaper, cup qualification, statistics and save games. All of them
  must follow the mod values.
- **Original data:** without `MOD2026.DAT` the game keeps 18 / 20 / 20 and every original rule.

### R1b Three-point rule (mod data only)

- A win gives 3 points, a draw 1 and a loss 0. Tables show the points as a single number, as is
  usual today, instead of the original's "plus:minus" pair, if the column fits. Otherwise the
  "plus:minus" layout stays with the 3-point values.
- **Ranking:** points, then goal difference, then goals scored, as the original ranks apart from
  the points.
- **Everywhere:** every place that computes, stores or shows league points follows the rule:
  tables, newspaper, statistics, eternal table, manager history, season-end ceremonies and the AI
  and finance logic that reads points.
- **Saves:** mod saves keep the save format. The points fields hold the 3-point values.

### R1d Euro instead of D-Mark (mod data only, owner decision 2026-10-24)

- **The unit changes, the amounts do not.** Wherever the currency DM appears, the mod shows "€"
  instead. The game keeps computing and storing the same values, so the balance is unchanged.
- **Texts:** every currency mention changes: "DM", "Mio. DM", "TDM", "D-Mark" and the like.
  - The text to replace is in strings in `crates/bmp` and `crates/bmp-game` (about 40 places)
    and in the newspaper and message templates.
  - The same goes for anything else that prints money: finances, loans, stadium, contracts,
    negotiation, transfers, gate receipts, sponsors, bonuses, statistics and season end.
- **Glyph:** none of the original fonts has a € glyph, and CP437 has none either.
  - Each game font gets one, drawn in that font's style and size, in a free code point. Choosing
    the code point is part of the ticket.
  - The 2x fonts (R5b) get it too.
- **Pictures:** if a picture has "DM" painted into it (check every `PIC/*` the money screens
  use), the generator writes a corrected copy into the mod data directory.
- **Layout:** amounts and their columns stay aligned where "€" (1 glyph) replaces "DM"
  (2 glyphs). The usual German form "1.000.000 €" is used where it fits, otherwise the position
  of the original unit.
- **Original data:** without `MOD2026.DAT` every "DM" stays.

### R1c Real names of the European cups (mod data only)

- **Renames:** in every text that names a European cup (statistics, history, newspaper, draws,
  match day, titles, the manager's trophy cabinet, offers):
  - "Pokal der Landesmeister" / "LANDESMEISTER" → "Champions League" / "CHAMPIONS LEAGUE";
  - "Pokal der Pokalsieger" / "POKALSIEGER" → "Conference League" / "CONFERENCE LEAGUE";
  - "UEFA-Pokal" / "UEFA-Cup" → "Europa League" / "EUROPA LEAGUE".
  (Owner decision 2026-10-25: the Cup Winners' Cup becomes the Conference League and the UEFA Cup
  the Europa League, not the other way round.)
  Abbreviated forms, for example "POKAL DER LANDESM.", get matching short forms ("CHAMPIONS L.",
  "EUROPA L.", "CONFERENCE L.") that fit the same widths.
  **Conference League and Europa League** (owner decisions 2026-10-25, #190, #256): they are wider
  than the names they replace, so their width is measured against the field or the screen at each
  place, not against the original text. The full name is shown wherever it fits there, otherwise
  "CONFERENCE L." / "EUROPA L.". The Champions League stays no wider than the original.
- **Format:** the competitions keep the original's format and qualification. Only the names
  change.
- **Text widths:** every renamed text must fit its original field. Where it does not, a shorter
  form is used and recorded in `docs/mod2026/CHANGES.md`.

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
- **No reserve teams** (owner decision 2026-10-24). The game has no concept of a club's second
  team, so no slot may hold one ("II", "U23", "Amateure"). Each reserve team of the real
  3. Liga 2026/27 (VfB Stuttgart II, TSG Hoffenheim II) is replaced by the next first team in
  sporting order: the clubs relegated from the 3. Liga 2025/26 with the most points that are not
  already in the 3. Liga (Erzgebirge Aue, SSV Ulm 1846). The pool is then filled by the pool rule
  of R1 (fans, followers, relevance), which only takes first teams.
- Strengths are recomputed from the table as the team editor does. 3. Liga clubs get table-based
  strengths too instead of the purely random ones.
- **Points:** MANA.DAT block 3 holds a [plus, minus] pair, and the strength recomputation reads
  it as 2-point values. The generator converts the real 2025/26 table into that pair
  (2 x wins + draws, 2 x losses + draws), so the recomputation keeps its meaning. The 3-point
  rule (R1b) applies to the game played in the mod.

### R4 Squads

- `docs/mod2026/data/squads.json` lists, for each club, the first-team squad as of October 2026:
  player name, position and market value in euros, with the source and the date fetched. Raw
  research data such as market values goes under `docs/mod2026/research/`, which the public copy
  leaves out (MISSION.md, Publication).
- **Squad cut:** a club has at most 20 player slots. If the squad is larger, the players with the
  lowest market value are left out. On a tie, the younger player is kept.

### R5 Crests

- Each of the 56 clubs gets a crest picture in its current design (several clubs changed their
  logo since 1992).
- Each crest is converted to the size and palette of the original crest pictures and committed as
  PNG under `docs/mod2026/crests/`. It is written as `PIC/<n>.VGA` by the generator.
- A contact sheet of all 56 crests rendered by the port goes to `progress/`.

### R5b 2x display layer with high-resolution crests and fonts (owner's decision)

The port keeps the original 320x200 (320x240) indexed frame as the logical screen. All game logic
uses it:
- hit tests;
- pixel-colour reads (`get_pixel`, the cheat check, the spinners);
- the headless/script host, screenshots and every visual test.

Beside it, when the mod data is active, the SDL host keeps a second indexed **display buffer at
2x (640x400 / 640x480)**:

- **Drawing:**
  - Every drawing primitive of `crate::gfx` also draws into the display buffer: blit (opaque and
    transparent), fill, line, pixel, XOR draws, text, page copy, clear, scrolling and the mouse
    pointer.
  - Content without a high-resolution version is drawn doubled (nearest neighbour), so it looks
    exactly as before.
- **Palette:** the display buffer uses the same 256-colour palette, so fades, palette changes and
  colour cycling keep working unchanged.
- **High-resolution crests:**
  - The generator stores a 2x (or larger, downsampled to 2x for display) version of each crest in
    the crest palette, under `docs/mod2026/crests/hires/`.
  - Wherever a crest picture is blitted, the display buffer gets the high-resolution version.
  - Every place a crest is drawn is covered: start menu carousel, office, tables, match day, cup
    draws, newspaper and team editor. Ticket 1 lists them.
- **High-resolution fonts:**
  - Each game font (`docs/re/font.md`) gets a 2x version with real 2x detail, not doubled pixels.
    It is drawn as a bitmap font with the same metrics: every glyph is exactly twice the original
    advance and height, so layout, centring and text widths in the logical screen do not change.
  - The 2x fonts are committed as data under `docs/mod2026/fonts/`, together with the tool or
    process that made them.
  - Text drawn through the font routines uses them in the display buffer.
- **Fallback:** the display buffer is presented instead of the logical frame. If it is off, the
  game is unchanged.
  - The layer is off with the original data, and in the headless host.
  - A command-line switch `--lowres` turns it off.
  - **Crest colours** (owner decision 2026-10-25): the crests may use 64 colours of their own in
    palette entries 64..127 (no original picture uses an entry above 31); the mod's `PIC/1.PAL`
    holds them and the fade covers them.
  - The 2x fonts are off by default (owner decision 2026-10-25): the display layer shows the
    original glyphs pixel-doubled, and `--hires-fonts` switches the 2x fonts on.
- **Tests:**
  - Unit tests check that a doubled-only frame equals the nearest-neighbour scale of the logical
    frame for a sample of existing scenarios.
  - Port-vs-port golden frames cover the 2x display of crests and text.

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
- Modern rule changes other than R1, R1b and R1c, unless the owner asks for them.
- The format of the European cups (only their names change, R1c).
- The English edition (#118).
