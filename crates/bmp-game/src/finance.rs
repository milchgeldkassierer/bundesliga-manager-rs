//! Finances: loans, the daily money processing and the monthly income and
//! expenses of a human manager.
//!
//! Every function works on the manager whose turn it is
//! ([`GameState::current_manager`], FAR:304a), like the original. Dialogs
//! and messages the original shows in the middle of the processing are
//! returned as [`FinanceEvent`]s in the order they appear; the messages are
//! posted to the inbox where the original posts them, as that draws a random
//! number.
//!
//! The screens that call into this module call `copy_protection_penalty`
//! 16c7:09f9, which draws random numbers only while the copy protection has
//! not been passed (DS:05ac == 0). The port assumes it was passed, so that
//! call never touches the RNG.
//!
//! Full documentation with Ghidra addresses: `docs/re/logic/finance.md`.

use crate::league::POINTS_PER_WIN;
use crate::news::{MessageArg, NO_FREE_SLOT, message_delete, message_post, team_slot_free_index};
use crate::rng::{NumberFormat, Rng, clamp_int, ldiv, lmul, lrem, lshr};
use crate::runtime::Runtime;
use crate::state::{Credit, GameState, Manager, TRANSFER_LIST};

/// Days per month, DS:07b8 (no leap years).
pub const DAYS_IN_MONTH: [i8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// Lender index of the bank in [`Manager::credits`]; 0..=3 are human managers.
pub const BANK: usize = 4;

/// Highest fan count that still grows at month end (exclusive).
const FAN_GROWTH_LIMIT: u16 = 95;

/// Something the original shows to the player while processing a day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinanceEvent {
    /// Construction of stadium item `item` (0..7, see [`Manager::stadium`])
    /// finished (`stadium_construction_daily`).
    ConstructionFinished {
        /// Stadium item index.
        item: usize,
        /// The message the original shows at once and deletes again (in the
        /// winter break and after the season's last day); `None`: it went to
        /// the inbox through [`message_queue_add`]. The second value is the
        /// manager's message count `DS:0618[FAR:5358]` while it is shown (the
        /// message itself included), which the viewer's "down" button reads.
        shown: Option<(Vec<u8>, u8)>,
    },
    /// "Ihr Kredit von `<lender>` wurde heute fällig." The loan was repaid.
    LoanDue {
        /// Lender index: a human manager, or [`BANK`].
        lender: usize,
    },
}

/// Which field of the loans [`credit_sum`] adds up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreditField {
    /// `which == 0`: the principal still owed.
    Principal,
    /// `which != 0`: the interest paid per payment.
    Interest,
}

fn current_index(state: &GameState) -> usize {
    state
        .current_index()
        .expect("FAR:304a holds a manager index")
}

/// Days of `month` (FAR:2e8e; the original indexes the table with its low word).
fn days_in_month(month: i32) -> i8 {
    DAYS_IN_MONTH[month as u16 as usize]
}

/// Sums [`CreditField`] over the used loan slots of `manager` taken from
/// `lender`, or from every lender when `lender` is `None` (the original's
/// lender 99).
///
/// Origin: BMMAIN.EXE 212a:0f8d (credit_sum)
pub fn credit_sum(manager: &Manager, field: CreditField, lender: Option<usize>) -> i32 {
    manager
        .credits
        .iter()
        .enumerate()
        .filter(|&(c, _)| lender.is_none_or(|l| l == c))
        .flat_map(|(_, slots)| slots)
        .filter(|credit| credit.principal != 0)
        .fold(0i32, |sum, credit| {
            sum.wrapping_add(match field {
                CreditField::Principal => credit.principal,
                CreditField::Interest => credit.interest_payment,
            })
        })
}

/// Takes a loan of `amount` at `rate` percent from `lender` for `months`
/// months, dated today. Returns `false` when all three slots of that lender
/// are used (the original then shows "Schon 3 Kredite aufgenommen."). The
/// caller moves the money.
///
/// The payment day is today, or the last day of the due month when that is
/// shorter.
///
/// # Panics
/// When the due month `month + months` is negative (the original reads
/// outside the days-per-month table).
///
/// Origin: BMMAIN.EXE 212a:1039 (credit_take)
pub fn credit_take(
    state: &mut GameState,
    amount: i32,
    rate: i8,
    lender: usize,
    months: i8,
) -> bool {
    let m = current_index(state);
    let (day, month, year) = (state.day_of_month, state.month, state.year);
    let Some(credit) = state.managers[m].credits[lender]
        .iter_mut()
        .find(|c| c.principal == 0)
    else {
        return false;
    };
    let mut due_year = year as i16;
    let mut due_month = (month as i8).wrapping_add(months);
    while due_month > 11 {
        due_year = due_year.wrapping_add(1);
        due_month -= 12;
    }
    let due_days = DAYS_IN_MONTH[due_month as u8 as usize];
    *credit = Credit {
        principal: amount,
        interest_payment: ldiv(lmul(i32::from(rate), amount), 100),
        day_taken: day as u8,
        payment_day: if i32::from(due_days) >= day {
            day as u8
        } else {
            due_days as u8
        },
        month_taken: month as u8,
        due_month: due_month as u8,
        rate: rate as u8,
        unknown_0d: credit.unknown_0d,
        year_taken: year as i16,
        due_year,
    };
    true
}

/// The six bank interest rates (3, 6, .., 18 months) derived from the base
/// rate: start at the base and step toward the far end of 2..7 with a
/// shrinking step, e.g. base 7 gives `7 5 3 2 2 2`, base 3 `3 5 6 7 7 7`.
fn interest_rate_table(base: i16) -> [u8; 6] {
    let mut rates = [0u8; 6];
    let mut v = base;
    let mut step = 5 - base;
    for rate in &mut rates {
        *rate = v as u8;
        v = v.wrapping_add(step);
        if step > 1 {
            step -= 1;
        }
        clamp_int(&mut v, 2, 7);
    }
    rates
}

/// Random walk of the base interest rate (DS:0630, clamped 2..7) and
/// recomputation of the bank rates from it. At levels above 2 the rate only
/// rises (0..+3), otherwise it moves by -1..+1.
///
/// Origin: BMMAIN.EXE 212a:000a (interest_rates_update)
pub fn interest_rates_update(state: &mut GameState, rng: &mut Rng) {
    let hard = i16::from(state.level as i8 > 2);
    let r = rng.random_range(0, hard + 2);
    state.base_interest_rate = state.base_interest_rate.wrapping_add(r + hard - 1);
    clamp_int(&mut state.base_interest_rate, 2, 7);
    state.interest_rates = interest_rate_table(state.base_interest_rate);
}

