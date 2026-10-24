//! The screens of the season loop (segment 2a58): the screen crate's [`SeasonUi`], which runs
//! the office turns, the match-day titles, the live match view and the newspapers while
//! [`bmp_game::season::season_main_loop`] drives the game. Spec: `docs/re/logic/season.md` §3.

use bmp_formats::savegame::HighScoreTable;
use bmp_game::finance::FinanceEvent;
use bmp_game::matchsim::TickerEvent;
use bmp_game::news::{NewsEnv, Newspaper};
use bmp_game::rng::{NumberFormat, Rng};
use bmp_game::runtime::Runtime;
use bmp_game::season::{GameOver, SeasonEndEvent, SeasonUi, SpecialDate, Title};
use bmp_game::state::GameState;

use super::cup_screens::DrawBoard;
use super::disk::{HighscoreMode, highscore_load, highscore_save, highscore_screen};
use super::live_match::LiveView;
use super::office::{self, OfficeBitmaps};
use super::start_menu::Pictures;
use super::{Exit, Session, newspaper, season_events};
use crate::host::{Host, Quit, Run};

/// Game font 2 (`flow`), whose widths the newspaper layout measures.
const NEWS_FONT: u16 = 1;

/// `rastport_text_width` of one font.
type TextWidth = dyn Fn(&[u8]) -> i16;

/// The season loop's screens: the host, the session (whose random generator and runtime the
/// loop holds while it runs, see [`ScreenSeason::lend`]) and the live match view.
pub struct ScreenSeason<'a> {
    pub(crate) host: &'a mut Host,
    pub(crate) session: &'a mut Session,
    pics: &'a Pictures,
    bitmaps: &'a OfficeBitmaps,
    /// The player quit inside a callback that cannot stop the loop; the next loop-level
    /// callback returns it.
    quit: Option<Quit>,
    /// `format_long_thousands` state handed to the logic ([`SeasonUi::number_format`]).
    fmt: NumberFormat,
    fmt_lent: bool,
    /// Glyph widths of game font 2 for [`SeasonUi::news_env`].
    text_width: Box<TextWidth>,
    pub(crate) live: LiveView,
    /// The cup draw screen.
    draw: DrawBoard,
}

impl<'a> ScreenSeason<'a> {
    pub fn new(
        host: &'a mut Host,
        session: &'a mut Session,
        pics: &'a Pictures,
        bitmaps: &'a OfficeBitmaps,
    ) -> Self {
        let widths: Vec<u16> = (0..=255u8)
            .map(|c| host.g.text_width(NEWS_FONT, &[c]))
            .collect();
        let text_width = Box::new(move |t: &[u8]| {
            t.iter()
                .fold(0u16, |a, &c| a.wrapping_add(widths[usize::from(c)])) as i16
        });
        let fmt = host.number_format();
        Self {
            host,
            session,
            pics,
            bitmaps,
            quit: None,
            fmt,
            fmt_lent: false,
            text_width,
            live: LiveView::default(),
            draw: DrawBoard::default(),
        }
    }

    /// Writes a number format the logic changed back into the host globals.
    pub(crate) fn sync_format(&mut self) {
        if self.fmt_lent {
            self.host.number_pad = self.fmt.pad_width;
            self.host.number_no_separators = self.fmt.no_separators;
            self.fmt_lent = false;
        }
    }

    /// Runs `f` with the loop's random generator and runtime moved back into the session, as
    /// the office screens read them from there.
    pub(crate) fn lend<R>(
        &mut self,
        rng: &mut Rng,
        rt: &mut Runtime,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        std::mem::swap(&mut self.session.rng, rng);
        std::mem::swap(&mut self.session.rt, rt);
        let r = f(self);
        std::mem::swap(&mut self.session.rng, rng);
        std::mem::swap(&mut self.session.rt, rt);
        r
    }

    /// `FAR:5354`: the crests of the managers' clubs.
    pub(crate) fn crest_cache(&self) -> crate::gfx::BitmapId {
        self.pics.crest_cache
    }

    /// `FAR:2f24`: the manager portraits.
    pub(crate) fn portraits(&self) -> crate::gfx::BitmapId {
        self.pics.portraits
    }

