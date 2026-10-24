//! Newspaper and message inbox of `BMMAIN.EXE` segments 3e3a and 4091: the
//! per-match event record, the post-match player grades, the match analysis
//! that picks the newspaper's templates, the template engine with its
//! justified text layout, and posting and deleting inbox messages.
//!
//! Drawing is the caller's job. [`newspaper_generate`] returns the photo
//! number, the expanded headline and the laid-out body and sidebar lines;
//! [`message_post`] returns the message text. The layout is logic because the
//! body stops at a full page, which decides how many random numbers the
//! template expansion draws, so text widths come in through
//! [`NewsEnv::text_width`].
//!
//! Full documentation with addresses: `docs/re/logic/news.md`.

mod templates;

use crate::matchsim::{player_position_mismatch, player_role_mismatch, team_strength_value};
use crate::rng::{NumberFormat, Rng, format_long_thousands, ldiv, long_abs_diff};
use crate::runtime::Runtime;
use crate::squad::squad_count_listed;
use crate::state::{GameState, Layout, MANAGER_SLOTS, NewsRecord, SQUAD_SIZE, SquadPlayer};
use bmp_formats::savegame::StringEntry;
use templates::{
    BODY_GROUP_SIZES, BODY_IMAGE, BODY_OFFSETS, HEADLINE_GROUP_SIZES, HEADLINE_IMAGE,
    HEADLINE_OFFSETS, HEAT_WEIGHTS, MESSAGE_TEMPLATES, MONTH_LENGTHS, MONTH_NAMES, SIDEBAR_GOALS,
    SIDEBAR_NO_GOALS, SIDEBAR_NONE, SIDEBAR_RED, SIDEBAR_YELLOW,
};

/// Event kind of [`news_record_match_event`]: a chance.
pub const EVENT_CHANCE: u8 = 0;
/// Event kind of [`news_record_match_event`]: a goal.
pub const EVENT_GOAL: u8 = 1;
/// Event kind of [`news_record_match_event`]: a red card.
pub const EVENT_RED_CARD: u8 = 2;
/// Event kind of [`news_record_match_event`]: a yellow card.
pub const EVENT_YELLOW_CARD: u8 = 3;
/// Player index of an event of the opposing team.
pub const OPPONENT: u8 = 0x64;

/// Index of the newspaper option DS:060b in [`GameState::options`] (DS:05fe).
const NEWSPAPER_OPTION: usize = 13;
/// Value of [`SquadPlayer::suspended`] that the grading turns into a
/// one-match suspension.
const SUSPENSION_PENDING: u8 = 0x64;
/// First lineup position of the bench.
const FIRST_SUBSTITUTE: u8 = 12;
/// PIC number of the first newspaper photo (PIC 200..=229).
const FIRST_PHOTO: u16 = 200;
/// Minutes covered by the heat map of the match analysis.
const HEAT_MINUTES: usize = 120;
/// Line width of the body text.
const BODY_WIDTH: i16 = 0xb4;
/// The body stops once a line starts below this y.
const BODY_LAST_Y: u8 = 0x94;
/// Line width of the sidebar; only lines of this width use [`LineStyle::LabelValue`].
const SIDEBAR_WIDTH: i16 = 0x127;
/// Line height of the text layout.
const LINE_HEIGHT: i16 = 7;
/// Word-end slots of the text layout (DS:a504).
const MAX_LINE_WORDS: usize = 50;
/// Spectator counts above this are reset to 0 by the sidebar.
const MAX_SPECTATORS: i32 = 200_000;

/// Offsets of the [`NewsRecord`] fields that are reached through the
/// record's byte image (see [`RecordImage`]).
mod rec {
    pub const DEFICIT_OWN: usize = 0x00;
    pub const DEFICIT_OPP: usize = 0x01;
    pub const LEAD_OWN: usize = 0x02;
    pub const LEAD_OPP: usize = 0x03;
    pub const GOALS_OWN: usize = 0x04;
    pub const GOALS_OPP: usize = 0x05;
    pub const N_EVENTS: usize = 0x06;
    pub const GOAL_DIFF: usize = 0x09;
    pub const MIN_DIFF: usize = 0x0a;
    pub const MAX_DIFF: usize = 0x0b;
    pub const RESULT: usize = 0x11;
    pub const GOAL_MINUTE: usize = 0x13;
    pub const GOAL_PLAYER: usize = 0x27;
    pub const CHANCE_MINUTE: usize = 0x3b;
    pub const CHANCE_PLAYER: usize = 0x4f;
    pub const GOAL_ARG4: usize = 0x63;
    pub const CHANCE_ARG4: usize = 0x77;
    pub const N_GOALS: usize = 0x8b;
    pub const N_CHANCES: usize = 0x8c;
    pub const YELLOW: usize = 0x8d;
    pub const RED: usize = 0x93;
    pub const N_YELLOW: usize = 0x94;
}

/// The byte image of all four records (FAR:90ca..9331), positioned at the
/// record of one manager. The original indexes the record arrays without
/// bounds checks, so an index past one record reaches the next.
struct RecordImage {
    bytes: Vec<u8>,
    base: usize,
}

impl RecordImage {
    fn of(rt: &Runtime, m: u8) -> Self {
        RecordImage {
            bytes: rt.news_records.to_bytes(),
            base: usize::from(m) * NewsRecord::LEN,
        }
    }

    // TODO(re): past the last record the original reaches FAR:9332.. (the
    // bytes before and inside match_history); those read as 0 here and
    // writes are dropped.
    fn get(&self, off: usize) -> u8 {
        self.bytes.get(self.base + off).copied().unwrap_or(0)
    }

    fn set(&mut self, off: usize, value: u8) {
        if let Some(b) = self.bytes.get_mut(self.base + off) {
            *b = value;
        }
    }

    fn bump(&mut self, off: usize) {
        self.set(off, self.get(off).wrapping_add(1));
    }

    fn store(self, rt: &mut Runtime) {
        rt.news_records = Layout::from_bytes(&self.bytes).expect("image of all records");
    }
}

/// Logs a match event of manager `m`'s match into the newspaper record:
/// `kind` is one of the `EVENT_*` codes, `player` the 0-based squad index of
/// manager `m`'s team or [`OPPONENT`]. A goal updates the score, the goal
/// difference, the result and the biggest deficit and lead; goals, chances
/// and yellow cards are appended to their arrays without a bound check, so
/// overflowing entries overwrite the fields that follow.
///
/// Origin: BMMAIN.EXE 3e3a:223e (news_record_match_event)
pub fn news_record_match_event(
    rt: &mut Runtime,
    m: u8,
    kind: u8,
    player: u8,
    arg4: u8,
    minute: u8,
) {
    let mut r = RecordImage::of(rt, m);
    if kind < EVENT_RED_CARD {
        r.bump(rec::N_EVENTS);
    }
    if kind == EVENT_GOAL {
        r.bump(if player != OPPONENT {
            rec::GOALS_OWN
        } else {
            rec::GOALS_OPP
        });
        let d = r.get(rec::GOALS_OWN).wrapping_sub(r.get(rec::GOALS_OPP)) as i8;
        r.set(rec::GOAL_DIFF, d as u8);
        r.set(rec::RESULT, u8::from(d > 0) + 2 * u8::from(d < 0));
        if (r.get(rec::MIN_DIFF) as i8) > d {
            r.set(rec::MIN_DIFF, d as u8);
            r.set(rec::DEFICIT_OWN, r.get(rec::GOALS_OWN));
            r.set(rec::DEFICIT_OPP, r.get(rec::GOALS_OPP));
        }
        if (r.get(rec::MAX_DIFF) as i8) < d {
            r.set(rec::MAX_DIFF, d as u8);
            r.set(rec::LEAD_OWN, r.get(rec::GOALS_OWN));
            r.set(rec::LEAD_OPP, r.get(rec::GOALS_OPP));
        }
        let k = usize::from(r.get(rec::N_GOALS));
        r.set(rec::GOAL_MINUTE + k, minute);
        r.set(rec::GOAL_PLAYER + k, player);
        r.set(rec::GOAL_ARG4 + k, arg4);
        r.bump(rec::N_GOALS);
    }
    if kind == EVENT_CHANCE {
        let k = usize::from(r.get(rec::N_CHANCES));
        r.set(rec::CHANCE_MINUTE + k, minute);
        r.set(rec::CHANCE_PLAYER + k, player);
        r.set(rec::CHANCE_ARG4 + k, arg4);
        r.bump(rec::N_CHANCES);
    }
    if kind == EVENT_RED_CARD {
        r.set(rec::RED, player.wrapping_add(1));
    }
    if kind == EVENT_YELLOW_CARD {
        let k = usize::from(r.get(rec::N_YELLOW));
        r.set(rec::YELLOW + k, player);
        r.bump(rec::N_YELLOW);
    }
    r.store(rt);
}

/// Match rating of a squad player (+0x15, reset to 0 per match, see
/// `docs/re/logic/match.md`).
fn match_rating(p: &SquadPlayer) -> i8 {
    p.match_rating as i8
}

/// Post-match grade of squad slot `i` of manager `m`: FAR:4d3c
/// `[((m << 5) + i) << 3]`, byte 0 of the cup goal-minute slot at that
/// address.
fn grade_mut(rt: &mut Runtime, m: usize, i: usize) -> &mut u8 {
    &mut rt.cup_goal_minutes[m][i][0]
}

/// Squad slot `flat` counted over all squads (FAR:774a + 0x34 * flat).
// TODO(re): the original reads outside FAR:774a for a flat index outside the
// five squads; such a slot names nobody here.
fn flat_slot(state: &GameState, flat: i16) -> Option<&SquadPlayer> {
    let flat = usize::try_from(flat).ok()?;
    state
        .squads
        .get(flat / SQUAD_SIZE)
        .map(|squad| &squad[flat % SQUAD_SIZE])
}

/// Name of the player in squad slot `flat` (see [`flat_slot`]).
fn flat_slot_name(state: &GameState, flat: i16) -> &[u8] {
    flat_slot(state, flat).map_or(b"", |p| state.players[usize::from(p.player_id)].name())
}

/// Name of the player in the 1-based squad slot `slot1` of manager `m`.
fn squad_name_1based(state: &GameState, m: u8, slot1: i8) -> &[u8] {
    flat_slot_name(
        state,
        i16::from(slot1) + i16::from(m) * SQUAD_SIZE as i16 - 1,
    )
}

/// Grades every listed player of manager `m` who played (lineup position
/// 1..=11) from 1 (best) to 6, and turns a pending suspension into a
/// one-match suspension. No random numbers.
///
/// Origin: BMMAIN.EXE 3e3a:23aa (match_rate_players_and_newspaper, loop body)
fn rate_players(state: &mut GameState, rt: &mut Runtime, m: usize) {
    let n = usize::from(squad_count_listed(state, m));
    for i in 0..n {
        let p = &mut state.squads[m][i];
        if p.suspended == SUSPENSION_PENDING {
            p.suspended = 1;
            p.status |= 1;
        }
        if p.lineup_position >= FIRST_SUBSTITUTE || p.lineup_position == 0 {
            continue;
        }
        let a = player_role_mismatch(state, m as u8, i as u8);
        let b = player_position_mismatch(state, m as u8, i as u8);
        let p = &state.squads[m][i];
        let f = i16::from(match_rating(p).clamp(-12, 36));
        let q = ldiv(long_abs_diff(30, i32::from(p.skills[2])), 6) as i16;
        let mut v = ((i16::from(p.skills[0]) + i16::from(p.skills[1])) / -33 - q + 12) * 2;
        let mut c = (6 + (-(f + 12)) / 6) * 4;
        c += i16::from(p.exhaustion / 33);
        c += b / 5;
        c += a;
        v += c;
        let g = ((v / 11) as i8).clamp(1, 6);
        *grade_mut(rt, m, i) = g as u8;
    }
}

/// Inputs of the newspaper that the original reads from globals the save
/// does not hold.
pub struct NewsEnv<'a> {
    /// DS:079c / DS:07b2 / DS:224e state of `format_long_thousands`.
    pub fmt: NumberFormat,
    /// `rastport_text_width` in game font 2.
    pub text_width: &'a dyn Fn(&[u8]) -> i16,
}

