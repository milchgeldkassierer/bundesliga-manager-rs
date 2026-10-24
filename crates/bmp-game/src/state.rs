//! The persistent game state of `BMMAIN.EXE`.
//!
//! The original keeps its state in plain global variables spread over two
//! data segments: DGROUP (`DS`, Ghidra segment `5cb3`) and the far data
//! segment (`FAR`, Ghidra segment `5238`). A `.MAN` save game stores exactly
//! 61 of these globals ("blocks", see `docs/re/savegame.md`) plus a dynamic
//! message-string table and a 12-byte raw tail.
//!
//! [`GameState`] mirrors that memory: one field per save block, in save
//! order, typed from the code that uses it. Records keep every byte (unknown
//! bytes live in `unknown_*` fields) so that
//! [`GameState::from_savegame`] / [`GameState::to_savegame`] round-trip a
//! save byte for byte. Integer widths and signedness follow the original
//! (`char` = `i8`, `int` = `i16`, `long` = `i32`, little-endian).
//!
//! Full documentation with Ghidra addresses: `docs/re/logic/state.md`.

use bmp_formats::savegame::{self, BLOCKS, SaveGame, StringEntry};

/// Fixed-size little-endian binary layout shared by every saved field.
pub trait Layout: Sized {
    /// Size in bytes in the original memory.
    const LEN: usize;
    /// Reads the value from the front of `r`.
    fn read(r: &mut Reader<'_>) -> Self;
    /// Appends the value to `w`.
    fn write(&self, w: &mut Vec<u8>);

    /// The all-zero value (what the C runtime leaves in BSS / far data).
    fn zeroed() -> Self {
        let zeros = vec![0u8; Self::LEN];
        Self::read(&mut Reader::new(&zeros))
    }

    /// Decodes exactly `Self::LEN` bytes.
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        (bytes.len() == Self::LEN).then(|| Self::read(&mut Reader::new(bytes)))
    }

    /// Encodes into exactly `Self::LEN` bytes.
    fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(Self::LEN);
        self.write(&mut v);
        v
    }
}

/// Byte cursor used by [`Layout::read`]. Callers guarantee the length.
pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Starts reading at the beginning of `bytes`.
    pub fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }

    fn take<const N: usize>(&mut self) -> [u8; N] {
        let out: [u8; N] = self.bytes[self.pos..self.pos + N].try_into().unwrap();
        self.pos += N;
        out
    }
}

macro_rules! int_layout {
    ($($t:ty),*) => {$(
        impl Layout for $t {
            const LEN: usize = std::mem::size_of::<$t>();
            fn read(r: &mut Reader<'_>) -> Self {
                <$t>::from_le_bytes(r.take())
            }
            fn write(&self, w: &mut Vec<u8>) {
                w.extend_from_slice(&self.to_le_bytes());
            }
        }
    )*};
}
int_layout!(u8, i8, u16, i16, u32, i32);

impl<T: Layout, const N: usize> Layout for [T; N] {
    const LEN: usize = T::LEN * N;
    fn read(r: &mut Reader<'_>) -> Self {
        std::array::from_fn(|_| T::read(r))
    }
    fn write(&self, w: &mut Vec<u8>) {
        for x in self {
            x.write(w);
        }
    }
}

