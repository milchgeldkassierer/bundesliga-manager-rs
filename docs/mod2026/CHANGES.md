# Mod 2026: changes to the original behaviour

Every change of the port's v1.0.0 behaviour made for Mod 2026 (`PRD.md`) is listed here and
marked `// MOD2026:` in the code. All of them switch on only when the data directory contains
the marker file `MOD2026.DAT`; without it the port behaves exactly like v1.0.0, except that it
refuses a Mod 2026 save (an obvious-bug fix of MISSION invariant 1, "Saves of the other league
layout").

## Generator and mod switch

- **`bmp mod2026 build --from <dir> --out <dir>`** (`crates/bmp/src/main.rs`,
  `crates/bmp-game/src/mod2026.rs`): writes the mod data directory, a deterministic copy of
  every file below `--from` plus the marker `MOD2026.DAT` with fixed content. Into the copy of
  `MANA.DAT` it writes the 64 German club slots from the dataset `docs/mod2026/data/`
  (`clubs.json`, `squads.json`, embedded in the program): the name, the crest picture number,
  the 20 player names (slots without a player all zero bytes, as in the original), the 2025/26
  table as the 2-point pair `[2W+D, 2L+D]`, the goals and the rank, and the strengths (below).
  The 136 foreign clubs keep their bytes; an invalid `MANA.DAT` or a dataset entry that does
  not fit it (a name too long, more than 20 players, a number above 255) is refused before
  anything is written; the dataset's own rules are checked by its tests. `--out` must not exist or be an empty directory, and is never
  the `--from` directory (or inside it) nor `<repo>/BMP` (or inside it); these are refused with a
  non-zero exit code before anything is written. A new command, so the original game is not
  affected.
- **Crest pictures** (`crates/bmp-game/src/mod2026.rs`, `crates/bmp-game/build.rs`, PRD R5):
  the generator encodes every `docs/mod2026/crests/<n>.png` (embedded in the program by
  `build.rs`) with the game's compressor `vga::encode`, in the PNG's size and palette indices,
  and writes it as `PIC/<n>.VGA`, replacing the copied original picture; picture numbers
  without a PNG keep the original. A crest PNG that is not 8-bit indexed is refused before
  anything is written. The game and the team editor load the crests unchanged, so the mod
  clubs show their 2026 crests wherever a crest is drawn.
- **Office captions** (`crates/bmp-game/src/mod2026.rs` `third_tier_captions`, PRD R1): the
  generator copies the "2. LIGA" caption of the tables submenu's table, fixtures and strength
  icons (`PIC/3.VGA` codes 13/16/19) over their "AM-OBERL." captions (14/17/20) and draws a
  "3" in place of the "2" (a 3 x 5 mask in the caption's own colours and shadow), so the
  buttons read "3. LIGA". Nothing of the original sheet is committed; a sheet whose captions
  are not laid out as in the original is refused before anything is written, a source without
  one gets none.
- **Cup submenu captions** (`crates/bmp-game/src/mod2026.rs` `cup_captions`, PRD R1c): the
  generator replaces the caption band (rows 12..21) of the cup submenu's European cup icons
  (`PIC/3.VGA` codes 28/29/30, "LANDES MEISTER", "POKAL SIEGER", "UEFA-CUP") by two lines in
  the 3 x 4 font of the original's two-line captions, in their colours and shadow, centred:
  "CHAMP." / "LEAGUE", "CONF." / "LEAGUE" and "EUROPA" / "LEAGUE". "CHAMPIONS" and
  "CONFERENCE" are 33 and 39 columns wide in this font and do not fit the 32-column button, so
  they are cut like the original's "BUNDESL.". The letters of the original's captions are
  committed as small masks, `C`, `F`, `H`, `U` and `.` are drawn new in the same style;
  nothing of the original sheet is committed. The band starts in the background: the
  single-line "UEFA-CUP" caption and the crests' bottom shadow pixels in row 12 give way. A sheet whose cup icons are not laid out as in the original (checked by redrawing
  "SIEGER" from the masks) is refused before anything is written.
- **Strength grouping** (`crates/bmp-game/src/strength.rs`, `strength::MOD2026`): the team
  editor's strength recomputation moved unchanged from `bmp` to `bmp-game`, with its league
  grouping as a parameter. The generator runs it with the groups 0..17 / 18..35 / 36..55 /
  56..63 (Bundesliga, 2. Bundesliga, 3. Liga, pool), base strengths 70 / 34 / 20 / 20 and
  league sizes 18 / 18 / 20 / 20 for the rank term `size - rank`, and no random group: unlike
  the original's amateurs (group 2, random strengths), the 3. Liga and the pool get strengths
  from their 2025/26 table. Ranks are used as stored (a promoted club's rank in its 2025/26
  league). The 2. Bundesliga scaling is unchanged; the RNG starts at the MSC start state
  (`srand(1)`), so the output is deterministic. The team editor's save ("MACH MAL !",
  `crates/bmp/src/teamed/save.rs`) follows the data: `strength::grouping(Host::mod2026)` picks
  `MOD2026` on mod data, so saving in the editor keeps the table-based 3. Liga and pool
  strengths instead of replacing them with random ones (#224), and `ORIGINAL` on the original
  data, unchanged from v1.0.0.
- **Mod switch** (`bmp_game::mod2026::is_active`): the game and the team editor read it once at
  start-up into `Host::mod2026`, the flag mod behaviour switches on.

## League structure

- **One owner** (`crates/bmp-game/src/league_layout.rs`, PRD R1): the league tables of the
  original (DS:225e..DS:2278: first club, club count and rounds per tier, relegation and play-off
  places, the club ranges) and the season calendar (DS:2280) are one `LeagueLayout`.
  `game::run_main` selects `MOD2026` when `Host::mod2026` is set and `ORIGINAL` otherwise,
  before any game state is built; `Runtime::default` and `GameState::program_start` copy it,
  every other reader calls `league_layout::layout()`.
- **Mod values**: 18 / 18 / 20 clubs with the ids 0..=17, 18..=35 and 36..=55, the pool
  56..=63 (no league), 34 / 34 / 38 rounds, 2 / 2 / 4 clubs relegated directly and a play-off
  place in the Bundesliga and the 2. Bundesliga. Tables, fixtures, statistics, the top scorers,
  the history graph and the season end follow them; the fixtures of the 18-club 2. Bundesliga
  are the Bundesliga's round permutations. The history graph's bars and the high score's
  position and season terms count from the league clubs (the original's immediates 0x3a, its 58
  league clubs, and 0x39, its last league club id), so the last club of the 3. Liga draws one
  line and scores nothing in both layouts. The save format is unchanged: the 18 / 18 / 20 tiers
  fit the `[tier][20]` and `[tier][38][20]` blocks.
- **Calendar**: the 2. Bundesliga plays on the Bundesliga's days. Week 23 (only a Bundesliga
  round, `0x01`) gets a 2. Bundesliga round (`0x03`), weeks 7, 13, 25, 29 and 69 (2. Bundesliga
  and 3. Liga, `0x06`) keep only the 3. Liga round (`0x04`). Without this the 2. Bundesliga
  would play four more match days that replay round 17 without counting.