/// How a laid-out line is drawn (`draw_line` of `news_text_flow`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineStyle {
    /// The whole line in one colour (`ui_draw_text_at`, colour 0).
    Plain,
    /// "label:" in colour 6 and the rest in colour 0x0e
    /// (`news_draw_label_value_line`, split with [`label_value_split`]).
    LabelValue,
}

/// One laid-out text line at its screen position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewsLine {
    /// The text, justified by inserted blanks.
    pub text: Vec<u8>,
    /// Left edge.
    pub x: i16,
    /// Top edge.
    pub y: i16,
    /// How the line is drawn.
    pub style: LineStyle,
}

/// The content of one newspaper page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Newspaper {
    /// PIC number of the photo, 200..=229.
    pub photo: u16,
    /// Index of the headline template.
    pub headline_index: u8,
    /// Selected body templates in page order (FAR:2e7e).
    pub body_templates: Vec<u8>,
    /// The two headline lines, drawn centred at y 0x29 and 0x33 in font 1.
    pub headline: [Vec<u8>; 2],
    /// Body lines (font 2).
    pub body: Vec<NewsLine>,
    /// Sidebar lines (font 2).
    pub sidebar: Vec<NewsLine>,
}

/// Splits a [`LineStyle::LabelValue`] line into the label (up to and
/// including the first `:`) and the value (after the character that follows
/// the `:`, which the original overwrites). `None`: no `:`, the whole line is
/// drawn in colour 0x0e.
///
/// Origin: BMMAIN.EXE 3e3a:060a (news_draw_label_value_line)
pub fn label_value_split(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let i = line.iter().position(|&c| c == b':')?;
    let value = line.get(i + 2..).unwrap_or(b"");
    Some((&line[..=i], value))
}

/// The word-wrapping, fully justified text layout: the BSS state of
/// `news_text_flow` (DS:a3a6 line, DS:a536 x, DS:a3a4 y, DS:a538 word count,
/// DS:a504 word ends) plus the lines it has drawn.
struct TextFlow<'a> {
    text_width: &'a dyn Fn(&[u8]) -> i16,
    line: Vec<u8>,
    x: i16,
    y: i16,
    words: i8,
    word_ends: [u8; MAX_LINE_WORDS],
    lines: Vec<NewsLine>,
}

impl<'a> TextFlow<'a> {
    fn new(text_width: &'a dyn Fn(&[u8]) -> i16) -> Self {
        TextFlow {
            text_width,
            line: Vec::new(),
            x: 0,
            y: 0,
            words: 0,
            word_ends: [0; MAX_LINE_WORDS],
            lines: Vec::new(),
        }
    }

    /// Mode 1: starts an empty line at (`x`, `y`).
    ///
    /// Origin: BMMAIN.EXE 3e3a:06df (news_text_flow, mode 1)
    fn start(&mut self, x: i16, y: i16) {
        self.x = x;
        self.y = y;
        self.words = 0;
        self.line.clear();
    }

    /// Mode 2: draws the pending line without a trailing `,` and resets the
    /// position to (0, 0).
    ///
    /// Origin: BMMAIN.EXE 3e3a:06df (news_text_flow, mode 2)
    fn flush(&mut self, width: i16, two_col: u8) {
        // An empty line tests DS:a3a5 instead, the high byte of y, which is
        // never ','.
        if self.line.last() == Some(&b',') {
            self.line.pop();
        }
        self.draw(width, two_col);
        self.start(0, 0);
    }

    /// Mode 0: appends `word`. When the line with the word would be wider
    /// than `width`, the line is first justified to `width`, drawn, and a
    /// new line begins 7 pixels lower. Returns y.
    ///
    /// Origin: BMMAIN.EXE 3e3a:06df (news_text_flow, mode 0)
    fn word(&mut self, word: &[u8], width: i16, two_col: u8) -> i16 {
        let mut candidate = self.line.clone();
        if !candidate.is_empty() {
            candidate.push(b' ');
        }
        candidate.extend_from_slice(word);
        if (self.text_width)(&candidate) > width {
            self.justify(width);
            self.draw(width, two_col);
            self.y = self.y.wrapping_add(LINE_HEIGHT);
            self.line.clear();
            self.words = 0;
        }
        if !self.line.is_empty() {
            self.line.push(b' ');
        }
        self.line.extend_from_slice(word);
        // TODO(re): a 51st word on one line writes DS:a536 (x) in the
        // original; it is dropped here.
        if let Some(end) = self.word_ends.get_mut(self.words as usize) {
            *end = self.line.len() as u8;
        }
        self.words = self.words.wrapping_add(1);
        self.y
    }

    /// Inserts blanks after the words of the line, round-robin from the
    /// left, until the line is `width` pixels wide. The end of the last word
    /// is no gap.
    fn justify(&mut self, width: i16) {
        let mut i: i8 = 0;
        if self.words != 0 {
            self.words -= 1;
        }
        while (self.text_width)(&self.line) < width {
            let pos = usize::from(self.word_ends[i as usize]);
            i += 1;
            // TODO(re): when the first word is wider than the line, the
            // original inserts blanks at a stale DS:a504 position past the
            // string end; justifying stops instead.
            if pos > self.line.len() {
                break;
            }
            self.line.insert(pos, b' ');
            for j in i..self.words {
                let end = &mut self.word_ends[j as usize];
                *end = end.wrapping_add(1);
            }
            if self.words <= i {
                i = 0;
            }
        }
    }

    fn draw(&mut self, width: i16, two_col: u8) {
        let style = if width == SIDEBAR_WIDTH && two_col != 0 {
            LineStyle::LabelValue
        } else {
            LineStyle::Plain
        };
        self.lines.push(NewsLine {
            text: self.line.clone(),
            x: self.x,
            y: self.y,
            style,
        });
    }
}

/// Flows `s` word by word (split at exactly one `' '` or `0xa0`) and returns
/// the low byte of the last y.
///
/// Origin: BMMAIN.EXE 3e3a:0d33 (news_flow_words)
fn news_flow_words(flow: &mut TextFlow<'_>, s: &[u8], width: i16) -> u8 {
    // TODO(re): uninitialised in the original for an empty string.
    let mut y = 0;
    let mut p = 0;
    while p < s.len() {
        let start = p;
        while p < s.len() && s[p] != b' ' && s[p] != 0xa0 {
            p += 1;
        }
        y = flow.word(&s[start..p], width, 0) as u8;
        if p < s.len() {
            p += 1;
        }
    }
    y
}

/// What `news_expand_template` reads besides the template: manager `m`'s
/// record, the selectors FAR:579b[1..=5] and the formatter globals.
struct Expander<'a> {
    state: &'a GameState,
    record: &'a NewsRecord,
    rng: &'a mut Rng,
    m: u8,
    fmt: NumberFormat,
    selectors: [u8; 6],
}

/// Byte `p` of `image`; outside the image reads as NUL.
fn byte_at(image: &[u8], p: usize) -> u8 {
    image.get(p).copied().unwrap_or(0)
}

/// From the `%` of a nested `%xNM` group, the position of the next `%`
/// after the selector digits (the group's end marker when its alternatives
/// hold no other code).
fn nested_group_end(image: &[u8], p: usize) -> usize {
    let mut p = p + 4;
    while p < image.len() && image[p] != b'%' {
        p += 1;
    }
    p
}

/// From the start of an alternative, the position of the `#` that ends it.
fn skip_alternative(image: &[u8], mut p: usize) -> usize {
    loop {
        if byte_at(image, p) == b'%' && byte_at(image, p + 1) == b'x' {
            p = nested_group_end(image, p);
        }
        p += 1;
        if p >= image.len() || image[p] == b'#' {
            return p;
        }
    }
}

/// Number of alternatives of the group whose first alternative starts at
/// `first`, counted with the original's skip rule: the group ends where an
/// alternative's `#` is followed by `%`.
fn alternative_count(image: &[u8], first: usize) -> u8 {
    let mut n = 0u8;
    let mut p = first;
    loop {
        p = skip_alternative(image, p);
        if p >= image.len() {
            return n;
        }
        p += 1;
        n = n.saturating_add(1);
        if byte_at(image, p) == b'%' {
            return n;
        }
    }
}

impl Expander<'_> {
    fn number(&self, value: u8) -> Vec<u8> {
        format_long_thousands(i32::from(value as i8), self.fmt)
    }

    /// Selector `n` of a `%xN` group. `D` (0x14) reads FAR:57af, which is
    /// FAR:579b + 0x14.
    fn selector(&self, n: u8) -> u8 {
        match n {
            1..=5 => self.selectors[usize::from(n)],
            0x14 => self.state.competition_slots[5],
            _ => unreachable!("selector {n} does not occur in the templates"),
        }
    }

    /// Appends the expansion of the template at `image[p..]` to `out` and
    /// returns where it stopped: at `#`, at NUL, or just after `%e`.
    ///
    /// Codes: `%0`..`%5` the score fields, `%9` the spectators, `%a`/`%b`
    /// the clubs, `%c`/`%d` the hero and the loser, `%t` the manager, `%e`
    /// stop. `%xNM` opens a group of `#`-separated alternatives ending with
    /// `#%`: `N = 0` picks one of 1..=M at random, otherwise selector `N`
    /// picks. The chosen alternative is expanded recursively. Any other code
    /// is dropped.
    ///
    /// Origin: BMMAIN.EXE 3e3a:0964 (news_expand_template)
    fn expand(&mut self, image: &[u8], mut p: usize, out: &mut Vec<u8>) -> usize {
        let state = self.state;
        let m = self.m;
        let rec = self.record;
        loop {
            let c = byte_at(image, p);
            if c == b'#' || c == 0 {
                return p;
            }
            if c != b'%' {
                out.push(c);
                p += 1;
                continue;
            }
            p += 1;
            match byte_at(image, p) {
                b'0' => out.extend(self.number(rec.goals_own)),
                b'1' => out.extend(self.number(rec.goals_opp)),
                b'2' => out.extend(self.number(rec.deficit_own)),
                b'3' => out.extend(self.number(rec.deficit_opp)),
                b'4' => out.extend(self.number(rec.lead_own)),
                b'5' => out.extend(self.number(rec.lead_opp)),
                b'9' => out.extend(format_long_thousands(rec.spectators, self.fmt)),
                b'a' => out.extend_from_slice(state.clubs[usize::from(rec.club_a)].name()),
                b'b' => out.extend_from_slice(state.clubs[usize::from(rec.club_b)].name()),
                b'c' => out.extend_from_slice(squad_name_1based(state, m, rec.hero as i8)),
                b'd' => out.extend_from_slice(squad_name_1based(state, m, rec.loser as i8)),
                b't' => out.extend_from_slice(state.managers[usize::from(m)].name()),
                b'e' => return p + 1,
                b'x' => {
                    p = self.alternatives(image, p, out);
                    continue;
                }
                _ => {}
            }
            p += 1;
        }
    }

    /// Expands the `%xNM` group whose `x` is at `p` and returns the position
    /// after its end marker.
    fn alternatives(&mut self, image: &[u8], mut p: usize, out: &mut Vec<u8>) -> usize {
        p += 1;
        let n = byte_at(image, p).wrapping_sub(b'0');
        p += 1;
        let mut s = if n == 0 {
            let count = byte_at(image, p).wrapping_sub(b'0');
            p += 1;
            self.rng.random_range(1, i16::from(count)) as u8
        } else {
            p += 1;
            self.selector(n)
        };
        // TODO(re): news.md §10 q1. A selector of 0, or one above the group's
        // alternative count (`%xD` with FAR:57af = 0x80), makes the original
        // skip past the group into whatever follows; the first alternative
        // is taken instead.
        if s == 0 || s > alternative_count(image, p) {
            s = 1;
        }
        for _ in 1..s {
            p = skip_alternative(image, p) + 1;
        }
        p = self.expand(image, p, out) + 1;
        loop {
            if byte_at(image, p) == b'%' && byte_at(image, p - 1) == b'#' {
                return p + 1;
            }
            if p >= image.len() {
                return p;
            }
            if image[p] == b'%' && byte_at(image, p + 1) == b'x' {
                p = nested_group_end(image, p);
            }
            p += 1;
        }
    }
}

