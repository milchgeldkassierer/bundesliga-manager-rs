# Input audit: team screens

Scope: office menu 2 ("Mannschaft") and the screens reached from it: squad / lineup and tactics
board (segment 2ecd), contracts and negotiation, transfer market and offers (3277), training and
training camp (212a), top scorers and player info (24a4).

Method: every caller of `input_poll_mouse` 176b:0319, `ui_hit_test_buttons` 16c7:0002,
`ui_scroll_text_box` 176b:07d7, `ui_value_spinner` 4091:0ba5, `dialog_two_choice` 4091:146c,
`dialog_okay` 4091:0e3a, `ui_dialog_box` 176b:0e57, `ui_wait_right_click_button` 4091:1722,
`input_wait_release` 176b:0e27 and `input_wait_click` 176b:0d0a was listed with `get_xrefs_to`
and filtered to the functions of this area. Reads of `FAR:2ea2` (mouse event) and `FAR:6dc0`
(pointer, `[0]` = y, `[1]` = x) go through the far-data selector table and have no direct xrefs,
so every function of the area was decompiled and read for them. Keyboard reads (`dos_getch`
45ac:326b, `dos_direct_console_io` 45ac:3272) have no caller in this area; the only keyboard input
is the number entry inside `ui_scroll_text_box`.

Mouse event codes (`mouse_pop_button_event`): 1 = left down, 2 = left up, 3 = right down,
4 = right up. The transfer market also tests `0xe8`, which the mouse queue never produces
(transfer.md §7, kept in the port as `SELECT_EVENT`).

## Input sites

