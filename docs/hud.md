# HUD (decoded from `State of War.exe`)

Code: `crates/opensow-game/src/hud.rs`; constants checked by `scripts/verify_hud_layout.py` (venv with capstone/pefile).
The HUD is a set of **widgets** (96-byte objects, class vtable `0x48fe50`) inside the local player's state block
`0x70ba50 + player*0xc68` (so the "HUD object" is the player block). Widget fields: `+4/+8` position, `+0xc/+0x10` target
position and `+0x14` slide speed, `+0x18..+0x24` click rectangle (x0,y0,x1,y1 relative to the position), `+0x28/+0x2c/+0x30`
frames, `+0x3c` state (2 = disabled), `+0x54` id, `+0x5c/+0x5d` "anchor x to the right / y to the bottom edge". Methods:
`0x456470` set position, `0x456490` set click rectangle, `0x456440` set frames, `0x4564b0` draw (adds the screen size
`[0x49a340]` x / `[0x49a344]` y when anchored, then `0x466140` blits the frame minus its hotspot). Positions are in the
original's screen pixels (640x480 .. 1024x768), nothing is scaled; `[0x49a33c]` is the mission number.

OpenSOW: `hud::Layout` places every widget at the same offsets from the same edges and `hud::draw` / `draw_slots` draw them; `infobox.rs` is the info box, `play.rs` the countdown and messages.

**Differs from the exe:** every HUD offset and sprite is multiplied by window height / 768 (never below 1, `Layout::new`), so the HUD keeps its size on large windows; the exe draws it unscaled.

Sprite arrays (loader `0x424800..0x4255..`, `0x45dab0(name, array, count)`): radar `0x5e60a0`, money `0x5e6168`, panel2nu
`0x5e6e10` (digits 0-9), ikony `0x5e6e50`, ikonyaura `0x5e7170`, infobox `0x5ea8d0`.

## Counters (`0x454570` decides, `0x430920` credits, `0x430b50` research)
- Shown per frame: credits tile if any player has credits > 0 or any building is a gold mine (kind 0x69); research tile
  if any research > 0 or any research lab (0x6a). Credits tile `money` frame 0 at **(3,3)**, five digits (value / 10000 ...,
  zero padded) frames 1+d at x+0x48+10k (**75+10k**), y+2. Research tile frame 11 at **(0x8f,3)** when credits show, else
  at (3,3); four digits frames 12+d at x+0x57+9k (**90+9k** from x=3), y+2. Frames are blitted by top-left (`0x45f1c0`),
  hotspot ignored. (Players with `[0x6bf734+p*40]` = 1 or 2 shift the credit digits by 15 px, the research digits to
  x+0x89: not used in the campaign.) OpenSOW: same (`hud.rs`); the network shift is not modelled.
## Radar (`0x454610`, widget `+0x900`, position set by `0x4550c0`)
- Widget x = -11 - mapW (anchored right), y = -1; hot (0,0), so the frame spans mapW+12 px and ends 1 px past the right edge.
  Frame `radar` 0 (76x76) for missions 1,2,6,9,11,12,14,16,20 (64x64 maps) else frame 1 (108x92). The map is drawn 1 px per
  cell at frame + (6,6): picture `radar` frame 5+mission (frames 6..27; Warmonger: its own `radar2.tsp` and small-frame list, docs/warmonger.md), then buildings, units, the camera, then the frame.
- Units: five-pixel plus per unit at its cell; list 0 colour 0x325f / 0x64bf (blue), list 1 0x3e0 / 0x7e0 (green) - the second
  of each pair is the RGB565 one (`[0x517aec] == 2`). Buildings: footprint rectangles `[b+4..b+0xc) x [b+8..b+0x10)` cells,
  colour by owner `[b+0x8c]` 0 blue, 1 green, 2 white (0xffff / 0x7fff), drawn only while `(t mod 60) < 45` (blink).
  **Differs from the exe:** in a skirmish with other player colours the unit and building dots take those colours (docs/skirmish.md "Player colours").
  Camera rectangle: white outline at camera/32, ceil(W/32) x ceil(H/32) cells. OpenSOW: same (`hud::draw`).

  **Differs from the exe:** while the view sits in the map's top-right corner, clicks on the radar go through to the map (`Play::radar_passes_through`), so objects under it stay reachable.