/// `random_range` over body template group `g`.
fn pick(rng: &mut Rng, group_start: &[i16; 25], g: usize) -> u8 {
    let lo = group_start[g];
    rng.random_range(lo, lo + BODY_GROUP_SIZES[g] - 1) as u8
}

/// The headline candidates: group `k` covers the next
/// `HEADLINE_GROUP_SIZES[k]` templates, and every taken group may replace
/// the chosen headline (`random_chance_pick`).
struct HeadlinePick {
    headline: u8,
    flag: u8,
    base: u8,
    group: usize,
}

impl HeadlinePick {
    fn next(&mut self, rng: &mut Rng, taken: bool, n: u8) {
        let size = HEADLINE_GROUP_SIZES[self.group] as u8;
        if taken {
            let hi = self.base + size - 1;
            self.headline = rng.chance_pick(n, self.headline, &mut self.flag, self.base, hi);
        }
        self.base += size;
        self.group += 1;
    }
}

/// What the match analysis found besides the record fields.
struct Analysis {
    early_own: bool,
    early_opp: bool,
    late_decider: bool,
    own_chances: i16,
    dominance: i32,
    selectors: [u8; 6],
}

/// Adds the weights of an event at minute `t0` to the minutes around it,
/// negative for the opponent.
fn add_heat(heat: &mut [i16; HEAT_MINUTES], t0: u8, opponent: bool) {
    for (k, &w) in HEAT_WEIGHTS.iter().enumerate() {
        let t = i16::from(t0) - 5 + k as i16;
        if t <= 0 || t >= HEAT_MINUTES as i16 {
            continue;
        }
        let h = &mut heat[t as usize];
        *h = if opponent {
            h.wrapping_sub(w)
        } else {
            h.wrapping_add(w)
        };
    }
}

/// The match analysis of `newspaper_generate_and_show` (news.md §4.2): the
/// heat map of goals and chances, the hero and loser with their grade
/// changes, the event-count clamp and the five selectors.
///
/// Origin: BMMAIN.EXE 3e3a:0ea3 (newspaper_generate_and_show)
fn analyse_match(state: &GameState, rt: &mut Runtime, rng: &mut Rng, m: u8, param: u8) -> Analysis {
    let mi = usize::from(m);
    let img = RecordImage::of(rt, m);
    let mut heat = [0i16; HEAT_MINUTES];
    let (mut own_goals, mut opp_goals) = (0i16, 0i16);
    let (mut early_own, mut early_opp, mut late_decider) = (false, false, false);
    let n_goals = usize::from(img.get(rec::N_GOALS));
    for i in 0..n_goals {
        let t0 = img.get(rec::GOAL_MINUTE + i);
        let opponent = img.get(rec::GOAL_PLAYER + i) == OPPONENT;
        let late = n_goals - i - 1 == 0 && t0 > 0x50 && param == 0;
        if !opponent {
            if t0 < 8 && !early_opp {
                early_own = true;
            }
            if late && (own_goals == opp_goals || own_goals - opp_goals == -1) {
                late_decider = true;
            }
            own_goals += 1;
        } else {
            if t0 < 8 && !early_own {
                early_opp = true;
            }
            if late && (own_goals == opp_goals || opp_goals - own_goals == -1) {
                late_decider = true;
            }
            opp_goals += 1;
        }
        add_heat(&mut heat, t0, opponent);
    }

    let mut hero = 0u8;
    let mut loser = 0u8;
    let n = usize::from(squad_count_listed(state, mi));
    for j in 0..n {
        let p = &state.squads[mi][j];
        let f = match_rating(p);
        let position = state.players[usize::from(p.player_id)].position_value;
        let grade = grade_mut(rt, mi, j);
        if position != 0 && f > 25 {
            hero = j as u8 + 1;
            let g = i16::from(*grade) - rng.random_range(1, 2);
            *grade = g.max(1) as u8;
        }
        if f < -15 && loser == 0 {
            loser = j as u8 + 1;
            let g = grade.wrapping_add(rng.random_range(1, 2) as u8);
            *grade = g.min(6);
        }
    }

    // The original also counts the opponent's chances (bp-0x150) but
    // never reads them.
    let mut own_chances = 0i16;
    for i in 0..usize::from(img.get(rec::N_CHANCES)) {
        let t0 = img.get(rec::CHANCE_MINUTE + i);
        let opponent = img.get(rec::CHANCE_PLAYER + i) == OPPONENT;
        add_heat(&mut heat, t0, opponent);
        if !opponent && (1..HEAT_MINUTES as u8).contains(&t0) {
            own_chances += 1;
        }
    }
    let dominance = ldiv(heat.iter().map(|&h| i32::from(h)).sum(), 120).min(2);

    let r = &mut rt.news_records[mi];
    r.hero = hero;
    r.loser = loser;
    r.n_events = r.n_events.clamp(3, 10);
    let r = r.clone();
    let quality = i8::from(r.result == 1) + (r.n_events - 3) / 2 + 1;
    let strength = |club: u8| {
        let sum = team_strength_value(state, club, 1, 0)
            .wrapping_add(team_strength_value(state, club, 0, 0))
            .wrapping_add(team_strength_value(state, club, 2, 0));
        sum.wrapping_mul(2) / 15
    };
    let (sa, sb) = (strength(r.club_a), strength(r.club_b));
    let away = r.spectators == 0;
    let satisfied =
        r.result == 1 || (r.result == 0 && sb > sa) || (r.result == 0 && away && sa + 10 < sb);
    // The coin is only drawn when the heat does not favour the own team.
    let deserved = if dominance > 0 || (rng.random_range(0, 1) != 0 && dominance == 0) {
        1
    } else {
        2
    };
    Analysis {
        early_own,
        early_opp,
        late_decider,
        own_chances,
        dominance,
        selectors: [
            0,
            u8::from(away) + 1,
            quality.min(4) as u8,
            r.result + 1,
            if satisfied { 2 } else { 1 },
            deserved,
        ],
    }
}

/// The body and headline template selection (news.md §4.3 and §4.4).
/// Returns the body templates and the headline index.
///
/// Origin: BMMAIN.EXE 3e3a:0ea3 (newspaper_generate_and_show)
fn select_templates(r: &NewsRecord, a: &Analysis, rng: &mut Rng) -> (Vec<u8>, u8) {
    let mut group_start = [0i16; 25];
    let mut s = 0;
    for (start, &size) in group_start.iter_mut().zip(&BODY_GROUP_SIZES) {
        *start = s;
        s += size;
    }
    let gs = &group_start;
    let result = r.result;
    let mut body = vec![pick(rng, gs, 0)];
    body.push(pick(rng, gs, if r.spectators != 0 { 1 } else { 2 }));
    if rng.random_range(0, 1) != 0 && r.n_yellow > 2 {
        body.push(pick(rng, gs, 3));
    }
    if rng.random_range(0, 1) != 0 && r.n_yellow == 0 {
        body.push(pick(rng, gs, 4));
    }
    if r.hero != 0 && result == 1 {
        body.push(pick(rng, gs, 10));
    }
    if r.loser != 0 && result == 2 {
        body.push(pick(rng, gs, 11));
    }
    body.push(pick(rng, gs, 5));
    let steps = [
        (a.late_decider, 18),
        (a.own_chances > 3 && r.goal_diff < 2, 12),
        (a.early_opp, 16),
        (a.early_own, 17),
        (a.own_chances < 2 && result == 1, 13),
        (result == 2, 19),
        (result == 1, 7),
    ];
    for (taken, g) in steps {
        if taken {
            body.push(pick(rng, gs, g));
        }
    }

    let d = r.goal_diff;
    let s5 = a.selectors[5];
    let ad = long_abs_diff(0, a.dominance);
    let mut h = HeadlinePick {
        headline: 0,
        flag: 0,
        base: 0,
        group: 0,
    };
    h.next(rng, d > 3, 5);
    h.next(rng, d > 0 && d < 2, 5);
    h.next(rng, d > 0 && d < 4, 5);
    h.next(rng, d < -3, 5);
    h.next(rng, d < 0 && d > -2, 5);
    h.next(rng, d < 0 && d > -4, 5);
    h.next(rng, d == 0, 2);
    h.next(rng, r.sold_out != 0, 3);
    if r.sold_out != 0 {
        body.push(pick(rng, gs, 6));
    }
    if r.min_diff != 0 && result != 2 {
        body.push(pick(rng, gs, 14));
    }
    if r.max_diff != 0 && result != 1 && d > -2 {
        body.push(pick(rng, gs, 15));
    }
    h.next(rng, r.min_diff != 0 && result == 1, 5);
    h.next(rng, r.max_diff != 0 && result == 2, 5);
    // Groups 10 and 11 talk about a lost lead and a comeback; their
    // conditions look swapped, as in the original (news.md §10 q2).
    h.next(rng, r.min_diff < -1 && result == 0, 4);
    h.next(rng, r.max_diff > 1 && result == 0, 4);
    h.next(rng, r.hero != 0 && result == 1, 4);
    h.next(rng, r.n_events > 7 && ad < 3, 3);
    h.next(rng, r.n_events < 4 && ad < 3, 3);
    h.next(rng, r.loser != 0 && result == 2, 4);
    h.next(rng, s5 == 1 && result == 2, 4);
    h.next(rng, s5 == 2 && result == 1, 4);
    h.next(rng, s5 == 1 && result == 1, 3);
    if s5 == 1 && result == 1 {
        body.push(pick(rng, gs, 8));
    }
    h.next(rng, s5 == 2 && result == 2, 3);
    if s5 == 2 && result == 2 {
        body.push(pick(rng, gs, 9));
    }
    h.next(rng, ad < 2 && result == 0, 4);
    h.next(rng, s5 == 2 && result == 0, 3);
    h.next(rng, s5 == 1 && result == 0, 3);
    (body, h.headline)
}

/// Expands the selected body templates into the text flow at (13, 60),
/// width 180, until the page is full.
///
/// Origin: BMMAIN.EXE 3e3a:0c0b (newspaper_draw_body)
fn layout_body(exp: &mut Expander<'_>, flow: &mut TextFlow<'_>, templates: &[u8]) {
    // TODO(re): news.md §10 q3, the original flows eight random filler lines
    // (DS:92de / DS:9298) instead when the long at DS:2252 differs from the
    // one at DS:224e; its writer is untraced, the check is taken to pass.
    flow.start(0x0d, 0x3c);
    for &template in templates {
        let mut text = Vec::new();
        let pos = usize::from(BODY_OFFSETS[usize::from(template)]);
        exp.expand(BODY_IMAGE, pos, &mut text);
        if news_flow_words(flow, &text, BODY_WIDTH) > BODY_LAST_Y {
            break;
        }
    }
    flow.flush(BODY_WIDTH, 0);
}

