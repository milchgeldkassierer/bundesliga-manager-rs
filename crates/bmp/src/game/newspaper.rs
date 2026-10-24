//! The newspaper after a match day (segment 3e3a). The page's content, its random photo and
//! the text layout come from [`bmp_game::news::newspaper_generate`]; this module draws it.
//! Spec: `docs/re/logic/news.md` §4.1, §4.5, §5.2-5.8.

use bmp_game::news::{LineStyle, NewsLine, Newspaper, label_value_split};

use super::widgets::input_wait_click;
use crate::gfx::{ui_draw_frame, ui_draw_text_at, ui_draw_text_centered_shadow, ui_fill_rect};
use crate::host::{Host, Run};

/// `PIC\44.VGA`: the masthead.
const MASTHEAD: u16 = 0x2c;
/// `DS:9394`: the grey palette of the page (`0x0RGB`).
const NEWS_PALETTE: [u16; 16] = [
    0x000, 0x888, 0x444, 0xccc, 0x222, 0xaaa, 0x666, 0xeee, 0x111, 0x999, 0x555, 0xddd, 0x333,
    0xbbb, 0x777, 0xfff,
];

/// Draws one laid-out line: plain in colour 0, or "label:" in colour 6 and the value in
/// colour 0x0e.
///
/// Origin: BMMAIN.EXE 3e3a:06df (news_text_flow, draw_line) and 3e3a:060a
/// (news_draw_label_value_line)
fn draw_line(host: &mut Host, line: &NewsLine) {
    let (g, rp) = (&mut host.g, &mut host.rp);
    match line.style {
        LineStyle::Plain => ui_draw_text_at(g, rp, &line.text, line.x, line.y, 0),
        LineStyle::LabelValue => match label_value_split(&line.text) {
            None => ui_draw_text_at(g, rp, &line.text, line.x, line.y, 0x0e),
            Some((label, value)) => {
                ui_draw_text_at(g, rp, label, line.x, line.y, 6);
                let w = g.rastport_text_width(rp, label);
                ui_draw_text_at(g, rp, value, line.x + w, line.y, 0x0e);
            }
        },
    }
}

/// Shows one newspaper page: the paper background with the masthead, the grey palette, the
/// framed photo centred in its box, the two headline lines (font 1), the body and the sidebar
/// (font 2); then it waits for a click and restores the game palette.
///
/// Origin: BMMAIN.EXE 3e3a:0ea3 (newspaper_generate_and_show, the screen), 3e3a:0dba
/// (newspaper_draw_headline), 3e3a:0c0b (newspaper_draw_body) and 3e3a:000e
/// (newspaper_draw_sidebar)
pub fn newspaper_show(host: &mut Host, paper: &Newspaper, auto: bool) -> Run<()> {
    ui_fill_rect(&mut host.g, &mut host.rp, 0, 0, 0x13f, 0xef, 9);
    ui_fill_rect(&mut host.g, &mut host.rp, 5, 1, 0x13a, 0xee, 0x0f);
    let screen = host.screen();
    {
        let pic = host.picture(MASTHEAD)?;
        host.g.blit_bitmap(pic, 0, 0, screen, 10, 3, 0x12b, 0x1e);
        host.g.free(pic);
    }
    host.g.palette_load_rgb4(&NEWS_PALETTE, 16);
    {
        let pic = host.picture(paper.photo)?;
        let (w, h) = {
            let bm = host.g.bitmap(pic);
            (bm.width, bm.height)
        };
        let x = (0x6b - w) / 2 + 0xca;
        let y = (0x6a - h) / 2 + 0x3e;
        ui_draw_frame(
            &mut host.g,
            &mut host.rp,
            x - 2,
            y - 2,
            x + w + 1,
            y + h + 1,
            0,
        );
        host.g.blit_bitmap(pic, 0, 0, screen, x, y, w, h);
        host.g.free(pic);
    }
    host.set_font(1);
    for (line, y) in paper.headline.iter().zip([0x29, 0x33]) {
        ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, line, 0, 0x13f, y, 0, 0xff);
    }
    host.set_font(2);
    for line in paper.body.iter().chain(&paper.sidebar) {
        draw_line(host, line);
    }
    host.screen_fade(1)?;
    input_wait_click(host, auto)?;
    host.screen_fade(0)?;
    host.g.palette_load_slot(0, 0x100);
    Ok(())
}