## Air pills (`0x453e80` add stock, `0x453f00` add pill, `0x455520` remove, `0x455100` initial positions)
- A pill exists for each air force with stock > 0. Kind 30..34 -> `ikony` frame 35..39 (bomber, carryall, fighter, tripler,
  asteroid; 62x50, hot (31,26)). At level start the stock is added in the order kinds 30, 34, 31, 33, 32 (`0x41d894..`: the
  header order); later arrivals append, a pill at 0 stock is removed and the others move up. Pill n rests at x = 0x1f (31)
  (starts at -19 and slides at speed 5), y = 0x3c + 0x33 n (**60 + 51 n**); click rectangle (-30,-26)..(28,23).
- Stock count in `panel2nu` digits (file `__extras\\panel2nu.ps6`, with the `__` prefix) (blit top-left) at the pill's x, y+12: one digit x+12, two digits x+10 / x+15, three
  x+7 / x+12 / x+17, every digit shifted 1 px left when the ones digit is not 1 (`0x4543a2..0x4544e9`).
- Hover glow: the pill widget's overlay `+0x34` is `ikonyaura` frame 2 (`[0x5e7178]`, stored at `0x454014`), added on top while the pointer is on the pill (like the slot icons' frame 0 and the tabs' frame 1).
- Disabled state: the per-tick loop sets every pill's state to 2 while the player's launch lock `[hud+0x40]` is above 0 (`0x454e4e`; set for 3 s by a launch, `0x467efb`), else 0. State 2 draws the pill half-additive (`0x45c220`); the hover loop (`0x454d02`) skips a disabled pill when it stores the hovered id, so it cannot be clicked, but the widget update `0x456350` still runs (glow and hover sound stay).
- OpenSOW: same (`hud::draw`, `Layout::pills_locked` from `World::strike_lock`; a click on a disabled pill goes on to the map, `Play::left_down`).

  **Differs from the exe:** the armed pill (an air strike waiting for its target click) keeps the hover glow while the pointer is elsewhere.

## Turret tabs (`0x4551f0..0x455500` build, `0x454e60` per tick)
- Up to five widgets, `ikony` frame 25+tier-1 (91x42, hot (46,21)), added in tier order for every tier whose flag
  `[player+0xa5..0xa9]` is set; widget j rests at x = **-11 - 57 j** (anchored right, so the rightmost tab is mostly under
  the edge) and y = **-21** (anchored bottom), click rectangle (-44,-21)..(13,21), slide-in speed 6. State 0 when
  `credits >= 200 * tier` else 2 (disabled).
- The flags are the level header block bytes 84..88 (player 0) and 94..98 (player 1), copied at `0x41d7f2` (block word w =
  `Level::header[w + 2]`). Missions without them (M01) have no tabs; M12 and M20 have all five.
- OpenSOW: same (`hud::Layout`, `World::turret_flags`); a hovered tab also shows its price above it (not in the exe).
## Info box (`0x43b3c0`, widget `+0x8a0`)
- Fully decoded, see the section "Info box (`0x43b3c0` draw, `0x43dfd0` mode switch ...)" below. `0x43dfd0` is only the mode
  switch; the names "infobox-income/outcome" (sounds 51/52) just mean open/close. The earlier "income/outcome content" note was wrong.
