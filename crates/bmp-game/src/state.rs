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
pub const CLUB_COUNT: usize = 200;
/// Number of German clubs (league clubs with table/history entries).
pub const GERMAN_CLUBS: usize = 64;
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
        /// +0x01f..0x03d not yet traced.
        pub unknown_01f: [u8; 31],
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
        /// +0x132 (FAR:2374) 5 bytes initialized to 0x1e (30);
        /// `startup_clear_state` re-sets `[2..5]` to 0x1e.
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
        /// +0x13a..0x13c per-manager state bytes (training, AI).
        pub values_13a: [u8; 3],
        /// +0x13d (FAR:237f) match aggression 0..=40, or 100 when fewer than
        /// 8 players start (`squad_recalc_team_strength` 1f9d:0002).
        pub aggression: u8,
        /// +0x13e per-manager state byte.
        pub value_13e: u8,
        /// +0x13f (FAR:2381) training intensity 0..=0x22 (`screen_training`);
        /// drives a monthly cost and [`Manager::fatigue_sum`].
        pub training_intensity: u8,
        /// +0x140 per-manager state byte.
        pub value_140: u8,
        /// +0x141 (FAR:2383) 5 bytes copied from DS:0286 on a new game
        /// (`screen_training`).
        pub training_141: [u8; 5],
        /// +0x146 (FAR:2388) 4 bytes copied from DS:028c on a new game.
        pub training_146: [u8; 4],
        /// +0x14a..0x15d not yet traced.
        pub unknown_14a: [u8; 20],
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
        /// +0x1a4 (FAR:23e6) season total `long`s updated by
        /// `season_main_loop` (observed values are DM-sized, i.e. income /
        /// expense totals).
        pub season_stats: [i32; 8],
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

impl Player {
    /// Player name without the NUL padding.
    pub fn name(&self) -> &[u8] {
        c_str(&self.name)
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
        /// +0x15 not yet traced.
        pub unknown_15: u8,
        /// +0x16 (FAR:7760) club the player moves to when sold; a value
        /// above 0x3f fixes an offer at 130 % (`player_value_calc`).
        pub sale_club: u8,
        /// +0x17 (FAR:7761) copied on a manager-to-manager sale.
        pub unknown_17: u8,
        /// +0x18 (FAR:7762) negotiation state: 0 willing, 1..=99 cool-down,
        /// 100..=127 insists on `value - 100` seasons, bit 7 retires when the
        /// contract ends.
        pub negotiation: u8,
        /// +0x19 (FAR:7763) tactics-board column 0..=7, [`BOARD_UNPLACED`] =
        /// not placed.
        pub board_col: i8,
        /// +0x1a (FAR:7764) tactics-board row 0..=7 (0 = attack, 7 = goal).
        pub board_row: i8,
        /// +0x1b not yet traced.
        pub unknown_1b: u8,
        /// +0x1c, +0x1e, +0x20 (FAR:7766) value terms weighted 50/20/30 by
        /// `player_value_calc` 3277:25de.
        pub value_terms: [u16; 3],
        /// +0x22..0x27 not yet traced.
        pub unknown_22: [u8; 6],
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
    /// #8 DS:07ac competition winner + 1 (DFB-Pokal, prestige bonus 20).
    pub winner_07ac: u8,
    /// #9 DS:07ad competition winner + 1 (prestige bonus 50).
    pub winner_07ad: u8,
    /// #10 DS:07ae competition winner + 1 (prestige bonus 40).
    pub winner_07ae: u8,
    /// #11 DS:07af competition winner + 1 (prestige bonus 30).
    pub winner_07af: u8,
    /// #12 DS:07b0 competition state byte (`competition_reset` 27c2:0ef2).
    pub competition_07b0: u8,
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
    /// #25 FAR:56fe team id list (`team_is_in_list` 27c2:11e7).
    pub cup_team_list: [u8; 10],
    /// #26 FAR:57aa competition slots, 0x80 = empty (`competition_reset`).
    pub competition_slots: [u8; 18],
    /// #27 FAR:2e6e relegation pairing (copied to FAR:21e2).
    pub relegation_pair: [u8; 2],
    /// #28 FAR:76ca cup pairings `[64][2]` (`cup_play_round` 27c2:16dc).
    pub cup_pairings: [[u8; 2]; 64],
    /// #29 FAR:21e2 relegation match table.
    pub relegation_table: [u8; 96],
    /// #30 FAR:0008 event state bytes (`random_weekly_events`, 3e3a:0fdf).
    pub event_state: [u8; 8],
    /// #31 FAR:1d18 relegation/cup state.
    pub far_1d18: [u8; 3],
    /// #32 FAR:535a club ids of each league tier `[tier][20]`, ordered.
    pub league_teams: [[u8; 20]; 3],
    /// #33 FAR:4c7e cup results `[64][2]`.
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
    /// #60 FAR:3060 byte read by `season_main_loop`.
    pub far_3060: u8,

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
    /// DS:5256 set at the end of every fourth month (months 0, 4, 8).
    pub quarter_flag: u8,
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
}

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
            12 competition_07b0: u8,
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
            25 cup_team_list: [u8; 10],
            26 competition_slots: [u8; 18],
            27 relegation_pair: [u8; 2],
            28 cup_pairings: [[u8; 2]; 64],
            29 relegation_table: [u8; 96],
            30 event_state: [u8; 8],
            31 far_1d18: [u8; 3],
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
            60 far_3060: u8,
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
        /// save lacks get their program-start values.
        ///
        /// Origin: BMMAIN.EXE 422b:120c (screen_load_game; block reads mirror
        /// savegame_write_man 422b:07fe)
        pub fn from_savegame(save: &SaveGame) -> Result<GameState, StateError> {
            if save.blocks.len() != BLOCKS.len() {
                return Err(StateError::BlockCount);
            }
            Ok(GameState {
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
                quarter_flag: 0,
            })
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
            }
        }

        /// A state with every saved global zeroed, as after program start
        /// (far data and BSS are zero-initialized), an empty message table and
        /// the runtime globals at their data-segment image values.
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
                quarter_flag: 0,
            }
        }
    };
}

impl GameState {
    for_each_block!(decode_blocks);

    /// The manager whose turn it is (FAR:304a), if the index is valid.
    pub fn current(&self) -> Option<&Manager> {
        usize::try_from(self.current_manager)
            .ok()
            .and_then(|m| self.managers.get(m))
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

    /// Number of occupied slots (`player_id != 0`) among the first 24 slots
    /// of `squad` (the first 12 for the transfer pool, squad 4). The slots
    /// need not be contiguous; slot 24 is never examined.
    ///
    /// Origin: BMMAIN.EXE 4091:1109 (squad_count_by_flag, mode 0)
    pub fn squad_player_count(&self, squad: usize) -> u8 {
        let n = if squad == TRANSFER_LIST {
            TRANSFER_LIST_SCAN
        } else {
            SQUAD_SCAN
        };
        self.squads[squad][..n]
            .iter()
            .filter(|p| p.player_id != 0)
            .count() as u8
    }

    /// Sorts the first `team_count` (DS:225e[tier]) entries of
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The `.MAN` saves in `$BMP_DATA` (and its `BMP` subdirectory).
    pub(crate) fn save_files() -> Vec<PathBuf> {
        let Some(dir) = bmp_formats::data_dir() else {
            return Vec::new();
        };
        let mut v = Vec::new();
        for d in [dir.clone(), dir.join("BMP")] {
            if let Ok(rd) = std::fs::read_dir(&d) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("man")) {
                        v.push(p);
                    }
                }
            }
        }
        v.sort();
        v
    }

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
