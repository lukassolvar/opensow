# Options menu (`CMenuOptions`)

Read from the exe: setup `0x44c590..0x44cb31` (loads the art, builds the buttons), click handler `0x44bf00..0x44c4b5` (two jump tables), draw `0x44ade0`, text overlay `0x44b320`, digit routine `0x44b1f0`; the settings object is the one of `docs/config.md` (`COptions`, `0x6bf710 + 0x28 * player`, saved by `0x453310`, reloaded by `0x452c30`) plus the audio object at `0x6b3d38`. Code: `crates/opensow-game/src/options.rs`.

## Art
`Data\PICTURES\Options.srf` (640x480, the gear picture with the arrow slots), `OptionsBackground.srf` (fills the window when the screen is larger than 640 wide, `0x44ade0`), `Data\Sprites2\Menu\options.tsp` (52 frames) and `options_numbers.tsp` (digits 0-9, 13x19, hotspot 6,9). Frame `extra` is the design resolution `[640, 480, 320, 240]`: a frame's top-left is the screen centre minus its `hot` (same as docs/lands.md).

| frames | what |
|---|---|
| 0 / 1 | CANCEL (bottom centre) / O.K. (bottom left); both pointers are the same frame, so no hover art |
| 2..9 | the word "On", one per toggle (positions where the On/Off text goes) |
| 10..12 | resolution captions 640x480 / 800x600 / 1024x768 at one spot (unused by the text path) |
| 13..26 | the 14 arrow buttons, normal; 27..40 are their highlighted versions (hover frame = normal + 14, `0x44ca35..0x44caeb`) |
| 41..51 | label art RADAR, QUAKE, CLOUDS, FX, VOICE, MUSIC, VBLANK, HW CURSOR, KEYS SCROLL, MOUSE SCROLL, RESOLUTION; the exe only uses their rectangles as the click areas of the 11 label buttons |

The visible text is drawn by the exe through GDI from the language file (`0x44b320`; strings are one higher than the 0-based file index, like docs/lands.md): title 4 `Options` (top, centre-ish), headings 6 `GFX` (top left), 7 `ACCESSORY` (middle left), 8 `SFX` (top middle), labels 9..19, On/Off 267/268 (file index, 0-based). Hovered buttons draw white text, idle ones grey (`0xb4`).

## Controls (button -> handler)
Label buttons (index = frame 41 + k, click = `0x44c12a` jump table, sound 34 `options-onoff`):

| k | label | effect |
|---|---|---|
| 0 | RADAR | toggle bool `+8` |
| 1 | QUAKE | toggle bool `+9` |
| 2 | CLOUDS | `+0xc` detail: 0 -> 2, anything else -> 0 |
| 3 | FX | toggle audio byte `0x6b3d96` (effects on/off) |
| 4 | VOICE | toggle `0x6b3d95` (speech) |
| 5 | MUSIC | `0x408390(!0x6b3d94)`: music on/off (starts/stops the player) |
| 6 | VBLANK | toggle bool `+0x11` |
| 7 | HW CURSOR | toggle bool `+0x10` |
| 8, 9, 10 | KEYS SCROLL, MOUSE SCROLL, RESOLUTION | nothing (only their arrows act) |

Arrow buttons (frame 13 + k, jump table `0x44c4d8`, sound 33 `options-arrow`; "-" is the left arrow):