## Countdown (`0x4209a4..0x4209d3` -> `0x430d50`)
- Drawn when the level's timer flag (`[0x5baacc]`, header `[65]`) is set, not in multiplayer and not after the end:
  `0x430d50(x = W/2, y = 13)`; text = language string + " " + minutes + ":" + seconds, each number zero-padded to two
  digits (minutes `[0x5baac4]`, seconds `[0x5baac8]`), GDI text left aligned at x = W/2 (the screenshots show it
  uppercase). Matches the original screenshot (starts at the screen's middle). OpenSOW: `Play::draw_hud`, same text (uppercased), place and colours.

  **Differs from the exe:** the text is the small bitmap font in English and the system's Times New Roman Bold at 15 px (`gdi.rs`) otherwise, not the exe's GDI face `[0x5ddf1c]`.

## Messages (`0x402ec0` -> `0x4036xx` add, `0x4036a0` draw, `0x4047a0` update)
- New entry: text, x = 10 - text width, y = -0x2a (-42, relative to the bottom edge), life 0x1e0 = 480; inserted at the
  front of the list (`0x4038f0`); every existing entry gets `+0x14` (20) px of pending push (`0x4032b5`). Per tick:
  x grows 3 px up to 10 (slide in from the left); then life--, and pending push moves y down by a table value
  (`[0x552ba8 + [0x517b10+i*4]]` = row 30 of the step table `0x5524a0` (60 bytes per row, written only by the table builder `0x436e01`): `steps_at(30, i)`, i.e. 30 px per second; `play.rs::message_pose` uses it); an entry is removed
  when its life ends or y > 0 (bottom edge). Drawn newest last at (x, y + H) with GDI text colour (255,243,148) and shadow
  (74,4,0) at +2,+2 (`0x466f40` args 0xff,0xf3,0x94 / 0x4a,4,0). OpenSOW: same (`play::message_pose`, `Play::draw_hud`); the F3/F4 speed lines and the chat lines (docs/chat.md) share the queue.

  **Differs from the exe:** the font is the small bitmap font in English and the system's Times New Roman Bold at 15 px (`gdi.rs`) otherwise, not the exe's 15 px bold GDI face `[0x5ddf38]`.

## Info box (`0x43b3c0` draw, `0x43dfd0` mode switch, `0x43cb30` tick, `0x43cf20` page text; `infobox.rs` / `formats::infobox`)
- Shown for the **selected own building** (`0x455b3b`: building with the player's selected flag) and for the **hovered
  production slot icon** (`0x454d30` -> `0x43cde0(slot index, kind, locked)`: only the slot loop `0x454ba0` stores the slot index in `[ebp-0x10]`; the turret tab loop `0x454b56` and the air pill loop `0x454cf0` only set the hovered id `[hud+4]`, the index stays -1 and `0x43cde0` hides the box - confirmed by footage of the original: **no info box for turret tabs or air pills**; the kind = the slot's unit kind, a locked slot shows
  "No Info Available" typed instead, see mode 3). Not for selected units. The widget (`+0x8a0`) rests at x = -65 (right anchored) and y = 99
  with hot (65,99): the 130x173 frame fills the top-right corner over the radar; hidden at y = -91, slide 13 px/tick.
- Modes (`[+0x24]`): 0 hidden; 1 sliding in, static `infobox` frame 2 (entering from 0/6 plays sound 51); 2 blue static
  frames 21..26, two ticks each, two cycles (`0x43cc04`: `[+0x28]` 1 -> -1); 4 stats page; 5 description page; 6 sliding out
  (sound 52 when leaving a visible mode); mode 3 is the page of a **locked production slot** (see below).
  Arrival of the slide triggers mode 2 (`0x454f4b`). Selecting another kind while open forces mode 2 again, silently.
- Mode 4/5: window = `infobox` frames 2..7 (green static, index every 9 ticks) at the frame's hot-corner + (6,6), the
  picture (below), 7 text lines typed one character per 3 ticks, caret `|` after the last typed character. When all
  characters are typed the picture runs 100 steps of 2 ticks, then mode 5 types the description (word-wrapped to 105 px,
  5 lines visible at (X-54, Y+13+9i), scrolling once the first line is typed).
- Lines (X = W-65, Y = current y), typed in this order: name (X-54, Y+13), type (X-54, Y+22), offence (X-54, Y+31),
  scope (X+8, Y+31), defence (X-54, Y+40), speed (X+8, Y+40), shell (X-54, Y+49); white GDI text.
- Picture: `Data\Sprites2\InfoBox\<folder>\<name>.tsp` (`0x43bce0`: units `3\aa..`, upgrades `6\gold1..`, turrets `2\tur1..`,
  air `1\bmbf..`, walkers `7\1..`, buildings `2\base..`), frame hot at (X, Y + offset) with offset -30..-50 per kind, 32
  frames (16 for kinds 16-21, 106, 108) advancing every 7 ticks.
- Text (`lang` indices are the exe's minus one): "NAME: %s" 283; type lines 286 ground unit / 287 walker / 288
  production / 289 sentry / 290 air forces / 291 advancer / 292 building; "OFFENCE: %d" 293, "DEFENCE: %d" 294, "OFFENCE:
  n/a/special/ultimate" 295-297, "SCOPE: %s" 298, "SPEED: %s" 299 / "SPEED: %d" 300, "cargo: reinforcements" 301; "SHELL: %s"
  285 (or "BLAST: %s" 284 for kinds 13, 14, 15, 27, 40, 41, 43, 45) with the shell text of `0x4321c0`; names `0x431ad0`,
  descriptions `0x43c410` (tables in `formats::infobox`, checked against the install's English file).
- Numbers: units/walkers/Advancer: offence = the weapon's `0x46b440` / 10, defence = hit points, scope `min-max` cells (or
  `max` when min = 0), speed = chassis speed (equals `walk_params().speed`); turrets: unit kinds 109..113 for the numbers,
  hit points from `0x4099a0`, speed 0; air forces: offence fixed (6000 / special + cargo line / 1000 / 20000 / ultimate),
  defence 480 / 360 / 8000 / 270 / 120, speed 120 (fighter 720, tripler 150); buildings: "OFFENCE: n/a", defence hit
  points / 100 (135), scope and speed "n/a". All gathered by running the exe's own functions
  (`scripts/emulate_infobox.py`, unicorn).
- OpenSOW: `infobox.rs` (modes, timings, slide, sounds) with the texts and numbers of `formats::infobox`.

  **Differs from the exe:** the box texts use the small bitmap font in English and a system Arial Bold at 9 px (`Gdi::arial_for_language`) otherwise, not the exe's GDI Arial `[0x5ddf24]`.

## Which building is shown (`0x455ab0`)
The first of the player's own buildings with the selected flag; if none, the building under the pointer (`0x418da0`, the map cell
grid), whoever owns it. Its slots (all kinds with slot data: factories, air base, gold mine, research lab, wind) and its info page
are shown; bars skip neutral owners and kinds 105-107, buying needs an own building. **Only the selected own building gets the
info page**: `0x455b3b` (the selected branch) calls `0x43ccc0` / `0x43dfd0(1)` / `0x43cf20`, the hover branch (`0x455b90`) hides the
box (`0x43dfd0(6)`) and only fills the slots. So pointing at an enemy building shows its production without clicking and without the
info box; clicking an enemy building selects nothing (`0x417b9a` tests `[b+0x8c] == player`). `play.rs`: `selected_building` (info box) and
`hovered_building`; `shown_building` = selected, else hovered (slots, bars).

**Differs from the exe:** a click on any building with nothing selected plays the building click sound, enemy and neutral ones included (`Play::left_up`).

## Production slot icons (selected building; draw `0x4541b0` loop `0x4542a6`, set-up `0x455c10..0x455d91`, click `0x454ba0`)
- Up to five `ikony` icons (frame = the slot's kind, hot (28,30), click rect x -26..27, y -29..26) in a column at x = W-28,
  first at y = H/2 + 50 + (60 - 60 n)/2 (+15 at 640 wide, +10 more for four or more), 60 px apart. Per slot three things
  are drawn in this order, all widgets of `0x4560d0`: the icon, the arrow, the bar.
- **Widget draw modes** (`0x4560d0`, field `+0x3c`): 0 plain copy (`0x45edf0`), 1 additive (`0x45cde0`), 2 half-additive
  (`0x45c220`, `dst + src/2`). The overlay frame `+0x34` is added on top (additive) when set; in play it is a hover glow (shown only under the pointer, confirmed by the user against the original). Frames `+0x28/+0x2c/+0x30` are
  chosen by `+0x38`/`+0x40`, which nothing in the HUD code changes, so frame 0 is what is seen.
- **Icon**: plain when the slot is active (`index < [bld+0x144]`), mode 2 when locked; overlay = `ikonyaura` frame 0
  (a brown glow border) on hover only. The turret tabs use `ikonyaura` frame 1 the same way (`0x455400`).
- **Arrow** (`ikonyrpoints` frame 0, 34x43, hot (15,22)) at the icon's x - 40, y - 3, click rect x -15..14, y -16..20; it
  exists only while `[slot+0x20] < [slot+0x24]` (level < limit; otherwise the widget is hidden), mode 0 when the slot is
  active and the side's research points reach the slot's unlock price `[slot+0x1c]`, else mode 2. **The arrow is the buy
  button**: `0x454ba0` hit-tests it, checks the building belongs to the side, `index < active` and the price, subtracts it
  and calls `0x40e730`. The icon only drives the hover (info box); the arrow hover shows the same page.
- **Bar** (`0x437390`, called at `0x454339`): 4 rows x 33 px at (icon x - 8, icon y + 21) in the 565 colour set; rows
  (flag 0, green) `0x960c 0xd7f4 0x6467 0x3ae3`, (flag 1, grey) `0x632c 0xa534 0x528a 0x39c7`; `0x517aec` = 2 means 565
  (the 555 set is used otherwise). The icon art already has a full green bar there. **Active slot**: paints the remainder
  from `progress * 33 / target` to the end in grey (skipped when the building's owner is not blue/green or its kind is
  105-107). **First locked slot only** (`[ebp-1]` once flag): paints `[bld+0x148] * 33 / target` pixels green, i.e. the
  Advancer's upgrade progress towards unlocking it.
- **Rally point button** (widget `+0x840`, id `0x7b`, `ikonyrpoints` frames 1 / 3): at the top slot icon + (4, -41); see docs/rally.md.
- OpenSOW: same layout, draw modes, glow, arrow buy rule and bars (`hud::draw_slots`, `Play::hud_click`).
## Locked slot page (mode 3), message push, black radar, widget frames
- `0x43cde0(kind, ?, flag)`: `flag` is the hovered slot icon's state (`[icon+0x3c]`, 2 = locked). With the flag the page text is
  not rebuilt: `0x43cf20(0)` sets `[+0x58] = 0`, and after the blue static `0x43cc17` goes to **mode 3** (`0x43e107`: six static
  frames of nine ticks, no picture) instead of the stats. Its one line is lang string 283 (1-based; "No Info Available",
  `0x43dea3`) typed one character per four ticks (`[+0x38]` = 3) up to length + 4, with a caret, at (x - 50, y - 50) of the
  box anchor (`0x43b45c`); the mode never ends by itself. (`[+0x58]` 1 = the selected building's page, 2 = a hovered icon's;
  `0x5ddfe0` / `0x5de080` are the two picture frame arrays, not text.) The arrow's hover shows the icon's page.
- **Kind 46** (the pod): the unit constructor builds a chassis but no weapon (emulated with `operator delete` hooked), so
  offence 0, hit points 1000, scope "n/a" (`0x46b4b0` prints `n/a` for a missing weapon), speed 30.
- **Message push**: `0x4047a0` adds `[0x552ba8 + [0x517b10 + i*4]]` per step; `[0x517b10+i*4]` is the tick counter `[0x5ddfc4]`
  (0..59) and `0x552ba8` is row 30 of the speed table built by `0x436df0` (row `v` = `0x5524a0 + 60 v`), i.e. 30 px per 60
  ticks: 20 px take 40 ticks. The push starts only after the slide-in (x = 10) and life (480) runs from there; the message is
  removed when y passes 0 (`play::message_pose`).
- **Radar clicks** (`0x416e21`): the radar case works on the left button held (`[esi+8]`): the camera centres on the clicked cell
  (`cell * 32 - screen / 2`, clamped to >= 0 and the map's end by `0x45ad80`); on the right button's release (`[esi+0x1b]`, the
  right-button edge flag set by `0x415b2d` / `0x416993`) it toggles `[0x5baad8]`, the **black radar**: `0x454610` then draws
  `radar` frame 4 (66x66, 64x64 maps) or 5 (98x82), plain, one pixel up and left of the map, instead of the mission picture
  (frame 5 + mission, additive); markers and the camera rectangle are drawn in both modes.
- **Widget hover / pressed frames**: the draw (`0x4560d0`) picks frame 2 when `[+0x38] > 0` and frame 1 when `[+0x40] == 0`, else
  frame 0. A scan of the whole exe finds nothing that sets those on the HUD's 0x60-byte widgets (the constructor sets `+0x40`
  = 1, the countdown `0x456300` only lowers `+0x38`): buttons never change frame, so nothing is missing.
- OpenSOW: the locked page (`infobox.rs` mode 3), the message push (`message_pose`), radar clicks (`Play::radar_look`, `radar_black`) and the pod's numbers are the same.

## GDI fonts
The `CreateFontA` faces and sizes are listed in docs/text.md ("Fonts the exe creates"); the port's stand-ins (bitmap fonts in English, `gdi.rs` otherwise) are described there too.

## Radar blit selector and timer colour (decoded)
- `0x454697`: the radar picture (frame 5+mission) is blitted **additively** (`0x45cde0`, over whatever the world drew there; the exe fills nothing
  first) unless `[0x49a354] == 2`, then plain (`0x45edf0`). `[0x49a354]` = level header word 64 (`0x41d7ed`, reset to -1 at `0x422e86`) = the
  weather: 2 in M06, M09, M13, M16, M20 (the same word makes the snow/rain code at `0x4586f3`, `0x46f806`, `0x46fe83` behave differently). The black
  radar plate is always plain. OpenSOW: same (`World::weather`, `ui::sprite_add_or_plain_mode`).
- Countdown text (`0x430d50` -> `0x467060` -> `0x466f40`): text colour (255,255,255), shadow (0,0,0) (the same shadow offset as messages).