| # | Screen | Input site (condition -> action) | Original | Port | Status |
|---|---|---|---|---|---|
| 1 | Squad | list row / icons via `squad_list_hover_info` (hover text + XOR row highlight; left or right click on a valid cell returns the row; button hit returns `0x64 + n`) | 2ecd:000e | `squad_screen.rs` `squad_list_hover_info` | ok |
| 2 | Squad | left click on the tactic slider (x 0x10b..0x13e, y 5..0x23) -> slider = x - 0x113 clamped 0..0x22 | 2ecd:1560 (loop 2ecd:1865) | `screen_squad_lineup` | ok |
| 3 | Squad | left click on nothing (no row, no board hit) -> `auto_lineup_disable` | 2ecd:1560 | `screen_squad_lineup` | ok |
| 4 | Squad | row click while auto lineup is on -> "Auto-Aufstellung ist aktiviert !" | 2ecd:1560 | `screen_squad_lineup` | ok |
| 5 | Squad | row click on suspended/injured, not lined up (match), no substitution left -> message boxes | 2ecd:1560 | `screen_squad_lineup` | ok |
| 6 | Squad | left click on a starter (or, in a match, a starter) -> remove (min-players check, board spot kept) | 2ecd:1d2d | `screen_squad_lineup` | ok |
| 7 | Squad | right click on a non-selected player (office only) -> substitute with the next free number ("Maximal n Reservespieler !") | 2ecd:1ec3 | `screen_squad_lineup` | ok |
| 8 | Squad | left click on a non-selected player or a substitute -> into the eleven (max-players check; in a match: substitution counters, appearances, exhaustion RNG) | 2ecd:2042 | `screen_squad_lineup` | ok |
| 9 | Squad | icon 0x66 -> open the tactics board | 2ecd:1560 | `screen_squad_lineup` | ok |
| 10 | Squad | icon 0x67 -> contracts tab (`contract_negotiation(-1)`) | 2ecd:1560 | `contract_negotiation_browse` | ok |
| 11 | Squad | icon 0x65 with the board open -> close the board | 2ecd:1560 | `screen_squad_lineup` | ok |
| 12 | Squad | exit icon 0x68 -> `ui_wait_right_click_button`, `squad_recalc_team_strength(m, p1 != 0)` | 2ecd:2474, 2ecd:24c0 | `screen_squad_lineup` | ok |
| 13 | Tactics board | left click on a formation button (office only, `p1 == 0`) -> formation k+1, auto lineup | 2ecd:29c6 | `board_handle_input` | ok |
| 14 | Tactics board | pointer over the board -> cursor sprite, list highlight of the starter, strength line | 2ecd:29c6 | `board_handle_input` | ok |
| 15 | Tactics board | left click on the board -> pick up a starter / drop on an empty spot / swap with a starter (and `auto_lineup_disable`) | 2ecd:29c6 | `board_handle_input` | ok |
| 16 | Contracts | row click; right click -> player info | 3277:2a8f (2dc1) | `contract_negotiation_browse` | ok |
| 17 | Contracts | icon 0x66 -> board in view mode (pick/drop works, no formation buttons); 0x65 closes it; 0x68 leaves the squad screen | 3277:2a8f | `contract_negotiation_browse` | ok |
| 18 | Contracts | loaned / retiring / "nicht verhandlungsbereit" players -> `dialog_okay`; pointer at (0,0) after the refusal bypasses it | 3277:2a8f (2e4e, 2ec3) | `contract_negotiation_browse` | ok |
| 19 | Negotiation | left click on NEUER VERTRAG / ABBRUCH | 3277:325e | `negotiate_input` | ok |
| 20 | Negotiation | left click in the seasons line (y 0xd3..0xd9) -> number entry (not for loaned players), salary suggestion | 3277:341d | `negotiate_input` | ok |
| 21 | Negotiation | left click in the salary line -> number entry | 3277:370d | `negotiate_input` | ok |
| 22 | Negotiation | decision dialogs (too long, "So dumm", not interested) | 3277:32bb, 383b, 39e8 | `negotiate` | ok |
| 23 | Transfer market | own list row left click -> pending offer (BEHALTEN/VERKAUFEN) or put on the market (3 max, loaned, list full, squad full) | 3277:0989 | `own_player_click` | ok |
| 24 | Transfer market | own list row right click -> player info | 3277:099b | `own_player_click` | ok |
| 25 | Transfer market | market row hover -> XOR highlight + "VON ..." line | 3277:08c8 | `screen_transfer_market` | ok |
| 26 | Transfer market | market row right click (or 0xe8) -> `market_click`; right click = info for an own listed player only | 3277:08c8, 3277:108d | `screen_transfer_market`, `market_click` | ok |
| 27 | Transfer market | click on the market panel -> row from y; below y 0x6d toggles KAUFEN/LEIHEN (not in the one-year games) | 3277:04a5 | `screen_transfer_market` | ok |
| 28 | Transfer market | bid entry "Ihr Angebot:" | 3277:11d9 | `market_click` | ok |
| 29 | Transfer market | other human manager asked (`dialog_two_choice`) | 3277:1522, 1668 | `market_click` | ok |
| 30 | Transfer market | purchase -> contract negotiation (single), "Der Transfer findet nicht statt", sponsor subsidy offer | 3277:1768, 19f0..1ab2 | `market_click` | ok |
| 31 | Transfer market | exit -> `ui_wait_right_click_button`, then `squad_recalc_team_strength(m, 0)` | 3277:1b3f, 3277:1b52 | `screen_transfer_market` | fixed (recalculation was missing, `TODO(#24)`) |
| 32 | Offer dialog | left click on BEHALTEN / VERKAUFEN | 3277:1e0b | `transfer_offer_received` | ok |
| 33 | Training | left click on the intensity bar (x 0xd1..0x104, y 0xc1..0xe8) | 212a:274c | `screen_training` | ok |
| 34 | Training | left click on a slot bar of group A (x 0x1f..0xbd) / group B (x 0xd2..0x120), rows by y; free-slot checks; row 4 (y 0xbd..0xcd) | 212a:274c | `screen_training` | ok |
| 35 | Training | exit -> `ui_wait_right_click_button` (no strength recalculation in the original) | 212a:3126 | `screen_training` | ok |
| 36 | Training camp | hover over a legend letter / camp -> status line with legend text or price | 212a:0145 | `screen_training_camp` | ok |
| 37 | Training camp | left click on a camp -> closed / no money / book (camp picture, training, cool-down RNG) | 212a:0145 (05a9, 0634) | `screen_training_camp` | ok |
| 38 | Training camp | exit (not after booking: `ui_wait_right_click_button`), then always `squad_recalc_team_strength(m, 0)` | 212a:070d, 212a:0720 | `screen_training_camp` | fixed (recalculation was missing, `TODO(#24)`) |
| 39 | Top scorers | left or right click on a listed row of a human manager's player -> player info | 24a4:1ef3, 24a4:231b | `screen_top_scorers` | ok |
| 40 | Top scorers | left click on LIGA / SPIELER -> toggle mode and redraw | 24a4:1ad5 | `screen_top_scorers` | ok |
| 41 | Top scorers | left click on exit -> `ui_wait_right_click_button` | 24a4:23ca | `screen_top_scorers` | ok |
| 42 | Player info | wait for a left click on the OKAY icon | 24a4:1495 | `screen_player_info` | ok |
| 43 | Squad insert | "Schon 12 Mann auf dem Transfermarkt" / "Schon 24 Mann im Team" | 2ecd:3822 | `insert_rejected` | ok |
| 44 | Auto lineup off | "Auto-Aufstellung ist deaktiviert !" | 2ecd:1540 | `auto_lineup_disable` | ok |

## Functions called in the original but not in the port

| Function | Original call | Port | Status |
|---|---|---|---|
| `squad_recalc_team_strength` 1f9d:0002 | 3277:1b52 (transfer market exit), 212a:0720 (training camp exit) | was a `TODO(#24)` in both screens | fixed |
| `screen_top_scorers(1)` | 2a58:3c2a (season end, top-scorer flag) | `bmp_game::stats` / season end | outside this area (season), not re-audited |

## Hooks with no-op defaults

None in this area: the team screens call the host and the `bmp_game` logic directly, there is no
trait with default methods between them.

## Result

44 input sites examined, 42 were correct, 2 fixed (one missing call, at two exits). No GitHub issue
was needed.