- **DFB-Pokal draw** (PRD R2): the draw takes clubs up to the last club of the lowest tier
  (DS:2277, 57 in the original), i.e. 0..=55 in the mod; the pool stays out like the original's
  reserve clubs 58..63.
- **Promotion and relegation** (PRD R1): the Bundesliga's 17th and 18th go down, its 16th plays
  the 2. Bundesliga's 3rd; the 2. Bundesliga's 1st and 2nd go up, its 17th and 18th go down,
  its 16th plays the 3. Liga's 3rd; the 3. Liga's 1st and 2nd go up and its 17th..20th swap
  with the pool clubs 59 or 60, then 58, 57 and 56 (the original's replacement rule,
  `relegation-replacement.md`), so no club leaves the game. The second play-off
  (`cups::matchday_second_relegation_match`) runs the first one's code on the same play-off
  days (weeks 91 and 92), right after it with its own "Relegationsspiel" title and results
  screen (the live match when a human manager's club plays; like the original's play-off it
  has no newspaper); the office notice and the "Relegation" banner reach its clubs too. Its first-leg
  goals, result and leg flag live in save block 59 (`DS:05b8`, read by no original code), so the
  save format is unchanged. The season end skips the swap of a play-off place whose club won
  (`season_promotion_relegation`).
- **Team editor** (`crates/bmp/src/teamed/mod.rs` `league_group`, `league_name`,
  `crates/bmp/src/teamed/club.rs` `group_ranks`, `set_rank`): `teamed::run` selects the layout
  like the game, and the editor's league groups, rank limits and rank swap read it: on mod data
  ids 18..=35 are the 2. Liga (ranks 1..18, the swap scans only those ids), 36..=55 the third
  tier (ranks 0..20), 56..=63 the pool, and the panel names the third group `3. LIGA` instead
  of `REGIONALLIGA`. Its save recomputes the strengths with the generator's grouping
  (`crates/bmp/src/teamed/save.rs`, `strength::grouping`, see "Strength grouping", #224).
- **Not yet**: which clubs fill the pool (#177).

## Texts

- **Third tier** (`crates/bmp/src/game/tables.rs` `league_names`,
  `crates/bmp/src/game/season_events.rs` `christmas_bonus_lines`, `league_layout::is_mod2026`):
  a mod game names the third tier `3. Liga` instead of `Am.-Oberliga` (`UI[44]`), and
  `3. LIGA` where the original upper-cases it or prints `AMATEUR-OBERLIGA`. This covers the
  table, fixture and strength titles, the club info, the DFB-Pokal group headings, the
  season-end rank line, the manager history and the Christmas bonus line. Every replacement is
  narrower than the original in all three fonts, so no short form is used. The newspaper, the
  offers and the start menu name no tier; the settings screen already reads `3.Liga`. The
  office's button captions are pictures, which the generator changes (see "Office captions").
- **European cups** (`crates/bmp/src/game/stats_screens.rs` `cup_names`, `trophies`,
  `history_cup_texts`, `crates/bmp/src/game/live_match.rs` `title_winner_texts`,
  `crates/bmp-game/src/finance.rs` `season_goal_messages`, `crates/bmp/src/teamed/uefa.rs`,
  PRD R1c): a mod game names the Pokal der Landesmeister `Champions League`, the Pokal der
  Pokalsieger `Conference League` and the UEFA-Pokal `Europa League` (`UI[128..130]`; owner
  decision of 2026-10-25, #256, which swapped #190's first mapping), in upper case where the
  original upper-cases. Format, qualification and draws are unchanged.
  The sites: the match list's title and the draw's question (`cup_screens.rs`), the title
  screen of a won cup, which reads "gewinnt die" for the three leagues instead of "gewinnt
  den", the manager history, the eternal table's trophy rows (`CHAMPIONS LEAGUE`, `CONFERENCE
  LEAGUE`, `EUROPA LEAGUE` for `LANDESMEISTER-POKALE`, `POKALSIEGER-POKALE`, `UEFA-POKALE`),
  the Bundesliga's season goal messages for the UEFA cup places (now the Europa League's), the
  team editor's refusal of too weak foreign clubs ("UEFA-CUP SONST ZU LEICHT."), and the status
  line of a player with status 3 in the player info and the squad list, which shows `UI[130]`
  (a quirk of the original: "STATUS: UEFA-Pokal", now "STATUS: Europa League").
  The Champions League is narrower than the Pokal der Landesmeister in every font, and the
  Conference League narrower than the Pokal der Pokalsieger. The Europa League is wider than
  the short "UEFA-Pokal" in every font, so like the Conference League it is measured against
  the field or screen at each site (PRD R1c: every renamed text must fit its original field).
  The history, which shortens two original names to `POKAL DER LANDESM.` and `POKAL DER
  POKALS.`, shows the real names whole because they fit its panel. The full names fit at every
  site but these:
  - **Short form**: the team editor's refusal reads `EUROPA L. SONST ZU LEICHT.`; the full
    name would not fit its 118 px message box.
  - **Reworded**: "die letzte Chance verspielt, / die EUROPA LEAGUE zu erreichen." would not
    fit the message box of the season goals, so the mod's message reads "die letzte Chance auf
    die / EUROPA LEAGUE verspielt."; the secured place is "den EUROPA-LEAGUE-PLATZ".
  The newspaper and the offers name no European cup by name ("EUROPAPOKAL", "EUROPA-POKAL"
  stay).

## Three-point rule

- **Owner** (PRD R1b): the points for a win (DS:2271, a byte of the league tables) are
  `LeagueLayout::points_per_win`, 2 in `ORIGINAL` and 3 in `MOD2026`; a draw gives 1, a loss 0.
  The constant `POINTS_PER_WIN` is gone.
- **Sites that follow it**: the match result (`league::apply_result`), the points still available
  in the season goal messages (`finance::season_goal_messages`), the high score's tier term
  (`highscore_compute_score` reads DS:2271 as `tier_base[3]`) and the manager's career points
  lost (`season_record_manager`: points per win × games − points; the original multiplies by 2,
  which the original layout keeps).
- **Display**: the league table shows a single points number in the mod, through the original's
  own branch for DS:2271 > 2. It fits under "PUNKTE": `"     ^6 "` is 22 px wide in the row
  font, the original's `"^6:^4"` 23 px; from 100 points on the goals column moves 4 px right.
  The career record on the eternal table screen shows its points as one number too, so 3-point
  values never appear as a plus:minus pair. Both screens ask `LeagueLayout::single_points_column`.
- **Ranking unchanged**: points, then fewer games played, goal difference, goals scored, as the
  original ranks (PRD R1b: "as the original ranks apart from the points"). The played-games key
  only separates clubs with a postponed match; DS:224d, which would drop it, is the original's
  international switch and stays off.
- **Unchanged**: the save format (at most 19 home wins × 3 = 57 points per side fit the bytes),
  MANA.DAT block 3 and the strength recomputation (2-point previous-season values, PRD R3), and
  the eternal table and board verdict (they rank by prestige, not points).

## Season 2026/27

- **Start year** (`crates/bmp-game/src/newgame.rs` `apply_game_options`, PRD R2): a new mod
  game with the modern start begins in 2026 (`MOD2026_START_YEAR`); with "Histor. Start" it
  keeps the original's 1963/64, with the mod's clubs, squads and rules (owner decision
  2026-10-25; before, both settings started in 2026). The game
  logic reads the mod switch from `Runtime::mod2026`, which `Runtime::default` sets when the
  selected league layout is `MOD2026`. The option still selects its end-year code and the endless and one-year
  rules as before.
- **End year** (`newgame::game_end_year`): save block 43 (FAR:513c) keeps the original's codes
  (1964 / 1966 / 22222 historical, 1993 / 1995 / 22251 modern). Where the original compares or
  shows the code as a year (the game end in `season_final`, the "ENDE:" line of the settings
  screen), a mod game uses its first season year (`start_year - logged_rounds`) plus the code's
  length instead: a one-year game ends in 2027, a three-year game in 2029, an endless game never.
  An original save is refused on mod data (#218, "Saves of the other league layout").
- **Youth players** (`season_end_youth_player`): the original lets youth players join from
  1967 (historical) or 1996 (modern), three seasons after the start. A mod game counts the three
  seasons from its first season year, i.e. from 2030 for a 2026 game, and also after loading
  (the original's FAR:56f4 is not saved, so a loaded original game falls back to the 1966 rule).
- **History season column** (`crates/bmp/src/game/stats_screens.rs`
  `screen_manager_history`): the original prints `1963 + 28 * later start + season`, which is
  1991 for the season 1992/93. A mod game prints the season label from the first season year in
  the original's two-digit form, `26/27`, `27/28`, ..., `99/00`: the 30 px column holds five
  micro4 glyphs, `2026/27` would run into the rank.
- **History places of an original save** (`crates/bmp/src/game/stats_screens.rs`
  `history_place`, no behaviour change, #260): a season's place is not stored as such. The season end
  (`crates/bmp-game/src/season.rs` `season_record_manager`) stores the overall rank across all
  league clubs, `table position + 1 + first_team[tier]`, under the layout in force then, and the
  history subtracts `first_team[tier]` of the layout in force when it is shown. A mod game writes
  and reads under the mod layout and shows every place exactly. An original save read under the
  mod layout would show each 3. Liga/Amateur season two places lower (`first_team[2]` is 38 in
  the original, 36 in the mod; e.g. 5./2. become 7./4.), but since #218 the loader refuses an
  original save on Mod 2026 data ("Saves of the other league layout"), so the history never
  reads a foreign-layout rank. The high-score term and the history graph bars read the same
  stored byte; the season-end rank line reads it right after the season end wrote it.
- **Saves of the other league layout** (`crates/bmp/src/game/load_game.rs` `read_save`,
  `crates/bmp-game/src/league_layout.rs` `saved_layout`, #218): the save format is unchanged and
  has no layout marker; the layout a save was played under is read from save block 32, each
  tier's row of club ids (original 18 / 20 / 20 clubs with tier 2 ids 38..=57, mod 18 / 18 / 20
  with 36..=55; every row is a permutation of its tier's ids, so a save fits only one). A save of
  the other layout is refused with a port-only box "Achtung ! / Spielstand passt nicht / zu
  diesen Spieldaten ! / Vorgang abgebrochen." and the running game is untouched; like a corrupt
  save, its id is forgotten again, so picking it again shows the box again instead of "Keinen
  heutigen Spielstand laden !". A save that fits no known layout loads as in v1.0.0. The shipped
  original saves that `bmp mod2026 build` copies into the mod directory are refused there. On the
  original data, a Mod 2026 save is refused the same way: a deviation from v1.0.0, which loads
  rows the original never writes (`docs/re/logic/stats.md` §7a item 4); every save the original
  or the port writes on the original data still loads. The mod display tests
  (`crates/bmp/src/display.rs`) therefore write a new mod game as `07072019.MAN` into their mod
  data and run `tests/mod2026/matchday_2026.port.txt` on it (`tests/display/README.md`).
- **No change needed**: the office calendar, the newspaper and message dates, the credits and
  the saves use the full year number; no site hard-codes a `19` century prefix. No other screen
  of the original shows a season label.

## 2x display layer

- **Display buffer** (`crates/bmp/src/display.rs`, `crates/bmp/src/gfx.rs`, PRD R5b): with
  Mod 2026 data the SDL host keeps a second indexed buffer at twice the resolution (640x480,
  640x484 in the team editor) and presents it instead of the logical frame (`host.rs`,
  `video.rs`). It mirrors VRAM with one 2x2 block per VRAM byte, so every VRAM write of the
  library also draws into it: blits, fills, lines, pixels, text, page copies, clears, the mouse
  pointer and the palette slots; scrolling and blanking show it like the logical page. XOR
  draws apply to each display pixel; page copies and blits carry the blocks. A memory bitmap
  gets a display side (one block per pixel) once a block other than its doubled pixel is
  written into it, so `blit_bitmap` from the screen and the pointer background keep the blocks
  instead of flattening them. Transparent blits are transparent per display pixel: display
  pixels 0 of the source keep the destination's, also where the logical pixel is 0. It uses
  the same DAC, so fades and palette changes are unchanged. Text and the mod crests have 2x
  detail (**2x fonts**, **High-resolution crests** below); every other block is the doubled
  logical pixel.
  Hit tests, `get_pixel`, the cheat check, the spinners, screenshots and the visual tests read
  the logical frame as before.
- **2x fonts** (`docs/mod2026/fonts/`, `crates/bmp-formats/src/hires_font.rs`,
  `crates/bmp/src/gfx.rs`, `crates/bmp/src/host.rs`, PRD R5b): micro4, flow and newbold have a
  2x version, the Scale2x image of every original glyph, generated by
  `cargo run -p bmp-formats --example hires_fonts -- <data dir>` and embedded in the program
  (`docs/mod2026/fonts/README.md`). The host installs them with the display layer only with
  the option `--hires-fonts` (owner decision 2026-10-25; by default the display layer shows
  the original glyphs pixel-doubled) (`Gfx::set_hires_fonts`); `draw_text` then gives each glyph's temporary bitmap the 2x glyph
  as its display side, in the foreground colour and the background colour (opaque) or 0
  (transparent). The logical frame, every advance, text width and centring are unchanged. Each
  file also holds the 2x euro glyph (`glyph 0xd5`, #205), the Scale2x image of the mod's 1x
  euro glyph (**Euro sign** below) with twice its advance and height; `draw_text` uses it for
  0xd5 when the mod's euro glyph is installed. `normal.font` stays a stub.
- **Crest colours** (`tools/crest-convert.py`, `docs/mod2026/crests/`, `mod2026::crest_colours`,
  `mod2026::build`, `Host::screen_fade`, `teamed::init`, owner decision 2026-10-25): the original
  crests use only the 32 colours of palette entries 0..31, which have no violet, no medium blue
  and only olive greens, so many logos came out grey-blue. No original picture uses an entry above
  31 (a test checks every `PIC/<n>.VGA`), so the mod gives the crests 64 colours of their own in
  entries 64..127: k-means centres of the logo pixels the 32 colours show worst, in 6-bit DAC
  precision, stored as entries 64..127 of the palette every crest PNG carries. The converter maps
  each logo pixel to the allowed colour that keeps its hue (a Lab distance that weighs chroma and
  hue more for saturated colours). The generator writes them into all three palettes of
  `PIC/1.PAL`; the fade (`screen_fade`), which in the original covers only entries 0..31, fades
  them along with the same stepping (`pal::fade_frames_rgb6`), and the team editor loads them
  after its own 32-colour palette. Without mod data nothing changes.
- **High-resolution crests** (`docs/mod2026/crests/hires/`, `tools/crest-convert.py --hires`,
  `crates/bmp-game/build.rs`, `mod2026::hires_crest`, `Host::load_picture`,
  `Gfx::set_display_side`, PRD R5b): every mod crest has a 2x version (76x76), converted from
  the same logo as the crest, in the crest palette and only with colour indices the crests use,
  and embedded in the program as `mod2026::HIRES_CRESTS` (the generator writes nothing new).
  While the display layer is on with its 2x fonts (`Host::enable_display_layer`),
  `Host::load_picture` gives a loaded picture the 2x crest of the same number as its display
  side, but only when the picture is exactly the mod crest, so an original crest or one changed
  in the team editor never shows another crest. The block propagation then carries it through
  the crest caches (`FAR:5354`, the live screen's badges, the team editor's crests) and every
  copy on screen: the start menu's carousel and manager slots, the office, the manager header,
  the live match panels and the team editor's grid, boxes and preview. Doubled by design: the
  team editor's enlarged crest (drawn pixel by pixel), painted pixels and imported pictures
  (an edited crest no longer equals the mod crest), and the foreign clubs' live-match crests
  (`PIC/144+`, no mod crest). The tables, cup draws and the newspaper draw no crest
  (`docs/mod2026/crest-sites.md`).
- **Off switch** (`crates/bmp/src/main.rs`, `host::display_layer_on`): the layer is off with
  the original data, in the headless (`--script`) host and with the new option `--lowres` of
  `run` and `teamed`.
  The live window path is covered by the test
  `host::tests::window_host_presents_the_display_layer_with_mod_data_only` (#210): it builds
  `Host::with_window` under SDL's `offscreen` (or `dummy`) video driver, so no window opens
  on the desktop, and checks that a frame on Mod 2026 data goes through
  `Window::present_display` (640x480 texture) and one with `--lowres` or on the original data
  through the 320x240 `Window::present`. It skips with a notice when no headless SDL video
  driver is available.

## Euro sign

- **Euro glyph** (`crates/bmp-formats/src/font.rs`, `crates/bmp-formats/data/euro_glyphs.txt`,
  `crates/bmp/src/gfx.rs`, `crates/bmp/src/host.rs`): the mod's texts write the euro sign as
  the byte code **0xd5** (`font::EURO`). 0xd5 is where IBM code page 858 puts the euro sign; in
  CP437 it is a box-drawing character the game never prints, it is not printable ASCII and
  none of the eight codes `map_char` translates, so v1.0.0 draws it as a space. The three
  glyph fonts **micro4** (5 x 6 pixels), **flow** (7 x 8) and **newbold** (13 x 16) got a
  euro glyph, new art in each font's style (its `C` with two bars), with the font's cell
  height, descent row and spacing column. With the mod flag `Gfx::set_euro_glyphs` installs
  them and text drawing and text widths use them for 0xd5; `map_char` and the font tables are
  unchanged, and without the flag 0xd5 stays a space. `encode_cp437('€')` gives 0xd5.
  `normal.font` stays a stub.
- **Euro in the season and statistics texts** (`crates/bmp-game/src/season.rs`,
  `crates/bmp/src/game/{season_events,season_end,stats_screens,scorers}.rs`, PRD R1d): with
  the mod flag (`Runtime::mod2026` in the season logic, `Host::mod2026` on the screens) the
  hooligan damage message, the season-end report (promotion bonuses, "Sie beginnen neu mit
  500.000 €.", the top scorer's bonus), the transfer fee of a leaving player, the Christmas
  bonus (`"BUNDESLIGA: 850.000 €, 2.LIGA: 425.000 €,"`), the forfeit fine, the balance, debts,
  monthly income and expenses of the statistics and the market value in the player info show
  `" €"` (`bmp_game::mod2026::currency`) where v1.0.0 shows `" DM"`. The unit stays where the original's
  was; every text starts and is centred as before and gets narrower. The amounts paid and
  stored are unchanged.
- **Euro on the money screens** (`crates/bmp/src/game/{credits,stadium,contract,negotiation,
  transfer_screen,squad_screen}.rs`, PRD R1d): with the mod flag (`Host::mod2026`) the loans,
  stadium, sponsor contract, contract negotiation, transfer market and squad list screens show
  every amount in euros: the `" DM"` after an amount becomes `" €"` (`bmp_game::mod2026::currency`, re-exported as `game::currency`), so
  `"ZINSEN PRO MONAT: 7.000 €"`, `"maximal 1 Mio. €"`, `"von 1.000.000 € belegt."`,
  `"GEHALT PRO MONAT (IN €)"`, `"150.000 € PRO 1000"`, `"1.000 €/Woche."` (training camp
  prices on the status line). The unit stays where the original's was, and the amounts keep
  their padding and position, so columns stay aligned; the euro glyph is narrower than `DM` in
  every font. The money values themselves are unchanged. Without the flag every text is
  v1.0.0's.
- **Pictures** (PRD R1d): no picture needed a correction. Every `PIC/*` the money screens
  load was rendered from the v1.0.0 data with its palette and checked by eye: the loans
  (`7`), stadium (`9`..`20`, `40`), value spinner (`21`), sponsor contract (`31`, `32`,
  `46.CP`), squad (`6`, `37`), training (`1`, `5`, `28`, `37`, `43`), statistics (`36`) and
  the office frame they share (`3`, `4`, `22.CP`, `23`..`26`, `39`), plus the newspaper and
  special-day pictures. None has "DM" or another currency unit painted in: the contract
  board's amount fields are empty and filled at run time, and the negotiation, transfer
  market, finance and season-end screens load no screen-specific picture, only the shared
  buttons (`37`) and value spinner (`21`). The generator copies these pictures unchanged,
  except `3` (`PIC/3.VGA`), whose third-tier captions it redraws (see "Office captions").