/// Lays out the sidebar: the players who played with their grades, the
/// goals with the running score from the home side, and the yellow and red
/// cards. Spectator counts above 200000 are reset to 0 first.
///
/// Origin: BMMAIN.EXE 3e3a:000e (newspaper_draw_sidebar)
fn layout_sidebar(
    state: &GameState,
    rt: &mut Runtime,
    m: u8,
    fmt: NumberFormat,
    flow: &mut TextFlow<'_>,
) {
    let mi = usize::from(m);
    // TODO(re): news.md §10 q8, 2a58:20b2 stores spectators + 200000 for
    // some venue; the analysis counts it as a home match before this reset.
    if rt.news_records[mi].spectators > MAX_SPECTATORS {
        rt.news_records[mi].spectators = 0;
    }
    let rt = &*rt;
    let img = RecordImage::of(rt, m);
    let r = &rt.news_records[mi];
    let w = SIDEBAR_WIDTH;
    let base = i16::from(m) * SQUAD_SIZE as i16;
    let number = |v: u8| format_long_thousands(i32::from(v), fmt);

    flow.start(0x0d, 0xbb);
    let mut label = state.clubs[usize::from(state.managers[mi].team)]
        .name()
        .to_vec();
    label.extend_from_slice(b": ");
    flow.word(&label, w, 6);
    for i in 0..usize::from(squad_count_listed(state, mi)) {
        let p = &state.squads[mi][i];
        if p.lineup_position == 0 || p.lineup_position >= FIRST_SUBSTITUTE {
            continue;
        }
        let mut s = state.players[usize::from(p.player_id)].name().to_vec();
        s.extend_from_slice(b" (");
        s.extend(number(rt.cup_goal_minutes[mi][i][0]));
        s.extend_from_slice(b"),");
        flow.word(&s, w, 0x0e);
    }
    flow.flush(w, 6);

    flow.start(0x0d, 0xd1);
    flow.word(SIDEBAR_GOALS, w, 6);
    let n_goals = usize::from(img.get(rec::N_GOALS));
    if n_goals == 0 {
        flow.word(SIDEBAR_NO_GOALS, w, 0x0e);
    }
    let (mut home_goals, mut away_goals) = (0u8, 0u8);
    let home = r.spectators != 0;
    for i in 0..n_goals {
        let scorer = img.get(rec::GOAL_PLAYER + i);
        let own = scorer != OPPONENT;
        let name = if own {
            flat_slot_name(state, base + i16::from(scorer))
        } else {
            b""
        };
        if own == home {
            home_goals = home_goals.wrapping_add(1);
        } else {
            away_goals = away_goals.wrapping_add(1);
        }
        let mut s = number(home_goals);
        s.push(b':');
        s.extend(number(away_goals));
        s.push(b' ');
        s.extend_from_slice(name);
        s.extend_from_slice(b" (");
        s.extend(number(img.get(rec::GOAL_MINUTE + i)));
        s.extend_from_slice(b".MIN)");
        if i + 1 != n_goals {
            s.push(b',');
        }
        flow.word(&s, w, 0);
    }
    flow.flush(w, 0);

    flow.start(0x0d, 0xe6);
    flow.word(SIDEBAR_YELLOW, w, 6);
    let n_yellow = usize::from(img.get(rec::N_YELLOW));
    if n_yellow == 0 {
        flow.word(SIDEBAR_NONE, w, 0x0e);
    }
    for i in 0..n_yellow {
        let mut s = flat_slot_name(state, base + i16::from(img.get(rec::YELLOW + i))).to_vec();
        if i + 1 != n_yellow {
            s.push(b',');
        }
        flow.word(&s, w, 0x0e);
    }
    flow.flush(w, 6);

    flow.start(0x0d, 0xed);
    flow.word(SIDEBAR_RED, w, 6);
    let red = if r.red == 0 {
        SIDEBAR_NONE
    } else {
        flat_slot_name(state, base + i16::from(r.red) - 1)
    };
    flow.word(red, w, 0x0e);
    flow.flush(w, 6);
}

/// The newspaper of manager `m` after a match, or `None` when the newspaper
/// option is off (then neither the grades nor the random stream change).
///
/// In the original's order: the photo, the match analysis with the hero and
/// loser grade changes, the body and headline selection, the headline, the
/// body (stopping at a full page) and the sidebar. `param != 0` (a cup
/// round) suppresses the late-decider analysis.
///
/// Origin: BMMAIN.EXE 3e3a:0ea3 (newspaper_generate_and_show); the headline
/// lines as in 3e3a:0dba (newspaper_draw_headline)
pub fn newspaper_generate(
    state: &GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    m: u8,
    param: u8,
    env: &NewsEnv<'_>,
) -> Option<Newspaper> {
    if state.options[NEWSPAPER_OPTION] == 0 {
        return None;
    }
    let photo = FIRST_PHOTO + rng.random_range(0, 0x1d) as u16;
    let analysis = analyse_match(state, rt, rng, m, param);
    let record = rt.news_records[usize::from(m)].clone();
    let (body_templates, headline_index) = select_templates(&record, &analysis, rng);

    let mut flow = TextFlow::new(env.text_width);
    let mut exp = Expander {
        state,
        record: &record,
        rng,
        m,
        fmt: env.fmt,
        selectors: analysis.selectors,
    };
    let pos = usize::from(HEADLINE_OFFSETS[usize::from(headline_index)]);
    let mut first = Vec::new();
    let rest = exp.expand(HEADLINE_IMAGE, pos, &mut first);
    let mut second = Vec::new();
    exp.expand(HEADLINE_IMAGE, rest, &mut second);
    layout_body(&mut exp, &mut flow, &body_templates);
    let body = std::mem::take(&mut flow.lines);
    layout_sidebar(state, rt, m, env.fmt, &mut flow);
    Some(Newspaper {
        photo,
        headline_index,
        body_templates,
        headline: [first, second],
        body,
        sidebar: flow.lines,
    })
}

/// After a match day: for every human manager whose club played, makes that
/// manager the saved manager (FAR:5358), grades the squad and, when
/// `param == 0`, generates the manager's newspaper. Returns the papers in manager order; the
/// original shows each one before grading the next manager, which reads
/// nothing the display writes.
///
/// Origin: BMMAIN.EXE 3e3a:23aa (match_rate_players_and_newspaper)
pub fn match_rate_players_and_newspaper(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    param: u8,
    env: &NewsEnv<'_>,
) -> Vec<Newspaper> {
    let mut papers = Vec::new();
    for m in 0..state.manager_count.min(MANAGER_SLOTS as u8) {
        if rt.manager_plays_now[usize::from(m)] == 0 {
            continue;
        }
        state.saved_manager = m as i8;
        rate_players(state, rt, usize::from(m));
        if param == 0 {
            papers.extend(newspaper_generate(state, rt, rng, m, 0, env));
        }
    }
    papers
}

/// An argument of a message template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageArg<'a> {
    /// Consumed by `$`.
    Text(&'a [u8]),
    /// Consumed by `#`, formatted without separators.
    Number(i32),
}

/// A message stored by [`message_post`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostedMessage {
    /// Identity of the message (the original's far pointer), see
    /// [`message_delete`].
    pub pointer: u32,
    /// The text without the NUL; `^` is the line break.
    pub text: Vec<u8>,
}