/// DS:28d8: the first message line of a finished stadium item.
const CONSTRUCTION_SUBJECT: [&[u8]; 7] = [
    b"Der Ausbau der Sitzpl{tze",
    b"Der Ausbau der Stehpl{tze",
    b"Die ]berdachung des Stadions",
    b"Die Errichtung der neuen",
    b"Die Errichtung der neuen",
    b"Die Stadionrenovierung",
    b"Die Verbesserung des",
];
/// The line that ends a construction message.
const CONSTRUCTION_DONE: &[u8] = b"ist abgeschlossen.";
/// Message template of [`message_queue_add`]'s kind 0 (`3 * kind + 2`: three text lines).
const QUEUED_TEMPLATE: u8 = 2;
/// Message template of a message shown at once (kind 1).
const SHOWN_TEMPLATE: usize = 5;
/// [`GameState::team_slots`] countdown of a queued message.
const QUEUED_DAYS: u8 = 3;

/// Posts a three-line message (template `3 * kind + 2`) to the inbox of the
/// current manager and keeps it in a free slot of row `row` of
/// [`GameState::team_slots`] with a countdown of 3; without a free slot the
/// message is deleted again.
///
/// Origin: BMMAIN.EXE 1000:239e (message_queue_add)
pub fn message_queue_add(
    state: &mut GameState,
    rng: &mut Rng,
    fmt: &mut NumberFormat,
    lines: [&[u8]; 3],
    row: u8,
    kind: u8,
) {
    state.saved_manager = state.current_manager;
    let slot = team_slot_free_index(state, row);
    let args = lines.map(MessageArg::Text);
    let tpl = usize::from(kind.wrapping_mul(3).wrapping_add(QUEUED_TEMPLATE));
    let msg = message_post(state, rng, fmt, kind != 0, tpl, &args);
    if slot == NO_FREE_SLOT {
        message_delete(state, msg.pointer);
    } else {
        let (r, s) = (usize::from(row), usize::from(slot));
        state.message_pointers[r][s] = msg.pointer;
        state.team_slots[r][s] = QUEUED_DAYS;
    }
}

/// Counts down the construction-firm locks (shared by all managers, so they
/// tick once per manager and day) and the current manager's construction
/// days; a finished item adds its pending amount to the current one, and its
/// message goes to the inbox ([`message_queue_add`], row 1), or in the winter
/// break and after the season's last day it is shown at once.
///
/// Origin: BMMAIN.EXE 1000:20e1 (stadium_construction_daily)
pub fn stadium_construction_daily(
    state: &mut GameState,
    rng: &mut Rng,
    fmt: &mut NumberFormat,
) -> Vec<FinanceEvent> {
    let m = current_index(state);
    let mut events = Vec::new();
    for (item, subject) in CONSTRUCTION_SUBJECT.into_iter().enumerate() {
        let lock = &mut state.construction[item];
        *lock = lock.saturating_sub(1);
        let manager = &mut state.managers[m];
        if !count_down(&mut manager.build_days[item]) {
            continue;
        }
        let [current, pending] = &mut manager.stadium[item];
        *current = current.wrapping_add(*pending);
        *pending = 0;
        let lines = match item {
            3 => [subject, b"Flutlichtmasten", CONSTRUCTION_DONE],
            4 => [subject, b"Anzeigetafel", CONSTRUCTION_DONE],
            6 => [subject, b"Stadion-Komforts", CONSTRUCTION_DONE],
            _ => [subject, CONSTRUCTION_DONE, b""],
        };
        let shown = if construction_shown_at_once(state.day_counter) {
            state.saved_manager = state.current_manager;
            let args = lines.map(MessageArg::Text);
            let msg = message_post(state, rng, fmt, true, SHOWN_TEMPLATE, &args);
            let saved = usize::from(state.saved_manager as u8);
            let count = state.message_counts.get(saved).copied().unwrap_or(0);
            message_delete(state, msg.pointer);
            Some((msg.text, count))
        } else {
            message_queue_add(state, rng, fmt, lines, 1, 0);
            None
        };
        events.push(FinanceEvent::ConstructionFinished { item, shown });
    }
    events
}

/// Whether a finished construction is shown at once on day `day`: strictly
/// between `DS:224d * 7 + 0x80` and `0xcf - DS:224d * 14` (DS:224d is 0), or
/// after the season's last day.
///
/// Origin: BMMAIN.EXE 1000:20e1 (stadium_construction_daily, the test at 1000:223a..2295)
fn construction_shown_at_once(day: i32) -> bool {
    (0x80 < day && day < 0xcf) || day > 0x142
}

/// Decrements a running countdown; `true` when this step reached 0.
fn count_down<T: Copy + PartialEq + From<u8> + std::ops::Sub<Output = T>>(value: &mut T) -> bool {
    let zero = T::from(0);
    if *value == zero {
        return false;
    }
    *value = *value - T::from(1);
    *value == zero
}

/// Runs one day of the current manager's finances. Called once per day for
/// every human manager, so the camp counters and the interest-rate walk run
/// `DS:07ab` times per day.
///
/// Order, as in the original: stadium construction, training-camp counters
/// (one `random(20, 70)` each, also for a counter that is already 0),
/// `random(0, 60)` for the interest-rate walk, the balance sum, the camp
/// cooldown, the month-end block on the last day of the month, then the
/// loan payments.
///
/// Origin: BMMAIN.EXE 212a:0a6d (finance_daily_processing)
pub fn finance_daily_processing(
    state: &mut GameState,
    rng: &mut Rng,
    fmt: &mut NumberFormat,
) -> Vec<FinanceEvent> {
    let m = current_index(state);
    let mut events = stadium_construction_daily(state, rng, fmt);

    for counter in &mut state.ds_0620 {
        let step = i16::from(*counter < 0) - i16::from(*counter > 0);
        *counter = counter.wrapping_add(step);
        if *counter == 0 {
            *counter = rng.random_range(20, 70).wrapping_mul(step);
        }
    }
    if rng.random_range(0, 60) == 0 {
        interest_rates_update(state, rng);
    }

    state.month_balance_sum[m] = state.month_balance_sum[m].wrapping_add(state.managers[m].balance);
    let manager = &mut state.managers[m];
    manager.camp_cooldown = manager.camp_cooldown.saturating_sub(1);

    if i32::from(days_in_month(state.month)) == state.day_of_month {
        month_end(state, rng, m);
    }
    pay_loans(state, m, &mut events);
    events
}

/// Month-end part of [`finance_daily_processing`].
fn month_end(state: &mut GameState, rng: &mut Rng, m: usize) {
    if lrem(state.month, 4) == 0 {
        state.autosave_pending = 1;
    }
    let contracts = &mut state.manager_contracts[m];
    if count_down(&mut state.far_0000[2 * m]) {
        contracts[0] = ldiv(contracts[0], 10);
    }
    for i in 0..6 {
        if count_down(&mut state.far_4b9a[2 * (6 * m + i)]) {
            contracts[1 + i] = ldiv(contracts[1 + i], 10);
        }
    }
    let manager = &mut state.managers[m];
    manager.fatigue_sum = manager
        .fatigue_sum
        .wrapping_add((u16::from(manager.training_intensity) * 4) as i16);

    let income = finance_monthly_income(state);
    let manager = &mut state.managers[m];
    manager.balance = manager.balance.wrapping_add(income);
    let expenses = finance_monthly_expenses(state);
    let manager = &mut state.managers[m];
    manager.balance = manager.balance.wrapping_sub(expenses);
    state.month_balance_sum[m] = 0;

    if manager.fans < FAN_GROWTH_LIMIT {
        let promotion = ldiv(state.manager_contracts[m][8], 2500);
        if promotion > i32::from(rng.random_range(0, 22)) {
            manager.fans = manager.fans.wrapping_add(rng.random_range(1, 3) as u16);
        }
    }
}