    /// Remembers a quit from a callback that cannot return it.
    pub(crate) fn keep(&mut self, r: Run<()>) {
        if let Err(q) = r {
            self.quit = Some(q);
        }
    }

    /// Whether the player quit; screens are skipped from then on.
    pub(crate) fn quitting(&self) -> bool {
        self.quit.is_some()
    }

    fn check(&mut self) -> Result<(), Quit> {
        self.sync_format();
        match self.quit.take() {
            Some(q) => Err(q),
            None => Ok(()),
        }
    }
}

impl SeasonUi for ScreenSeason<'_> {
    type Stop = Quit;

    fn office_turn(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        arg: u8,
    ) -> Result<(), Quit> {
        self.check()?;
        let (pics, bitmaps) = (self.pics, self.bitmaps);
        self.lend(rng, rt, |ui| {
            ui.host.input_wait_release()?;
            office::office_menu_dispatcher(ui.host, ui.session, pics, bitmaps, state, arg)
        })
    }

    /// Origin: BMMAIN.EXE 1310:01f2 (matchday_show_title)
    fn title(&mut self, _state: &GameState, title: Title) -> Result<(), Quit> {
        self.check()?;
        let text: &[u8] = match title {
            Title::League => b"Ligaspiel",
            Title::DfbCup => b"DFB-Pokal",
            Title::EuropeanCup => b"Europapokal",
            Title::Relegation => b"Relegationsspiel",
            Title::Rescheduled => b"Nachholspiele",
            Title::SeasonEnd => b"SAISONENDE",
        };
        let auto = self.session.auto_play;
        super::live_match::matchday_show_title(self.host, text, auto)
    }

    fn winter_break(&mut self) -> Result<(), Quit> {
        self.check()?;
        let auto = self.session.auto_play;
        season_events::winter_break(self.host, auto)
    }

    fn special_date(&mut self, state: &GameState, date: SpecialDate) -> Result<(), Quit> {
        self.check()?;
        let auto = self.session.auto_play;
        season_events::special_date_screen(self.host, state, date, auto)
    }

    fn championship_winner(&mut self, state: &GameState, m: u8) -> Result<(), Quit> {
        self.check()?;
        let auto = self.session.auto_play;
        super::live_match::screen_title_winner(self.host, state, m, 0, auto)
    }

    fn clear_screen(&mut self) {
        let (g, rp) = (&mut self.host.g, &mut self.host.rp);
        crate::gfx::ui_fill_rect(g, rp, 0, 0, 319, 239, 0);
    }

    fn highscores(&mut self, state: &GameState) -> HighScoreTable {
        highscore_load(&self.host.data, state).0
    }

    fn highscore_insert(&mut self, state: &GameState, rt: &mut Runtime, rng: &mut Rng) {
        let mut st = state.clone();
        let r = self.lend(rng, rt, |ui| {
            highscore_screen(ui.host, ui.session, &mut st, HighscoreMode::Insert)
        });
        self.keep(r);
    }

    /// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop, 2a58:432a..4341)
    fn game_end(&mut self, state: &GameState, table: &HighScoreTable) -> Result<(), Quit> {
        self.check()?;
        highscore_save(&self.host.data, state, table);
        let mut st = state.clone();
        highscore_screen(self.host, self.session, &mut st, HighscoreMode::Show)?;
        let auto = self.session.auto_play;
        super::season_end::game_end_screen(self.host, auto)
    }

    fn ask_draw(&mut self, cup: u8) -> bool {
        if self.quitting() {
            return false;
        }
        let auto = self.session.auto_play;
        let r = self.draw.ask(self.host, auto, cup);
        r.unwrap_or_else(|q| {
            self.quit = Some(q);
            false
        })
    }

    fn draw_slot(&mut self, state: &GameState, cup: u8, slot: u8, team: u8) {
        if !self.quitting() {
            let r = self.draw.animate_slot(self.host, state, cup, slot, team);
            self.keep(r);
        }
    }

    fn draw_close(&mut self, state: &mut GameState, cup: u8) {
        if !self.quitting() {
            let r = self.draw.close(self.host, self.session, state, cup);
            self.keep(r);
        }
    }

    fn cup_winner(&mut self, state: &GameState, m: u8, comp: u8) {
        if !self.quitting() {
            let auto = self.session.auto_play;
            let r = super::live_match::screen_title_winner(self.host, state, m, comp, auto);
            self.keep(r);
        }
    }

    fn forfeit_penalty(&mut self, state: &GameState, m: u8) {
        self.sync_format();
        if self.quitting() {
            return;
        }
        let auto = self.session.auto_play;
        let r = season_events::forfeit_dialog(self.host, state, m, auto);
        self.keep(r);
    }

    fn finance_events(&mut self, state: &GameState, m: u8, events: &[FinanceEvent]) {
        if events.is_empty() || self.quitting() {
            return;
        }
        self.sync_format();
        let bitmaps = season_events::HeaderBitmaps {
            portraits: self.pics.portraits,
            crests: self.pics.crest_cache,
        };
        let auto = self.session.auto_play;
        let r = season_events::finance_dialogs(self.host, state, m, events, bitmaps, auto);
        self.keep(r);
    }

    fn number_format(&mut self) -> &mut NumberFormat {
        if !self.fmt_lent {
            self.fmt.pad_width = self.host.number_pad;
            self.fmt.no_separators = self.host.number_no_separators;
            self.fmt_lent = true;
        }
        &mut self.fmt
    }

    fn auto_play(&self) -> bool {
        self.session.auto_play
    }

    fn goal_scene(
        &mut self,
        _state: &GameState,
        rng: &mut Rng,
        goal: bool,
        side: u8,
        penalty: u8,
    ) -> u8 {
        self.sync_format();
        if self.quitting() {
            return penalty;
        }
        let ybase = self.live.event_base();
        let auto = self.session.auto_play;
        let r = super::goal_scene::goal_scene(
            self.host,
            &mut self.live,
            rng,
            auto,
            goal,
            side,
            ybase,
            penalty,
            &mut self.session.goal_scene_author,
        );
        match r {
            Ok(scene) => scene,
            Err(q) => {
                self.quit = Some(q);
                penalty
            }
        }
    }

    fn substitution(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        m: u8,
        cup_mode: u8,
        stretcher: bool,
    ) {
        self.sync_format();
        if self.quitting() {
            return;
        }
        let r = super::live_match::injury_and_substitution(
            self, state, rt, rng, m, cup_mode, stretcher,
        );
        self.keep(r);
    }

    fn season_end(&mut self, state: &GameState, event: SeasonEndEvent<'_>) {
        self.sync_format();
        if !self.quitting() {
            let r = super::season_end::season_end_event(self, state, event);
            self.keep(r);
        }
    }

    /// `contract_negotiation(slot, 1, 0, 0)`: the single mode, whose terms the season end
    /// decides by the late-season rules.
    ///
    /// Origin: BMMAIN.EXE 3277:2a8f (contract_negotiation)
    fn contract_negotiation(
        &mut self,
        state: &mut GameState,
        _rt: &Runtime,
        rng: &mut Rng,
        slot: u8,
    ) -> bool {
        self.sync_format();
        if self.quitting() {
            return false;
        }
        std::mem::swap(&mut self.session.rng, rng);
        let r =
            super::negotiation::contract_negotiation_single(self.host, self.session, state, slot);
        std::mem::swap(&mut self.session.rng, rng);
        match r {
            Ok(signed) => signed,
            Err(q) => {
                self.quit = Some(q);
                false
            }
        }
    }

    fn news_env(&self) -> NewsEnv<'_> {
        // The newspaper loads its pictures (vga_load_picture sets DS:079c to 1) before it
        // expands the texts.
        NewsEnv {
            fmt: NumberFormat {
                pad_width: 1,
                ..self.host.number_format()
            },
            text_width: &*self.text_width,
        }
    }

    fn show_newspapers(&mut self, _state: &GameState, papers: Vec<Newspaper>) {
        self.sync_format();
        for paper in &papers {
            if self.quitting() {
                return;
            }
            let auto = self.session.auto_play;
            let r = newspaper::newspaper_show(self.host, paper, auto);
            self.keep(r);
        }
    }

    fn penalty_shootout_start(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        cup: u8,
        slot: u8,
    ) {
        self.sync_format();
        if !self.quitting() {
            let r = super::live_match::penalty_shootout_start(self, state, rt, cup, slot);
            self.keep(r);
        }
    }

    fn penalty_shootout_end(
        &mut self,
        state: &mut GameState,
        _rt: &mut Runtime,
        cup: u8,
        slot: u8,
    ) {
        self.sync_format();
        if !self.quitting() {
            let r = super::live_match::penalty_shootout_end(self, state, cup, slot);
            self.keep(r);
        }
    }

    fn period_start(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        league_mask: u8,
        cup_mask: u8,
        from: u8,
    ) {
        self.sync_format();
        if !self.quitting() {
            let r = super::live_match::period_start(self, state, rt, league_mask, cup_mask, from);
            self.keep(r);
        }
    }

    fn period_minute(&mut self, state: &GameState, rt: &Runtime, to: u8, minute: u8) {
        if !self.quitting() {
            let r = super::live_match::period_minute(self, state, rt, to, minute);
            self.keep(r);
        }
    }

    fn pause_requested(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        _minute: u8,
    ) -> bool {
        self.sync_format();
        if self.quitting() {
            return false;
        }
        match super::live_match::match_pause_poll(self, state, rt, rng) {
            Ok(paused) => paused,
            Err(q) => {
                self.quit = Some(q);
                false
            }
        }
    }

    fn match_handle_panel_click(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        cup_mask: u8,
        home: u8,
        away: u8,
        _slot: u8,
        _comp: u8,
        _minute: u8,
    ) -> Option<u8> {
        self.sync_format();
        if self.quitting() {
            return None;
        }
        let (m, r) =
            super::live_match::match_handle_panel_click(self, state, rt, rng, cup_mask, home, away);
        self.keep(r);
        m
    }

    fn period_end(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        league_mask: u8,
        cup_mask: u8,
        tier: Option<u8>,
        to: u8,
    ) {
        self.sync_format();
        if !self.quitting() {
            let r = self.lend(rng, rt, |ui| {
                super::live_match::period_end(ui, state, league_mask, cup_mask, tier, to)
            });
            self.keep(r);
        }
    }

    fn matchday_results(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        league_mask: u8,
        cup_mask: u8,
        penalties: bool,
    ) {
        self.sync_format();
        if !self.quitting() {
            let relegation = rt.relegation_mode != 0;
            let r = self.lend(rng, rt, |ui| {
                super::live_match::matchday_results(
                    ui,
                    state,
                    league_mask,
                    cup_mask,
                    penalties,
                    relegation,
                )
            });
            self.keep(r);
        }
    }

    fn ticker_event_start(&mut self, state: &GameState, rt: &Runtime, event: &TickerEvent) {
        self.sync_format();
        if !self.quitting() {
            let r = super::live_match::ticker_event_start(self, state, rt, event);
            self.keep(r);
        }
    }

    fn ticker_event_end(&mut self, state: &GameState, rt: &Runtime, event: &TickerEvent) {
        self.sync_format();
        if !self.quitting() {
            let r = super::live_match::ticker_event_end(self, state, rt, event);
            self.keep(r);
        }
    }
}

/// `season_main_loop(resume)`: runs the season loop with the screens until the game ends or
/// the player quits.
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop)
pub fn season_main_loop(
    host: &mut Host,
    session: &mut Session,
    pics: &Pictures,
    bitmaps: &OfficeBitmaps,
    state: &mut GameState,
    resume: bool,
) -> Run<Exit> {
    // The loop owns the random generator and the runtime while it runs.
    let mut rng = std::mem::take(&mut session.rng);
    let mut rt = std::mem::take(&mut session.rt);
    let mut ui = ScreenSeason::new(host, session, pics, bitmaps);
    let r = bmp_game::season::season_main_loop(state, &mut rt, &mut rng, &mut ui, resume);
    ui.sync_format();
    drop(ui);
    session.rng = rng;
    session.rt = rt;
    match r {
        Ok(GameOver) => Ok(Exit::GameOver),
        Err(q) => Err(q),
    }
}