/// Builds a message from template `tpl` (0..=5), dated 0..3 days back, and
/// inserts it at position 0 of the inbox of the saved manager (FAR:5358).
/// When `show_now` is false, the manager's new-message flag is set; when it
/// is true the caller shows the message at once.
///
/// `fmt` is the formatter state: the original turns separators off at entry
/// and the pad width to 1 after the day, and never restores either. A new
/// message's identity is one above every stored one, so it is never 0.
///
/// # Panics
/// When `args` has fewer entries than the template's `$` and `#`, or an
/// entry of the wrong kind (a caller bug).
///
/// Origin: BMMAIN.EXE 4091:0190 (message_post)
pub fn message_post(
    state: &mut GameState,
    rng: &mut Rng,
    fmt: &mut NumberFormat,
    show_now: bool,
    tpl: usize,
    args: &[MessageArg<'_>],
) -> PostedMessage {
    fmt.no_separators = true;
    let mut day = (state.day_of_month as i16).wrapping_sub(rng.random_range(0, 3));
    let month_len = i16::from(MONTH_LENGTHS[state.month as i16 as usize]);
    if month_len < day {
        day = month_len;
    }
    let mut month = state.month as i8;
    let mut year = state.year as i16;
    if day < 1 {
        month -= 1;
        if month < 0 {
            month = 11;
            year = year.wrapping_sub(1);
        }
        day += i16::from(MONTH_LENGTHS[month as usize]);
    }
    let mut text = format_long_thousands(i32::from(day), *fmt);
    text.extend_from_slice(b". ");
    text.extend_from_slice(MONTH_NAMES[month as usize]);
    text.push(b' ');
    fmt.pad_width = 1;
    text.extend(format_long_thousands(i32::from(year), *fmt));
    text.push(b'^');

    let mut args = args.iter();
    for &c in MESSAGE_TEMPLATES[tpl] {
        match c {
            b'$' => match args.next() {
                Some(MessageArg::Text(s)) => text.extend_from_slice(s),
                other => panic!("message template {tpl}: `$` needs a text, got {other:?}"),
            },
            b'#' => match args.next() {
                Some(&MessageArg::Number(v)) => text.extend(format_long_thousands(v, *fmt)),
                other => panic!("message template {tpl}: `#` needs a number, got {other:?}"),
            },
            _ => text.push(c),
        }
    }

    let pointer = state
        .messages
        .iter()
        .flatten()
        .map(|e| e.old_pointer)
        .max()
        .unwrap_or(0)
        .wrapping_add(1);
    let m = state.saved_manager as u8;
    let groups = usize::from(state.manager_count).min(MANAGER_SLOTS);
    if state.messages.len() < groups {
        state.messages.resize_with(groups, Vec::new);
    }
    // TODO(re): for a manager index of 4 and above the original increments
    // DS:0618[m] and writes FAR:5524[m * 20] outside the saved tables, and it
    // keeps no 20-message limit (entry 20 overwrites the next manager's
    // FAR:5524 slots); here only managers with a saved group store messages,
    // while DS:0618[m] and FAR:56e2[m] still change for every m below 4.
    if usize::from(m) < MANAGER_SLOTS {
        let mi = usize::from(m);
        state.message_counts[mi] = state.message_counts[mi].wrapping_add(1);
        if mi < groups {
            let mut bytes = text.clone();
            bytes.push(0);
            state.messages[mi].insert(
                0,
                StringEntry {
                    old_pointer: pointer,
                    bytes,
                },
            );
        }
        if !show_now {
            state.far_56e2[mi] = 1;
        }
    }
    PostedMessage { pointer, text }
}

/// Removes the message `pointer` from the inbox of the first manager that
/// holds it and moves the later messages up. Pointer 0 and unknown pointers
/// change nothing.
///
/// Origin: BMMAIN.EXE 4091:0044 (message_delete)
pub fn message_delete(state: &mut GameState, pointer: u32) {
    if pointer == 0 {
        return;
    }
    let groups = usize::from(state.manager_count).min(MANAGER_SLOTS);
    for m in 0..groups {
        let Some(group) = state.messages.get_mut(m) else {
            return;
        };
        let count = usize::from(state.message_counts[m]);
        if let Some(j) = group
            .iter()
            .take(count)
            .position(|e| e.old_pointer == pointer)
        {
            group.remove(j);
            state.message_counts[m] = state.message_counts[m].wrapping_sub(1);
            return;
        }
    }
}

/// [`team_slot_free_index`] when row `t` has no free slot.
pub const NO_FREE_SLOT: u8 = 0x7f;

/// The first free slot (countdown 0) of row `t` of [`GameState::team_slots`], or
/// [`NO_FREE_SLOT`].
///
/// Origin: BMMAIN.EXE 4091:0000 (team_slot_free_index)
pub fn team_slot_free_index(state: &GameState, t: u8) -> u8 {
    // TODO(re): `random_weekly_events` passes manager 3, whose row lies past the three saved
    // rows (in the high-score table at FAR:1d52); here that row has no free slot.
    state
        .team_slots
        .get(usize::from(t))
        .and_then(|row| row.iter().position(|&c| c == 0))
        .map_or(NO_FREE_SLOT, |i| i as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::BOARD_UNPLACED;
    use bmp_formats::save_files;
    use bmp_formats::savegame::SaveGame;

    fn grades(rt: &Runtime, m: usize, n: usize) -> Vec<u8> {
        (0..n).map(|i| rt.cup_goal_minutes[m][i][0]).collect()
    }

    fn set_grades(rt: &mut Runtime, m: usize, values: &[u8]) {
        for (i, &g) in values.iter().enumerate() {
            *grade_mut(rt, m, i) = g;
        }
    }

    fn set_name(dst: &mut [u8], name: &[u8]) {
        dst.fill(0);
        dst[..name.len()].copy_from_slice(name);
    }

    /// Four pixels per byte.
    fn narrow(s: &[u8]) -> i16 {
        (s.len() * 4) as i16
    }

    /// A state with one manager "TRAINER" of club 1 "A" against club 2 "B",
    /// and players "P1".."P3" in squad slots 0..2 of manager 0 (all starters).
    fn match_state() -> (GameState, Runtime) {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.options[NEWSPAPER_OPTION] = 1;
        set_name(&mut s.managers[0].name, b"TRAINER");
        s.managers[0].team = 1;
        set_name(&mut s.clubs[1].name, b"A");
        set_name(&mut s.clubs[2].name, b"B");
        for i in 0..3 {
            let id = i as u8 + 1;
            set_name(
                &mut s.players[usize::from(id)].name,
                format!("P{id}").as_bytes(),
            );
            let p = &mut s.squads[0][i];
            p.player_id = id;
            p.lineup_position = id;
        }
        let mut rt = Runtime::default();
        let r = &mut rt.news_records[0];
        r.club_a = 1;
        r.club_b = 2;
        (s, rt)
    }

    fn expander<'a>(
        state: &'a GameState,
        rt: &'a Runtime,
        rng: &'a mut Rng,
        selectors: [u8; 6],
    ) -> Expander<'a> {
        Expander {
            state,
            record: &rt.news_records[0],
            rng,
            m: 0,
            fmt: NumberFormat {
                pad_width: 1,
                ..NumberFormat::default()
            },
            selectors,
        }
    }

    fn expand_body(exp: &mut Expander<'_>, index: usize) -> Vec<u8> {
        let mut out = Vec::new();
        exp.expand(BODY_IMAGE, usize::from(BODY_OFFSETS[index]), &mut out);
        out
    }

    fn headline(exp: &mut Expander<'_>, index: usize) -> [Vec<u8>; 2] {
        let (mut first, mut second) = (Vec::new(), Vec::new());
        let rest = exp.expand(
            HEADLINE_IMAGE,
            usize::from(HEADLINE_OFFSETS[index]),
            &mut first,
        );
        exp.expand(HEADLINE_IMAGE, rest, &mut second);
        [first, second]
    }

    #[test]
    fn record_events_by_hand() {
        let mut rt = Runtime::default();
        news_record_match_event(&mut rt, 0, EVENT_GOAL, 3, 7, 10);
        let r = &rt.news_records[0];
        assert_eq!(
            (r.goals_own, r.goals_opp, r.goal_diff, r.result),
            (1, 0, 1, 1)
        );
        assert_eq!((r.max_diff, r.lead_own, r.lead_opp), (1, 1, 0));
        news_record_match_event(&mut rt, 0, EVENT_GOAL, OPPONENT, OPPONENT, 20);
        assert_eq!(
            (rt.news_records[0].goal_diff, rt.news_records[0].result),
            (0, 0)
        );
        news_record_match_event(&mut rt, 0, EVENT_GOAL, OPPONENT, OPPONENT, 30);
        let r = &rt.news_records[0];
        assert_eq!((r.goal_diff, r.result), (-1, 2));
        assert_eq!((r.min_diff, r.deficit_own, r.deficit_opp), (-1, 1, 2));
        news_record_match_event(&mut rt, 0, EVENT_GOAL, 5, 0, 85);
        let r = &rt.news_records[0];
        assert_eq!(
            (r.goals_own, r.goals_opp, r.goal_diff, r.result),
            (2, 2, 0, 0)
        );
        // The snapshots keep the first time the extreme was reached.
        assert_eq!((r.min_diff, r.max_diff), (-1, 1));
        assert_eq!(
            (r.deficit_own, r.deficit_opp, r.lead_own, r.lead_opp),
            (1, 2, 1, 0)
        );
        assert_eq!(r.n_goals, 4);
        assert_eq!(r.goal_minute[..4], [10, 20, 30, 85]);
        assert_eq!(r.goal_player[..4], [3, OPPONENT, OPPONENT, 5]);
        assert_eq!(r.goal_arg4[..4], [7, OPPONENT, OPPONENT, 0]);
        assert_eq!(r.n_events, 4);

        news_record_match_event(&mut rt, 0, EVENT_CHANCE, 2, 9, 40);
        let r = &rt.news_records[0];
        assert_eq!(
            (r.n_events, r.n_chances, r.n_goals, r.goals_own),
            (5, 1, 4, 2)
        );
        assert_eq!(
            (r.chance_minute[0], r.chance_player[0], r.chance_arg4[0]),
            (40, 2, 9)
        );

        news_record_match_event(&mut rt, 0, EVENT_RED_CARD, 4, 0, 0);
        news_record_match_event(&mut rt, 0, EVENT_YELLOW_CARD, 1, 0, 0);
        let r = &rt.news_records[0];
        assert_eq!((r.red, r.n_yellow, r.yellow[0], r.n_events), (5, 1, 1, 5));
        assert_eq!(rt.news_records[1], NewsRecord::zeroed());
    }

    #[test]
    fn record_arrays_overflow_like_the_original() {
        let mut rt = Runtime::default();
        for k in 0..7 {
            news_record_match_event(&mut rt, 0, EVENT_YELLOW_CARD, 10 + k, 0, 0);
        }
        let r = &rt.news_records[0];
        assert_eq!(r.yellow, [10, 11, 12, 13, 14, 15]);
        assert_eq!((r.red, r.n_yellow), (16, 7));
        // The 8th card is written over the count itself, then counted.
        news_record_match_event(&mut rt, 0, EVENT_YELLOW_CARD, 30, 0, 0);
        assert_eq!(rt.news_records[0].n_yellow, 31);

        let mut rt = Runtime::default();
        for minute in 1..=20 {
            news_record_match_event(&mut rt, 1, EVENT_GOAL, 0, 0, minute);
        }
        news_record_match_event(&mut rt, 1, EVENT_GOAL, 2, 6, 77);
        let r = &rt.news_records[1];
        assert_eq!(r.n_goals, 21);
        assert_eq!(r.goal_player[0], 77);
        assert_eq!(r.chance_minute[0], 2);
        assert_eq!(r.chance_arg4[0], 6);

        // Past the end of a record into the next one, and past the last
        // record (dropped).
        let mut rt = Runtime::default();
        rt.news_records[0].n_yellow = 13;
        news_record_match_event(&mut rt, 0, EVENT_YELLOW_CARD, 42, 0, 0);
        assert_eq!(rt.news_records[1].deficit_own, 42);
        rt.news_records[3].n_yellow = 200;
        news_record_match_event(&mut rt, 3, EVENT_YELLOW_CARD, 42, 0, 0);
        assert_eq!(rt.news_records[3].n_yellow, 201);
    }

    /// Manager 0 with one starter in slot 0 who has no lineup mismatch.
    fn grading_state() -> (GameState, Runtime) {
        let (mut s, mut rt) = match_state();
        rt.manager_plays_now[0] = 1;
        let p = &mut s.squads[0][0];
        p.skills = [33, 33, 30];
        p.exhaustion = 66;
        p.board_col = 2;
        p.board_row = 0;
        s.players[1].attr_20 = 2;
        s.players[1].position_value = 80;
        (s, rt)
    }

    fn grade_of(s: &GameState, rt: &Runtime) -> u8 {
        let (mut s, mut rt) = (s.clone(), rt.clone());
        let env = NewsEnv {
            fmt: NumberFormat::default(),
            text_width: &narrow,
        };
        let rng = Rng::new(1);
        let mut r = rng;
        assert!(match_rate_players_and_newspaper(&mut s, &mut rt, &mut r, 1, &env).is_empty());
        assert_eq!(r, rng);
        rt.cup_goal_minutes[0][0][0]
    }

    #[test]
    fn grades_by_hand() {
        let (mut s, rt) = grading_state();
        // q = 0, v = (66 / -33 + 12) * 2 = 20, c = (6 - 12 / 6) * 4 + 66 / 33
        // = 18, (20 + 18) / 11 = 3.
        assert_eq!(grade_of(&s, &rt), 3);
        // Skill 3 at 0 rounds |30 - 0| / 6 to q = 5: v = 10, 28 / 11 = 2.
        s.squads[0][0].skills[2] = 0;
        assert_eq!(grade_of(&s, &rt), 2);
        s.squads[0][0].skills[2] = 30;
        // Foot 8 on board column 2 adds a = 6: 44 / 11 = 4.
        s.players[1].attr_20 = 8;
        assert_eq!(grade_of(&s, &rt), 4);
        s.players[1].attr_20 = 2;
        // A rating below -12 counts as -12: c = 6 * 4 + 2, 46 / 11 = 4.
        s.squads[0][0].match_rating = -13i8 as u8;
        assert_eq!(grade_of(&s, &rt), 4);
        // Rating -5: -(7) / 6 truncates to -1 (floor would give -2). With a
        // role mismatch of 10 (+2): c = 20 + 2 + 2, 44 / 11 = 4.
        s.squads[0][0].match_rating = -5i8 as u8;
        s.players[1].position_value = 70;
        assert_eq!(grade_of(&s, &rt), 4);
        // Very strong and in top form: -14 / 11 = -1, raised to 1.
        let p = &mut s.squads[0][0];
        p.skills = [255, 255, 30];
        p.match_rating = 36;
        p.exhaustion = 0;
        s.players[1].position_value = 80;
        assert_eq!(grade_of(&s, &rt), 1);
        // Weak, worn out and misplaced: 326 / 11 = 29, lowered to 6.
        let p = &mut s.squads[0][0];
        p.skills = [0, 0, 30];
        p.match_rating = -12i8 as u8;
        p.exhaustion = 255;
        p.board_row = 7;
        p.board_col = BOARD_UNPLACED;
        s.players[1].attr_20 = 255;
        assert_eq!(grade_of(&s, &rt), 6);
    }

    #[test]
    fn grading_skips_players_who_did_not_play() {
        let (mut s, mut rt) = grading_state();
        s.manager_count = 2;
        s.saved_manager = 3;
        s.squads[0][1].lineup_position = 0;
        s.squads[0][1].suspended = SUSPENSION_PENDING;
        s.squads[0][2].lineup_position = FIRST_SUBSTITUTE;
        set_grades(&mut rt, 0, &[0, 5, 2]);
        s.squads[1][0].player_id = 9;
        s.squads[1][0].lineup_position = 1;
        let mut rng = Rng::new(1);
        let env = NewsEnv {
            fmt: NumberFormat::default(),
            text_width: &narrow,
        };
        assert!(match_rate_players_and_newspaper(&mut s, &mut rt, &mut rng, 1, &env).is_empty());
        assert_eq!(grades(&rt, 0, 3), [3, 5, 2]);
        assert_eq!((s.squads[0][1].suspended, s.squads[0][1].status), (1, 1));
        // Manager 1 had no match: neither graded nor made the saved manager.
        assert_eq!(grades(&rt, 1, 1), [0]);
        assert_eq!(s.saved_manager, 0);
    }

    #[test]
    fn expansion_uses_selectors_and_record() {
        let (s, mut rt) = match_state();
        let r = &mut rt.news_records[0];
        r.goals_own = 2;
        r.goals_opp = 1;
        r.spectators = 12345;
        let mut rng = Rng::new(7);
        let start = rng;
        let mut exp = expander(&s, &rt, &mut rng, [0, 1, 2, 1, 2, 1]);
        assert_eq!(
            expand_body(&mut exp, 0),
            b"EIN M[SSIGES SPIEL A GEGEN B ENDETE 2:1."
        );
        exp.selectors = [0, 2, 4, 1, 2, 1];
        // The last alternative is followed by the end marker at once.
        assert_eq!(
            expand_body(&mut exp, 0),
            b"EIN TOLLES SPIEL A BEI B ENDETE 2:1."
        );
        // Nested groups inside the skipped and the chosen alternatives.
        exp.selectors = [0, 1, 1, 1, 2, 1];
        assert_eq!(
            expand_body(&mut exp, 12),
            b"DIE 12.345 FANS IM STADION SAHEN EINE RECHT GUTE VORSTELLUNG IHRER MANNSCHAFT."
        );
        exp.selectors = [0, 1, 1, 2, 2, 1];
        assert_eq!(
            expand_body(&mut exp, 12),
            b"DIE 12.345 FANS IM STADION FEIERTEN DEN VERDIENTEN ERFOLG IHRER MANNSCHAFT."
        );
        exp.selectors = [0, 1, 1, 3, 2, 2];
        assert_eq!(
            expand_body(&mut exp, 12),
            b"DIE 12.345 FANS IM STADION BETRAUERTEN DIE UNGL]CKLICHE NIEDERLAGE IHRER MANNSCHAFT."
        );
        assert_eq!(rng, start);
    }

    #[test]
    fn expansion_draws_one_number_per_random_group() {
        let (s, rt) = match_state();
        let mut rng = Rng::new(99);
        let mut replay = rng;
        let mut exp = expander(&s, &rt, &mut rng, [0, 1, 1, 1, 1, 1]);
        let text = expand_body(&mut exp, 24);
        let k = replay.random_range(1, 3);
        let expected: &[u8] = match k {
            1 => b"DIE SPIELER GINGEN MIT ENTSCHIEDEN ZUVIEL EINSATZ ZUR SACHE.",
            2 => b"DIE SPIELER BESIEGTEN B MIT SEHR HARTEM EINSATZ.",
            _ => b"DIE SPIELER KONNTEN DIE NIEDERLAGE AUCH NICHT DURCH DEN TEILWEISE ERSCHRECKEND HARTEN EINSATZ ABWENDEN.",
        };
        assert_eq!(text, expected);
        // Body 31 with selector 4 = 1: a random group nested in the chosen
        // alternative; with selector 4 = 2 the nested group is skipped and
        // the random group of the second alternative is drawn instead.
        let text = expand_body(&mut exp, 31);
        let anger: &[u8] = match replay.random_range(1, 3) {
            1 => b"N UNMUT",
            2 => b"N ZORN",
            _ => b" WUT",
        };
        let mut expected = b"TRAINER WOLLTE SEINE".to_vec();
        expected.extend_from_slice(anger);
        expected.extend_from_slice(b" NICHT VERBERGEN.");
        assert_eq!(text, expected);
        exp.selectors[4] = 2;
        let text = expand_body(&mut exp, 31);
        let what: &[u8] = if replay.random_range(1, 2) == 1 {
            b"ERGEBNIS"
        } else {
            b"SPIEL"
        };
        let mut expected = b"TRAINER ZEIGTE SICHTLICHE ZUFRIEDENHEIT ]BER DAS ".to_vec();
        expected.extend_from_slice(what);
        expected.push(b'.');
        assert_eq!(text, expected);
        assert_eq!(rng, replay);
    }

    #[test]
    fn headline_lines_and_competition_selector() {
        let (mut s, mut rt) = match_state();
        rt.news_records[0].goals_own = 4;
        let mut rng = Rng::new(3);
        let mut exp = expander(&s, &rt, &mut rng, [0, 1, 1, 1, 1, 1]);
        assert_eq!(
            headline(&mut exp, 0),
            [b"A }BERROLLT".to_vec(), b"B MIT 4:0".to_vec()]
        );
        for (slot, expected) in [
            (1, &b"B 4:0 KLASSIERT"[..]),
            (2, b"B 4:0 ]BERROLLT"),
            // TODO(re) fallback: the original runs past the group.
            (0, b"B 4:0 KLASSIERT"),
            (0x80, b"B 4:0 KLASSIERT"),
        ] {
            s.competition_slots[5] = slot;
            let mut exp = expander(&s, &rt, &mut rng, [0, 1, 1, 1, 1, 1]);
            assert_eq!(
                headline(&mut exp, 9),
                [b"A WIRD VON".to_vec(), expected.to_vec()],
                "slot {slot}"
            );
        }
    }

    #[test]
    fn expansion_codes() {
        let (s, mut rt) = match_state();
        let r = &mut rt.news_records[0];
        r.goals_own = 0xff;
        r.hero = 2;
        r.loser = 3;
        let mut rng = Rng::new(3);
        let mut exp = expander(&s, &rt, &mut rng, [0; 6]);
        let mut out = Vec::new();
        let image = b"<%0|%c|%d|%t|%z|%1>#REST\0";
        let stop = exp.expand(image, 0, &mut out);
        // %0 sign-extends the byte; the unknown %z drops both characters.
        assert_eq!(out, b"<-1|P2|P3|TRAINER||0>");
        assert_eq!(image[stop], b'#');
        // With pad width 0 a zero is the empty string.
        exp.fmt.pad_width = 0;
        out.clear();
        exp.expand(b"%1\0", 0, &mut out);
        assert_eq!(out, b"");

        let (s, mut rt) = match_state();
        let r = &mut rt.news_records[0];
        (r.deficit_own, r.deficit_opp, r.lead_own, r.lead_opp) = (2, 3, 4, 5);
        let mut rng = Rng::new(3);
        let mut exp = expander(&s, &rt, &mut rng, [0; 6]);
        let mut out = Vec::new();
        exp.expand(b"%2|%3|%4|%5\0", 0, &mut out);
        assert_eq!(out, b"2|3|4|5");
    }

    fn flow_lines(words: &[&[u8]], width: i16) -> Vec<NewsLine> {
        let mut flow = TextFlow::new(&narrow);
        flow.start(3, 50);
        for w in words {
            flow.word(w, width, 0);
        }
        flow.flush(width, 0);
        flow.lines
    }

    #[test]
    fn text_flow_justifies_round_robin() {
        let lines = flow_lines(&[b"AAA", b"BB", b"CCCC"], 40);
        let texts: Vec<_> = lines.iter().map(|l| l.text.clone()).collect();
        assert_eq!(texts, [b"AAA     BB".to_vec(), b"CCCC".to_vec()]);
        assert_eq!((lines[0].x, lines[0].y, lines[1].y), (3, 50, 57));
        assert!(lines.iter().all(|l| l.style == LineStyle::Plain));
        // Blanks go after the words from the left, one per gap per round;
        // the end of the last word is no gap.
        let lines = flow_lines(&[b"A", b"B", b"C", b"DDDDDDDD"], 40);
        assert_eq!(lines[0].text, b"A    B   C");
        // A single word is padded at its end.
        let lines = flow_lines(&[b"AB,", b"CDEFGHIJ"], 40);
        assert_eq!(lines[0].text, b"AB,       ");
    }

    #[test]
    fn text_flow_drops_trailing_comma_on_flush_only() {
        let lines = flow_lines(&[b"X,", b"Y,"], 40);
        assert_eq!(lines[0].text, b"X, Y");
        let lines = flow_lines(&[], 40);
        assert_eq!(lines[0].text, b"");
    }

    #[test]
    fn text_flow_label_value_style() {
        let style = |width, flag| {
            let mut flow = TextFlow::new(&narrow);
            flow.start(0, 0);
            flow.word(b"TORE:", width, 0);
            flow.flush(width, flag);
            flow.lines[0].style
        };
        assert_eq!(style(SIDEBAR_WIDTH, 6), LineStyle::LabelValue);
        assert_eq!(style(SIDEBAR_WIDTH, 0), LineStyle::Plain);
        assert_eq!(style(BODY_WIDTH, 6), LineStyle::Plain);
    }

    #[test]
    fn flow_words_splits_at_one_separator() {
        let mut flow = TextFlow::new(&narrow);
        flow.start(0, 0x105);
        assert_eq!(news_flow_words(&mut flow, b"A  B\xa0C", 400), 0x05);
        flow.flush(400, 0);
        assert_eq!(flow.lines[0].text, b"A  B C");
        assert_eq!(news_flow_words(&mut flow, b"", 400), 0);
    }

    #[test]
    fn label_value_split_drops_the_char_after_the_colon() {
        assert_eq!(
            label_value_split(b"TORE: 1:0 X"),
            Some((&b"TORE:"[..], &b"1:0 X"[..]))
        );
        assert_eq!(label_value_split(b"A:"), Some((&b"A:"[..], &b""[..])));
        assert_eq!(label_value_split(b"KEINE"), None);
    }

    /// A 2:0 home win: two early-enough goals of slot 0 (the hero, rating
    /// 30), slot 1 rated -20 (the loser).
    fn home_win() -> (GameState, Runtime) {
        let (mut s, mut rt) = match_state();
        news_record_match_event(&mut rt, 0, EVENT_GOAL, 0, 0, 10);
        news_record_match_event(&mut rt, 0, EVENT_GOAL, 0, 0, 50);
        rt.news_records[0].spectators = 30_000;
        s.squads[0][0].match_rating = 30;
        s.players[1].position_value = 50;
        s.squads[0][1].match_rating = -20i8 as u8;
        set_grades(&mut rt, 0, &[4, 5]);
        (s, rt)
    }

    #[test]
    fn analysis_of_a_home_win() {
        let (s, mut rt) = home_win();
        let mut rng = Rng::new(5);
        let mut replay = rng;
        let a = analyse_match(&s, &mut rt, &mut rng, 0, 0);
        let hero = 4 - replay.random_range(1, 2);
        let loser = (5 + replay.random_range(1, 2)).min(6);
        // dom = (66 + 66) / 120 = 1 > 0: no coin for selector 5.
        assert_eq!(rng, replay);
        assert_eq!(a.dominance, 1);
        assert_eq!(a.selectors, [0, 1, 2, 2, 2, 1]);
        assert!(!a.early_own && !a.early_opp && !a.late_decider);
        let r = &rt.news_records[0];
        assert_eq!((r.hero, r.loser, r.n_events), (1, 2, 3));
        assert_eq!(grades(&rt, 0, 2), [hero as u8, loser as u8]);
    }

    #[test]
    fn analysis_satisfaction_follows_team_strength() {
        // Goalless home draw. Strength 100 in every line gives 3 * (100 -
        // 44) * 2 / 15 = 22; a club with none gives -132 * 2 / 15 = -17.
        let (mut s, mut rt) = match_state();
        rt.news_records[0].spectators = 10_000;
        s.clubs[2].strength = [100; 3];
        let a = analyse_match(&s, &mut rt.clone(), &mut Rng::new(4), 0, 0);
        assert_eq!(a.selectors[..5], [0, 1, 1, 1, 2]);
        s.clubs[1].strength = [100; 3];
        s.clubs[2].strength = [0; 3];
        let a = analyse_match(&s, &mut rt, &mut Rng::new(4), 0, 0);
        assert_eq!(a.selectors[4], 1);
    }

    #[test]
    fn analysis_clamps_many_events() {
        // A 1:0 win with eleven own chances: 12 events, clamped to 10, so
        // quality 1 + 7 / 2 + 1 = 5 is capped at 4.
        let (s, mut rt) = match_state();
        news_record_match_event(&mut rt, 0, EVENT_GOAL, 0, 0, 10);
        for minute in 20..31 {
            news_record_match_event(&mut rt, 0, EVENT_CHANCE, 1, 0, minute);
        }
        assert_eq!(rt.news_records[0].n_events, 12);
        let a = analyse_match(&s, &mut rt, &mut Rng::new(4), 0, 0);
        assert_eq!(rt.news_records[0].n_events, 10);
        assert_eq!(a.selectors[..4], [0, 2, 4, 2]);
    }

    #[test]
    fn hero_and_loser_limits() {
        let (mut s, mut rt) = home_win();
        // Slot 2 also qualifies as hero (the last one wins) and as loser
        // (only the first one counts); grade 1 stays 1, grade 6 stays 6.
        s.squads[0][2].match_rating = 26;
        s.players[3].position_value = 1;
        set_grades(&mut rt, 0, &[1, 6, 1]);
        let mut rng = Rng::new(5);
        analyse_match(&s, &mut rt, &mut rng, 0, 0);
        assert_eq!((rt.news_records[0].hero, rt.news_records[0].loser), (3, 2));
        assert_eq!(grades(&rt, 0, 3), [1, 6, 1]);
        // A hero needs a non-zero player-table +0x1f.
        let (mut s, mut rt) = home_win();
        s.players[1].position_value = 0;
        let mut rng = Rng::new(5);
        let mut replay = rng;
        analyse_match(&s, &mut rt, &mut rng, 0, 0);
        replay.random_range(1, 2);
        assert_eq!(rng, replay);
        assert_eq!(rt.news_records[0].hero, 0);
    }

    #[test]
    fn analysis_of_a_goalless_away_draw_draws_the_coin() {
        let (s, mut rt) = match_state();
        let mut rng = Rng::new(11);
        let mut replay = rng;
        let a = analyse_match(&s, &mut rt, &mut rng, 0, 0);
        let coin = replay.random_range(0, 1);
        assert_eq!(rng, replay);
        assert_eq!(a.dominance, 0);
        assert_eq!(a.selectors[1], 2);
        assert_eq!(a.selectors[5], if coin != 0 { 1 } else { 2 });
        assert_eq!(rt.news_records[0].n_events, 3);
    }

    #[test]
    fn late_equaliser_only_outside_the_cup() {
        let (s, mut rt) = match_state();
        news_record_match_event(&mut rt, 0, EVENT_GOAL, OPPONENT, OPPONENT, 30);
        news_record_match_event(&mut rt, 0, EVENT_GOAL, 1, 0, 85);
        for _ in 0..5 {
            news_record_match_event(&mut rt, 0, EVENT_CHANCE, 2, 0, 60);
        }
        let mut t = rt.clone();
        let a = analyse_match(&s, &mut t, &mut Rng::new(2), 0, 0);
        assert!(a.late_decider);
        assert_eq!(a.own_chances, 5);
        assert_eq!(t.news_records[0].n_events, 7);
        let a = analyse_match(&s, &mut rt, &mut Rng::new(2), 0, 1);
        assert!(!a.late_decider);
        // An early opponent goal blocks the early-lead flag.
        let (s, mut rt) = match_state();
        news_record_match_event(&mut rt, 0, EVENT_GOAL, OPPONENT, OPPONENT, 3);
        news_record_match_event(&mut rt, 0, EVENT_GOAL, 1, 0, 5);
        let a = analyse_match(&s, &mut rt, &mut Rng::new(2), 0, 0);
        assert!(a.early_opp && !a.early_own);
    }

    #[test]
    fn template_selection_of_a_home_win() {
        let (s, mut rt) = home_win();
        let mut rng = Rng::new(5);
        let a = analyse_match(&s, &mut rt, &mut rng, 0, 0);
        let r = rt.news_records[0].clone();
        let mut replay = rng;
        let (body, hl) = select_templates(&r, &a, &mut rng);

        let mut expected = vec![
            replay.random_range(0, 7) as u8,
            replay.random_range(8, 12) as u8,
        ];
        replay.random_range(0, 1); // hard play: no yellow cards
        if replay.random_range(0, 1) != 0 {
            expected.push(replay.random_range(25, 29) as u8);
        }
        expected.push(replay.random_range(66, 69) as u8); // hero
        expected.push(replay.random_range(30, 36) as u8); // manager quote
        expected.push(replay.random_range(81, 86) as u8); // clinical finishing
        expected.push(replay.random_range(41, 47) as u8); // win verdict
        let (mut h, mut flag) = (0, 0);
        h = replay.chance_pick(5, h, &mut flag, 6, 8); // k2: 0 < d < 4
        h = replay.chance_pick(4, h, &mut flag, 32, 38); // k12: hero
        h = replay.chance_pick(3, h, &mut flag, 43, 47); // k14: few events
        h = replay.chance_pick(3, h, &mut flag, 57, 59); // k18: deserved win
        expected.push(replay.random_range(48, 55) as u8); // superiority
        assert_eq!(body, expected);
        assert_eq!(hl, h);
        assert_eq!(rng, replay);
    }

    #[test]
    fn template_selection_extra_blocks() {
        // A 2:3 home loss after leading 2:0, sold out, lucky for the winner.
        let mut rt = match_state().1;
        for (player, minute) in [
            (0, 20),
            (0, 25),
            (OPPONENT, 30),
            (OPPONENT, 40),
            (OPPONENT, 60),
        ] {
            news_record_match_event(&mut rt, 0, EVENT_GOAL, player, player, minute);
        }
        rt.news_records[0].sold_out = 1;
        rt.news_records[0].spectators = 40_000;
        let r = rt.news_records[0].clone();
        let a = Analysis {
            early_own: false,
            early_opp: false,
            late_decider: false,
            own_chances: 0,
            dominance: 1,
            selectors: [0, 1, 4, 3, 1, 1],
        };
        let mut rng = Rng::new(8);
        let mut replay = rng;
        let (body, hl) = select_templates(&r, &a, &mut rng);
        let mut expected = vec![
            replay.random_range(0, 7) as u8,
            replay.random_range(8, 12) as u8,
        ];
        replay.random_range(0, 1); // hard play: no yellow cards
        if replay.random_range(0, 1) != 0 {
            expected.push(replay.random_range(25, 29) as u8);
        }
        expected.push(replay.random_range(30, 36) as u8);
        expected.push(replay.random_range(102, 108) as u8); // loss verdict
        let (mut h, mut flag) = (0, 0);
        h = replay.chance_pick(5, h, &mut flag, 13, 14); // k4: d = -1
        h = replay.chance_pick(5, h, &mut flag, 15, 17); // k5
        h = replay.chance_pick(3, h, &mut flag, 19, 20); // k7: sold out
        expected.push(replay.random_range(37, 40) as u8);
        expected.push(replay.random_range(93, 97) as u8); // lead given away
        h = replay.chance_pick(5, h, &mut flag, 25, 27); // k9
        h = replay.chance_pick(4, h, &mut flag, 51, 53); // k16
        assert_eq!(body, expected);
        assert_eq!(hl, h);
        assert_eq!(rng, replay);
    }

    /// A generator for which, after drawing `before` ranges, the next
    /// `random_range(0, 1)` is 1.
    fn rng_with_coin_after(before: &[(i16, i16)]) -> Rng {
        (1..)
            .map(Rng::new)
            .find(|r| {
                let mut r = *r;
                for &(lo, hi) in before {
                    r.random_range(lo, hi);
                }
                r.random_range(0, 1) == 1
            })
            .unwrap()
    }

    #[test]
    fn template_selection_of_a_late_away_loss() {
        // 1:2 away after leading 1:0, three yellow cards, a scapegoat, and
        // every conditional body step of the analysis taken.
        let mut rt = match_state().1;
        let r = &mut rt.news_records[0];
        (r.result, r.goal_diff, r.min_diff, r.max_diff) = (2, -1, -1, 1);
        (r.n_yellow, r.loser, r.n_events) = (3, 1, 5);
        let r = r.clone();
        let a = Analysis {
            early_own: true,
            early_opp: true,
            late_decider: true,
            own_chances: 4,
            dominance: 0,
            selectors: [0, 2, 2, 3, 1, 1],
        };
        let mut rng = rng_with_coin_after(&[(0, 7), (13, 16)]);
        let mut replay = rng;
        let (body, hl) = select_templates(&r, &a, &mut rng);
        let mut expected = vec![
            replay.random_range(0, 7) as u8,
            replay.random_range(13, 16) as u8, // travelling fans
        ];
        assert_eq!(replay.random_range(0, 1), 1);
        expected.push(replay.random_range(17, 24) as u8); // hard play
        replay.random_range(0, 1); // fair play: three yellow cards
        expected.push(replay.random_range(70, 73) as u8); // scapegoat
        expected.push(replay.random_range(30, 36) as u8); // manager quote
        expected.push(replay.random_range(100, 101) as u8); // late result
        expected.push(replay.random_range(74, 80) as u8); // poor finishing
        expected.push(replay.random_range(98, 98) as u8); // early opponent goal
        expected.push(replay.random_range(99, 99) as u8); // early own goal
        expected.push(replay.random_range(102, 108) as u8); // loss verdict
        let (mut h, mut flag) = (0, 0);
        h = replay.chance_pick(5, h, &mut flag, 13, 14); // k4: d = -1
        h = replay.chance_pick(5, h, &mut flag, 15, 17); // k5
        expected.push(replay.random_range(93, 97) as u8); // lead given away
        h = replay.chance_pick(5, h, &mut flag, 25, 27); // k9
        h = replay.chance_pick(4, h, &mut flag, 48, 50); // k15: scapegoat
        h = replay.chance_pick(4, h, &mut flag, 51, 53); // k16
        assert_eq!(body, expected);
        assert_eq!(hl, h);
        assert_eq!(rng, replay);
    }

    #[test]
    fn template_selection_of_a_draw_with_swings() {
        // 2:2 at home after trailing by two and leading by two, lucky.
        let mut rt = match_state().1;
        let r = &mut rt.news_records[0];
        (r.min_diff, r.max_diff, r.n_events, r.spectators) = (-2, 2, 3, 1_000);
        let r = r.clone();
        let a = Analysis {
            early_own: false,
            early_opp: false,
            late_decider: false,
            own_chances: 0,
            dominance: -1,
            selectors: [0, 1, 1, 1, 1, 2],
        };
        let mut rng = Rng::new(6);
        let mut replay = rng;
        let (body, hl) = select_templates(&r, &a, &mut rng);
        let mut expected = vec![
            replay.random_range(0, 7) as u8,
            replay.random_range(8, 12) as u8,
        ];
        replay.random_range(0, 1); // hard play: no yellow cards
        if replay.random_range(0, 1) != 0 {
            expected.push(replay.random_range(25, 29) as u8);
        }
        expected.push(replay.random_range(30, 36) as u8);
        let (mut h, mut flag) = (0, 0);
        h = replay.chance_pick(2, h, &mut flag, 18, 18); // k6: draw
        expected.push(replay.random_range(87, 92) as u8); // trailed
        expected.push(replay.random_range(93, 97) as u8); // led
        h = replay.chance_pick(4, h, &mut flag, 28, 29); // k10
        h = replay.chance_pick(4, h, &mut flag, 30, 31); // k11
        h = replay.chance_pick(3, h, &mut flag, 43, 47); // k14: few events
        h = replay.chance_pick(4, h, &mut flag, 63, 64); // k20: even draw
        h = replay.chance_pick(3, h, &mut flag, 65, 66); // k21: lucky draw
        assert_eq!(body, expected);
        assert_eq!(hl, h);
        assert_eq!(rng, replay);
    }

    fn env(text_width: &dyn Fn(&[u8]) -> i16) -> NewsEnv<'_> {
        NewsEnv {
            fmt: NumberFormat::default(),
            text_width,
        }
    }

    #[test]
    fn newspaper_option_off_changes_nothing() {
        let (mut s, mut rt) = home_win();
        s.options[NEWSPAPER_OPTION] = 0;
        let before = rt.clone();
        let mut rng = Rng::new(5);
        assert_eq!(
            newspaper_generate(&s, &mut rt, &mut rng, 0, 0, &env(&narrow)),
            None
        );
        assert_eq!(rng, Rng::new(5));
        assert_eq!(rt, before);
    }

    #[test]
    fn newspaper_of_a_home_win() {
        let (s, mut rt) = home_win();
        let mut rng = Rng::new(5);
        let mut replay = rng;
        let paper = newspaper_generate(&s, &mut rt, &mut rng, 0, 0, &env(&narrow)).unwrap();
        assert_eq!(paper.photo, 200 + replay.random_range(0, 0x1d) as u16);
        assert!(paper.headline.iter().all(|l| !l.is_empty()));
        let texts = paper
            .headline
            .iter()
            .chain(paper.body.iter().chain(&paper.sidebar).map(|l| &l.text));
        for t in texts {
            assert!(!t.contains(&b'%') && !t.contains(&b'#'), "{t:?}");
        }
        assert!(
            paper
                .body
                .iter()
                .all(|l| l.x == 13 && l.style == LineStyle::Plain)
        );
        assert_eq!(paper.body[0].y, 60);
        let (t, mut t_rt) = home_win();
        let again = newspaper_generate(&t, &mut t_rt, &mut Rng::new(5), 0, 0, &env(&narrow));
        assert_eq!(again.as_ref(), Some(&paper));
        assert_eq!(t_rt, rt);
    }

    #[test]
    fn body_stops_at_a_full_page() {
        let (s, rt) = match_state();
        // Body 11 has no random group, body 24 one: with a wide font the
        // page is full before body 24 and it draws nothing.
        let templates = [11, 11, 11, 11, 24];
        let wide = |t: &[u8]| (t.len() * 30) as i16;
        let mut rng = Rng::new(4);
        let mut exp = expander(&s, &rt, &mut rng, [0; 6]);
        let mut flow = TextFlow::new(&wide);
        layout_body(&mut exp, &mut flow, &templates);
        assert!(
            flow.lines
                .iter()
                .all(|l| !l.text.starts_with(b"DIE SPIELER"))
        );
        assert!(flow.lines.last().unwrap().y > i16::from(BODY_LAST_Y));
        assert_eq!(rng, Rng::new(4));

        let tiny = |t: &[u8]| t.len() as i16;
        let mut exp = expander(&s, &rt, &mut rng, [0; 6]);
        let mut flow = TextFlow::new(&tiny);
        layout_body(&mut exp, &mut flow, &templates);
        let mut replay = Rng::new(4);
        replay.random_range(1, 3);
        assert_eq!(rng, replay);
    }

    fn sidebar_texts(s: &GameState, rt: &mut Runtime) -> Vec<(Vec<u8>, LineStyle)> {
        let mut flow = TextFlow::new(&narrow);
        let fmt = NumberFormat {
            pad_width: 1,
            ..NumberFormat::default()
        };
        layout_sidebar(s, rt, 0, fmt, &mut flow);
        flow.lines.into_iter().map(|l| (l.text, l.style)).collect()
    }

    #[test]
    fn sidebar_lines() {
        let (mut s, mut rt) = match_state();
        set_grades(&mut rt, 0, &[2, 3, 4]);
        s.squads[0][2].lineup_position = FIRST_SUBSTITUTE;
        news_record_match_event(&mut rt, 0, EVENT_GOAL, 1, 0, 10);
        news_record_match_event(&mut rt, 0, EVENT_GOAL, OPPONENT, OPPONENT, 20);
        news_record_match_event(&mut rt, 0, EVENT_YELLOW_CARD, 0, 0, 0);
        news_record_match_event(&mut rt, 0, EVENT_YELLOW_CARD, 2, 0, 0);
        news_record_match_event(&mut rt, 0, EVENT_RED_CARD, 1, 0, 0);
        rt.news_records[0].spectators = 200_001;
        let lines = sidebar_texts(&s, &mut rt);
        assert_eq!(rt.news_records[0].spectators, 0);
        use LineStyle::*;
        assert_eq!(
            lines,
            [
                // The labels end with a blank, and the flow adds another.
                (b"A:  P1 (2), P2 (3)".to_vec(), LabelValue),
                // Away match: the own goal counts for the away side.
                (b"TORE:  0:1 P2 (10.MIN), 1:1  (20.MIN)".to_vec(), Plain),
                (b"GELBE KARTEN:  P1, P3".to_vec(), LabelValue),
                (b"ROTE KARTEN  :  P2".to_vec(), LabelValue),
            ]
        );
        let (s, mut rt) = match_state();
        rt.news_records[0].spectators = 5;
        let lines = sidebar_texts(&s, &mut rt);
        assert_eq!(lines[1].0, b"TORE:  FEHLANZEIGE");
        assert_eq!(lines[2].0, b"GELBE KARTEN:  KEINE");
        assert_eq!(lines[3].0, b"ROTE KARTEN  :  KEINE");
        assert_eq!(rt.news_records[0].spectators, 5);
    }

    #[test]
    fn rate_and_newspaper_for_every_manager_who_played() {
        let (mut s, mut rt) = home_win();
        s.manager_count = 2;
        rt.manager_plays_now = [1, 0, 0, 0];
        let mut rng = Rng::new(9);
        let papers = match_rate_players_and_newspaper(&mut s, &mut rt, &mut rng, 0, &env(&narrow));
        assert_eq!(papers.len(), 1);
        assert_eq!(s.saved_manager, 0);
        let (mut t, mut t_rt) = home_win();
        t.manager_count = 2;
        t_rt.manager_plays_now = [1, 0, 0, 0];
        let mut rng = Rng::new(9);
        assert!(
            match_rate_players_and_newspaper(&mut t, &mut t_rt, &mut rng, 1, &env(&narrow))
                .is_empty()
        );
        assert_eq!(rng, Rng::new(9));
    }

    fn message_state() -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 2;
        s.saved_manager = 1;
        s.day_of_month = 2;
        s.month = 2;
        s.year = 1965;
        s
    }

    /// A generator whose next `random_range(0, 3)` is `k`.
    fn rng_drawing(k: i16) -> Rng {
        (1..)
            .map(Rng::new)
            .find(|r| r.clone().random_range(0, 3) == k)
            .unwrap()
    }

    fn post(s: &mut GameState, k: i16, fmt: &mut NumberFormat, show_now: bool) -> PostedMessage {
        let mut rng = rng_drawing(k);
        message_post(
            s,
            &mut rng,
            fmt,
            show_now,
            1,
            &[MessageArg::Text(b"X"), MessageArg::Text(b"Y")],
        )
    }

    #[test]
    fn message_dates() {
        let mut s = message_state();
        let mut fmt = NumberFormat::default();
        // Day 2 - 3 = -1 of March: 27 February (28 days).
        assert!(
            post(&mut s, 3, &mut fmt, true)
                .text
                .starts_with(b"27. Februar 1965^")
        );
        // Day 1 of January minus 1: 31 December of the year before.
        s.day_of_month = 1;
        s.month = 0;
        assert!(
            post(&mut s, 1, &mut fmt, true)
                .text
                .starts_with(b"31. Dezember 1964^")
        );
        // Day 31 of April is capped at 30.
        s.day_of_month = 31;
        s.month = 3;
        assert!(
            post(&mut s, 0, &mut fmt, true)
                .text
                .starts_with(b"30. April 1965^")
        );
    }

    #[test]
    fn message_number_format() {
        let mut s = message_state();
        s.day_of_month = 5;
        let mut fmt = NumberFormat {
            pad_width: 2,
            ..NumberFormat::default()
        };
        let mut rng = rng_drawing(0);
        let msg = message_post(
            &mut s,
            &mut rng,
            &mut fmt,
            true,
            0,
            &[
                MessageArg::Text(b"MEYER"),
                MessageArg::Number(2),
                MessageArg::Number(1234),
            ],
        );
        assert_eq!(
            msg.text,
            b"^5. M{rz 1965^MEYER bietet an,^von 2 auf 1234^Jahre zu verl{ngern.^"
        );
        assert_eq!(
            fmt,
            NumberFormat {
                no_separators: true,
                comma_separator: false,
                pad_width: 1,
            }
        );
    }

    #[test]
    fn message_inbox_insert_and_delete() {
        let mut s = message_state();
        let mut fmt = NumberFormat::default();
        let first = post(&mut s, 0, &mut fmt, true);
        assert_eq!(s.far_56e2, [0; 4]);
        let second = post(&mut s, 0, &mut fmt, false);
        assert_eq!(s.far_56e2, [0, 1, 0, 0]);
        assert_eq!(s.message_counts, [0, 2, 0, 0]);
        assert_eq!(s.messages.len(), 2);
        assert!(first.pointer != 0 && second.pointer != first.pointer);
        let inbox = &s.messages[1];
        assert_eq!(inbox[0].old_pointer, second.pointer);
        assert_eq!(inbox[1].old_pointer, first.pointer);
        let mut bytes = first.text.clone();
        bytes.push(0);
        assert_eq!(inbox[1].bytes, bytes);
        assert!(s.to_savegame().to_bytes().is_ok());

        let before = s.clone();
        message_delete(&mut s, 0);
        message_delete(&mut s, 0xdead);
        assert_eq!(s, before);
        message_delete(&mut s, second.pointer);
        assert_eq!(s.message_counts[1], 1);
        assert_eq!(s.messages[1][0].old_pointer, first.pointer);
        message_delete(&mut s, first.pointer);
        assert_eq!(s.message_counts[1], 0);
        assert!(s.messages[1].is_empty());
    }

    #[test]
    fn message_pointer_stays_unique_after_a_delete() {
        let mut s = message_state();
        let mut fmt = NumberFormat::default();
        let a = post(&mut s, 0, &mut fmt, true).pointer;
        let b = post(&mut s, 0, &mut fmt, true).pointer;
        let c = post(&mut s, 0, &mut fmt, true).pointer;
        message_delete(&mut s, a);
        let d = post(&mut s, 0, &mut fmt, true).pointer;
        assert!(d != b && d != c);
        message_delete(&mut s, c);
        let inbox: Vec<u32> = s.messages[1].iter().map(|e| e.old_pointer).collect();
        assert_eq!(inbox, [d, b]);
        assert_eq!(s.message_counts[1], 2);
    }

    #[test]
    fn message_for_a_manager_without_inbox_is_not_stored() {
        let mut s = message_state();
        s.saved_manager = 3;
        let mut fmt = NumberFormat::default();
        let msg = post(&mut s, 0, &mut fmt, false);
        assert!(msg.text.ends_with(b"X^ist an Y^interessiert.^"));
        assert_eq!(s.message_counts, [0, 0, 0, 1]);
        assert!(s.messages.iter().all(Vec::is_empty));
        assert!(s.to_savegame().to_bytes().is_ok());
    }

    fn load(path: &std::path::Path) -> (Vec<u8>, GameState) {
        let data = std::fs::read(path).unwrap();
        let state = GameState::from_savegame(&SaveGame::parse(&data).unwrap()).unwrap();
        (data, state)
    }

    #[test]
    fn real_saves_message_post_then_delete_round_trips() {
        for path in save_files() {
            let (data, original) = load(&path);
            let mut s = original.clone();
            for m in 0..s.active_managers().len() {
                s.saved_manager = m as i8;
                let mut rng = Rng::new(1);
                let mut fmt = NumberFormat::default();
                let args = [MessageArg::Text(b"A"), MessageArg::Text(b"B")];
                let msg = message_post(&mut s, &mut rng, &mut fmt, true, 1, &args);
                assert!(s.to_savegame().to_bytes().is_ok(), "{path:?}");
                message_delete(&mut s, msg.pointer);
            }
            s.saved_manager = original.saved_manager;
            assert_eq!(s.to_savegame().to_bytes().unwrap(), data, "{path:?}");
        }
    }

    #[test]
    fn real_saves_newspaper_smoke() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let Ok(fonts) = bmp_formats::font::FontSet::load(&dir) else {
            return;
        };
        let font = fonts.get(bmp_formats::font::FontId::Micro4);
        let width = |t: &[u8]| font.text_length(t) as i16;
        for path in save_files() {
            let (data, original) = load(&path);
            let mut s = original.clone();
            s.options[NEWSPAPER_OPTION] = 1;
            let mut rt = Runtime::default();
            for m in 0..s.active_managers().len() as u8 {
                let mi = usize::from(m);
                rt.manager_plays_now[mi] = 1;
                let r = &mut rt.news_records[mi];
                r.club_a = s.managers[mi].team;
                r.club_b = 0;
                r.spectators = 25_000;
                let events = [
                    (EVENT_CHANCE, 0, 12),
                    (EVENT_GOAL, 0, 17),
                    (EVENT_YELLOW_CARD, 1, 0),
                    (EVENT_GOAL, OPPONENT, 44),
                    (EVENT_CHANCE, OPPONENT, 70),
                    (EVENT_GOAL, 2, 88),
                ];
                for (kind, player, minute) in events {
                    news_record_match_event(&mut rt, m, kind, player, 0, minute);
                }
            }
            let env = env(&width);
            let (mut t, mut t_rt) = (s.clone(), rt.clone());
            let mut rng = Rng::new(1);
            let papers = match_rate_players_and_newspaper(&mut s, &mut rt, &mut rng, 0, &env);
            assert_eq!(papers.len(), s.active_managers().len(), "{path:?}");
            for (m, paper) in papers.iter().enumerate() {
                for (i, p) in s.squads[m].iter().enumerate().take(24) {
                    if p.player_id != 0 && (1..12).contains(&p.lineup_position) {
                        let grade = rt.cup_goal_minutes[m][i][0];
                        assert!((1..=6).contains(&grade), "{path:?}");
                    }
                }
                let lines = paper.body.iter().chain(&paper.sidebar);
                for t in paper.headline.iter().chain(lines.map(|l| &l.text)) {
                    assert!(!t.contains(&b'%') && !t.contains(&b'#'), "{path:?} {t:?}");
                }
                for l in &paper.body {
                    // Justification stops at the first blank that reaches
                    // the width, so a line may overshoot by less than one.
                    let w = width(&l.text);
                    assert!(l.x == 13 && w < BODY_WIDTH + width(b" "), "{path:?} {w}");
                }
            }
            let mut rng2 = Rng::new(1);
            assert_eq!(
                match_rate_players_and_newspaper(&mut t, &mut t_rt, &mut rng2, 0, &env),
                papers
            );
            assert_eq!((rng2, &t, &t_rt), (rng, &s, &rt));
            // Besides the fields set above, only the suspensions change.
            s.squads = original.squads.clone();
            s.saved_manager = original.saved_manager;
            s.options = original.options;
            assert_eq!(s.to_savegame().to_bytes().unwrap(), data, "{path:?}");
        }
    }
}