/// Declares a record struct whose fields are laid out back to back, exactly
/// like the original C struct (no padding: MSC packs these byte tables).
macro_rules! record {
    (
        $(#[$m:meta])*
        pub struct $name:ident {
            $( $(#[$fm:meta])* pub $f:ident : $t:ty, )*
        }
    ) => {
        $(#[$m])*
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct $name {
            $( $(#[$fm])* pub $f: $t, )*
        }

        impl Layout for $name {
            const LEN: usize = 0 $( + <$t as Layout>::LEN )*;
            fn read(r: &mut Reader<'_>) -> Self {
                $name { $( $f: <$t as Layout>::read(r), )* }
            }
            fn write(&self, w: &mut Vec<u8>) {
                $( self.$f.write(w); )*
            }
        }
    };
}

/// Returns the bytes of a NUL-terminated C string field.
pub fn c_str(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    &bytes[..end]
}

// TODO(re): a string with no room for its NUL overruns into the next field
// in the original; the shipped data has none.
/// `strcpy` into a fixed field: the C string `src` and its NUL. The bytes
/// after the NUL keep their values; a string that fills the field is cut.
pub(crate) fn strcpy(dst: &mut [u8], src: &[u8]) {
    let s = c_str(src);
    let n = s.len().min(dst.len());
    dst[..n].copy_from_slice(&s[..n]);
    if let Some(nul) = dst.get_mut(n) {
        *nul = 0;
    }
}

/// Club index stored in [`Manager::team`] when the slot has no club
/// (written by `start_menu_new_or_load` 18bc:2184).
pub const NO_TEAM: u8 = 100;
/// Value of [`Player::owner_manager`] for players no human manager owns
/// (`mana_load_player_names` 422b:035c).
pub const NO_OWNER: u8 = 5;
/// Byte offsets into a [`GameState::rescheduled_matches`] entry (`+0` is unused): the day
/// the game is moved to (0 = free slot), its league round, tier and pair.
pub const RESCHEDULED_DAY: usize = 1;
pub const RESCHEDULED_ROUND: usize = 2;
pub const RESCHEDULED_TIER: usize = 3;
pub const RESCHEDULED_PAIR: usize = 4;
/// [`GameState::week_competitions`] bit of a day that carries rescheduled games.
pub const RESCHEDULED_GAMES_DAY: u8 = 0x80;
/// Number of human-manager slots in every per-manager table.
pub const MANAGER_SLOTS: usize = 4;
/// Number of clubs in the club table FAR:3066.
pub const CLUB_COUNT: usize = bmp_formats::mana::CLUB_COUNT;
/// Number of German clubs (league clubs with table/history entries).
pub const GERMAN_CLUBS: usize = bmp_formats::mana::GERMAN_CLUB_COUNT;
/// Number of entries of the player master table FAR:57dd (entry 0 unused).
pub const PLAYER_COUNT: usize = 151;
/// Squad slots per squad in FAR:774a.
pub const SQUAD_SIZE: usize = 25;
/// Number of squads in FAR:774a (4 human squads + transfer pool).
pub const SQUAD_COUNT: usize = 5;
/// Squad index of the transfer pool (transfer list) in FAR:774a.
pub const TRANSFER_LIST: usize = 4;
/// Records 0..24 of a human squad are scanned; record 24 is only reached by
/// the shifts of insert and remove.
pub const SQUAD_SCAN: usize = 24;
/// Records 0..12 of the transfer list are scanned.
pub const TRANSFER_LIST_SCAN: usize = 12;
/// Value of [`Player::owner_manager`] for a player listed by his club. It
/// equals the transfer list's squad index: `squad_remove_player` 2ecd:10ee
/// compares the owner with `FAR:304a`, which is the squad index.
pub const CLUB_LISTED: u8 = TRANSFER_LIST as u8;
/// Value of [`SquadPlayer::board_col`] for a player without a tactics-board
/// spot (0xff in the original).
pub const BOARD_UNPLACED: i8 = -1;
/// Last row of the tactics board in [`SquadPlayer::board_row`]: the original
/// uses rows 0..=7 (0 = attack, 7 = goal).
pub const BOARD_ROW_MAX: i8 = 7;
/// Value of [`SquadPlayer::loan`] that selects price mode 3 of
/// `player_value_calc` 3277:25de (`'c'`).
pub const LOAN_PRICE_MODE: u8 = 0x63;

record! {
    /// One loan (`Kredit`), 0x12 bytes, inside [`Manager::credits`].
    ///
    /// Users: `finance_weekly_processing` 212a:0a6d, `finance_sum_entries`
    /// 212a:0f8d, `credit_take` 212a:1039, `credit_list_entry` 212a:119b.
    /// A slot is free when `principal == 0`.
    pub struct Credit {
        /// +0x00 `long` amount still owed; repaid (and credited to the lending
        /// manager unless the lender is the bank) when due.
        pub principal: i32,
        /// +0x04 `long` interest paid on every payment day (summed by
        /// `finance_sum_entries(1, ..)`).
        pub interest_payment: i32,
        /// +0x08 day of month the loan was taken.
        pub day_taken: u8,
        /// +0x09 payment day of month, compared with FAR:2eba.
        pub payment_day: u8,
        /// +0x0a month (0-based) the loan was taken.
        pub month_taken: u8,
        /// +0x0b due month, compared with FAR:2e8e.
        pub due_month: u8,
        /// +0x0c interest rate in percent.
        pub rate: u8,
        /// +0x0d never written by `credit_take`.
        pub unknown_0d: u8,
        /// +0x0e `int` year the loan was taken.
        pub year_taken: i16,
        /// +0x10 `int` due year, compared with FAR:a7a0.
        pub due_year: i16,
    }
}

record! {
    /// Human manager record, 0x30a bytes at FAR:2242 + m * 0x30a (save block 13).
    ///
    /// Initialized for a new game in `start_menu_new_or_load` 18bc:2184
    /// (manager loop at 18bc:2595..2a20) and `startup_clear_state` 18bc:01fb.
    pub struct Manager {
        /// +0x000 manager name, NUL terminated (`ui_draw_manager_header`).
        pub name: [u8; 29],
        /// +0x01d (FAR:225f) slot number 1..4 (`startup_clear_state` writes
        /// `m + 1`; `screen_load_game` repairs zero).
        pub slot: u8,
        /// +0x01e (FAR:2260) club index into [`GameState::clubs`]; [`NO_TEAM`]
        /// = none. Searched by `manager_index_of_team` 1310:32b1.
        pub team: u8,
        /// +0x01f..0x038 not yet traced.
        pub unknown_01f: [u8; 26],
        /// +0x039 (FAR:227b) championships won (`season_main_loop` 2a58:3763).
        pub titles: u8,
        /// +0x03a (FAR:227c) cups won: DFB-Pokal, then European cups 1..3
        /// (`season_main_loop` 2a58:3763).
        pub cup_wins: [u8; 4],
        /// +0x03e (FAR:2280) per-round result log, 50 entries of 4 bytes,
        /// initialized to 0xff (`start_menu_new_or_load`). `season_main_loop`
        /// shifts it down when full (count DS:07e2, capped at 0x31).
        pub round_log: [[u8; 4]; 50],
        /// +0x106..0x109 not yet traced.
        pub unknown_106: [u8; 4],
        /// +0x10a (FAR:234c) popularity; new game: `(scenario + 8) * 2 -
        /// 3 * league`. Read by the stadium screens and
        /// `match_attendance_estimate` 1f9d:11e0.
        pub value_10a: u8,
        /// +0x10b (FAR:234d) league-position history, one byte per round
        /// (38 rounds), 0-based table position. Initialized to 10 on a new
        /// game. At the end of the table update 3a41:2d33 (3a41:3786..37b5)
        /// every manager gets `position_history[competition_rounds[tier] - 1]
        /// = team_tables[team].table_position`, where `tier` is the tier
        /// just updated (the write happens for all managers, whatever
        /// their own tier).
        pub position_history: [u8; 38],
        /// +0x131 (FAR:2373) lineup (tactic) slider 0..=34, initialized to
        /// 0x10 (neutral); set by the lineup screen, read by
        /// `squad_recalc_team_strength` 1f9d:0002.
        pub value_131: u8,
        /// +0x132 (FAR:2374) `manager_cup_stage[c]` of cup row `c` (and a fifth byte):
        /// 0x1e = not taking part, 1 = in round 1, `k + 1` = won round `k`. Initialized to
        /// 0x1e; `startup_clear_state` re-sets `[2..5]` to 0x1e.
        pub values_132: [u8; 5],
        /// +0x137 not yet traced.
        pub unknown_137: u8,
        /// +0x138 (FAR:237a) league tier of the club: 0 = Bundesliga,
        /// 1 = 2. Bundesliga, 2 = amateur (compared with
        /// `league_of_rank(..) >> 1`).
        pub league: u8,
        /// +0x139 (FAR:237b) days until the next training camp may be
        /// booked: `random(7, 18)` after a booking, decremented daily by
        /// `finance_daily_processing` 212a:0a6d.
        pub camp_cooldown: u8,
        /// +0x13a..0x13c per-manager state bytes: `[0]` league home games of
        /// the season (2a58:20b2), `[1]` seasons dropped from the full
        /// [`Manager::round_log`] (2a58:3763), `[2]` not yet traced.
        pub values_13a: [u8; 3],
        /// +0x13d (FAR:237f) match aggression 0..=40, or 100 when fewer than
        /// 8 players start (`squad_recalc_team_strength` 1f9d:0002).
        pub aggression: u8,
        /// +0x13e per-manager state byte.
        pub value_13e: u8,
        /// +0x13f (FAR:2381) training intensity 0..=0x22 (`screen_training`);
        /// drives a monthly cost and [`Manager::fatigue_sum`].
        pub training_intensity: u8,
        /// +0x140 (FAR:2382) season-end status: 1 = promoted, 2 = relegated, 4 = licence
        /// withdrawn, 8 = dropped out of the Amateur-Oberliga (`season_main_loop` 2a58:3d99).
        pub value_140: u8,
        /// +0x141 (FAR:2383) 5 bytes copied from DS:0286 on a new game
        /// (`screen_training`).
        pub training_141: [u8; 5],
        /// +0x146 (FAR:2388) 4 bytes copied from DS:028c on a new game.
        pub training_146: [u8; 4],
        /// +0x14a attendance of each league home game of the season in
        /// thousands (low byte), indexed by `values_13a[0]` (2a58:20b2).
        pub home_attendance: [u8; 20],
        /// +0x15e (FAR:23a0) seven stadium `long` pairs: `[item][0]` = current
        /// value, `[item][1]` = value under construction (0 if none). New game
        /// (`s` = 1 if FAR:513c is 0x7ac/0x7c9, `t` = 1 if it is 0x7ae/0x7cb):
        /// items 0 and 2 = `(2 - league) * k` and item 1 = `(5 - league) * k`
        /// (`_aFlmul`; +5000 / +10000 when `s`), item 3 = `3 - t`,
        /// item 4 = `3 - 2t` (both only when FAR:513c < 2000), item 5 = `4 + s`,
        /// item 6 = `3 + s`. Users: `screen_stadium` 1000:0602,
        /// `stadium_expansion_order` 1000:0000,
        /// `stadium_construction_progress` 1000:20e1, `random_weekly_events`.
        pub stadium: [[i32; 2]; 7],
        /// +0x196 (FAR:23d8) construction countdown in days per stadium item
        /// (`stadium_expansion_order` 1000:0000 sets it,
        /// `stadium_construction_daily` 1000:20e1 counts it down).
        pub build_days: [u16; 7],
        /// +0x1a4 (FAR:23e6) career totals, two `u16` halves (home, away)
        /// per field, added up at every season end by `season_main_loop`
        /// 2a58:3763: points, goals for, goals against, points lost
        /// (`2 * played - points`), played, wins, losses, draws
        /// (`played - losses - wins`).
        pub career_stats: [[u16; 2]; 8],
        /// +0x1c4..0x1db not yet traced.
        pub unknown_1c4: [u8; 24],
        /// +0x1dc (FAR:241e) fans / popularity, new game: `50 - 20 * league`
        /// (`office_random_event`); grows at month end while below 95
        /// (unsigned compare).
        pub fans: u16,
        /// +0x1de (FAR:2420) monthly expense item (`office_random_event`).
        pub extra_costs: u16,
        /// +0x1e0 (FAR:2422) low word of the monthly base income, written by
        /// `finance_monthly_income` 24a4:2636.
        pub income_base: u16,
        /// +0x1e2 (FAR:2424) `int`, `+= 4 * training_intensity` at every
        /// month end (`season_end_messages`).
        pub fatigue_sum: i16,
        /// +0x1e4 (FAR:2426) record `long`s; `[2]` (FAR:242e) is
        /// initialized to 99999.
        pub records: [i32; 3],
        /// +0x1f0 (FAR:2432) account balance (`KONTOSTAND`) in DM. New
        /// game: 1 500 000, 1 900 000 at level DS:4a28 == 4.
        pub balance: i32,
        /// +0x1f4..0x1fb `int`s written by 2a58:20b2.
        pub values_1f4: [i16; 4],
        /// +0x1fc (FAR:243e) loans: 5 lenders x 3 slots. Lender 0..3 =
        /// fellow human manager, lender 4 = bank.
        pub credits: [[Credit; 3]; 5],
    }
}

impl Manager {
    /// Manager name without the NUL padding.
    pub fn name(&self) -> &[u8] {
        c_str(&self.name)
    }

    /// Level of the level-type [`Manager::stadium`] item `item` (3..7,
    /// 0-based): built + ordered, added as 16-bit words the way
    /// `screen_stadium` 1000:0602 indexes its name tables with it.
    pub fn stadium_level(&self, item: usize) -> i16 {
        let [built, ordered] = self.stadium[item];
        (built as u16).wrapping_add(ordered as u16) as i16
    }

    /// Whether the [`Manager::stadium`] item `item` (0-based) is a quality
    /// item (5 and 6, condition and comfort) rather than a size item
    /// (3 and 4, floodlight and scoreboard).
    pub const fn stadium_item_is_quality(item: usize) -> bool {
        item >= 5
    }

    /// Number of levels of the level-type [`Manager::stadium`] item `item`:
    /// [`STADIUM_QUALITY_LEVELS`] for a quality item, else
    /// [`STADIUM_SIZE_LEVELS`].
    pub const fn stadium_item_levels(item: usize) -> usize {
        if Self::stadium_item_is_quality(item) {
            STADIUM_QUALITY_LEVELS
        } else {
            STADIUM_SIZE_LEVELS
        }
    }
}

record! {
    /// Club record, 0x22 bytes at FAR:3066 + c * 0x22 (save block 15).
    ///
    /// Filled by `mana_load_club_table` 362f:36ec from MANA.DAT.
    pub struct Club {
        /// +0x00 club name, NUL terminated (23 bytes, as in MANA.DAT).
        pub name: [u8; 23],
        /// +0x17 (FAR:307d) form/morale, new game `random(0x2d, 0x37)`;
        /// changed after every match by 3a41:1fec.
        pub morale: u8,
        /// +0x18 (FAR:307e) current strength of the three team parts
        /// (`team_strength_value` 1310:1468).
        pub strength: [u8; 3],
        /// +0x1b (FAR:3081) base strength (MANA.DAT values).
        pub base_strength: [u8; 3],
        /// +0x1e (FAR:3084) third per-part value, new game
        /// `random(0x2d, 0x37)` (minus 5 for clubs above 64).
        pub strength_1e: [u8; 3],
        /// +0x21 (FAR:3087) crest picture number - 0x50 in bits 0..6;
        /// bit 7 is a runtime flag.
        pub crest: u8,
    }
}

impl Club {
    /// Club name without the NUL padding.
    pub fn name(&self) -> &[u8] {
        c_str(&self.name)
    }
}

record! {
    /// Player master record, 0x25 bytes at FAR:57dd + p * 0x25 (save
    /// block 17). Entry 0 is unused; squads reference players by index.
    ///
    /// Generated by `mana_load_player_names` 422b:035c.
    pub struct Player {
        /// +0x00 name, NUL terminated.
        pub name: [u8; 26],
        /// +0x1a (FAR:57f7) age in years (random at generation).
        pub age: u8,
        /// +0x1b (FAR:57f8) mean of `skills` (`(s0 + s1 + s2) / 3`, unsigned).
        pub average_skill: u8,
        /// +0x1c (FAR:57f9) three skill values.
        pub skills: [u8; 3],
        /// +0x1f (FAR:57fc) 0 for players 1..20, else a random value in
        /// bands 25+, 50+, 75+ by index (position tendency).
        pub position_value: u8,
        /// +0x20 (FAR:57fd) random attribute set at generation.
        pub attr_20: u8,
        /// +0x21 (FAR:57fe) owning manager index, [`NO_OWNER`] = none.
        pub owner_manager: u8,
        /// +0x22 league goals and +0x23 league games this season
        /// (`match_assign_ai_scorers` 24a4:14d4, `screen_top_scorers`).
        pub counters: [u8; 2],
        /// +0x24 (FAR:5801) club index of the owner, 0xff = none.
        pub team: u8,
    }
}

/// Width of one position band of [`Player::position_value`]: the original
/// maps the code to a position by `value / 25` (`auto_lineup_pick_best`
/// 2ecd:3635, `random_weekly_events` 1cb5:13bd).
pub const POSITION_BAND_WIDTH: u8 = 25;

/// Number of position bands, 0 (GK) ..= 3 (ATT).
pub const POSITION_BANDS: u8 = 4;

impl Player {
    /// Player name without the NUL padding.
    pub fn name(&self) -> &[u8] {
        c_str(&self.name)
    }

    /// Position band of [`Player::position_value`]: `value / 25`, 0 (GK)
    /// ..= 3 (ATT) for a valid code (see [`POSITION_BAND_WIDTH`]).
    pub fn position_band(&self) -> u8 {
        self.position_value / POSITION_BAND_WIDTH
    }
}

record! {
    /// Squad slot, 0x34 bytes at FAR:774a + (s * 25 + i) * 0x34 (save block 18).
    /// Squads 0..3 belong to the human managers, squad 4 is the transfer pool.
    pub struct SquadPlayer {
        /// +0x00..0x02 (FAR:774a) red cards, yellow cards, unused
        /// (`squad.md` §1.2); `6 * [0] + [1]` is a value term of
        /// `player_value_calc` 3277:25de.
        pub cards: [u8; 3],
        /// +0x03..0x05 (FAR:774d) goals in league, DFB-Pokal, Europapokal.
        /// In transfer-list records `[0]` doubles as the "bid rejected" mask
        /// (bit `m` = manager `m`, bit 7 = any).
        pub goals: [u8; 3],
        /// +0x06..0x08 (FAR:7750) appearances in league, DFB-Pokal,
        /// Europapokal.
        pub appearances: [u8; 3],
        /// +0x09 (FAR:7753) status bits: bits 0-1 availability (0 available,
        /// 1 suspended, 2 injured, 3 unavailable), bits 6-7 transfer flags.
        pub status: u8,
        /// +0x0a (FAR:7754) lineup position: 0 = not in the lineup,
        /// 1..=11 = starting eleven, 12.. = bench. `squad_count_by_flag`
        /// 4091:1109 mode 1 counts `1 <= v < 12`, mode 2 counts `v >= 12`.
        pub lineup_position: u8,
        /// +0x0b (FAR:7755) contract length in seasons.
        pub contract_years: u8,
        /// +0x0c (FAR:7756) loan marker; [`LOAN_PRICE_MODE`] selects value
        /// mode 3 of `player_value_calc` 3277:25de.
        pub loan: u8,
        /// +0x0d (FAR:7757) remaining suspension games.
        pub suspended: u8,
        /// +0x0e (FAR:7758) tendency, 35..=65 (`player_init_random_attrs`
        /// 3277:000a).
        pub tendency: u8,
        /// +0x0f (FAR:7759) index into [`GameState::players`], 0 = empty slot.
        pub player_id: u8,
        /// +0x10 (FAR:775a) three skill values (random on a new game).
        pub skills: [u8; 3],
        /// +0x13 (FAR:775d) exhaustion, 80..=120 when the player joins.
        pub exhaustion: u8,
        /// +0x14 (FAR:775e) bit field from `player_init_random_attrs`
        /// 3277:000a: bits 0-3, bits 4-5 and bit 7.
        pub traits: u8,
        /// +0x15 (FAR:775f) signed rating of the current match: 0 at
        /// kick-off (2a58:20b2), +15 per goal, +10 per assist, -10 per
        /// missed chance or card (`match_ticker_events` 2a58:0ca3).
        pub match_rating: u8,
        /// +0x16 (FAR:7760) club the player moves to when sold; a value
        /// above 0x3f fixes an offer at 130 % (`player_value_calc`).
        pub sale_club: u8,
        /// +0x17 (FAR:7761) injury type, an index into
        /// [`crate::stats::INJURY_NAMES`] (`player_injure` 24a4:30cf); copied
        /// on a manager-to-manager sale.
        pub injury: u8,
        /// +0x18 (FAR:7762) negotiation state: 0 willing, 1..=99 cool-down,
        /// 100..=127 insists on `value - 100` seasons, bit 7 retires when the
        /// contract ends.
        pub negotiation: u8,
        /// +0x19 (FAR:7763) tactics-board column 0..=7, [`BOARD_UNPLACED`] =
        /// not placed.
        pub board_col: i8,
        /// +0x1a (FAR:7764) tactics-board row 0..=[`BOARD_ROW_MAX`] (0 = attack,
        /// [`BOARD_ROW_MAX`] = goal).
        pub board_row: i8,
        /// +0x1b not yet traced.
        pub unknown_1b: u8,
        /// +0x1c, +0x1e, +0x20 (FAR:7766) value terms weighted 50/20/30 by
        /// `player_value_calc` 3277:25de.
        pub value_terms: [u16; 3],
        /// +0x22 (FAR:776c) goals in league, DFB-Pokal and Europapokal for
        /// the club, not reset at the season end (`match_ticker_events`
        /// 2a58:0ca3).
        pub club_goals: [u16; 3],
        /// +0x28 (FAR:7772) `long` from `player_value_calc` 3277:25de: the
        /// monthly salary in squads 0..3 (`finance_monthly_expenses` 24a4:2822
        /// adds it up), the asking fee in the transfer list.
        pub market_value: i32,
        /// +0x2c..0x2f not yet traced.
        pub unknown_2c: [u8; 4],
        /// +0x30 (FAR:777a) far pointer to a message string (offset low,
        /// segment high); patched by the loader through the string table.
        pub message_ptr: u32,
    }
}

impl SquadPlayer {
    /// Mean of the three skills, rounded down.
    pub fn average_skill(&self) -> i16 {
        (self.skills.iter().map(|&s| u16::from(s)).sum::<u16>() / 3) as i16
    }
}

record! {
    /// Newspaper match record of one human manager, 0x9a bytes at
    /// FAR:90ca + m * 0x9a (not saved). Zero-filled and partly set at kickoff
    /// by the match-day driver 2a58:20b2, filled during the match by
    /// `news_record_match_event` 3e3a:223e (`docs/re/logic/news.md` §2).
    pub struct NewsRecord {
        /// +0x00 own goals at the moment of the biggest deficit (`%2`).
        pub deficit_own: u8,
        /// +0x01 opponent goals at that moment (`%3`).
        pub deficit_opp: u8,
        /// +0x02 own goals at the moment of the biggest lead (`%4`).
        pub lead_own: u8,
        /// +0x03 opponent goals at that moment (`%5`).
        pub lead_opp: u8,
        /// +0x04 own goals (`%0`).
        pub goals_own: u8,
        /// +0x05 opponent goals (`%1`).
        pub goals_opp: u8,
        /// +0x06 number of chances and goals; clamped to 3..=10 by the
        /// newspaper.
        pub n_events: i8,
        /// +0x07 `FAR:0efa[0x36 * opponent]`, set at kickoff, not read here.
        pub opponent_0efa: u8,
        /// +0x08 not written in this area.
        pub unknown_08: u8,
        /// +0x09 `goals_own - goals_opp` after the last goal.
        pub goal_diff: i8,
        /// +0x0a most negative goal difference reached (<= 0).
        pub min_diff: i8,
        /// +0x0b most positive goal difference reached (>= 0).
        pub max_diff: i8,
        /// +0x0c 1-based squad index of the hero (`%c`), 0 = none.
        pub hero: u8,
        /// +0x0d 1-based squad index of the loser (`%d`), 0 = none.
        pub loser: u8,
        /// +0x0e own club index (`%a`).
        pub club_a: u8,
        /// +0x0f opponent club index (`%b`).
        pub club_b: u8,
        /// +0x10 not written in this area.
        pub unknown_10: u8,
        /// +0x11 0 = draw, 1 = ahead/won, 2 = behind/lost after the last goal.
        pub result: u8,
        /// +0x12 1 if the home stadium was sold out.
        pub sold_out: u8,
        /// +0x13 minute of each goal.
        pub goal_minute: [u8; 20],
        /// +0x27 0-based squad index of each scorer, 0x64 = opponent goal.
        pub goal_player: [u8; 20],
        /// +0x3b minute of each chance.
        pub chance_minute: [u8; 20],
        /// +0x4f 0-based squad index of each chance, 0x64 = opponent.
        pub chance_player: [u8; 20],
        /// +0x63 fourth argument of each goal event (not read here).
        pub goal_arg4: [u8; 20],
        /// +0x77 fourth argument of each chance event (not read here).
        pub chance_arg4: [u8; 20],
        /// +0x8b used entries of the goal arrays (unbounded).
        pub n_goals: u8,
        /// +0x8c used entries of the chance arrays (unbounded).
        pub n_chances: u8,
        /// +0x8d 0-based squad indices of the booked players.
        pub yellow: [u8; 6],
        /// +0x93 1-based squad index of the player sent off, 0 = none.
        pub red: u8,
        /// +0x94 used entries of `yellow` (unbounded).
        pub n_yellow: u8,
        /// +0x95 not written in this area.
        pub unknown_95: u8,
        /// +0x96 spectators (`%9`); 0 means away match.
        pub spectators: i32,
    }
}

record! {
    /// League table record of a German club, 0x36 bytes at FAR:0ecc + t * 0x36
    /// (save block 16). Index 0 of each 4-byte group is the home value, 1 the
    /// away value, 2/3 a saved copy (`3a41:1ef4` copies slot a to slot b).
    /// Updated per match in 3a41:2d33.
    pub struct TeamTable {
        /// +0x00 points (a win adds DS:2271, a draw 1).
        pub points: [u8; 4],
        /// +0x04 two 9-byte form strings: last 8 results as letters `S`/`U`/`N`
        /// (win/draw/loss), lower case = home match, `@` = no match yet
        /// (placeholder DS:54c9 from `mana_load_club_table`); shifted by 3a41:1fb0,
        /// row 1 is the saved copy (3a41:1ef4).
        pub form: [[u8; 9]; 2],
        /// +0x16 goals scored.
        pub goals_for: [u8; 4],
        /// +0x1a goals conceded.
        pub goals_against: [u8; 4],
        /// +0x1e matches played: 3a41:2d33 adds 1 to the home slot of the
        /// home club and the away slot of the away club in every match.
        /// Second table sort key (fewer is better), ignored when
        /// DS:224d != 0. Draws = `played - wins - losses`.
        pub played: [u8; 4],
        /// +0x22 not yet traced.
        pub unknown_22: [u8; 4],
        /// +0x26 wins.
        pub wins: [u8; 4],
        /// +0x2a losses.
        pub losses: [u8; 4],
        /// +0x2e current 0-based table position inside the club's tier,
        /// rewritten after every table sort in 3a41:2d33 (3a41:3769):
        /// `team_tables[league_teams[tier][i]].table_position = i`.
        pub table_position: u8,
        /// +0x2f..0x31 not yet traced.
        pub unknown_2f: [u8; 3],
        /// +0x32 prestige `long` accumulated by `season_end` 2a58:4597
        /// (rank bonus plus 20/30/40/50 for competition winners).
        pub prestige: i32,
    }
}

record! {
    /// Head-to-head and record tables, 0x1394 bytes at FAR:9336 (save block 35).
    /// Cleared by `startup_clear_state` 18bc:01fb.
    pub struct MatchHistory {
        /// FAR:9336 `[manager][team][5][2]`: last five results of each manager
        /// against each club, `goals_a * 16 + goals_b`, 0xff = none
        /// (3a41:2073).
        pub recent_results: [[[[u8; 2]; 5]; 64]; 4],
        /// FAR:9d36 `[team][7][3]` series counters (3a41:2d33).
        pub team_series: [[[u8; 3]; 7]; 64],
        /// FAR:a276 `[manager][7][3]` best series of each manager.
        pub manager_series_best: [[[u8; 3]; 7]; 4],
        /// FAR:a2ca `[team][8]` record results (`goals_a * 16 + goals_b`).
        pub record_scores: [[u8; 8]; 64],
        /// FAR:a4ca `[team][8]` opponent club of each record result.
        pub record_opponents: [[u8; 8]; 64],
    }
}

/// The complete persistent game state: every global that a `.MAN` save
/// stores, in save order. Field docs give the original address and the save
/// block number (`docs/re/savegame.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameState {
    // ---- header (not game state, kept for byte-exact saves) ----
    /// Signature as stored (DS:93bc).
    pub signature: [u8; 25],
    /// Version tag (`>V2\0`).
    pub version: [u8; 4],
    /// Random save id (checked against the session history FAR:a6d8).
    pub save_id: u32,
    /// Ignored random pad byte.
    pub pad: u8,
    /// Initial cipher key (FAR:57cf).
    pub initial_key: u8,

    // ---- the 61 blocks ----
    /// #0 DS:060e loan interest rates in percent (lender index; computed in
    /// 212a:000a from the base rate DS:0630, clamped 2..7).
    pub interest_rates: [u8; 6],
    /// #1 DS:0618 number of message strings per manager (string-table group
    /// sizes, FAR:5524).
    pub message_counts: [u8; 4],
    /// #2 FAR:5350 match-ticker line slots, 0xff = free.
    pub ticker_slots: [u8; 4],
    /// #3 FAR:304a index of the manager whose turn it is (`char`).
    pub current_manager: i8,
    /// #4 FAR:5358 saved copy of the current manager index (`char`).
    pub saved_manager: i8,
    /// #5 DS:079e per-manager byte pair (`[m][0]` lineup state, `> 1` asks
    /// the manager to fix the lineup, 2ecd:14c7).
    pub lineup_state: [[u8; 2]; 4],
    /// #6 FAR:6ddc league results `[tier][round 0..37][slot 0..19]`: goals
    /// of the home (even slot) and away (odd slot) team, 0x1e = not played.
    pub league_results: [[[u8; 20]; 38]; 3],
    /// #7 DS:07ab number of human managers (also the string-table group count).
    pub manager_count: u8,
    /// #8 DS:07ac DFB-Pokal winner + 1, 0 = none (prestige bonus 20); set by
    /// `cup_round_complete` 27c2:16dc.
    pub winner_07ac: u8,
    /// #9 DS:07ad German Champions' Cup holder + 1, 0 = none or a foreign winner (prestige
    /// bonus 50).
    pub winner_07ad: u8,
    /// #10 DS:07ae German Cup Winners' Cup holder + 1, 0 = none or a foreign winner (prestige
    /// bonus 40).
    pub winner_07ae: u8,
    /// #11 DS:07af German UEFA Cup holder + 1, 0 = none or a foreign winner (prestige bonus 30).
    pub winner_07af: u8,
    /// #12 DS:07b0 DFB-Pokal runner-up + 1 (`cup_round_complete` 27c2:16dc, read by
    /// `euro_cup_qualifiers_build` 27c2:0ef2).
    pub dfb_cup_runner_up: u8,
    /// #13 FAR:2242 the four human-manager records.
    pub managers: [Manager; 4],
    /// #14 FAR:5714 rescheduled matches (`Nachholspiele`), 20 x 5 bytes.
    pub rescheduled_matches: [[u8; 5]; 20],
    /// #15 FAR:3066 club table.
    pub clubs: [Club; 200],
    /// #16 FAR:0ecc league table records of the 64 German clubs.
    pub team_tables: [TeamTable; 64],
    /// #17 FAR:57dd player master table.
    pub players: [Player; 151],
    /// #18 FAR:774a squads.
    pub squads: [[SquadPlayer; 25]; 5],
    /// #19 FAR:4b5e current match-day pairings `[tier][10][home, away]`,
    /// 0x46 (`'F'`) = no match.
    pub matchday_pairings: [[[u8; 2]; 10]; 3],
    /// #20 FAR:2eba current day of month (`long`).
    pub day_of_month: i32,
    /// #21 FAR:2e8e current month, 0-based (`long`).
    pub month: i32,
    /// #22 FAR:a7a0 current year (`long`), compared with loan due years.
    pub year: i32,
    /// #23 DS:07dc running day counter of `season_main_loop` (`long`).
    pub day_counter: i32,
    /// #24 DS:07e0 year word (start year of the game).
    pub start_year: u16,
    /// #25 FAR:56fe the German clubs qualified for a European cup; the length FAR:2e74
    /// ([`crate::runtime::Runtime::euro_qualified_count`]) is not saved
    /// (`euro_cup_qualifiers_build` 27c2:0ef2, `team_is_euro_qualified` 27c2:11e7).
    pub euro_qualified_list: [u8; 10],
    /// #26 FAR:57aa `euro_qualifiers[3][6]`: the German qualifiers of European cup `c` in row
    /// `c - 1`, 0x80 = empty (`euro_cup_qualifiers_build` 27c2:0ef2).
    pub competition_slots: [u8; 18],
    /// #27 FAR:2e6e relegation play-off first-leg goals, re-oriented to the return-leg slot
    /// order (`matchday_relegation_match` 1310:02bf).
    pub relegation_first_leg: [u8; 2],
    /// #28 FAR:76ca `cup_slots[4][32]` as pairs: club of each draw slot of cup row `c`
    /// (0 = DFB-Pokal, 1..3 = European cups); slot `2k` is the home club.
    pub cup_pairings: [[u8; 2]; 64],
    /// #29 FAR:21e2 European first-leg goals, rows 1..3 of `FAR:21c2[4][32]` (row 0 is never
    /// used), already re-oriented to the return-leg slot order (`cup_round_complete`
    /// 27c2:16dc).
    pub cup_first_leg_goals: [u8; 96],
    /// #30 FAR:0008 event state bytes; `[0..4]` is the round of cup row `c` (0 = draw in
    /// progress, 1 = first round .. 5 = final), also read by `random_weekly_events` 3e3a:0fdf.
    pub event_state: [u8; 8],
    /// #31 FAR:1d18 `euro_second_leg_pending[c - 1]` of European cup `c`: 1 after the first
    /// leg was played (`cup_round_complete` 27c2:16dc).
    pub euro_second_leg_pending: [u8; 3],
    /// #32 FAR:535a club ids of each league tier `[tier][20]`, ordered.
    pub league_teams: [[u8; 20]; 3],
    /// #33 FAR:4c7e `cup_goals[4][32]` as pairs: goals of each draw slot in the cup match
    /// just played; +10 = after extra time, +20 = after penalties.
    pub cup_results: [[u8; 2]; 64],
    /// #34 DS:225a per-competition round counters.
    pub competition_rounds: [u8; 3],
    /// #35 FAR:9336 head-to-head and record tables.
    pub match_history: MatchHistory,
    /// #36 DS:05fe option flags (`screen_settings` 362f:01b8).
    pub options: [u8; 15],
    /// #37 FAR:0000 eight state bytes; `[2 * m]` is the main-sponsor
    /// contract countdown in months.
    pub far_0000: [u8; 8],
    /// #38 FAR:4b9a contract/job-offer state (`screen_manager_contract`);
    /// `[2 * (6 * m + i)]` is the countdown in months of advertising
    /// contract `i`.
    pub far_4b9a: [u8; 48],
    /// #39 FAR:2ec2 job-offer state.
    pub far_2ec2: [u8; 80],
    /// #40 FAR:002e 320-byte table (player development, events).
    pub far_002e: [u8; 320],
    /// #41 DS:066c manager contracts `[m][9]` `long`s: `[m][0..7]` monthly
    /// contract income (`[0]` main sponsor, `[1..6]` advertising boards),
    /// `[m][8]` a monthly cost item, initialized to 30000.
    pub manager_contracts: [[i32; 9]; 4],
    /// #42 DS:4a28 difficulty level (`5 - user level`).
    pub level: u8,
    /// #43 FAR:513c scenario / start-year selector (0x7ac, 0x7c9, 0x7ae, 0x7cb
    /// special-cased; 0x56ce in all samples).
    pub scenario: u16,
    /// #44 FAR:1d34 three rows of 10 slot bytes (`team_slot_free_index`).
    pub team_slots: [[u8; 10]; 3],
    /// #45 FAR:5664 3 x 10 far pointers to message strings (patched on load).
    pub message_pointers: [[u32; 10]; 3],
    /// #46 FAR:5780 per-item construction-firm lock in days, shared by all
    /// managers.
    pub construction: [u8; 7],
    /// #47 DS:063a match-day option word (`matchday_options_dialog`).
    pub match_options: u16,
    /// #48 DS:07e2 number of logged rounds (capped at 0x31).
    pub logged_rounds: u16,
    /// #49 FAR:016e current week index into `week_competitions`.
    pub season_week: u8,
    /// #50 DS:2280 per-week competition bitmask (bits 0..2 = competitions,
    /// 9 = special week).
    pub week_competitions: [u8; 95],
    /// #51 DS:0620 training-camp availability counters (> 0 closed, < 0
    /// open), stepped toward 0 daily by `finance_daily_processing`.
    pub ds_0620: [i16; 8],
    /// #52 FAR:4bce month copy at loop start.
    pub loop_month: u8,
    /// #53 FAR:56e2 four bytes (`office_menu_dispatcher`, `message_box`).
    pub far_56e2: [u8; 4],
    /// #54 DS:05fa four bytes; `[0]` = copy-protection flag DS:05ac at save.
    pub ds_05fa: [u8; 4],
    /// #55 FAR:76c6 cup state bytes.
    pub far_76c6: [u8; 4],
    /// #56 FAR:90c2 (no code reference found).
    pub far_90c2: [u8; 4],
    /// #57 DS:05d6 (no code reference found).
    pub ds_05d6: [u8; 4],
    /// #58 DS:061c (no code reference found).
    pub ds_061c: [u8; 4],
    /// #59 DS:05b8 (no code reference found).
    pub ds_05b8: [u8; 4],
    /// #60 FAR:3060 relegation play-off result `cup_tie_winner(1, 0)`: 0 = the Bundesliga club
    /// won, 1 = the 2. Bundesliga club won, 0x1e = undecided (`matchday_relegation_match`
    /// 1310:02bf, read by `season_main_loop`).
    pub relegation_result: u8,

    // ---- after the blocks ----
    /// Message strings per manager (string table, rebuilt into FAR:5524).
    pub messages: Vec<Vec<StringEntry>>,
    /// Raw tail: DS:05f6 (`long`).
    pub ds_05f6: u32,
    /// Raw tail: FAR:2e7a (`long`).
    pub far_2e7a: u32,
    /// Raw tail: FAR:2e94 (`long`).
    pub far_2e94: u32,

    // ---- runtime globals the save does not store ----
    /// DS:0630 base interest rate (2..7) from which
    /// [`GameState::interest_rates`] is derived.
    pub base_interest_rate: i16,
    /// DS:0644 sum of each manager's daily balances in the current month.
    pub month_balance_sum: [i32; 4],
    /// DS:5256 autosave request. Producer: month end 212a:0a6d sets it
    /// when `month % 4 == 0` (months 0, 4, 8). Consumer: the entry of
    /// `office_menu_dispatcher` 18bc:0b5f saves `"autosave.man"` via
    /// `screen_save_game` 422b:07fe and clears it.
    pub autosave_pending: u8,
}

/// DS:0630 in the data segment image.
const INITIAL_BASE_INTEREST_RATE: i16 = 5;

/// Error converting between [`SaveGame`] and [`GameState`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// Block `index` does not have the length of its typed field.
    BlockSize {
        /// Save block number.
        index: usize,
    },
    /// The save has the wrong number of blocks.
    BlockCount,
    /// Player `player` has a position code of 100 or more. The original's
    /// `auto_lineup_pick_best` 2ecd:3635 loops forever once such a player is
    /// available in records 0..23 of a manager's squad (`squad.md` §3.2);
    /// the market can move any player there, so every player is checked.
    PositionCode {
        /// Index into [`GameState::players`].
        player: usize,
    },
    /// Record `slot` of squad `squad` holds a `player_id` past the
    /// [`PLAYER_COUNT`]-entry player table; the original reads whatever
    /// follows the table (`state.md` §1.1).
    SquadPlayerId {
        /// Index into [`GameState::squads`] ([`TRANSFER_LIST`] = transfer pool).
        squad: usize,
        /// Record index within the squad.
        slot: usize,
        /// The saved [`SquadPlayer::player_id`].
        player_id: u8,
    },
    /// Entry `index` of the saved field `field` holds a value the game never
    /// writes and the port uses as an index past its table (`state.md` §1.1).
    SavedIndex {
        /// The field, named after its row of the guard table in `state.md` §1.1.
        field: &'static str,
        /// Entry of the (flattened) field; 0 for a scalar, the manager,
        /// player or flattened squad record for a per-record field.
        index: usize,
    },
    /// Entry `index` of the saved field `field` holds a value the game never
    /// writes and the port would divide by zero, overflow or find no picture
    /// file for (`state.md` §1.1).
    SavedValue {
        /// The field, named after its row of the value table in `state.md` §1.1.
        field: &'static str,
        /// Entry of the field; 0 for a scalar, the manager for a per-manager field.
        index: usize,
    },
}

/// Size levels of the [`Manager::stadium`] items 3 and 4 (floodlight,
/// scoreboard): the names `DS:2858` that `screen_stadium` 1000:0602 indexes
/// with built + ordered.
pub const STADIUM_SIZE_LEVELS: usize = 4;

/// Quality levels of the [`Manager::stadium`] items 5 and 6 (condition,
/// comfort): the names `DS:283c`, indexed like [`STADIUM_SIZE_LEVELS`].
pub const STADIUM_QUALITY_LEVELS: usize = 7;

/// Offers per sponsor kind and manager in FAR:2ec2; the logo bytes of the
/// running contracts (FAR:0000 and FAR:4b9a, odd bytes) index them.
pub const OFFERS_PER_KIND: usize = 10;

/// Smallest [`Player::position_value`] that is no position:
/// [`Player::position_band`] has to be 0 (GK) ..= 3 (ATT).
const POSITION_CODE_LIMIT: u8 = POSITION_BAND_WIDTH * POSITION_BANDS;

/// Highest [`GameState::day_counter`] the game reaches: the season-end loop
/// counts on to day 364 (28 July of the next year) and one more before it
/// resets the counter to 0. `calendar_compute_date` 1310:1190 maps 0..=365
/// to a real date.
pub const DAY_COUNTER_MAX: i32 = 0x16d;

/// Crest codes (`Club::crest & 0x7f`) with a picture `PIC\<code + 0x50>.VGA`
/// in the shipped data: `PIC\80.VGA` ..= `PIC\147.VGA`. MANA.DAT holds the
/// codes 0..=63.
pub const CREST_CODES: u8 = 0x44;

/// Calls `$m!(index, field, Type)` for every save block in save order.
macro_rules! for_each_block {
    ($m:ident) => {
        $m! {
            0 interest_rates: [u8; 6],
            1 message_counts: [u8; 4],
            2 ticker_slots: [u8; 4],
            3 current_manager: i8,
            4 saved_manager: i8,
            5 lineup_state: [[u8; 2]; 4],
            6 league_results: [[[u8; 20]; 38]; 3],
            7 manager_count: u8,
            8 winner_07ac: u8,
            9 winner_07ad: u8,
            10 winner_07ae: u8,
            11 winner_07af: u8,
            12 dfb_cup_runner_up: u8,
            13 managers: [Manager; 4],
            14 rescheduled_matches: [[u8; 5]; 20],
            15 clubs: [Club; 200],
            16 team_tables: [TeamTable; 64],
            17 players: [Player; 151],
            18 squads: [[SquadPlayer; 25]; 5],
            19 matchday_pairings: [[[u8; 2]; 10]; 3],
            20 day_of_month: i32,
            21 month: i32,
            22 year: i32,
            23 day_counter: i32,
            24 start_year: u16,
            25 euro_qualified_list: [u8; 10],
            26 competition_slots: [u8; 18],
            27 relegation_first_leg: [u8; 2],
            28 cup_pairings: [[u8; 2]; 64],
            29 cup_first_leg_goals: [u8; 96],
            30 event_state: [u8; 8],
            31 euro_second_leg_pending: [u8; 3],
            32 league_teams: [[u8; 20]; 3],
            33 cup_results: [[u8; 2]; 64],
            34 competition_rounds: [u8; 3],
            35 match_history: MatchHistory,
            36 options: [u8; 15],
            37 far_0000: [u8; 8],
            38 far_4b9a: [u8; 48],
            39 far_2ec2: [u8; 80],
            40 far_002e: [u8; 320],
            41 manager_contracts: [[i32; 9]; 4],
            42 level: u8,
            43 scenario: u16,
            44 team_slots: [[u8; 10]; 3],
            45 message_pointers: [[u32; 10]; 3],
            46 construction: [u8; 7],
            47 match_options: u16,
            48 logged_rounds: u16,
            49 season_week: u8,
            50 week_competitions: [u8; 95],
            51 ds_0620: [i16; 8],
            52 loop_month: u8,
            53 far_56e2: [u8; 4],
            54 ds_05fa: [u8; 4],
            55 far_76c6: [u8; 4],
            56 far_90c2: [u8; 4],
            57 ds_05d6: [u8; 4],
            58 ds_061c: [u8; 4],
            59 ds_05b8: [u8; 4],
            60 relegation_result: u8,
        }
    };
}

/// Byte length of each block's typed field, in save order.
macro_rules! block_lens {
    ($($i:literal $f:ident : $t:ty,)*) => {
        /// Length of the typed field of every save block (must equal
        /// `savegame::BLOCKS[i].len`).
        pub const BLOCK_FIELD_LENS: [usize; 61] = [$(<$t as Layout>::LEN,)*];
    };
}
for_each_block!(block_lens);

macro_rules! decode_blocks {
    ($($i:literal $f:ident : $t:ty,)*) => {
        // TODO(re): the original keeps whatever the running program holds in
        // the unsaved runtime globals when a game is loaded mid-session
        // (finance.md §7 q.2).
        /// Builds the state from a decoded save game. The runtime globals the
        /// save lacks get their program-start values. Unlike the original it
        /// rejects a save with a player whose position code would hang the
        /// lineup logic ([`StateError::PositionCode`]), a squad record past
        /// the player table ([`StateError::SquadPlayerId`]), another saved
        /// index past its table ([`StateError::SavedIndex`]) or a value the
        /// port would divide by zero, overflow or find no picture for
        /// ([`StateError::SavedValue`]), `state.md` §1.1.
        ///
        /// Origin: BMMAIN.EXE 422b:120c (screen_load_game; block reads mirror
        /// savegame_write_man 422b:07fe)
        pub fn from_savegame(save: &SaveGame) -> Result<GameState, StateError> {
            if save.blocks.len() != BLOCKS.len() {
                return Err(StateError::BlockCount);
            }
            let state = GameState {
                signature: save.signature,
                version: save.version,
                save_id: save.save_id,
                pad: save.pad,
                initial_key: save.initial_key,
                $( $f: <$t as Layout>::from_bytes(&save.blocks[$i])
                    .ok_or(StateError::BlockSize { index: $i })?, )*
                messages: save.strings.clone(),
                ds_05f6: u32::from_le_bytes(save.tail[0..4].try_into().unwrap()),
                far_2e7a: u32::from_le_bytes(save.tail[4..8].try_into().unwrap()),
                far_2e94: u32::from_le_bytes(save.tail[8..12].try_into().unwrap()),
                base_interest_rate: INITIAL_BASE_INTEREST_RATE,
                month_balance_sum: [0; 4],
                autosave_pending: 0,
            };
            state.check_position_codes()?;
            state.check_squad_records()?;
            state.check_saved_indices()?;
            state.check_saved_values()?;
            Ok(state)
        }

        /// Converts the state back into save blocks, byte for byte.
        ///
        /// Origin: BMMAIN.EXE 422b:07fe (screen_save_game / savegame_write_man)
        pub fn to_savegame(&self) -> SaveGame {
            let blocks = vec![$( self.$f.to_bytes(), )*];
            let mut tail = [0u8; 12];
            tail[0..4].copy_from_slice(&self.ds_05f6.to_le_bytes());
            tail[4..8].copy_from_slice(&self.far_2e7a.to_le_bytes());
            tail[8..12].copy_from_slice(&self.far_2e94.to_le_bytes());
            SaveGame {
                signature: self.signature,
                version: self.version,
                save_id: self.save_id,
                pad: self.pad,
                initial_key: self.initial_key,
                blocks,
                strings: self.messages.clone(),
                tail,
                tail_len: 12,
            }
        }

        /// A state with every saved global zero, an empty message table and
        /// the runtime globals at their data-segment image values. This is
        /// not the program-start state: some saved DGROUP globals start
        /// non-zero (see [`GameState::program_start`]).
        pub fn zeroed() -> GameState {
            GameState {
                signature: *savegame::SIGNATURE,
                version: savegame::VERSION_V2,
                save_id: 0,
                pad: 0,
                initial_key: 0,
                $( $f: <$t as Layout>::zeroed(), )*
                messages: Vec::new(),
                ds_05f6: 0,
                far_2e7a: 0,
                far_2e94: 0,
                base_interest_rate: INITIAL_BASE_INTEREST_RATE,
                month_balance_sum: [0; 4],
                autosave_pending: 0,
            }
        }
    };
}

impl GameState {
    for_each_block!(decode_blocks);

    /// The saved globals as the program starts: far data and BSS are zero,
    /// the initialised DGROUP blocks hold their image values.
    ///
    /// Origin: BMMAIN.EXE DGROUP image (initialised data of segment 5cb3)
    pub fn program_start() -> GameState {
        let mut s = GameState::zeroed();
        s.interest_rates = [7, 6, 5, 4, 3, 2];
        s.lineup_state = [[2; 2]; 4];
        s.winner_07ac = 7;
        s.start_year = 0x7ab;
        s.competition_rounds = [1; 3];
        s.options = [1; 15];
        s.options[14] = 0;
        s.manager_contracts = [[50000, 6000, 6000, 6000, 6000, 6000, 6000, 0, 5000]; 4];
        s.level = 2;
        s.match_options = 0x28;
        s.week_competitions = crate::league_layout::layout().week_competitions;
        s.ds_0620 = [-80, -50, -100, -60, 80, 50, 70, 60];
        s.ds_05fa = [1, 1, 1, 2];
        s.ds_05d6 = [0x0b, 0x0e, 0x0b, 0x0b];
        s.ds_061c = [0x31, 0x20, 0x0b, 0x0c];
        s.ds_05b8 = [7; 4];
        s
    }

    /// The load-time guard of [`GameState::from_savegame`]: the first real
    /// player (index 1..) with a position code of [`POSITION_CODE_LIMIT`] or
    /// more. A deliberate deviation; the original loads such a save and hangs
    /// in `auto_lineup_pick_best` 2ecd:3635 once the player is in a squad.
    fn check_position_codes(&self) -> Result<(), StateError> {
        match (1..self.players.len())
            .find(|&i| self.players[i].position_value >= POSITION_CODE_LIMIT)
        {
            Some(player) => Err(StateError::PositionCode { player }),
            None => Ok(()),
        }
    }

    /// The load-time guard of [`GameState::from_savegame`] for the squad
    /// records: the first whose `player_id` is no index into
    /// [`GameState::players`], or, holding a player, whose board row or sale
    /// club is past its table.
    // DEVIATION: the original loads such a save and reads the record's player
    // past the 151-entry table FAR:57dd (`board_auto_place_player` 2ecd:1266,
    // `auto_lineup_pick_best` 2ecd:3635, ...), or past the table its board row
    // or sale club indexes (`state.md` §1.1); the port would panic there.
    fn check_squad_records(&self) -> Result<(), StateError> {
        for (squad, records) in self.squads.iter().enumerate() {
            for (slot, rec) in records.iter().enumerate() {
                if usize::from(rec.player_id) >= self.players.len() {
                    return Err(StateError::SquadPlayerId {
                        squad,
                        slot,
                        player_id: rec.player_id,
                    });
                }
                if rec.player_id == 0 {
                    continue;
                }
                let index = squad * SQUAD_SIZE + slot;
                if !(BOARD_UNPLACED..=BOARD_ROW_MAX).contains(&rec.board_row) {
                    let field = "squads.board_row";
                    return Err(StateError::SavedIndex { field, index });
                }
                if usize::from(rec.sale_club) >= self.clubs.len() {
                    let field = "squads.sale_club";
                    return Err(StateError::SavedIndex { field, index });
                }
            }
        }
        Ok(())
    }

    /// The load-time guard of [`GameState::from_savegame`] for the other
    /// saved values the port uses as table indices: the first one outside
    /// every value the game writes (sentinels included).
    // DEVIATION: the original loads such a save and reads past the indexed
    // table (`state.md` §1.1 lists the fields); the port would panic.
    fn check_saved_indices(&self) -> Result<(), StateError> {
        let clubs = self.clubs.len();
        let german_clubs = self.team_tables.len();
        let tiers = self.league_teams.len();
        let check = |field: &'static str, index: usize, ok: bool| {
            if ok {
                Ok(())
            } else {
                Err(StateError::SavedIndex { field, index })
            }
        };
        check(
            "manager_count",
            0,
            usize::from(self.manager_count) <= MANAGER_SLOTS,
        )?;
        check(
            "current_manager",
            0,
            self.current_index().is_some_and(|m| m < MANAGER_SLOTS),
        )?;
        check(
            "season_week",
            0,
            usize::from(self.season_week) < self.week_competitions.len(),
        )?;
        check(
            "logged_rounds",
            0,
            i16::try_from(self.logged_rounds).is_ok(),
        )?;
        let holders = [
            self.winner_07ac,
            self.winner_07ad,
            self.winner_07ae,
            self.winner_07af,
            self.dfb_cup_runner_up,
        ];
        for (i, &h) in holders.iter().enumerate() {
            check("cup holders + 1", i, usize::from(h) <= clubs)?;
        }
        for (i, &r) in self.event_state[..4].iter().enumerate() {
            check(
                "event_state cup round",
                i,
                usize::from(r) < crate::cups::CUP_ROUND_PAIRINGS.len(),
            )?;
        }
        for (i, &t) in self.league_teams.as_flattened().iter().enumerate() {
            check("league_teams", i, usize::from(t) < german_clubs)?;
        }
        for (i, &t) in self.cup_pairings.as_flattened().iter().enumerate() {
            check("cup_pairings", i, usize::from(t) < clubs)?;
        }
        for (i, e) in self.rescheduled_matches.iter().enumerate() {
            check(
                "rescheduled_matches",
                i,
                usize::from(e[RESCHEDULED_TIER]) < tiers
                    && usize::from(e[RESCHEDULED_PAIR]) < self.matchday_pairings[0].len(),
            )?;
        }
        let opponents = self.match_history.record_opponents.as_flattened();
        for (i, &t) in opponents.iter().enumerate() {
            check("match_history.record_opponents", i, usize::from(t) < clubs)?;
        }
        for (m, man) in self.managers.iter().enumerate() {
            check("managers.team", m, usize::from(man.team) < clubs)?;
            for item in 3..man.stadium.len() {
                let levels = Manager::stadium_item_levels(item);
                check(
                    "managers.stadium level",
                    m,
                    usize::try_from(man.stadium_level(item)).is_ok_and(|l| l < levels),
                )?;
            }
        }
        let logos = self
            .far_0000
            .iter()
            .chain(&self.far_4b9a)
            .skip(1)
            .step_by(2);
        for (i, &logo) in logos.enumerate() {
            check("sponsor logos", i, usize::from(logo) < OFFERS_PER_KIND)?;
        }
        for (m, man) in self.active_managers().iter().enumerate() {
            check(
                "active managers.team",
                m,
                usize::from(man.team) < german_clubs,
            )?;
            check("active managers.league", m, usize::from(man.league) < tiers)?;
            check(
                "active managers.values_13a[0]",
                m,
                usize::from(man.values_13a[0]) < man.home_attendance.len(),
            )?;
            let due_months = man
                .credits
                .as_flattened()
                .iter()
                .filter(|c| c.principal != 0);
            for c in due_months {
                check(
                    "active managers.credits.due_month",
                    m,
                    usize::from(c.due_month) < crate::finance::DAYS_IN_MONTH.len(),
                )?;
            }
        }
        for (p, player) in self.players.iter().enumerate().skip(1) {
            check("players.owner_manager", p, player.owner_manager <= NO_OWNER)?;
            check(
                "players.team",
                p,
                usize::from(player.team) < clubs || player.team == 0xff,
            )?;
        }
        Ok(())
    }

    /// The load-time guard of [`GameState::from_savegame`] for the saved
    /// values that are no table index but would still stop the port: the
    /// first one outside every value the game writes.
    // DEVIATION: the original loads such a save and then aborts or misbehaves:
    // a ticket price `Manager::value_10a` of 0 divides by zero in
    // `finance_weekly_balance_per_round` 24a4:2b15 (R6003), a day counter past
    // the season overflows or gives day 0 of a month in `calendar_compute_date`
    // 1310:1190, and a crest code without a picture file ends the program in
    // `screen_load_game` (`fatal_error_exit` 2). The port refuses such saves
    // up front (`state.md` §1.1).
    fn check_saved_values(&self) -> Result<(), StateError> {
        let check = |field: &'static str, index: usize, ok: bool| {
            if ok {
                Ok(())
            } else {
                Err(StateError::SavedValue { field, index })
            }
        };
        check(
            "day_counter",
            0,
            (0..=DAY_COUNTER_MAX).contains(&self.day_counter),
        )?;
        for (m, man) in self.active_managers().iter().enumerate() {
            check("active managers.value_10a", m, man.value_10a != 0)?;
            let crest = self.clubs.get(usize::from(man.team)).map_or(0, |c| c.crest);
            check("active managers' club crest", m, crest & 0x7f < CREST_CODES)?;
        }
        Ok(())
    }

    // TODO(re): far globals the save does not store read as zero here; the
    // running program may hold other values in them.
    /// The 64 KiB far data segment (FAR:0000..FFFF) with every saved far
    /// block at its address. For ports that read past the end of a far
    /// table, as the original does.
    pub(crate) fn far_data_image(&self) -> Vec<u8> {
        let mut img = vec![0u8; 0x10000];
        for (desc, bytes) in BLOCKS.iter().zip(&self.to_savegame().blocks) {
            if desc.segment == savegame::Segment::FarData {
                let o = desc.offset as usize;
                img[o..o + bytes.len()].copy_from_slice(bytes);
            }
        }
        img
    }

    /// FAR:304a as an index, or `None` when it is negative. Callers apply
    /// their own upper bound (manager slot, squad group, ...).
    pub fn current_index(&self) -> Option<usize> {
        usize::try_from(self.current_manager).ok()
    }

    /// The manager whose turn it is (FAR:304a), if the index is valid.
    pub fn current(&self) -> Option<&Manager> {
        self.current_index().and_then(|m| self.managers.get(m))
    }

    /// The active human managers (`0..DS:07ab`, clamped to the 4 slots).
    pub fn active_managers(&self) -> &[Manager] {
        &self.managers[..(self.manager_count as usize).min(MANAGER_SLOTS)]
    }

    /// Returns `1 + m` if team `a` is managed by human manager `m`, `0x81 + m`
    /// if team `b` is, else 0. Scans managers `0..DS:07ab` and returns at the
    /// first manager matching either team (team `a` checked first).
    ///
    /// Origin: BMMAIN.EXE 1310:32b1 (manager_index_of_team)
    pub fn manager_index_of_team(&self, a: u8, b: u8) -> u8 {
        // The original loops with a byte counter against the unclamped
        // DS:07ab; only 4 records exist.
        for m in 0..self.manager_count.min(MANAGER_SLOTS as u8) {
            let team = self.managers[m as usize].team;
            if team == a {
                return m + 1;
            }
            if team == b {
                return m + 0x81;
            }
        }
        0
    }

    /// Exchanges the club slots `a` and `b` (both German, below [`GERMAN_CLUBS`]; otherwise nothing
    /// happens): the players' and managers' club ids, with `with_refs` also the cup title
    /// holders, then the club records, league table records and history rows of the two
    /// slots and every record opponent. Team ids are positional, so promotion, relegation,
    /// the yearly league shuffle and the start menu move clubs with this swap.
    ///
    /// Origin: BMMAIN.EXE 1310:0b24 (team_swap_slots)
    pub fn team_swap_slots(&mut self, a: u8, b: u8, with_refs: bool) {
        if usize::from(a) >= GERMAN_CLUBS || usize::from(b) >= GERMAN_CLUBS {
            return;
        }
        let rename = |t: &mut u8| {
            if *t == a {
                *t = b;
            } else if *t == b {
                *t = a;
            }
        };
        for p in &mut self.players[1..PLAYER_COUNT] {
            rename(&mut p.team);
        }
        let managers = self.active_managers().len();
        for m in &mut self.managers[..managers] {
            rename(&mut m.team);
        }
        if with_refs {
            for holder in [
                &mut self.winner_07ac,
                &mut self.winner_07ad,
                &mut self.winner_07ae,
                &mut self.winner_07af,
                &mut self.dfb_cup_runner_up,
            ] {
                swap_team_ref_byte(holder, a, b);
            }
        }
        let (a, b) = (usize::from(a), usize::from(b));
        self.clubs.swap(a, b);
        self.team_tables.swap(a, b);
        let history = &mut self.match_history;
        history.team_series.swap(a, b);
        history.record_scores.swap(a, b);
        history.record_opponents.swap(a, b);
        team_swap_in_table_a4ca(&mut history.record_opponents, a as u8, b as u8);
        for results in &mut history.recent_results[..managers] {
            results.swap(a, b);
        }
    }

    /// Number of occupied slots of `squad`; a method-call shorthand for
    /// [`crate::squad::squad_count_listed`], which owns the port.
    pub fn squad_player_count(&self, squad: usize) -> u8 {
        crate::squad::squad_count_listed(self, squad)
    }

    /// Sorts the first `team_count` (`DS:225e[tier]`) entries of
    /// `league_teams[tier]` into table order and rewrites every club's
    /// [`TeamTable::table_position`].
    ///
    /// `view` selects the 4-byte slots that are summed: 0 = home slot only,
    /// 1 = home + away (full table), 2 = away slot only. `ignore_played`
    /// is DS:224d != 0.
    ///
    /// The original is an exchange sort: for `i < j`, the entries are
    /// swapped when club `j` ranks strictly higher than club `i`. Keys, in
    /// order: more points (unsigned sum), fewer matches played (skipped
    /// when `ignore_played`), higher goal difference (signed 16-bit), more
    /// goals scored. Fully equal clubs keep their relative order only as
    /// far as the exchange sort does.
    ///
    /// Origin: BMMAIN.EXE 3a41:2d33 (FUN_3a41_2d33), sort loop at
    /// 3a41:33d0..3a41:353f and position loop at 3a41:3750..3a41:3784.
    pub fn sort_league_table(
        &mut self,
        tier: usize,
        team_count: usize,
        view: u8,
        ignore_played: bool,
    ) {
        let n = team_count.min(20);
        let tables = &self.team_tables;
        let sum = |t: u8, f: fn(&TeamTable) -> &[u8; 4]| -> u16 {
            let g = f(&tables[t as usize]);
            let mut v = 0u16;
            if view < 2 {
                v = g[0] as u16;
            }
            if view != 0 {
                v = v.wrapping_add(g[1] as u16);
            }
            v
        };
        let diff = |t: u8| -> i16 {
            let r = &tables[t as usize];
            let mut v = 0i16;
            if view < 2 {
                v = r.goals_for[0] as i16 - r.goals_against[0] as i16;
            }
            if view != 0 {
                v = v.wrapping_add(r.goals_for[1] as i16 - r.goals_against[1] as i16);
            }
            v
        };
        // `true` when club `b` (at index j) must move above club `a` (index i).
        let swap = |a: u8, b: u8| -> bool {
            let (pa, pb) = (sum(a, |r| &r.points), sum(b, |r| &r.points));
            if pb != pa {
                return pb > pa;
            }
            let (ga, gb) = if ignore_played {
                (0, 0)
            } else {
                (sum(a, |r| &r.played), sum(b, |r| &r.played))
            };
            if gb != ga {
                return gb < ga;
            }
            let (da, db) = (diff(a), diff(b));
            if da != db {
                return da < db;
            }
            sum(a, |r| &r.goals_for) < sum(b, |r| &r.goals_for)
        };
        let mut row = self.league_teams[tier];
        for i in 0..n.saturating_sub(1) {
            for j in i + 1..n {
                if swap(row[i], row[j]) {
                    row.swap(i, j);
                }
            }
        }
        self.league_teams[tier] = row;
        for (i, &t) in row.iter().take(n).enumerate() {
            self.team_tables[t as usize].table_position = i as u8;
        }
    }

    /// Stores each human manager's current table position in
    /// [`Manager::position_history`] at index `competition_rounds[tier] - 1`
    /// (signed byte). Index -1 (round counter 0) lands on the byte before the
    /// array, [`Manager::value_10a`], exactly as the original's
    /// `+0x234d + idx` store does; indices past 37 are outside any round the
    /// game plays and are ignored here.
    ///
    /// Origin: BMMAIN.EXE 3a41:2d33 (FUN_3a41_2d33), loop at 3a41:3786..3a41:37c7.
    pub fn record_position_history(&mut self, tier: usize) {
        let idx = self.competition_rounds[tier].wrapping_sub(1) as i8;
        let count = (self.manager_count as usize).min(MANAGER_SLOTS);
        for m in 0..count {
            let team = self.managers[m].team as usize;
            let pos = self.team_tables.get(team).map_or(0, |t| t.table_position);
            let rec = &mut self.managers[m];
            match idx {
                -1 => rec.value_10a = pos,
                0..=37 => rec.position_history[idx as usize] = pos,
                _ => {}
            }
        }
    }
}

/// Renames a 1-based club reference (`club + 1`, 0 = none) from `a` to `b` or back. Only
/// called with German clubs, so `club + 1` cannot wrap.
///
/// Origin: BMMAIN.EXE 1310:0ae8 (swap_team_ref_byte)
fn swap_team_ref_byte(p: &mut u8, a: u8, b: u8) {
    if *p == a + 1 {
        *p = b + 1;
    } else if *p == b + 1 {
        *p = a + 1;
    }
}

/// Renames club `a` to `b` and back in every record-opponent entry.
///
/// Origin: BMMAIN.EXE 1310:0f44 (team_swap_in_table_a4ca)
fn team_swap_in_table_a4ca(table: &mut [[u8; 8]; 64], a: u8, b: u8) {
    for x in table.as_flattened_mut() {
        if *x == a {
            *x = b;
        } else if *x == b {
            *x = a;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use bmp_formats::save_files;

    #[test]
    fn record_sizes_match_original() {
        assert_eq!(Credit::LEN, 0x12);
        assert_eq!(Manager::LEN, savegame::MANAGER_LEN);
        assert_eq!(Club::LEN, savegame::CLUB_LEN);
        assert_eq!(Player::LEN, savegame::PLAYER_LEN);
        assert_eq!(SquadPlayer::LEN, 0x34);
        assert_eq!(TeamTable::LEN, 0x36);
        assert_eq!(MatchHistory::LEN, 0x1394);
    }

    #[test]
    fn squad_player_named_offsets() {
        let bytes: Vec<u8> = (0..0x34).collect();
        let p = SquadPlayer::from_bytes(&bytes).unwrap();
        assert_eq!(p.status, 0x09);
        assert_eq!(p.lineup_position, 0x0a);
        assert_eq!(p.contract_years, 0x0b);
        assert_eq!(p.loan, 0x0c);
        assert_eq!(p.suspended, 0x0d);
        assert_eq!(p.tendency, 0x0e);
        assert_eq!(p.player_id, 0x0f);
        assert_eq!(p.exhaustion, 0x13);
        assert_eq!(p.traits, 0x14);
        assert_eq!((p.cards[0], p.goals[0], p.appearances[0]), (0, 3, 6));
        assert_eq!((p.sale_club, p.negotiation), (0x16, 0x18));
        assert_eq!(p.value_terms, [0x1d1c, 0x1f1e, 0x2120]);
        assert_eq!(p.board_col, 0x19);
        assert_eq!(p.board_row, 0x1a);
        assert_eq!(p.market_value, 0x2b2a_2928);
        assert_eq!(p.to_bytes(), bytes);
    }

    #[test]
    fn block_fields_match_save_layout() {
        for (i, (d, &n)) in BLOCKS.iter().zip(&BLOCK_FIELD_LENS).enumerate() {
            assert_eq!(d.len as usize, n, "block {i}");
        }
    }

    #[test]
    fn zeroed_state_round_trips() {
        let s = GameState::zeroed();
        let save = s.to_savegame();
        let bytes = save.to_bytes().unwrap();
        assert_eq!(bytes.len(), savegame::MAN_FILE_LEN);
        let back = GameState::from_savegame(&SaveGame::parse(&bytes).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn news_record_layout() {
        assert_eq!(NewsRecord::LEN, 0x9a);
        let bytes: Vec<u8> = (0..0x9a).collect();
        let r = NewsRecord::from_bytes(&bytes).unwrap();
        assert_eq!(r.n_events, 0x06);
        assert_eq!(r.goal_diff, 0x09);
        assert_eq!(r.hero, 0x0c);
        assert_eq!(r.club_b, 0x0f);
        assert_eq!(r.result, 0x11);
        assert_eq!(r.goal_minute[0], 0x13);
        assert_eq!(r.goal_player[0], 0x27);
        assert_eq!(r.chance_arg4[19], 0x8a);
        assert_eq!(r.n_goals, 0x8b);
        assert_eq!(r.yellow[0], 0x8d);
        assert_eq!(r.red, 0x93);
        assert_eq!(r.n_yellow, 0x94);
        assert_eq!(r.spectators, i32::from_le_bytes([0x96, 0x97, 0x98, 0x99]));
        assert_eq!(r.to_bytes(), bytes);
    }

    #[test]
    fn program_start_has_the_image_values_and_round_trips() {
        let s = GameState::program_start();
        assert_eq!(s.start_year, 0x7ab);
        assert_eq!(s.level, 2);
        assert_eq!(s.winner_07ac, 7);
        assert_eq!(
            s.manager_contracts[3],
            [50000, 6000, 6000, 6000, 6000, 6000, 6000, 0, 5000]
        );
        assert_eq!(s.ds_0620[0], -80);
        assert_eq!(s.week_competitions[93], 0);
        let bytes = s.to_savegame().to_bytes().unwrap();
        let back = GameState::from_savegame(&SaveGame::parse(&bytes).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn far_data_image_places_blocks() {
        let mut s = GameState::zeroed();
        s.players[3].age = 27;
        s.squads[0][0].player_id = 9;
        s.squads[0][0].market_value = 0x0102_0304;
        s.match_history.recent_results[0][0][0][0] = 0xab;
        let img = s.far_data_image();
        assert_eq!(img.len(), 0x10000);
        assert_eq!(img[0x57dd + 0x25 * 3 + 0x1a], 27);
        assert_eq!(&img[0x774a..0x774a + 0x34], s.squads[0][0].to_bytes());
        assert_eq!(img[0x9336], 0xab);
    }

    #[test]
    fn manager_index_of_team_matches_original() {
        let mut s = GameState::zeroed();
        s.manager_count = 2;
        s.managers[0].team = 7;
        s.managers[1].team = 9;
        s.managers[2].team = 11; // beyond DS:07ab, never matched
        assert_eq!(s.manager_index_of_team(7, 9), 1);
        assert_eq!(s.manager_index_of_team(9, 7), 0x81);
        assert_eq!(s.manager_index_of_team(1, 9), 0x82);
        assert_eq!(s.manager_index_of_team(11, 11), 0);
    }

    /// A state whose per-team rows of clubs 2 and 5 carry distinct markers.
    fn swap_state() -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.players[0].team = 2;
        s.players[1].team = 2;
        s.players[150].team = 5;
        s.managers[0].team = 5;
        s.managers[1].team = 2;
        s.winner_07ac = 3;
        s.dfb_cup_runner_up = 6;
        s.winner_07ad = 9;
        for (t, mark) in [(2, 0x22), (5, 0x55)] {
            s.clubs[t].name[0] = mark;
            s.team_tables[t].points[0] = mark;
            let h = &mut s.match_history;
            h.team_series[t][0][0] = mark;
            h.record_scores[t][0] = mark;
            h.recent_results[0][t][0][0] = mark;
            h.recent_results[1][t][0][0] = mark;
        }
        s.match_history.record_opponents[9][..3].copy_from_slice(&[2, 5, 7]);
        s.match_history.record_opponents[2][0] = 0x22;
        s
    }

    #[test]
    fn team_swap_slots_moves_every_reference() {
        let before = swap_state();
        let mut s = before.clone();
        s.team_swap_slots(2, 5, true);
        assert_eq!(s.players[0].team, 2, "player 0 is not a real player");
        assert_eq!((s.players[1].team, s.players[150].team), (5, 2));
        assert_eq!((s.managers[0].team, s.managers[1].team), (2, 2));
        assert_eq!((s.winner_07ac, s.dfb_cup_runner_up), (6, 3));
        assert_eq!(s.winner_07ad, 9);
        assert_eq!(s.clubs[2].name[0], 0x55);
        assert_eq!(s.clubs[5].name[0], 0x22);
        assert_eq!(s.team_tables[2].points[0], 0x55);
        let h = &s.match_history;
        assert_eq!(h.team_series[5][0][0], 0x22);
        assert_eq!(h.record_scores[5][0], 0x22);
        assert_eq!(h.recent_results[0][5][0][0], 0x22);
        assert_eq!(h.recent_results[1][5][0][0], 0x55, "manager 1 is inactive");
        assert_eq!(h.record_opponents[9][..3], [5, 2, 7]);
        // The rows move first, then every entry is renamed: row 5 now holds club 2's old row.
        assert_eq!(h.record_opponents[5][0], 0x22);

        let mut s = before.clone();
        s.team_swap_slots(2, 5, false);
        assert_eq!((s.winner_07ac, s.dfb_cup_runner_up), (3, 6));
        assert_eq!(s.managers[0].team, 2);

        for (a, b) in [(64, 2), (2, 64)] {
            let mut s = before.clone();
            s.team_swap_slots(a, b, true);
            assert_eq!(s, before);
        }
    }

    #[test]
    fn real_saves_team_swap_twice_restores_the_state() {
        for (path, before) in crate::test_saves::save_states() {
            let pairs = [
                (before.managers[0].team, 0),
                (before.league_teams[0][0], before.league_teams[1][0]),
                (3, 3),
            ];
            for (a, b) in pairs {
                let mut s = before.clone();
                s.team_swap_slots(a, b, true);
                s.team_swap_slots(a, b, true);
                assert_eq!(s, before, "{} swap {a}/{b}", path.display());
            }
        }
    }

    #[test]
    fn named_record_bytes_keep_their_offsets() {
        let mut bytes = vec![0u8; SquadPlayer::LEN];
        bytes[0x03] = 7;
        bytes[0x06] = 21;
        bytes[0x13] = 130;
        bytes[0x19] = 0xff;
        bytes[0x1a] = 6;
        let p = SquadPlayer::from_bytes(&bytes).unwrap();
        assert_eq!(p.goals, [7, 0, 0]);
        assert_eq!(p.appearances, [21, 0, 0]);
        assert_eq!(p.exhaustion, 130);
        assert_eq!(p.board_col, -1);
        assert_eq!(p.board_row, 6);
        assert_eq!(p.to_bytes(), bytes);

        let mut bytes = vec![0u8; Manager::LEN];
        bytes[0x13d] = 100;
        let m = Manager::from_bytes(&bytes).unwrap();
        assert_eq!(m.aggression, 100);
        assert_eq!(m.to_bytes(), bytes);
    }

    #[test]
    fn real_saves_round_trip_byte_exact() {
        for path in save_files() {
            let data = std::fs::read(&path).unwrap();
            let save = SaveGame::parse(&data).unwrap_or_else(|e| panic!("{path:?}: {e}"));
            let state = GameState::from_savegame(&save).unwrap();
            let back = state.to_savegame();
            assert_eq!(back, save, "{path:?}");
            assert_eq!(back.to_bytes().unwrap(), data, "{path:?}");
        }
    }

    /// The bytes of `path` with the position code of player `pick(state)`
    /// set to `code`, and that player's index.
    fn save_with_position_code(
        path: &std::path::Path,
        pick: fn(&GameState) -> usize,
        code: u8,
    ) -> (Vec<u8>, usize) {
        let data = std::fs::read(path).unwrap();
        let mut state = GameState::from_savegame(&SaveGame::parse(&data).unwrap()).unwrap();
        let player = pick(&state);
        state.players[player].position_value = code;
        (state.to_savegame().to_bytes().unwrap(), player)
    }

    fn first_squad_player(s: &GameState) -> usize {
        let rec = s.squads[0].iter().find(|r| r.player_id != 0).unwrap();
        usize::from(rec.player_id)
    }

    /// A player no squad record holds: the market can hand it to a squad.
    fn first_unowned_player(s: &GameState) -> usize {
        (1..s.players.len())
            .find(|&i| {
                s.players[i].owner_manager == NO_OWNER
                    && !s
                        .squads
                        .iter()
                        .flatten()
                        .any(|r| usize::from(r.player_id) == i)
            })
            .unwrap()
    }

    #[test]
    fn position_code_limit_is_four_bands_of_25() {
        assert_eq!(POSITION_CODE_LIMIT, 100);
        let mut player = Player::zeroed();
        for (code, band) in [(0, 0), (24, 0), (25, 1), (74, 2), (75, 3), (99, 3)] {
            player.position_value = code;
            assert_eq!(player.position_band(), band);
        }
    }

    #[test]
    fn real_save_with_position_code_100_is_rejected() {
        let Some(path) = save_files().into_iter().next() else {
            return;
        };
        for pick in [first_squad_player, first_unowned_player] {
            let (bytes, player) = save_with_position_code(&path, pick, POSITION_CODE_LIMIT);
            let save = SaveGame::parse(&bytes).unwrap();
            assert_eq!(
                GameState::from_savegame(&save),
                Err(StateError::PositionCode { player })
            );
            let (bytes, _) = save_with_position_code(&path, pick, POSITION_CODE_LIMIT - 1);
            let save = SaveGame::parse(&bytes).unwrap();
            assert!(GameState::from_savegame(&save).is_ok());
        }
    }

    #[test]
    fn position_code_of_player_0_is_not_checked() {
        let mut s = GameState::zeroed();
        s.players[0].position_value = 0xff;
        let save = SaveGame::parse(&s.to_savegame().to_bytes().unwrap()).unwrap();
        assert!(GameState::from_savegame(&save).is_ok());
    }

    /// [`GameState::from_savegame`] of `s` written out and parsed again.
    fn reload(s: &GameState) -> Result<GameState, StateError> {
        GameState::from_savegame(&SaveGame::parse(&s.to_savegame().to_bytes().unwrap()).unwrap())
    }

    #[test]
    fn squad_player_id_past_the_player_table_is_rejected() {
        for (squad, slot, player_id) in [(0, 0, 151), (TRANSFER_LIST, SQUAD_SIZE - 1, 0xff)] {
            let mut s = GameState::zeroed();
            s.squads[squad][slot].player_id = player_id;
            assert_eq!(
                reload(&s),
                Err(StateError::SquadPlayerId {
                    squad,
                    slot,
                    player_id
                })
            );
        }
        let mut s = GameState::zeroed();
        s.squads[1][3].player_id = (PLAYER_COUNT - 1) as u8;
        s.squads[1][4].player_id = 0;
        assert!(reload(&s).is_ok());
    }

    /// One change per guarded field: the last value the game writes (or its
    /// sentinel) loads, the next one is rejected naming the field and entry.
    #[test]
    fn saved_index_past_its_table_is_rejected() {
        type Patch = fn(&mut GameState, bool);
        let cases: [(&str, usize, Patch); 29] = [
            ("manager_count", 0, |s, bad| {
                s.manager_count = 4 + u8::from(bad)
            }),
            ("current_manager", 0, |s, bad| {
                s.current_manager = if bad { -1 } else { 3 }
            }),
            ("current_manager", 0, |s, bad| {
                s.current_manager = 3 + i8::from(bad)
            }),
            ("season_week", 0, |s, bad| {
                s.season_week = 94 + u8::from(bad)
            }),
            ("logged_rounds", 0, |s, bad| {
                s.logged_rounds = 0x7fff + u16::from(bad)
            }),
            ("cup holders + 1", 0, |s, bad| {
                s.winner_07ac = 200 + u8::from(bad)
            }),
            ("cup holders + 1", 1, |s, bad| {
                s.winner_07ad = 200 + u8::from(bad)
            }),
            ("cup holders + 1", 2, |s, bad| {
                s.winner_07ae = 200 + u8::from(bad)
            }),
            ("cup holders + 1", 3, |s, bad| {
                s.winner_07af = 200 + u8::from(bad)
            }),
            ("cup holders + 1", 4, |s, bad| {
                s.dfb_cup_runner_up = 200 + u8::from(bad)
            }),
            ("event_state cup round", 2, |s, bad| {
                s.event_state[2] = 5 + u8::from(bad)
            }),
            ("league_teams", 39, |s, bad| {
                s.league_teams[1][19] = 63 + u8::from(bad)
            }),
            ("cup_pairings", 127, |s, bad| {
                s.cup_pairings[63][1] = 199 + u8::from(bad)
            }),
            ("rescheduled_matches", 5, |s, bad| {
                s.rescheduled_matches[5][RESCHEDULED_TIER] = 2 + u8::from(bad)
            }),
            ("rescheduled_matches", 6, |s, bad| {
                s.rescheduled_matches[6][RESCHEDULED_PAIR] = 9 + u8::from(bad)
            }),
            ("match_history.record_opponents", 8, |s, bad| {
                s.match_history.record_opponents[1][0] = 199 + u8::from(bad)
            }),
            ("managers.team", 3, |s, bad| {
                s.managers[3].team = if bad { 200 } else { NO_TEAM }
            }),
            ("active managers.team", 0, |s, bad| {
                s.manager_count = 1;
                s.managers[0].team = 63 + u8::from(bad);
            }),
            ("active managers.league", 0, |s, bad| {
                s.manager_count = 1;
                s.managers[0].league = 2 + u8::from(bad);
            }),
            ("active managers.values_13a[0]", 0, |s, bad| {
                s.manager_count = 1;
                s.managers[0].values_13a[0] = 19 + u8::from(bad);
            }),
            ("active managers.credits.due_month", 0, |s, bad| {
                s.manager_count = 1;
                s.managers[0].credits[4][2].principal = 1;
                s.managers[0].credits[4][2].due_month = 11 + u8::from(bad);
            }),
            ("managers.stadium level", 2, |s, bad| {
                s.managers[2].stadium[6] = [4, 2 + i32::from(bad)]
            }),
            ("managers.stadium level", 1, |s, bad| {
                s.managers[1].stadium[3] = [if bad { -1 } else { 3 }, 0]
            }),
            ("sponsor logos", 4 + 6 * 2 + 5, |s, bad| {
                s.far_4b9a[2 * (6 * 2 + 5) + 1] = 9 + u8::from(bad)
            }),
            ("players.owner_manager", 150, |s, bad| {
                s.players[150].owner_manager = NO_OWNER + u8::from(bad)
            }),
            ("players.team", 1, |s, bad| {
                s.players[1].team = if bad { 200 } else { 0xff }
            }),
            ("squads.board_row", SQUAD_SIZE + 2, |s, bad| {
                s.squads[1][2].player_id = 1;
                s.squads[1][2].board_row = if bad { 8 } else { BOARD_UNPLACED };
            }),
            ("squads.board_row", SQUAD_SIZE + 3, |s, bad| {
                s.squads[1][3].player_id = 1;
                s.squads[1][3].board_row = BOARD_ROW_MAX + i8::from(bad);
            }),
            ("squads.sale_club", 0, |s, bad| {
                s.squads[0][0].player_id = 1;
                s.squads[0][0].sale_club = 199 + u8::from(bad);
            }),
        ];
        // One (empty) message group per manager keeps the string table writable.
        let reload = |s: &mut GameState| {
            s.messages = vec![Vec::new(); usize::from(s.manager_count)];
            reload(s)
        };
        for (field, index, patch) in cases {
            let mut s = GameState::zeroed();
            // A ticket price the game writes (`check_saved_values`).
            s.managers.iter_mut().for_each(|m| m.value_10a = 10);
            patch(&mut s, false);
            assert!(reload(&mut s).is_ok(), "{field} {index}");
            patch(&mut s, true);
            assert_eq!(
                reload(&mut s),
                Err(StateError::SavedIndex { field, index }),
                "{field} {index}"
            );
        }
    }

    /// One change per guarded value: the extremes the game writes load, the
    /// values past them are rejected naming the field and entry.
    #[test]
    fn saved_value_outside_game_range_is_rejected() {
        type Patch = fn(&mut GameState);
        let ok: [Patch; 6] = [
            |s| s.day_counter = 0,
            |s| s.day_counter = DAY_COUNTER_MAX,
            |s| s.managers[3].value_10a = 1,
            |s| s.managers[3].value_10a = 0xff,
            |s| s.clubs[63].crest = CREST_CODES - 1,
            // Bit 7 is a runtime flag, not part of the code.
            |s| s.clubs[63].crest = 0x80 | (CREST_CODES - 1),
        ];
        let bad: [(&str, usize, Patch); 8] = [
            ("day_counter", 0, |s| s.day_counter = -1),
            // calendar_compute_date: day 0 of a month.
            ("day_counter", 0, |s| s.day_counter = -0xd2),
            ("day_counter", 0, |s| s.day_counter = DAY_COUNTER_MAX + 1),
            // calendar_compute_date: `day += 0xd2` overflows.
            ("day_counter", 0, |s| s.day_counter = i32::MAX - 0x10),
            ("day_counter", 0, |s| s.day_counter = i32::MAX),
            ("active managers.value_10a", 3, |s| {
                s.managers[3].value_10a = 0
            }),
            ("active managers' club crest", 3, |s| {
                s.clubs[63].crest = CREST_CODES
            }),
            ("active managers' club crest", 3, |s| {
                s.clubs[63].crest = 0xff
            }),
        ];
        let base = || {
            let mut s = GameState::zeroed();
            s.manager_count = 4;
            s.messages = vec![Vec::new(); 4];
            for (m, man) in s.managers.iter_mut().enumerate() {
                man.team = 60 + m as u8;
                man.value_10a = 10;
            }
            s
        };
        assert!(reload(&base()).is_ok());
        for (i, patch) in ok.iter().enumerate() {
            let mut s = base();
            patch(&mut s);
            assert!(reload(&s).is_ok(), "case {i}");
        }
        for (field, index, patch) in bad {
            let mut s = base();
            patch(&mut s);
            assert_eq!(
                reload(&s),
                Err(StateError::SavedValue { field, index }),
                "{field} {index}"
            );
        }
        // Only the active managers count: an empty slot keeps 0.
        let mut s = base();
        s.manager_count = 3;
        s.messages.truncate(3);
        s.managers[3].value_10a = 0;
        s.clubs[63].crest = 0x7f;
        assert!(reload(&s).is_ok());
    }

    /// Number of used entries of `league_teams[tier]`: the longest prefix
    /// whose table positions are exactly 0..n.
    fn tier_len(s: &GameState, tier: usize) -> usize {
        s.league_teams[tier]
            .iter()
            .enumerate()
            .take_while(|&(i, &t)| {
                (t as usize) < s.team_tables.len()
                    && s.team_tables[t as usize].table_position as usize == i
            })
            .count()
    }

    #[test]
    fn real_saves_table_semantics() {
        for path in save_files() {
            let data = std::fs::read(&path).unwrap();
            let s = GameState::from_savegame(&SaveGame::parse(&data).unwrap()).unwrap();
            for tier in 0..3 {
                let n = tier_len(&s, tier);
                assert!(n >= 18, "{path:?} tier {tier}: {n}");
                // Re-sorting the saved (already sorted) full table is a no-op.
                let mut t = s.clone();
                t.sort_league_table(tier, n, 1, false);
                assert_eq!(
                    t.league_teams[tier], s.league_teams[tier],
                    "{path:?} {tier}"
                );
                // Each club: points == 2 * wins + draws (2-point rule).
                for &c in &s.league_teams[tier][..n] {
                    let r = &s.team_tables[c as usize];
                    for k in 0..2 {
                        let draws = r.played[k] - r.wins[k] - r.losses[k];
                        assert_eq!(r.points[k], 2 * r.wins[k] + draws, "{path:?} club {c}");
                    }
                }
            }
            // Lineup positions 1..=11 are used exactly once per human squad.
            for sq in &s.squads[..s.active_managers().len()] {
                for p in 1..=11u8 {
                    let k = sq.iter().filter(|x| x.lineup_position == p).count();
                    assert!(k <= 1, "{path:?} lineup {p}");
                }
            }
            // The history entry of the last updated round equals the current
            // position of the manager's club. A save taken after the round
            // counter advanced but before that round's table update
            // (AUTOSAVE.MAN) has it one entry earlier.
            for m in s.active_managers() {
                let tier = m.league as usize;
                let round = s.competition_rounds.get(tier).copied().unwrap_or(0) as usize;
                if (2..=38).contains(&round) && (m.team as usize) < 64 {
                    let pos = s.team_tables[m.team as usize].table_position;
                    let h = &m.position_history;
                    assert!(h[round - 1] == pos || h[round - 2] == pos, "{path:?}");
                }
            }
        }
    }

    #[test]
    fn sort_league_table_keys() {
        let mut s = GameState::zeroed();
        s.league_teams[0][..4].copy_from_slice(&[0, 1, 2, 3]);
        let set = |s: &mut GameState, t: usize, pts: u8, pl: u8, gf: u8, ga: u8| {
            let r = &mut s.team_tables[t];
            r.points = [pts, 0, 0, 0];
            r.played = [pl, 0, 0, 0];
            r.goals_for = [gf, 0, 0, 0];
            r.goals_against = [ga, 0, 0, 0];
        };
        set(&mut s, 0, 10, 8, 5, 9); // same points, more games
        set(&mut s, 1, 10, 7, 5, 9); // fewer games ranks higher
        set(&mut s, 2, 10, 7, 6, 9); // better goal difference
        set(&mut s, 3, 12, 9, 0, 9); // most points
        let mut a = s.clone();
        a.sort_league_table(0, 4, 1, false);
        assert_eq!(&a.league_teams[0][..4], &[3, 2, 1, 0]);
        assert_eq!(a.team_tables[0].table_position, 3);
        // DS:224d set: played ignored, so 0 and 1 compare by goals only.
        set(&mut s, 0, 10, 8, 6, 9);
        let mut b = s.clone();
        b.sort_league_table(0, 4, 1, true);
        assert_eq!(&b.league_teams[0][..4], &[3, 2, 0, 1]);
        // Away view ignores the home slots entirely.
        let mut c = s.clone();
        c.sort_league_table(0, 4, 2, false);
        assert_eq!(&c.league_teams[0][..4], &[0, 1, 2, 3]);
    }

    #[test]
    fn autosave_semantics() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let Ok(data) = std::fs::read(dir.join("AUTOSAVE.MAN")) else {
            return;
        };
        let s = GameState::from_savegame(&SaveGame::parse(&data).unwrap()).unwrap();
        assert_eq!(s.managers[0].name(), b"OLIVER");
        let club = &s.clubs[s.managers[0].team as usize];
        assert_eq!(club.name(), b"BAYER 05 UERDINGEN");
        assert_eq!(s.clubs[0].name(), b"BORUSSIA DORTMUND");
        assert_eq!(s.players[1].name(), b"STEIN");
        assert_eq!(s.manager_index_of_team(s.managers[0].team, 0xfe), 1);
        for m in s.active_managers() {
            assert!(m.slot >= 1 && m.slot <= 4);
        }
        // Players owned by manager 0 carry the manager's club in +0x24.
        let team = s.managers[0].team;
        let owned: Vec<_> = s.players[1..]
            .iter()
            .filter(|p| p.owner_manager == 0)
            .collect();
        assert!(!owned.is_empty());
        assert!(owned.iter().all(|p| p.team == team));
        // Form strings use S/U/N (upper or lower case) and the placeholder `@`.
        for t in &s.team_tables {
            for row in &t.form {
                assert!(c_str(row).iter().all(|c| b"SUNsun@".contains(c)), "{row:?}");
            }
        }
    }
}
