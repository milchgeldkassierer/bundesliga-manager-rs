# Input audit: office

Scope: the office turn `office_menu_dispatcher` 18bc:0b5f with `office_handle_free_click`
18bc:08f2 (calendar sheet, header, desk/mail, automatic-play corners), `quit_game_confirm`
18bc:1c1d, the mail `screen_messages` 4091:05c4, and the info screens of main icon 1:
statistics 362f:0af7, eternal table 362f:18e7, history 362f:390e, stadium 1000:0602 (with
`stadium_expansion_order` 1000:0000), loans 212a:167f (with `credit_take` 212a:1039), plus the
sponsor offer 1000:272d and the shared dialogs of 4091/176b they use.

Method: `get_xrefs_to` on `input_poll_mouse` 176b:0319, `ui_hit_test_buttons` 16c7:0002,
`ui_scroll_text_box` 176b:07d7, `ui_value_spinner` 4091:0ba5, `dialog_two_choice` 4091:146c,
`dialog_okay` 4091:0e3a, `ui_dialog_box` 176b:0e57, `ui_wait_right_click_button` 4091:1722,
`ui_hit_icon_slot` 4091:11cc, `ui_messages_wait_button` 4091:0aaa, `input_wait_click`
176b:0d0a, `input_wait_release` 176b:0e27, `dos_getch` 45ac:326b and `dos_direct_console_io`
45ac:3272, filtered to the functions above; then every read of `FAR:2ea2` (button event) and
`FAR:6dc0` (pointer y/x) inside them. No keyboard read exists in this area (the only key input
is `ui_scroll_text_box`'s number entry of the loans screen). The dispatcher does not decompile
(overlapping instructions); it was read from the disassembly.

Status: **ok** = the port does the same, **fixed** = missing or wrong, ported with this audit.

## Office dispatcher 18bc:0b5f

| # | Input site / call | Original | Port | Status |
|---|---|---|---|---|
| 1 | Autosave at the start of the turn: `DS:5256` (set by the month end 212a:0a6d, months 0/4/8) → `screen_save_game("autosave.man")`, then clear | 18bc:0b74..0b8f | `office.rs` `office_autosave` (before: comment "DS:5256 is 0", never saved) | **fixed** |
| 2 | Main level: left click on a main icon (`ui_hit_test_buttons(rects, 6, 1)`), ignored for icon 6 and for the selected icon | 18bc:144f..143b | `office_menu_dispatcher` `'main` loop | ok |
| 3 | Main level: right click (`2ea2 == 3`) on the quit icon (hit test without event check == 6) → `quit_game_confirm`, then the stale result of the last free click is evaluated | 18bc:146a..1494, 14a3 | same | ok |
| 4 | Main level: any other event → `office_handle_free_click(sel)` (1 ends the turn, 2 redraws) | 18bc:1496..14b4 | same | ok |
| 5 | Submenu: left click among 15 rects (9 sub icons + 6 main icons), validity: not the own main icon, not past the icon count, not icon 6, not disabled (camp cooldown, contract scenarios 0x7ac/0x7c9, 4 managers) | 18bc:1744..17eb | same (`sub_icon_disabled`) | ok |
| 6 | Submenu: right click on the quit icon → `quit_game_confirm`, then `office_handle_free_click` | 18bc:17ee..182c | same | ok |
| 7 | Submenu: right click elsewhere → restore the submenu area from page 2, redraw the main icon, **no icon selected** (`[bp-0x14] = 0xff`), reopen = 0 | 18bc:182f..18df | before: `sel` kept, so the same main icon did not open its submenu again and a desk click did not open the mail; now `sel = -1` | **fixed** |
| 8 | Submenu: other invalid events → `office_handle_free_click(sel)` | 18bc:1818..18ec | same | ok |
| 9 | Submenu: click on another main icon → reopen with that icon | 18bc:18ef..195a | same | ok |
| 10 | Submenu: sub icon chosen → pressed frame until release (`ui_wait_right_click_button`), `input_wait_release`, then the screen (jump tables 18bc:1a15 / 18bc:1b42) | 18bc:195d..1a95 | same | ok |

## office_handle_free_click 18bc:08f2

| # | Input site | Original | Port | Status |
|---|---|---|---|---|
| 11 | Pointer on (0, 0) during automatic play → "Spiele nicht alleine !", automatic play off | 18bc:0920..0953 | `office_handle_free_click` | ok |
| 12 | Left click on the calendar sheet (x 0xf3..0x131, y 0x32..0x65), or automatic play, unless saving is blocked (`DS:05ad`) → end of turn | 18bc:0958..09b6 | same | ok |
| 13 | Left click on (1, 1) → "Spiele automatisch weiter !", automatic play on, `DS:063a = 0x47` | 18bc:09b9..09ef | same | ok |
| 14 | Left click on the header (y < 0x2c) → `matchday_info_panel` 1cb5:2bf9 (waits with `input_wait_click`), redraw | 18bc:09f2..0a2e | `matchday_info_panel` | ok |
| 15 | Left click on the desk (x 0x3a..0x107, y 0x72..0xfc) while no main icon is selected → mail | 18bc:0a31..0a58 | same (reachable after a right-click close only with fix 7) | ok |
| 16 | `quit_game_confirm` 18bc:1c1d: `dialog_two_choice` "LEIDER JA" ends the program | 18bc:1c97 | `disk.rs` `quit_game_confirm` | ok |

## Mail 4091:05c4

| # | Input site | Original | Port | Status |
|---|---|---|---|---|
| 17 | `ui_messages_wait_button` 4091:0aaa: left click on the strip x 0xf6..0x105, y 0x6c..0xe7 → (y-0x6c)/0x29 (up / exit / down); automatic play → exit | 4091:0ace..0aea | `messages.rs` `ui_messages_wait_button` | ok |
| 18 | Pressed strip until the button is released (`2ea2 == 2`) or automatic play; paging rules | 4091:0922..09cf | `screen_messages`, `messages_click` | ok |

## Info screens (362f)

| # | Input site | Original | Port | Status |
|---|---|---|---|---|
| 19 | Statistics: wait for the exit icon (`ui_hit_test_buttons(rects, 1, 1)`) | 362f:18b7 | `stats_screens.rs` `screen_statistics` (`wait_hit`) | ok |
| 20 | Statistics: pressed exit icon until release, `DS:079c = 1` | 362f:18d0..18db | same | ok |
| 21 | Eternal table: exit icon | 362f:21bd | `screen_eternal_table` | ok |
| 22 | Eternal table: pressed exit icon | 362f:21d4 | same | ok |
| 23 | History: two rects (list = next page, exit); the last page only reacts to exit | 362f:404d..40c9 | `screen_manager_history` | ok |
| 24 | History without entries: wait for exit | 362f:40fe..410d | same | ok |
| 25 | History: pressed exit icon, `DS:079c = 1` | 362f:4117..4122 | same | ok |

## Stadium 1000:0602 / 1000:0000

| # | Input site | Original | Port | Status |
|---|---|---|---|---|
| 26 | Exit icon (`ui_hit_test_buttons(rects, 1, 1)`) | 1000:0760 | `stadium.rs` `screen_stadium` | ok |
| 27 | Hover over the table (x 7..0x120, y 0x73..0xb8): row (y-0x72)>>3 → status line help; leaving clears it (with the x/row comparison quirk) | 1000:076e..07c0, 1688..16c3 | same (`probe`) | ok |
| 28 | Left click on rows 1..8 | 1000:07cf | same | ok |
| 29 | Row locked by construction → "Im Moment keine Baufirma aufzutreiben." | 1000:0809 | `handle_row` | ok |
| 30 | Rows 1..3: amount spinner (0..max, step 1000), then the order | 1000:0c8e..0cdf | `handle_row` | ok |
| 31 | Row 8: ticket price spinner (step 1/10 by option), stored in the manager | 1000:0d97..0e9a | `handle_row` | ok |
| 32 | Rows 4..7: level selection loop, right click cancels | 1000:1427..1437 | `choose_level` | ok |
| 33 | Rows 4..7: left click on an affordable, higher level → order | 1000:150c..1446 | `choose_level` | ok |
| 34 | Rows 4..7: hover highlight (x 0x97..0x11f, y 0x1a..0x41, row (y-0x1c)/7 ≤ max) | 1000:1543..1609 | `choose_level` | ok |
| 35 | Pressed exit icon until release | 1000:16d6 | `screen_stadium` | ok |
| 36 | Expansion order "NA KLAR!" / "ACH NEE" (`ui_hit_icon_slot`, left click); refusal draws `random_range(15, 55)` | 1000:0547..0584 | `stadium_expansion_order` | ok |

## Loans 212a:167f / 212a:1039

| # | Input site | Original | Port | Status |
|---|---|---|---|---|
| 37 | Left click on a lender button (fellow managers, bank) other than the current one → switch lender, redraw list | 212a:1adf..1b99 | `credits.rs` `screen_credits` | ok |
| 38 | Left click in the column x 0x84..0x91 outside y 0x6a..0x83 and 0xa0..0xba → toggle "debts/interest here" view | 212a:1bb4..1c40 | same | ok |
| 39 | Hover over the rate table (x 0xa1..0x102, y 0x1f..0x5c) → status line (bank: rate, manager: name) | 212a:1e5x..1ec2 | same | ok |
| 40 | Left click on a row → amount input (`ui_scroll_text_box`, digits only) and the lender/limit checks (`dialog_okay`) | 212a:1ea7..20b0 | `take_loan` | ok |
| 41 | Fellow manager: months input, > 24 → "Maximal 24 Monate." | 212a:211a..2189 | `take_loan` | ok |
| 42 | Fellow manager: rate input, > 13 → the two "Wucher" dialogs, < 3 → "Mindest-Prozentsatz" | 212a:21d6..22ca | `take_loan` | ok |
| 43 | `credit_take` full → "Schon 3 Kredite aufgenommen." | 212a:10b3 | `take_loan` | ok |
| 44 | Exit icon (index count+1), pressed until release | 212a:23a7 | `screen_credits` | ok |

## Sponsor offer and shared dialogs

| # | Input site | Original | Port | Status |
|---|---|---|---|---|
| 45 | Sponsor subsidy "ABER IMMER!" / "LIEBER NICHT." (`dialog_two_choice`) | 1000:2861 | `transfer_screen.rs` `sponsor_subsidy_offer` | ok |
| 46 | `dialog_okay`: pointer put on OKAY, left click on the button or automatic play, pointer restored | 4091:0f78..0ffb | `widgets.rs` `dialog_okay_auto` | ok |
| 47 | `dialog_two_choice`: pointer on the right button, left click by coordinates, automatic play → -1 | 4091:1654..1712 | `dialog_two_choice` | ok |
| 48 | `ui_dialog_box`: timed, no input | 176b:0e57 | `ui_dialog_box` | ok |
| 49 | `ui_value_spinner`: left click on a pixel of colour 4 in the upper/lower half, right click ends | 4091:0ba5 | `ui_value_spinner` | ok |
| 50 | `ui_wait_right_click_button`: pressed frame until left release or automatic play | 4091:1722 | `ui_wait_right_click_button` | ok |
| 51 | `ui_hit_icon_slot` text buttons | 4091:11cc | `TextButtons::hit` | ok |
| 52 | `input_wait_click` of the header panel | 176b:0d0a | `input_wait_click` (TODO(re) on `FAR:3046` predates this audit) | ok |

## Result

52 sites, 50 already correct, 2 fixed:

* **Autosave** (#1): the port never consumed `autosave_pending`, so AUTOSAVE.MAN was never
  written and the RNG draws of the save (`savegame_write_man`) were missing after the month
  ends of January, May and September. `office_autosave` now saves `"autosave.man"` at the start
  of the office turn and clears the request; `screen_save_game` resolves a fixed name like DOS
  (an existing file whatever its case, else upper case). Unit test
  `office::tests::a_pending_autosave_is_written_and_cleared`.
* **Right-click close** (#7): after closing a submenu with a right click the port kept the main
  icon selected. Visual scenario `office_reopen` (DOSBox references): the same icon reopens its
  submenu (o03) and the desk opens the mail (o04); both frames were 76-77 % before the fix.

No hook with a no-op default belongs to this area (`SeasonUi::office_turn` is required).
