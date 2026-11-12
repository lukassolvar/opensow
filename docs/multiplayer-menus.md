# Multiplayer screens (`CMenuNetwork`)

Read from the exe. Entry: main-menu button 3 (MULTIPLAYER, `0x446079` jump table `0x4461b0`; hover sound 29) calls `0x42c260(0x15)`. Dispatcher state `0x15` (jump table `0x42d624`, `0x42d1e7`) = `0x4340f0`: on the first frame `0x449fc0` (init: loads the art, lists the maps; failure goes to state `0x1c`), then every frame `0x4482a0` (update + click handling + draw) on the object `0x71a298`; when the state leaves `0x15` the art is released. Escape (`[0x6bed6d]`) returns to the main menu (`0x42c260(0x1c)`) from every page. Code: `crates/opensow-game/src/multiplayer.rs` (UI only; DirectPlay is stubbed).

The object has a page at `[+8]`: **0 connection type**, **1 sessions**, **2 lobby**. `[+0xc]` = "page just entered" (the page's setup runs once). Buttons are 0x60-byte objects (ctor `0x456470(centre x, y)` + frame rect `0x456490` + frames `0x456440(normal, hover)`, hit test `0x456350`, draw `0x4560d0`, click `0x456410(5)` = pressed frame for 5 ticks + sound `[+0x58]`; hover sound `[+0x54]` = 30 everywhere). Frames sit at *screen centre - hotspot* like docs/options.md.

## Art (all 640x480 design, `Data\PICTURES` and `Data\Sprites2\Menu`)
- `Network.srf` (pages 0 and 1), `Network2.srf` (lobby: left column, two side panels with the five air-force icons and five turret plates each, the CHAT box and two empty ovals at the bottom left); `NetworkBackground.srf` / `NetworkBackground2.srf` (1920x1080) fill the window around the 640x480 picture.
  **Differs from the exe:** the exe centre-crops them (`0x446ca0`); OpenSOW stretches them to the window (`multiplayer.rs`).
- `net_join.tsp` (9 frames, normal / highlighted pairs, the words are in the art): 0 caption "NOW SELECT THE TYPE OF CONNECTION:" (122, 7); 1/2 CONTINUE (238, 405); 3/4 CREATE SESSION (17, 31); 5/6 SHOW ALL SESSIONS (336, 30); 7/8 JOIN SESSION (398, 381).
- `net_others.tsp` (7): 0 ACCEPT (9, 436), 1 BACK (111, 433) (one frame each), 2/3 BLUE SIDE (262, 3), 4/5 GREEN SIDE (467, 3), 6 the orange X (drawn over a switched-off turret).
- `net_arrows.tsp` (56): 28 arrow buttons (frame k; highlighted = k + 28), 14 per side, then `-`/`+` pairs for fighter, bomber, tripler, carryall, asteroid (the five rows), credits, research. Blue arrows at x 221/280 (counts) and 310/397 (credits, research); the green side is the same shifted by 210.
- `net_turrets.tsp` (10): the ten turret plates (blue 0..4 at x 341, green 5..9 at x 551), `options_numbers.tsp` the digits.
- Sounds: hover 30; ACCEPT 38 (`ingame-ano`); BACK, CONTINUE, CREATE, SHOW ALL, JOIN 39 (`ingame-nie`); arrows 33 (`options-arrow`); a player joining 29. (`0x44a3e9..0x44a8ac`: `+0x54 = 0x1e`, `+0x58` 0x27 / 0x26 / 0x21.) The turret plates' click sound was not read; the port uses 33.

## Page 0: connection type
`0x44ebf0` creates `IDirectPlay4A` and enumerates the service providers (`0x44f1d0` count, `0x44f1f0(i)` name). If any exist the first frame selects one **without showing the page**: `0x44f270` returns the first whose name is longer than 35 characters and has `TCP/I` at characters 9..14 ("Internet TCP/IP Connection For DirectPlay"), else 0; `0x44ecf0(index)` initialises it ("Cannot initialize connection" on failure) and the page becomes 1. Only a machine with no provider at all stays on page 0 (caption, an empty list, CONTINUE not offered): rows would be drawn centred at y = cy - 0x8c + 32 i (white, gold `0x00dcff` COLORREF when selected), clickable by `(y - (cy - 0x99)) / 32`. `multiplayer::preferred_connection` is that rule.

## Page 1: sessions
Buttons CREATE SESSION, SHOW ALL SESSIONS and (only with a selected session) JOIN SESSION; no BACK (Escape). The list: `0x44eaa0` enumerates sessions ("Can't enumerate sessions."), `0x44f1e0` count, `0x44f230(i)` name; selecting is by clicking a row (`x` 350..622, row `(y - 100) / 30`, the first row at y = cy - 0x8c, pitch 30); selected row red (`0xff,0,0`), the others grey (192), black shadow (`0x466f40` = text with a 2 px shadow, args: dc, x, y, colour, shadow colour).
**Differs from the exe:** the exe computes x as `W - width/2`, which looks wrong; OpenSOW centres the names in the click column.
The GDI strings 264 / 265 (`CREATE SESSION`, `SHOW ALL SESSIONS`, exe indices = `LangFile` + 1, docs/text.md) are drawn over the art buttons (non-English builds draw them on the art) and string 365 (an empty line) when a session is selected.
- **CREATE SESSION** (`0x44e500`): creates the DirectPlay session "State of War session" ("Cannot open new session.") with the local player "Player1" ("Can't create the net-player"), becomes player 0 / blue side, page 2.
- **JOIN SESSION** (`0x44ef10(selected)`): opens that session ("Cannot open session ...") with player "Player2", becomes player 1 / green side, page 2.
- **SHOW ALL SESSIONS**: enumerates again.

## Page 2: the lobby
Local side `[0x575730]` is 0 (blue, the host) or 1 (green). Both sides configure only their own column; the host additionally owns the map. On entry the map list is read (`data\levels\*.net`, `0x44a111`, sorted by name; shown without the extension) and `0x4499d0(0)` loads the first one (`0x41c3a0` / `0x41c440` fill the setup table from the `.net` header, see below).

**Layout** (design coordinates): the map list in the left column at x 12, y = 16 + 15 i (host only; hovered row red `0xff0000`, selected row in the highlight font, others white; clicking selects it with sound 33); a guest sees "Selected level:" (string 263, grey `0xebebeb`) and the name. The CREDITS (string 276, gold 247/239/99) and RESEARCH (277, white) labels at x cx+12 / cx+3 (green +208..+212), y cy-197 / cy-140. Numbers: `0x4467b0(value, x, y)` draws up to five digits (cap 99999), 12 px apart, centred on x, no leading zeros, at (centre x, arrow top + 10): counts at x 265, credits and research at 365 (green +210). Chat: the last 7 lines (`0x447ce0`), 15 px apart, in the CHAT box; a line starting with `1` is blue (`0xff8080` COLORREF = light blue), `2` green (`0xff00`), the digit is dropped; typed text goes to the line `[+0x1174]`.

**Controls** (`0x4489b0` loop over 28 arrows, jump table `0x449340`; all gated by `[0x575730]` == the arrow's side):

| arrows | value (blue / green) | step | limits |
|---|---|---|---|
| 0/1 | fighters `0x71cafc` / `0x71cb00` | 1 | 0..20 |
| 2/3 | bombers `0x71cacc` / `0x71cad0` | 1 | 0..20 |
| 4/5 | triplers `0x71caf0` / `0x71caf4` | 1 | 0..20 |
| 6/7 | carryalls `0x71cae4` / `0x71cae8` | 1 | 0..20 |
| 8/9 | asteroids `0x71cad8` / `0x71cadc` | 1 | 0..20 |
| 10/11 | credits `0x71cac0` / `0x71cac4` | 500 | `+` while < 19000; `-` while > 0 (the exe subtracts first, so 800 goes to 300 and then -200) |
| 12/13 | research `0x71cb4c` / `0x71cb50` | 100 | `+` while < 2000, `-` while > 0 |

(`-` is the even arrow, `+` the odd; green arrows are k + 14.) Rows were tied to forces by the packet order (`cacc, cad8, cae4, caf0, cafc` = the level header's air groups bomber, asteroid, carryall, tripler, fighter, docs/air.md) and the row icons. Ten turret buttons (`0x448e17`, blue: only the host, green: only the guest) toggle the bytes `0x71cb14..18` (blue) / `0x71cb1e..22` (green) (turret types 1..5); a switched-off turret gets the X (`0x4476b8`). BLUE SIDE / GREEN SIDE (`0x448908`, `0x448951`) set `[0x575730]` (the host can switch to green and the guest to blue, copying the mouse position fields).
**Differs from the exe:** OpenSOW's BLUE / GREEN SIDE only changes the side (what the exe copies was not traced).
ACCEPT (host only, `0x44883e`): sends the setup with the start flag (`0x449b70(1)`), stores the options file (`0x452c30`), `0x42c260(7)` starts the game. BACK (`0x4488ca`): closes the session (`0x44f030`) and returns to the main menu (`0x42c260(0x1c)`).

**Defaults** come from the map (`0x41c440`): credits = header words 2 / 3 (800 on First Spark), research = words 37 / 38, air forces = the groups' counts in slot 1 (blue) / slot 2 (green) (docs/air.md, which slot is whose is untraced), turrets all available. The exe's function was not read; this is the assumption `Setup::from_level` makes.

**Chat messages**: string 266 "2Green player has joined the game" (with sound 29) when the peer arrives, 267 when it leaves; 278 "Session Lost", 279 / 280 "Green / Blue Player has left the game" are the in-game messages when a player disconnects.

## Lobby packet (message type 7, built by `0x449b70(flag)`, read by `0x4493e0`)
Written with `0x4527c0` (header: type 7, sender slot) and `0x4528f0(value, n)` (n bits, LSB first, docs/network.md): 1 bit "has map name" then the name string (`0x452650`); 8 bits = 1; 8 bits = 6; 1 bit = the flag (1 = full setup, also "start"); with the flag, for each side: 16 bits credits, 16 bits research, 8 bits each for bomber, asteroid, carryall, tripler, fighter counts (the order of the header groups), 5 single bits for the turrets. Sent every 15 ticks while the lobby is open and `[+0x11b0]` (a player is in the session) is set, and after every change; the receiver (`0x4493e0` dispatches on a 0x50-entry table `0x449970` / `0x449960`) loads the map by name, applies the fields, and on the start flag runs `0x452c30` + `0x42c260(7)`.

## What the port does
`multiplayer.rs` reproduces the three pages with the original art, positions, string indices, colours and sounds; the connection is a stub (a TCP/IP provider exists, no other session is found, so the host flow is CREATE SESSION -> lobby; `NetMenu::set_sessions`, `peer_joined` give the guest flow and the chat messages offline). ACCEPT returns `Event::Start{map, local, setup}`.
- **Differs from the exe:** there is no DirectPlay and no network play: CREATE SESSION opens no session, nobody can join, and no lobby packet (type 7) is sent or read.
- **Differs from the exe:** ACCEPT does not start a game (the exe sends the start flag, stores the options file and enters state 7): `Game::net_event` shows a notice instead, because the `.net` match itself (two human sides over the network, docs/network.md) is not playable yet. For a match on these maps against the computer see docs/skirmish.md.

Headless: `opensow-game --screenshot out.png --size 640x480 --screen multiplayer --page 0|1|2 [--sessions] [--guest] [--chat] [--hover ARROW]`.

## Open
- The state-sync records (building `0x40a2d0` / `0x40a160`, bullet `0x4128f0` / `0x412620`, air squad `0x4688d0` / `0x468790`, unit `0x46c5c0` / `0x46c300`) and the whole-game sender `0x4296a0` are decoded and exe-verified as formats (`formats::netsync`, docs/network.md), but nothing applies them to a running game yet; the lobby packet above has no Rust reader/writer.
- The map-defaults function `0x41c3a0` / `0x41c440`, the session-list x position, the highlight font `[0x5ddf38]` of the selected map row, the GDI text overlays on the art buttons (264 / 265 / 273 / 274 / 353 / 354) and what BLUE/GREEN SIDE copies were not traced; the send cadence and what the receiver does for the other message types (0x449970 table) neither. The key-handling of the chat input (`0x4289d0`) was not read; the port accepts printable characters, Backspace and Enter.
- The connection page is unreachable on a normal install, so its row layout is from the code only.