/// Loan part of [`finance_daily_processing`]: pays the interest of every
/// loan whose payment day is today and repays the loans due this month.
///
/// A loan also pays on the last day of a month that is shorter than its due
/// month, whatever its payment day, so a loan whose payment day comes
/// earlier in that month pays twice. Kept as in the original.
fn pay_loans(state: &mut GameState, m: usize, events: &mut Vec<FinanceEvent>) {
    let today = state.day_of_month;
    let month_days = days_in_month(state.month);
    for lender in 0..5 {
        for slot in 0..3 {
            let credit = &state.managers[m].credits[lender][slot];
            if credit.principal == 0 {
                continue;
            }
            let pay = i32::from(credit.payment_day) == today
                || (DAYS_IN_MONTH[usize::from(credit.due_month)] > month_days
                    && i32::from(month_days) == today);
            if !pay {
                continue;
            }
            let due = i32::from(credit.due_year) == state.year
                && i32::from(credit.due_month) == state.month;
            let (principal, interest) = (credit.principal, credit.interest_payment);
            let manager = &mut state.managers[m];
            manager.balance = manager.balance.wrapping_sub(interest);
            if !due {
                continue;
            }
            events.push(FinanceEvent::LoanDue { lender });
            manager.balance = manager.balance.wrapping_sub(principal);
            // The interest field is left stale, as in the original.
            manager.credits[lender][slot].principal = 0;
            if lender != BANK {
                let creditor = &mut state.managers[lender];
                creditor.balance = creditor.balance.wrapping_add(principal);
            }
        }
    }
}

// TODO(re): ticket income (attendance x ticket price) is presumably booked in
// the match area (FUN_2a58_20b2); the meaning of the base income is
// unconfirmed (finance.md §7 q.1).
/// The current manager's monthly income: a base income from club strength,
/// squad size and fans (also stored in [`Manager::income_base`]), the eight
/// contract incomes, **every** interest payment of the other human managers
/// (to the bank and to third managers too), and interest on a positive
/// average balance of the month. Clamped to 0. Sets
/// [`GameState::saved_manager`] to the current manager as a side effect.
///
/// Ticket income is not part of it.
///
/// # Panics
/// When the balance sum is positive and the day of month is 0 (R6003).
///
/// Origin: BMMAIN.EXE 24a4:2636 (finance_monthly_income; Ghidra
/// finance_weekly_income)
pub fn finance_monthly_income(state: &mut GameState) -> i32 {
    let m = current_index(state);
    let players = i32::from(state.squad_player_count(m));
    let club = &state.clubs[usize::from(state.managers[m].team)];
    let strength = club
        .strength
        .iter()
        .zip(&club.base_strength)
        .fold(0i32, |s, (&now, &base)| {
            s + i32::from(now) + i32::from(base)
        });
    let strength = lmul(strength, 15);
    let manager = &mut state.managers[m];
    let mut income = ldiv(lmul(strength, players), 3)
        .wrapping_add(i32::from(manager.fans / 5))
        .wrapping_add(25);
    manager.income_base = income as u16;
    for &contract in &state.manager_contracts[m][..8] {
        income = income.wrapping_add(contract);
    }
    state.saved_manager = state.current_manager;
    for (other, manager) in state.active_managers().iter().enumerate() {
        if other != m {
            income = income.wrapping_add(credit_sum(manager, CreditField::Interest, None));
        }
    }
    let balance_sum = state.month_balance_sum[m];
    if balance_sum > 0 {
        income = income.wrapping_add(ldiv(ldiv(balance_sum, state.day_of_month), 75));
    }
    income.max(0)
}

/// The current manager's monthly expenses: a fixed 150 000, the salaries
/// (market values) of the own squad and of the pool players the manager
/// owns, stadium running costs by league, the extra cost item, training
/// intensity, the cost contract item, the own loan interest, 10 % of a
/// negative average balance and a wealth charge above 2 Mio. DM (not in the
/// amateur league). Clamped to 0.
///
/// # Panics
/// When the balance sum is negative and the day of month is 0 (R6003).
///
/// Origin: BMMAIN.EXE 24a4:2822 (finance_monthly_expenses; Ghidra
/// finance_weekly_expenses)
pub fn finance_monthly_expenses(state: &GameState) -> i32 {
    let m = current_index(state);
    let manager = &state.managers[m];
    let mut expenses: i32 = 150_000;
    let own = usize::from(state.squad_player_count(m));
    for player in &state.squads[m][..own] {
        expenses = expenses.wrapping_add(player.market_value);
    }
    let pool = usize::from(state.squad_player_count(TRANSFER_LIST));
    for player in &state.squads[TRANSFER_LIST][..pool] {
        if usize::from(state.players[usize::from(player.player_id)].owner_manager) == m {
            expenses = expenses.wrapping_add(player.market_value);
        }
    }

    let item = |i: usize| manager.stadium[i][0];
    let mut stadium = lmul(item(6), 3000)
        .wrapping_add(lmul(item(3).wrapping_add(item(4)), 1375))
        .wrapping_add(lmul(3 - i32::from(manager.league), 43750))
        .wrapping_add(item(0));
    stadium = lmul(stadium, 2).wrapping_add(item(1));
    stadium = lmul(stadium, 2).wrapping_add(item(2));
    expenses = expenses.wrapping_add(stadium);

    expenses = expenses.wrapping_add(i32::from(manager.extra_costs));
    let training = lshr(lmul(i32::from(manager.training_intensity) + 1, 10_000), 4);
    expenses = expenses.wrapping_add(training.wrapping_add(state.manager_contracts[m][8]));
    expenses = expenses.wrapping_add(credit_sum(manager, CreditField::Interest, None));

    let balance_sum = state.month_balance_sum[m];
    if balance_sum < 0 {
        expenses = expenses.wrapping_add(ldiv(ldiv(balance_sum, state.day_of_month), -10));
    }
    if manager.league != 2 && manager.balance > 2_000_000 {
        let mut charge = lmul(ldiv(manager.balance - 2_000_000, 500_000), 30_000) + 20_000;
        if manager.balance >= 4_000_000 {
            charge = ldiv(lmul(charge, 3), 2);
        }
        expenses = expenses.wrapping_add(charge);
    }
    expenses.max(0)
}