| k | where | effect |
|---|---|---|
| 0 / 1 | clouds row | detail `-1` (min 0) / `+1` (max 4), `0x452bc0` / `0x452c00` |
| 2 / 3 | FX row | effect volume `0x6b3d90` `-5` / `+5` via `0x4084c0` (clamped 0..100) |
| 4 / 5 | VOICE row | speech volume `0x6b3d8c` `-5` / `+5` (`0x4084e0`) |
| 6 / 7 | MUSIC row | music volume `0x6b3d88` `-5` / `+5` (`0x408490`; also sets the playing music's volume) |
| 8 / 9 | KEYS SCROLL | `+0x18` `-1` (min 0) / `+1` (max 10) (`0x452bd0` / `0x452c10`) |
| 10 / 11 | MOUSE SCROLL | `+0x14` the same (`0x452be0` / `0x452c20`) |
| 12 / 13 | RESOLUTION | previous / next of 640x480, 800x600, 1024x768, 1600x900, 1920x1080 (table `0x49ce68`, 8 bytes each; no match = nothing); this port adds 2560x1440 and 3840x2160 |

OpenSOW: `options.rs` (`OptionsMenu`, `Hit::Label` / `Hit::Arrow`) has the same buttons, effects, limits and click sounds (`--hover 0..13` lights an arrow, `14+k` a label headless).

Numbers are drawn with `options_numbers` by `0x44b1f0(x, y, value)`: the value is capped at 999; three digits at x-12, x, x+12; two digits at x-6, x+6; one digit at x (no leading zeros). Shown: detail (x = centre-65), the three volumes (x = centre+230), the two scroll speeds (x = centre-36); the resolution shows its text from the table instead. OpenSOW: same (`OptionsMenu::number`).

## Buttons and sounds
Every button's hover sound is 30 (`mainmenu-move`; `+0x54 = 0x1e`). Click sounds (`+0x58`, played by `0x456410`): labels 34, arrows 33, O.K. 38 `ingame-ano`, CANCEL 39 `ingame-nie` (setup `0x44c883..0x44c937`). O.K. stores the chosen size (`+0x1c/+0x20` from the screen-size pair `[+0xaf0]/[+0xaf4]`) and writes the config file; CANCEL reloads it (including the audio object, see below). Both then close the menu (back to the main menu or, in a game, the in-game menu). OpenSOW: same sounds and returns; O.K. writes `State of War_1.cfg` and `opensow.cfg` (`GameOptions::save`), CANCEL restores the values the menu was opened with (`Event::Cancel`), i.e. the saved ones.

## Gears (animated)
`0x44ade0` draws two turning cogs from `extras\kruh` (`.ps6` in `sprites.data`, loaded into the frame array `0x5e9658` at start-up, 20 slots, 16 real frames 0..15, hot spot = centre ~(77,77), design 640x480): first both shadows with the stencil blit `0x460f60` (`Blit::Shade`, offset +3,+3), then both cogs with the additive blit `0x45cde0`; always under the buttons. Positions are screen-centre relative (cog, shadow = cog + 3,3): cog 1 at (+0x69, +0x50), cog 2 at (+0xba, +0x51). The animation runs inside the draw (`0x44ae78`): for each logic tick elapsed since the last draw (`[0x552494]`) a counter `[0x71cdd4]` is decremented; below 0 it is reset to 3 and both frame indices step by one (period 4 ticks = 15 steps/s, 16 steps = 64 ticks per turn). Cog 1 shows frame `a = [0x71cdd8]` (starts 0), cog 2 shows frame `15 - b` with `b = [0x49ce90]` (starts 8): they turn opposite ways, half a turn apart. All three are globals that keep their values between visits. Code: `options::Gears`, `blit`.

## What the settings do (read from the exe)
- **RADAR** `+8`: the HUD only creates/draws/updates the radar widget (`+0x900` of the HUD object) when it is set (`0x4541de` draw, `0x454b1d` click, `0x4559cf` reset).
- **QUAKE** `+9`: the screen shake (camera object `0x6bf980`) is started by a destroyed building (`0x409130` -> `0x458620(duration, x, y)`, duration 0x57 = 87
  for buildings, 0x1e = 30 for turrets 25-29, epicentre = the building's centre; `duration / 2` ticks, the list of pending steps is dropped). Per tick
  `0x458930(player)`: switched off it clears the timer and the step list. Otherwise while ticks are left: distance `d` from the epicentre to the screen
  middle, `m = max(0, 9 - d/256) / 2`, each axis `sign = rand(2)`, `r = rand(m)`, step `r + 2` or `-2 - r` (x first, then y; the lockstep generator `0x437b10`),
  `0x457f80(dx, dy)` moves the camera by **twice** the step (clamped to the map) and the step is pushed on a list. When the time is over it pops one step
  per tick at a random index and applies its negative. The exe has no owner test, so enemy captures near the view do shake. OpenSOW: `World::quake_step`, `Quake`, test `quake_shakes_then_undoes_every_step`. Open: `0x437780` (the
  distance) compares an uninitialised stack slot with 200 first; taken as a plain square root.

  **Differs from the exe:** when `m` is 0 (epicentre 1792+ px from the screen middle) the port does not shake at all (the exe would still move +-2); requested by the user.

  **Differs from the exe:** the HQ's destruction also jumps the exe's camera to it (`0x4588c0`); not ported.

- **CLOUDS** `+0xc`: the cloud/smoke list limit (`Options::cloud_cap`, `0x453060`).
- **KEYS SCROLL** `+0x18` (menu rows 8/9) and **MOUSE SCROLL** `+0x14` (10/11), 0..10: the per-step map scroll is `ticks * speed` per axis, then `0x457f80(dx, dy, player)` adds **twice** that to the camera (clamped to the map). Arrow keys use KEYS (`0x415ef1`, held key state 2), the pointer at the window edge (x < 4, x > w-5, same for y; diagonal moves both axes) uses MOUSE (`0x416b8b`). Net: `120 * speed` px/s per axis (keys 5 -> 600, mouse 3 -> 360); speed 0 turns that scroll off. Right-button drag scrolling is by the pointer movement and independent of both. In the game: `options::scroll_px_per_second`, `Settings::scroll_keys/scroll_mouse`, `Play::update`.

  **Differs from the exe:** the edge band is 6 px (`Play::update`), and the right edge does not scroll while the pointer is over the HUD.

- **VBLANK** `+0x11`: when presenting a frame (`0x42d444..0x42d4a0`) it calls `IDirectDraw::WaitForVerticalBlank(DDWAITVB_BLOCKBEGIN)` first, and blits to the primary surface `[0x5dde34]` instead of the back buffer `[0x5ddec8]`; `0x42d2b1` also reads it for the cursor rule below. `[0x5baa8a]` (set at `0x42c6c2`) says the primary has an attached back buffer (a flipping chain, only in full screen with enough video memory); the wait happens only without it. No effect on the simulation (a vsync / tearing option).
- **HW CURSOR** `+0x10` ("HWMOUSE" in the file): the mouse thread (`0x4269e0`, DirectInput `GetDeviceState`) then draws the 32x32 pointer straight onto the front surface: it restores the saved rectangle under the old pointer (`0x418ca0`, a `Blt` from the back buffer), moves it (`0x419d50`) and redraws it (`0x415370`) at once. When it is off (or in a window, `[0x5baa89]`, or with no flip chain) the frame loop draws the pointer itself (`0x419c30` + `0x4152d0` at `0x42d2c5`). Only latency / flicker differ; nothing for us to do, the pointer is a window overlay here.
- The language id `+0x24` is stored by `0x453b60(id)`, which also writes the file at once. OpenSOW keeps the field as read (`config::Options`); the language comes from `lang.dat` / the launcher (docs/text.md, docs/launcher.md).

## First run
`0x452c30` loads `State of War_1.cfg` from the current directory (opened by the name at `+4`); when it cannot be opened or its archive throws (error message, then the same path) it stores `0x452fd1..0x453000`: radar and quake on (`+8` = `+9` = 1), detail 0 (clouds **off**), `+0x10` = 0 (HW cursor off), `+0x11` = 1 (VBLANK on), mouse scroll 3, keys scroll 5, size 1024 x 768 (`0x400`, `0x300`; the constructor `0x452a90`'s 1920 x 1080 is overwritten), `ResetEvent([0x6b3d08])`, and `0x408510` for the audio object: all three volumes 100, music on (`0x408390(1)`), speech and effects on. `Options::first_run`, `Audio::default`, `GameOptions::default`.

## The file holds the audio object too
`0x452c30` / `0x453310` serialize the audio object `0x6b3d38` (`0x407ff0`) into the same archive right after the options record: header `u16 1, u16 0x6121`, then six `u32`: music, speech, effect volume (clamped 0..=100 on load) and music, speech, effect switches (non-zero = on). So CANCEL (`0x44bf2e` calls `0x452c30`) reloads the volumes and switches too, and the sounds follow (music starts or stops) - the earlier note that they stay was wrong. Without a file CANCEL gives the first-run values. `scripts/verify_config.py` runs both exe serializers on random values and decodes the bytes with `config::SettingsFile` (60 bytes). The game reads/writes `State of War_1.cfg` in the current directory (`GameOptions::load/save`; O.K. saves, tests never touch the disk) and applies the audio, scroll speeds and volumes at start-up.

## Open
- RADAR off (no radar widget: not drawn, not clickable) and QUAKE are honoured (`Settings::radar_on/quake_on`). CLOUDS is honoured too (`Settings::cloud_detail`, docs/clouds.md).
- Resolution is the size the game draws at (docs/game.md).

**Differs from the exe:** VBLANK and HW CURSOR are stored only: the window is presented by the OS with vsync (`gpu.rs`), and the pointer is the system cursor (docs/cursor.md).

**Differs from the exe:** after the exe's table the resolution arrows offer 2560x1440, 3840x2160 and NATIVE (stored as 0 x 0, `options::NATIVE`): the game draws at the window's own pixel size, the whole screen when fullscreen.

**Differs from the exe:** NATIVE is our first-run value (no settings file, `GameOptions::first_run`) instead of the exe's 1024 x 768.

**Differs from the exe:** the camera scroll (arrow keys, screen edge) is `options::SCROLL_BOOST` = 1.5 times the exe's `2 * ticks * speed` (the original defaults feel slow on large windows); the Options values and the right-drag scroll are unchanged. Set the constant to 1.0 for the original speed.

**Differs from the exe:** the page is drawn at 640x480 and scaled to the largest centred 4:3 rectangle over `OptionsBackground` stretched to the window (`OptionsMenu::draw`); the exe draws the picture 1:1 at the screen centre.

**Differs from the exe:** the GDI texts (title, headings, labels, On/Off, resolution) use the medium bitmap font in English and the system's Times New Roman Bold through `gdi.rs` for other languages (docs/text.md), not the exe's own `CreateFontA` faces.

**Differs from the exe:** a green arrow at the page edge leads to the port's page 2, and a CONTROLS button opens the key bindings (both below); the exe's page has neither.

## Page 2: this port's options (not in the exe)
A green arrow at the right edge of the exe's page (design 596,227) leads to page 2 ("2/2" at the top right); a left arrow at the left edge (12,227) leads back. Page 2 uses the same picture and cogs; the baked-in arrow slots of the picture are filled from their surroundings (`wipe_arrow_slots`) because page 2 has no such arrows. Rows are `OWN_ROWS`: the label, then the value (On/Off from strings 267/268) between the exe's green left/right arrow art (frames 13/14, highlighted 27/28), like the page-1 rows with arrows, and a dimmer one-line hint under it. Either arrow toggles (sound 33), so does clicking the label (sound 34). These options do not exist in the language files: their texts (rows, hints, the heading, and the resolution caption NATIVE) are this port's own in English, Slovak, Czech and both Chinese scripts (`Tr::option_rows`, `Tr::enhancements`, `Tr::native`); the title is string 4 like page 1. `--hover 300+k` / `400+k` lights row k's left / right arrow headless. O.K. / CANCEL work for both pages (CANCEL restores page 2's values too).

| row | setting | effect |
|---|---|---|
| Enhanced textures | `Extra::enhance` (default on) | run-time 2x sprite/ground enhancement above zoom 1 (docs/enhance.md); applied on O.K. (`Game::apply_enhance`); `--no-enhance` / `OPENSOW_ENHANCE=0` force it off for a run |
| Soft shadows | `Extra::soft_shadows` (on) | blurred shadow edges (docs/fx.md) |
| Contact shadows | `Extra::contact_shadows` (on) | dark halo around buildings and under units (docs/fx.md) |
| Explosion glow | `Extra::glow` (on) | explosions and muzzle flashes light the ground (docs/fx.md); the three effects are forced off by `--no-fx` / `OPENSOW_FX=0` |
| Turret shadows | `Extra::turret_shadows` (on) | every turret casts a shadow, not only the A.A. turret (docs/fx.md) |
| Enhanced sound | `Extra::sound` (on) | sinc resampling, 2/3 ms fades and a limiter on the effects mix (docs/audio.md, "Sound enhancement"); `--no-sound-enhance` / `OPENSOW_SOUND_ENHANCE=0` force it off |
| Turret range | `Extra::turret_range` (on) | while a turret is being placed, the cells its gun reaches are tinted (docs/hotkeys.md); reaches the play screen through `Settings::turret_range` |
| Map zoom | `Extra::zoom` (on) | the mouse wheel zooms the mission map (docs/game.md, "Camera zoom"); off = the original fixed scale 1 (`Play::fixed_scale`: the wheel is ignored, the view returns to 1 around its middle, or to the least zoom that fills a window larger than the map); reaches the play screen through `Settings::zoom`. The map editor keeps its zoom |
| Swap mouse buttons | `Extra::swap_buttons` (off) | orders go on the right click, selecting stays on the left (docs/controls.md, "Swapped mouse buttons"); reaches the play screen through `Settings::swap_buttons` |

These values live in `opensow.cfg` (current directory, next to the exe's `State of War_1.cfg`, whose fixed layout is left alone): one `name=0|1` line per option (`enhance_textures`, `soft_shadows`, `contact_shadows`, `explosion_glow`, `turret_shadows`, `enhance_sound`, `turret_range`, `map_zoom`, `swap_buttons`), unknown lines ignored, missing file = defaults. `--screen options --page 1 [--hover 100]` renders page 2 headless.

## Controls view (not in the exe)
The exe's page (page 1) has a CONTROLS button (design 456,212, 128x22, under the sound rows where the picture is empty; `Hit::Controls`, sound 34); page 2 has none. It opens the Controls view over the options: three pages of its own ("1/3" at the top right, the same green page arrows), title "Controls". Its O.K. (sound 38) keeps the key changes, its CANCEL and Esc (sound 39) put back the keys it opened with; both lead back to page 1. Nothing is written yet: the options' O.K. saves the keys with everything else, their CANCEL forgets them too.

Every mission key is rebindable here, to a key or to a mouse button other than left and right (docs/hotkeys.md, "Rebindable"); Esc and the left / right mouse buttons are fixed. Code: `keybinds.rs` (`Act`, `Keybinds`), `OptionsMenu::render_controls`, `Hit::Bind` / `Hit::Defaults`. Page 1 (CAMERA) holds scrolling, HQ, last message, centre on selection and the nine bookmarks, page 2 (UNITS) next unit / Advancer, the numpad selections, select all, scatter, rally / repair and turrets 1-5, page 3 (GAME) the air strikes, buy all affordable upgrades, pause, chat, key help, leave mission, game speed, quick save and fullscreen (`keybinds::PAGE_ROWS`).

Each row is the action's name and two key slots (a key per slot, "-" when empty), with a heading, DEFAULTS at the top right and a help line under the heading:
- **Click a slot** (sound 34): it shows "press a key" and the help line says so; the next key press, or a press of the middle, back, forward or another extra mouse button (named "Mouse 3", "Mouse 4", ...), is bound there (sound 33). A key that was elsewhere moves: its old slot empties and the help line names the action it left ("X no longer does: Scatter"). Esc gives up waiting; a left click anywhere else too. Shift, Ctrl, Alt, the system keys and (in development builds) F12 / `` ` `` are refused (sound 39, a help line saying which keys are fixed) and the slot keeps waiting.
- **Right click a slot** empties it (sound 34).
- **DEFAULTS** (sound 34) puts every key back to the exe's (and this port's shortcut) keys.

Keys are physical (winit `KeyCode`) and named by their US-layout labels ("Num 1", "Page Up", ...). The texts are this port's own in the five languages (`Tr::bind_label`, `Tr::controls_heading`, ...).

The keys live in `opensow.cfg` too, one `key_<action>=<code>,<code>` line per action (winit's key names, e.g. `key_pause=KeyP,` or `key_scroll_up=ArrowUp,KeyW`; mouse buttons are `MouseMiddle`, `MouseBack`, `MouseForward` and `MouseOther<n>`; `Keybinds::write`). A missing line keeps that action's defaults, an empty or unknown name leaves the slot empty, Esc or another fixed key is dropped, and a key named twice keeps its first place (`Keybinds::parse`). The launcher keeps these lines (docs/launcher.md).

Headless: `--screen options --controls 0|1|2 [--hover 500+2k+slot | 600] [--capture 2k+slot]` opens the view on that page (`--hover` lights key slot `slot` of action `k` or DEFAULTS, `--hover 700` lights CONTROLS on page 1, `--capture` shows a slot waiting for a key); checked at 640x480, 1280x720 and 1920x1080 in English, Slovak and Chinese.