/// Byte `0x109 + r` of a manager record: the league position after round `r`
/// (`round_position`, [`Manager::position_history`] from `r` = 2 on).
fn round_position(m: &Manager, r: u8) -> u8 {
    match 0x109 + usize::from(r) {
        o @ 0x106..=0x109 => m.unknown_106[o - 0x106],
        0x10a => m.value_10a,
        o => m.position_history.get(o - 0x10b).copied().unwrap_or(0),
    }
}

/// The four lines of a `dialog_okay` box.
pub type DialogLines = [Vec<u8>; 4];

/// The messages about the season goals the office shows once per turn of the current
/// manager: the relegation play-off ahead, the last match day before the winter break (2
/// December), and per goal (title, promotion or UEFA cup place, staying up) once a season
/// that it can no longer be reached or is secured. The goals' announcements are remembered
/// in [`Runtime::goal_message_flags`]. The title is only announced in the Bundesliga.
///
/// Origin: BMMAIN.EXE 212a:314d (season_goal_messages)
pub fn season_goal_messages(state: &GameState, rt: &mut Runtime) -> Vec<DialogLines> {
    // DS:224e is 1 and DS:2252 == DS:224e in the shipped data: `flag4` is false, no swap.
    let flag4 = 0;
    let line = |t: &[u8]| t.to_vec();
    let m = current_index(state);
    let mgr = &state.managers[m];
    let league = usize::from(mgr.league).min(2);
    let n = i16::from(rt.tier_team_count[league]);
    let own = usize::from(mgr.team);
    let pts = |t: usize| {
        let p = &state.team_tables[t].points;
        i16::from(p[0]) + i16::from(p[1])
    };
    let left = |t: usize| {
        let p = &state.team_tables[t].played;
        (i16::from(rt.tier_round_count[league]) - i16::from(p[0]) - i16::from(p[1]))
            * i16::from(POINTS_PER_WIN)
    };
    let rank = |r: i16| usize::from(state.league_teams[league][(r as usize).min(19)]);
    let round = state.competition_rounds[league];
    let mut out = Vec::new();
    if crate::league_layout::layout().relegation_playoff[0]
        && state.month == 5
        && state.day_of_month == 13
        && i16::from(round as i8) - i16::from(rt.tier_round_count[league]) == 1
        && ((league == 0 && round_position(mgr, round) == 15)
            || (league == 1 && round_position(mgr, round) == 2))
    {
        out.push([
            line(b"Ihre Mannschaft"),
            line(b"bestreitet das"),
            line(b"Relegationsspiel."),
            line(b"Viel Gl}ck."),
        ]);
        return out;
    }
    if state.month == 5 && state.day_of_month > 13 {
        return out;
    }
    if state.day_of_month == 2 + flag4 && state.month == 11 {
        out.push([
            line(b"Achtung !"),
            line(b"Dies ist der letzte"),
            line(b"Spieltag vor der"),
            line(b"Winterpause."),
        ]);
    }
    let rk: [i16; 3] = match league {
        0 => [0, 4, n - flag4 as i16 - 4],
        1 => [0, 2 * flag4 as i16 + 1, n - 5],
        _ => [0, 3, n - 5],
    };
    let mine = pts(own);
    let own_left = left(own);
    let name = state.clubs[own].name();
    let mut has = name.to_vec();
    has.extend_from_slice(b" hat");
    for (k, &rk_k) in rk.iter().enumerate() {
        let flags = &mut rt.goal_message_flags[m];
        if pts(rank(rk_k)) > own_left + mine && *flags & (1 << k) == 0 {
            *flags |= 1 << k;
            let goal: &[u8] = match (k, league) {
                (0, _) => b"MEISTER zu werden.",
                (1, 0) => b"den UEFA-CUP zu erreichen.",
                (1, _) => b"AUFZUSTEIGEN.",
                _ => b"DIE KLASSE ZU ERHALTEN.",
            };
            if league == 0 || k != 0 {
                out.push([
                    mgr.name().to_vec(),
                    has.clone(),
                    line(b"die letzte Chance verspielt,"),
                    line(goal),
                ]);
            }
        }
        let best = ((rk_k + 1)..n)
            .map(|r| left(rank(r)) + pts(rank(r)))
            .fold(0, i16::max);
        let flags = &mut rt.goal_message_flags[m];
        if best < mine && *flags & (1 << (k + 3)) == 0 {
            *flags |= 1 << (k + 3);
            let goal: &[u8] = match (k, league) {
                (0, _) => b"die MEISTERSCHAFT",
                (1, 0) => b"den UEFA-CUP-PLATZ",
                (1, _) => b"den AUFSTIEG",
                _ => b"den KLASSENERHALT",
            };
            if league == 0 || k != 0 {
                out.push([
                    mgr.name().to_vec(),
                    has.clone(),
                    line(goal),
                    line(b"auf jeden Fall gesichert."),
                ]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Layout;
    use bmp_formats::savegame::SaveGame;

    /// One active manager (index 0), dated `day`.`month + 1`.`year`.
    /// Manager 0 coaches club 20 of the 2. Bundesliga, which stands at rank `own_rank`.
    fn goal_state(own_rank: usize) -> GameState {
        let mut s = state_on(10, 2, 1968);
        s.managers[0].league = 1;
        s.managers[0].team = 20;
        s.managers[0].name[..3].copy_from_slice(b"ADI");
        s.clubs[20].name[..5].copy_from_slice(b"KICKS");
        let mut ids: Vec<u8> = (18..38).filter(|&t| t != 20).collect();
        ids.insert(own_rank, 20);
        s.league_teams[1].copy_from_slice(&ids);
        s
    }

    #[test]
    fn the_last_match_day_before_the_winter_break_is_announced_on_2_december() {
        let mut s = goal_state(0);
        let mut rt = Runtime::default();
        assert!(season_goal_messages(&s, &mut rt).is_empty());
        (s.day_of_month, s.month) = (2, 11);
        let msgs = season_goal_messages(&s, &mut rt);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0][0], b"Achtung !");
        assert_eq!(msgs[0][3], b"Winterpause.");
        // It is shown on every office turn of that day.
        assert_eq!(season_goal_messages(&s, &mut rt).len(), 1);
    }

    #[test]
    fn a_lost_promotion_is_announced_once() {
        let mut s = goal_state(5);
        // The runner-up has more points than the club can still reach (38 rounds x 2 points).
        let second = usize::from(s.league_teams[1][1]);
        s.team_tables[second].points = [40, 37, 0, 0];
        let mut rt = Runtime::default();
        let msgs = season_goal_messages(&s, &mut rt);
        assert_eq!(
            msgs,
            [[
                b"ADI".to_vec(),
                b"KICKS hat".to_vec(),
                b"die letzte Chance verspielt,".to_vec(),
                b"AUFZUSTEIGEN.".to_vec()
            ]]
        );
        assert_eq!(rt.goal_message_flags[0], 0b10);
        assert!(season_goal_messages(&s, &mut rt).is_empty());
    }

    #[test]
    fn secured_goals_are_announced_but_the_title_only_in_the_bundesliga() {
        let mut s = goal_state(0);
        for t in 18..38 {
            s.team_tables[t].played = [19, 19, 0, 0];
            s.team_tables[t].points = [5, 5, 0, 0];
        }
        s.team_tables[20].points = [30, 30, 0, 0];
        let mut rt = Runtime::default();
        let msgs = season_goal_messages(&s, &mut rt);
        let goals: Vec<&[u8]> = msgs.iter().map(|m| &m[2][..]).collect();
        assert_eq!(goals, [&b"den AUFSTIEG"[..], b"den KLASSENERHALT"]);
        assert_eq!(rt.goal_message_flags[0], 0b111000);
    }

    fn state_on(day: i32, month: i32, year: i32) -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.day_of_month = day;
        s.month = month;
        s.year = year;
        s
    }

    fn loan(
        principal: i32,
        interest: i32,
        payment_day: u8,
        due_month: u8,
        due_year: i16,
    ) -> Credit {
        Credit {
            principal,
            interest_payment: interest,
            payment_day,
            due_month,
            due_year,
            ..Credit::zeroed()
        }
    }

    /// A generator whose first `random(0, 60)` draw skips the interest-rate walk.
    fn rng_without_rate_walk() -> Rng {
        (0..=u16::MAX)
            .map(Rng::new)
            .find(|&r| {
                let mut r = r;
                r.random_range(0, 60) != 0
            })
            .unwrap()
    }

    #[test]
    fn credit_sum_filters_lender_and_free_slots() {
        let mut m = GameState::zeroed().managers[0].clone();
        m.credits[1][0] = loan(1000, 70, 1, 0, 0);
        m.credits[1][2] = loan(500, 15, 1, 0, 0);
        m.credits[BANK][1] = loan(4000, 200, 1, 0, 0);
        m.credits[2][0] = loan(0, 999, 1, 0, 0); // free slot with stale interest
        assert_eq!(credit_sum(&m, CreditField::Principal, Some(1)), 1500);
        assert_eq!(credit_sum(&m, CreditField::Interest, Some(1)), 85);
        assert_eq!(credit_sum(&m, CreditField::Principal, None), 5500);
        assert_eq!(credit_sum(&m, CreditField::Interest, None), 285);
        assert_eq!(credit_sum(&m, CreditField::Interest, Some(2)), 0);
    }

    #[test]
    fn credit_take_dates_and_interest() {
        // 15 Nov 1966 + 3 months: due February 1967, payment day 15.
        let mut s = state_on(15, 10, 1966);
        assert!(credit_take(&mut s, 100_000, 7, BANK, 3));
        let c = &s.managers[0].credits[BANK][0];
        assert_eq!(
            (c.principal, c.interest_payment, c.rate),
            (100_000, 7000, 7)
        );
        assert_eq!((c.day_taken, c.month_taken, c.year_taken), (15, 10, 1966));
        assert_eq!((c.payment_day, c.due_month, c.due_year), (15, 1, 1967));

        // 31 Jan + 1 month: February is shorter, so the payment day is 28.
        let mut s = state_on(31, 0, 1966);
        assert!(credit_take(&mut s, 333, 3, 1, 1));
        let c = &s.managers[0].credits[1][0];
        assert_eq!((c.payment_day, c.due_month, c.due_year), (28, 1, 1966));
        assert_eq!(c.interest_payment, 9); // 999 / 100 truncates

        // Interest truncates toward zero; 24 months from December cross two years.
        let mut s = state_on(1, 11, 1966);
        assert!(credit_take(&mut s, -333, 3, 2, 24));
        let c = &s.managers[0].credits[2][0];
        assert_eq!(c.interest_payment, -9);
        assert_eq!((c.due_month, c.due_year), (11, 1968));
    }

    #[test]
    fn credit_take_refuses_a_fourth_loan_per_lender() {
        let mut s = state_on(1, 0, 1966);
        for amount in [1000, 2000, 3000] {
            assert!(credit_take(&mut s, amount, 5, BANK, 3));
        }
        let before = s.clone();
        assert!(!credit_take(&mut s, 4000, 5, BANK, 3));
        assert_eq!(s, before);
        assert!(credit_take(&mut s, 4000, 5, 0, 3));
        // A repaid slot is reused first.
        s.managers[0].credits[BANK][1].principal = 0;
        assert!(credit_take(&mut s, 5000, 5, BANK, 3));
        assert_eq!(s.managers[0].credits[BANK][1].principal, 5000);
    }

    #[test]
    fn interest_rate_table_examples() {
        assert_eq!(interest_rate_table(7), [7, 5, 3, 2, 2, 2]);
        assert_eq!(interest_rate_table(5), [5; 6]);
        assert_eq!(interest_rate_table(3), [3, 5, 6, 7, 7, 7]);
        assert_eq!(interest_rate_table(2), [2, 5, 7, 7, 7, 7]);
    }

    #[test]
    fn interest_rates_update_walks_and_clamps() {
        for (level, base, hard) in [(0u8, 2i16, 0i16), (2, 7, 0), (3, 6, 1), (5, 7, 1)] {
            for seed in 0..20 {
                let mut s = GameState::zeroed();
                s.level = level;
                s.base_interest_rate = base;
                let mut rng = Rng::new(seed);
                let mut want = rng;
                let r = want.random_range(0, hard + 2);
                interest_rates_update(&mut s, &mut rng);
                assert_eq!(rng, want, "one draw");
                assert_eq!(s.base_interest_rate, (base + r + hard - 1).clamp(2, 7));
                assert_eq!(s.interest_rates, interest_rate_table(s.base_interest_rate));
            }
        }
    }

    #[test]
    fn construction_finishes_items_and_ticks_locks() {
        let mut s = state_on(10, 0, 1966);
        s.construction = [3, 0, 1, 0, 0, 0, 0];
        let m = &mut s.managers[0];
        m.build_days = [1, 2, 0, 0, 0, 0, 0];
        m.stadium[0] = [1000, 500];
        m.stadium[1] = [2000, 700];
        m.stadium[2] = [0, 300]; // pending without a countdown never finishes
        let mut rng = Rng::new(5);
        let mut fmt = NumberFormat::default();

        let events = stadium_construction_daily(&mut s, &mut rng, &mut fmt);
        let finished = |item| FinanceEvent::ConstructionFinished { item, shown: None };
        assert_eq!(events, [finished(0)]);
        assert_eq!(s.construction, [2, 0, 0, 0, 0, 0, 0]);
        let m = &s.managers[0];
        assert_eq!(m.build_days, [0, 1, 0, 0, 0, 0, 0]);
        assert_eq!(m.stadium[0], [1500, 0]);
        assert_eq!(m.stadium[1], [2000, 700]);

        let events = stadium_construction_daily(&mut s, &mut rng, &mut fmt);
        assert_eq!(events, [finished(1)]);
        assert_eq!(s.managers[0].stadium[1], [2700, 0]);
        assert_eq!(s.managers[0].stadium[2], [0, 300]);
    }

    /// The inbox texts of manager 0 after the date line.
    fn inbox(s: &GameState) -> Vec<&[u8]> {
        s.messages[0]
            .iter()
            .map(|e| {
                let text = e.bytes.strip_suffix(&[0]).unwrap();
                let date = text.iter().position(|&c| c == b'^').unwrap();
                &text[date + 1..]
            })
            .collect()
    }

    #[test]
    fn finished_constructions_are_queued_in_row_1_during_the_season() {
        let mut s = state_on(10, 0, 1966);
        s.day_counter = 0x80;
        s.team_slots[1] = [3, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        s.managers[0].build_days = [0, 0, 0, 1, 0, 0, 1];
        let mut rng = Rng::new(5);
        let mut want = rng;
        want.random_range(0, 3);
        want.random_range(0, 3);

        let events = stadium_construction_daily(&mut s, &mut rng, &mut NumberFormat::default());
        assert!(
            events
                .iter()
                .all(|e| matches!(e, FinanceEvent::ConstructionFinished { shown: None, .. }))
        );
        assert_eq!(rng, want, "one date draw per message");
        assert_eq!(
            inbox(&s),
            [
                &b"Die Verbesserung des^Stadion-Komforts^ist abgeschlossen. ^"[..],
                b"Die Errichtung der neuen^Flutlichtmasten^ist abgeschlossen. ^",
            ]
        );
        assert_eq!(s.team_slots[1], [3, 1, 3, 3, 0, 0, 0, 0, 0, 0]);
        let ptrs: Vec<u32> = s.messages[0].iter().map(|e| e.old_pointer).collect();
        assert_eq!(s.message_pointers[1][2..4], [ptrs[1], ptrs[0]]);
        assert_eq!(s.far_56e2[0], 1, "new-message flag");
    }

    #[test]
    fn finished_constructions_are_shown_at_once_in_the_winter_break_and_after_the_season() {
        for day in [0x81, 0xce, 0x143] {
            let mut s = state_on(10, 0, 1966);
            s.day_counter = day;
            s.managers[0].build_days[0] = 1;
            let mut rng = Rng::new(5);
            let mut want = rng;
            want.random_range(0, 3);

            let events = stadium_construction_daily(&mut s, &mut rng, &mut NumberFormat::default());
            let [
                FinanceEvent::ConstructionFinished {
                    item: 0,
                    shown: Some((text, _)),
                },
            ] = &events[..]
            else {
                panic!("day {day}: {events:?}");
            };
            assert!(text.ends_with(b"^Der Ausbau der Sitzpl{tze^ist abgeschlossen.^ ^"));
            assert_eq!(rng, want);
            assert!(inbox(&s).is_empty(), "day {day}: deleted again");
            assert_eq!(s.team_slots, [[0; 10]; 3]);
            assert_eq!(s.far_56e2[0], 0);
        }
    }

    #[test]
    fn a_queued_message_without_a_free_slot_is_deleted() {
        let mut s = state_on(10, 0, 1966);
        s.team_slots[2] = [1; 10];
        let mut rng = Rng::new(5);
        message_queue_add(
            &mut s,
            &mut rng,
            &mut NumberFormat::default(),
            [b"A", b"B", b"C"],
            2,
            0,
        );
        assert!(inbox(&s).is_empty());
        assert_eq!(s.message_pointers, [[0; 10]; 3]);
        assert_eq!(s.team_slots[2], [1; 10]);
        assert_eq!(team_slot_free_index(&s, 2), NO_FREE_SLOT);
        assert_eq!(team_slot_free_index(&s, 3), NO_FREE_SLOT);
    }

    #[test]
    fn loans_pay_on_payment_day_and_last_day_of_shorter_months() {
        // Payment day 15, due in January (31 days) 1967.
        let paid = |day: i32, month: i32| {
            let mut s = state_on(day, month, 1966);
            s.managers[0].balance = 10_000;
            s.managers[0].credits[BANK][0] = loan(50_000, 300, 15, 0, 1967);
            let mut events = Vec::new();
            pay_loans(&mut s, 0, &mut events);
            assert!(events.is_empty());
            10_000 - s.managers[0].balance
        };
        assert_eq!(paid(15, 3), 300);
        assert_eq!(paid(29, 3), 0);
        assert_eq!(paid(30, 3), 300); // second payment in April
        assert_eq!(paid(28, 1), 300); // end of February
        assert_eq!(paid(31, 4), 0); // May is as long as January
    }

    #[test]
    fn due_loans_are_repaid_to_the_lender() {
        let mut s = state_on(15, 0, 1967);
        s.manager_count = 3;
        s.managers[0].balance = 100_000;
        s.managers[2].balance = 1000;
        s.managers[0].credits[2][1] = loan(40_000, 400, 15, 0, 1967);
        s.managers[0].credits[BANK][0] = loan(20_000, 100, 15, 0, 1967);
        s.managers[0].credits[BANK][2] = loan(30_000, 50, 15, 0, 1968); // next year
        let mut events = Vec::new();
        pay_loans(&mut s, 0, &mut events);
        assert_eq!(
            events,
            [
                FinanceEvent::LoanDue { lender: 2 },
                FinanceEvent::LoanDue { lender: BANK }
            ]
        );
        assert_eq!(
            s.managers[0].balance,
            100_000 - 400 - 40_000 - 100 - 20_000 - 50
        );
        assert_eq!(s.managers[2].balance, 41_000);
        let c = &s.managers[0].credits;
        assert_eq!((c[2][1].principal, c[2][1].interest_payment), (0, 400));
        assert_eq!(c[BANK][0].principal, 0);
        assert_eq!(c[BANK][2].principal, 30_000);
    }

    #[test]
    fn monthly_income_by_hand() {
        let mut s = state_on(30, 3, 1966);
        s.manager_count = 3;
        s.current_manager = 1;
        s.saved_manager = 0;
        s.managers[1].team = 3;
        s.clubs[3].strength = [10, 20, 30];
        s.clubs[3].base_strength = [5, 5, 5];
        for slot in [0, 1, 5, 24] {
            s.squads[1][slot].player_id = 9; // slot 24 is never counted
        }
        s.managers[1].fans = 47;
        s.manager_contracts[1] = [1000, 100, 100, 100, 100, 100, 100, 7, 999_999];
        // Interest paid by the other managers, whoever the lender is.
        s.managers[0].credits[BANK][0] = loan(10_000, 300, 1, 0, 0);
        s.managers[2].credits[0][0] = loan(10_000, 50, 1, 0, 0);
        s.managers[2].credits[0][1] = loan(0, 999, 1, 0, 0);
        s.managers[1].credits[BANK][0] = loan(10_000, 4000, 1, 0, 0); // own: not income
        s.managers[3].credits[BANK][0] = loan(10_000, 5000, 1, 0, 0); // inactive slot
        s.month_balance_sum[1] = 225_000;

        // (75 * 15) * 3 / 3 + 47 / 5 + 25 = 1159
        let base = 1159;
        let want = base + 1607 + 350 + 225_000 / 30 / 75;
        assert_eq!(finance_monthly_income(&mut s), want);
        assert_eq!(s.managers[1].income_base, base as u16);
        assert_eq!(s.saved_manager, 1);

        s.month_balance_sum[1] = 224_999; // 7499 / 75 = 99
        assert_eq!(finance_monthly_income(&mut s), want - 1);
        s.month_balance_sum[1] = -1_000_000; // a negative sum is no income
        assert_eq!(finance_monthly_income(&mut s), want - 100);

        s.manager_contracts[1][0] = -1_000_000;
        assert_eq!(finance_monthly_income(&mut s), 0);
        assert_eq!(s.managers[1].income_base, base as u16);
    }

    #[test]
    fn monthly_expenses_by_hand() {
        let mut s = state_on(31, 0, 1966);
        // Two occupied slots, but the scan walks slots 0..2: the empty slot 1
        // counts, the player in slot 2 does not.
        s.squads[0][0].player_id = 1;
        s.squads[0][0].market_value = 20_000;
        s.squads[0][1].market_value = 7;
        s.squads[0][2].player_id = 2;
        s.squads[0][2].market_value = 30_000;
        // Pool players are paid by the manager who owns them.
        s.squads[TRANSFER_LIST][0].player_id = 5;
        s.squads[TRANSFER_LIST][0].market_value = 1000;
        s.squads[TRANSFER_LIST][1].player_id = 6;
        s.squads[TRANSFER_LIST][1].market_value = 2000;
        s.players[5].owner_manager = 0;
        s.players[6].owner_manager = 1;
        let m = &mut s.managers[0];
        m.league = 1;
        m.stadium = [
            [10_000, 99],
            [20_000, 99],
            [5000, 99],
            [2, 1],
            [1, 1],
            [5, 1],
            [3, 1],
        ];
        m.extra_costs = 1234;
        m.training_intensity = 34;
        m.balance = 1_000_000;
        m.credits[BANK][0] = loan(10_000, 700, 1, 0, 0);
        s.manager_contracts[0][8] = 5000;
        s.month_balance_sum[0] = -310_000;

        // Stadium: ((3*3000 + 3*1375 + 2*43750 + 10000) * 2 + 20000) * 2 + 5000.
        let stadium = 487_500;
        let training = (35 * 10_000) >> 4;
        let want = 150_000 + 20_007 + 1000 + stadium + 1234 + training + 5000 + 700 + 1000;
        assert_eq!(finance_monthly_expenses(&s), want);

        s.manager_contracts[0][8] = -10_000_000;
        assert_eq!(finance_monthly_expenses(&s), 0);
    }

    #[test]
    fn monthly_expenses_wealth_charge() {
        let charge = |balance: i32, league: u8| {
            let mut s = state_on(31, 0, 1966);
            s.managers[0].league = league;
            let base = finance_monthly_expenses(&s);
            s.managers[0].balance = balance;
            finance_monthly_expenses(&s) - base
        };
        assert_eq!(charge(2_000_000, 0), 0);
        assert_eq!(charge(2_000_001, 0), 20_000);
        assert_eq!(charge(2_499_999, 1), 20_000);
        assert_eq!(charge(2_500_000, 1), 50_000);
        assert_eq!(charge(3_999_999, 0), 110_000);
        assert_eq!(charge(4_000_000, 0), 210_000);
        assert_eq!(charge(9_000_000, 2), 0);
    }

    #[test]
    fn daily_processing_draws_in_original_order() {
        let mut s = state_on(10, 0, 1966);
        s.ds_0620 = [-1, 1, 0, 5, -5, 2, -2, 70];
        s.managers[0].balance = 1234;
        s.managers[0].camp_cooldown = 3;
        s.month_balance_sum[0] = 10;
        let mut rng = rng_without_rate_walk();
        let mut want = rng;
        let a = want.random_range(20, 70);
        let b = want.random_range(20, 70);
        want.random_range(20, 70); // counter already 0: drawn, times 0
        want.random_range(0, 60);

        let events = finance_daily_processing(&mut s, &mut rng, &mut NumberFormat::default());
        assert!(events.is_empty());
        assert_eq!(rng, want);
        assert_eq!(s.ds_0620, [a, -b, 0, 4, -4, 1, -1, 69]);
        assert_eq!(s.month_balance_sum[0], 1244);
        assert_eq!(s.managers[0].camp_cooldown, 2);
        assert_eq!(s.interest_rates, [0; 6]);
    }

    #[test]
    fn daily_processing_runs_the_rate_walk_after_the_counters() {
        let mut s = state_on(10, 0, 1966);
        s.ds_0620 = [0; 8];
        let counter_draws = |rng: &mut Rng| {
            for _ in 0..8 {
                rng.random_range(20, 70);
            }
        };
        let mut rng = (0..=u16::MAX)
            .map(Rng::new)
            .find(|r| {
                let mut r = *r;
                counter_draws(&mut r);
                r.random_range(0, 60) == 0
            })
            .unwrap();
        let mut want = rng;
        counter_draws(&mut want);
        want.random_range(0, 60);
        let mut walked = s.clone();
        interest_rates_update(&mut walked, &mut want);

        finance_daily_processing(&mut s, &mut rng, &mut NumberFormat::default());
        assert_eq!(rng, want);
        assert_eq!(s.base_interest_rate, walked.base_interest_rate);
        assert_eq!(s.interest_rates, walked.interest_rates);
        assert_ne!(s.interest_rates, [0; 6]);
    }

    /// January 31: month-end state shared by the month-end tests.
    fn month_end_state() -> GameState {
        let mut s = state_on(31, 0, 1966);
        s.ds_0620 = [10; 8]; // no counter reaches 0: no draws
        s.managers[0].team = 1;
        s.managers[0].fans = 50;
        s.managers[0].training_intensity = 10;
        s.managers[0].fatigue_sum = 7;
        s.manager_contracts[0] = [50_000, 6000, 6000, 6000, 6000, 6000, 6000, 0, 50_000];
        s.far_0000[0] = 1;
        s.far_4b9a[0] = 3;
        s.far_4b9a[2] = 1;
        s
    }

    #[test]
    fn month_end_books_income_then_expenses() {
        let mut s = month_end_state();
        s.managers[0].balance = 1_990_000;
        s.managers[0].credits[BANK][0] = loan(10_000, 100, 31, 5, 1966);
        let mut rng = rng_without_rate_walk();

        // The month-end block sees today's balance in the sum and the
        // shortened contracts; expenses see the balance after the income.
        let mut want = s.clone();
        want.month_balance_sum[0] = 1_990_000;
        want.manager_contracts[0][0] = 5000;
        want.manager_contracts[0][2] = 600;
        let income = finance_monthly_income(&mut want);
        want.managers[0].balance += income;
        let expenses = finance_monthly_expenses(&want);
        assert!(expenses > finance_monthly_expenses(&month_end_state()) + 20_000);

        let mut draws = rng;
        draws.random_range(0, 60);
        let promotion_draw = draws.random_range(0, 22);
        let grow = if 20 > i32::from(promotion_draw) {
            draws.random_range(1, 3) as u16
        } else {
            0
        };

        finance_daily_processing(&mut s, &mut rng, &mut NumberFormat::default());
        assert_eq!(rng, draws);
        let m = &s.managers[0];
        assert_eq!(m.balance, 1_990_000 + income - expenses - 100);
        assert_eq!(m.income_base, want.managers[0].income_base);
        assert_eq!(m.fatigue_sum, 47);
        assert_eq!(m.fans, 50 + grow);
        assert_eq!(s.month_balance_sum[0], 0);
        assert_eq!(s.autosave_pending, 1);
        assert_eq!(s.far_0000[0], 0);
        assert_eq!(&s.far_4b9a[..4], &[2, 0, 0, 0]);
        assert_eq!(
            s.manager_contracts[0],
            [5000, 6000, 600, 6000, 6000, 6000, 6000, 0, 50_000]
        );
    }

    #[test]
    fn month_end_fans_draw_only_below_95() {
        for (fans, month, autosave) in [(95u16, 1, 0u8), (u16::MAX, 4, 1)] {
            let mut s = month_end_state();
            s.month = month;
            s.day_of_month = i32::from(DAYS_IN_MONTH[month as usize]);
            s.managers[0].fans = fans;
            let mut rng = rng_without_rate_walk();
            let mut want = rng;
            want.random_range(0, 60);
            finance_daily_processing(&mut s, &mut rng, &mut NumberFormat::default());
            assert_eq!(rng, want);
            assert_eq!(s.managers[0].fans, fans);
            assert_eq!(s.autosave_pending, autosave);
        }
    }

    #[test]
    fn month_end_works_on_the_current_manager_only() {
        // A seed that skips the rate walk and whose promotion draw `d` lets
        // a divisor other than 2500 flip the result at the 2500 * (d + 1)
        // boundary.
        let (rng, d) = (0..=u16::MAX)
            .map(Rng::new)
            .find_map(|r| {
                let mut draws = r;
                (draws.random_range(0, 60) != 0)
                    .then(|| draws.random_range(0, 22))
                    .filter(|d| (5..=20).contains(d))
                    .map(|d| (r, i32::from(d)))
            })
            .unwrap();

        for (promotion, grows) in [(2500 * (d + 1), true), (2500 * (d + 1) - 1, false)] {
            let mut s = month_end_state();
            s.manager_count = 3;
            s.current_manager = 2;
            s.managers[2] = s.managers[0].clone();
            s.manager_contracts[0][8] = 0;
            s.manager_contracts[2] = [80_000, 7000, 7000, 7000, 7000, 7000, 7000, 0, promotion];
            s.far_0000[4] = 1;
            s.far_4b9a[2 * (6 * 2)] = 3;
            s.far_4b9a[2 * (6 * 2 + 1)] = 1;
            let before = s.clone();

            let mut rng = rng;
            let mut want = rng;
            want.random_range(0, 60);
            want.random_range(0, 22);
            let growth = if grows {
                want.random_range(1, 3) as u16
            } else {
                0
            };

            finance_daily_processing(&mut s, &mut rng, &mut NumberFormat::default());
            assert_eq!(rng, want, "{promotion}");
            assert_eq!(s.managers[2].fans, 50 + growth, "{promotion}");
            assert_eq!(s.managers[2].fatigue_sum, 47);
            assert_eq!(s.far_0000[4], 0);
            assert_eq!(&s.far_4b9a[24..28], &[2, 0, 0, 0]);
            assert_eq!(
                s.manager_contracts[2],
                [8000, 7000, 700, 7000, 7000, 7000, 7000, 0, promotion]
            );

            assert_eq!(s.managers[0], before.managers[0]);
            assert_eq!(s.managers[1], before.managers[1]);
            assert_eq!(&s.far_0000[..4], &before.far_0000[..4]);
            assert_eq!(&s.far_4b9a[..24], &before.far_4b9a[..24]);
            assert_eq!(&s.manager_contracts[..2], &before.manager_contracts[..2]);
        }
    }

    /// `None` only when `BMP_DATA` is unset; a missing save fails the test.
    fn load(name: &str) -> Option<GameState> {
        let path = bmp_formats::data_dir()?.join(name);
        let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        Some(GameState::from_savegame(&SaveGame::parse(&data).unwrap()).unwrap())
    }

    const SAVES: [&str; 6] = [
        "07072019.MAN",
        "2020-JUN.MAN",
        "2021-AUG.MAN",
        "AUTOSAVE.MAN",
        "BMP/AUTOSAVE.MAN",
        "BMP/MICHEL1.MAN",
    ];

    #[test]
    fn real_saves_hold_loans_and_rates_this_module_produces() {
        for name in SAVES {
            let Some(s) = load(name) else { continue };
            let initial = [7, 6, 5, 4, 3, 2];
            assert!(
                s.interest_rates == initial
                    || (2..=7).any(|b| interest_rate_table(b) == s.interest_rates),
                "{name}: {:?}",
                s.interest_rates
            );
            for (m, manager) in s.active_managers().iter().enumerate() {
                for (lender, slots) in manager.credits.iter().enumerate() {
                    for c in slots.iter().filter(|c| c.principal != 0) {
                        let months = (i32::from(c.due_year) * 12 + i32::from(c.due_month))
                            - (i32::from(c.year_taken) * 12 + i32::from(c.month_taken));
                        let mut t = state_on(
                            i32::from(c.day_taken),
                            i32::from(c.month_taken),
                            i32::from(c.year_taken),
                        );
                        t.current_manager = m as i8;
                        assert!(credit_take(
                            &mut t,
                            c.principal,
                            c.rate as i8,
                            lender,
                            months as i8
                        ));
                        assert_eq!(&t.managers[m].credits[lender][0], c, "{name}");
                    }
                }
            }
        }
    }

    #[test]
    fn real_save_income_base_matches() {
        // Saved right after the statistics screen recomputed the income.
        let Some(mut s) = load("2021-AUG.MAN") else {
            return;
        };
        let saved = s.managers[0].income_base;
        finance_monthly_income(&mut s);
        assert_eq!(s.managers[0].income_base, saved);
        assert_eq!(saved, 32592);
    }

    #[test]
    fn real_saves_three_days_of_camp_counters() {
        // The same game three days apart (21 and 24 January 1965).
        let (Some(mut s), Some(later)) = (load("BMP/AUTOSAVE.MAN"), load("BMP/MICHEL1.MAN")) else {
            return;
        };
        let mut rng = Rng::default();
        for day in 22..=24 {
            s.day_of_month = day;
            finance_daily_processing(&mut s, &mut rng, &mut NumberFormat::default());
        }
        // Counter 2 hit 0 on the first day and was re-drawn; the rest only step.
        for i in (0..8).filter(|&i| i != 2) {
            assert_eq!(s.ds_0620[i], later.ds_0620[i], "counter {i}");
        }
        assert!(later.ds_0620[2] > 0 && s.ds_0620[2] > 0);
    }
}
